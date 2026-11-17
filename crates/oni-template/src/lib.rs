//! A generic, swap-code-driven decoder for Oni template instances.
//!
//! Every Oni template was declared in C with a `tm_template(...)` macro, from
//! which the engine's build generated a *swap-code* array describing the field
//! layout (`TMtSwapCode` in `BFW_TM_Private.h`). The engine's byte-swap and
//! pointer-fixup routines are interpreters of that bytecode walking over an
//! instance's bytes. This crate does the same: given a [`Layout`] and an
//! instance's data, [`decode`] walks the layout and yields a named,
//! offset-tagged [`Record`] — no hand-found byte offsets.
//!
//! Field-type macros map onto field [`Kind`]s like this:
//!
//! | C declaration        | [`Kind`]                      |
//! |----------------------|-------------------------------|
//! | `UUtUns8` / `char`   | [`Kind::U8`] / [`Kind::I8`]   |
//! | `UUtUns16`           | [`Kind::U16`] / [`Kind::I16`] |
//! | `UUtUns32` / `float` | [`Kind::U32`] / [`Kind::F32`] |
//! | `UUtUns64`           | [`Kind::U64`]                 |
//! | `tm_enum`            | [`Kind::Enum`] (4 bytes)      |
//! | `tm_pad char x[n]`   | [`Kind::Pad`]`(n)`            |
//! | `tm_templateref`     | [`Kind::TemplateRef`]         |
//! | `tm_raw(T)`          | [`Kind::RawPtr`]              |
//! | `tm_separate`        | [`Kind::SeparateIndex`]       |
//! | `tm_varindex UUtUns32` | [`Kind::VarIndex`]          |
//! | `tm_vararray T x[1]` | [`Kind::VarArray`]            |
//! | fixed `T x[n]`       | [`Kind::Array`]               |
//!
//! Stage 1 (this module) is the layout model + walker, with layouts authored by
//! hand. Stage 2 will parse the `tm_template` headers to generate layouts
//! automatically for every template.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::collections::HashMap;

use oni_bytes::{Endian, ReadError, Reader};
use oni_fourcc::FourCc;

pub mod generated;
pub mod overrides;

/// A tag → [`Layout`] registry built from the generated, standalone layouts
/// (no dependency on the original C source), with [`overrides`] applied for
/// templates whose released source drifted from the released data.
pub struct Registry {
    by_tag: HashMap<FourCc, Layout>,
}

impl Registry {
    /// Loads every generated template layout, then applies hand-verified
    /// overrides (which replace the generated layout for the same tag).
    pub fn load() -> Registry {
        let mut by_tag: HashMap<FourCc, Layout> = generated::layouts()
            .into_iter()
            .map(|l| (l.tag, l))
            .collect();
        for l in overrides::overrides() {
            by_tag.insert(l.tag, l);
        }
        Registry { by_tag }
    }

    /// Looks up a template layout by tag.
    pub fn get(&self, tag: FourCc) -> Option<&Layout> {
        self.by_tag.get(&tag)
    }

    /// Number of known templates.
    pub fn len(&self) -> usize {
        self.by_tag.len()
    }

    /// True when no layouts are loaded.
    pub fn is_empty(&self) -> bool {
        self.by_tag.is_empty()
    }
}

/// How a scalar/reference field is laid out and interpreted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Unsigned 8-bit.
    U8,
    /// Signed 8-bit.
    I8,
    /// Unsigned 16-bit.
    U16,
    /// Signed 16-bit.
    I16,
    /// Unsigned 32-bit.
    U32,
    /// Signed 32-bit.
    I32,
    /// 32-bit IEEE float.
    F32,
    /// Unsigned 64-bit.
    U64,
    /// 4-byte enum (`tm_enum`).
    Enum,
    /// `n` bytes of padding/opaque data (`tm_pad`), skipped.
    Pad(u32),
    /// 4-byte reference to another instance (`tm_templateref`, or a bare
    /// template pointer).
    TemplateRef,
    /// 4-byte byte offset into the companion `.raw` file (`tm_raw`).
    RawPtr,
    /// 4-byte byte offset into the companion `.sep` file (`tm_separate`).
    SeparateIndex,
    /// 4-byte `u32` whose value is the length of the following [`Kind::VarArray`]
    /// (`tm_varindex`).
    VarIndex,
    /// A fixed-length array of `len` elements, each described by `elem`.
    Array {
        /// Number of elements.
        len: u32,
        /// Layout of one element.
        elem: Vec<Field>,
    },
    /// A variable-length array whose element count is the most recent
    /// [`Kind::VarIndex`] value (`tm_vararray`; must be the final field).
    VarArray {
        /// Layout of one element.
        elem: Vec<Field>,
    },
}

impl Kind {
    /// Fixed byte width of a scalar/reference kind, or `None` for aggregates
    /// ([`Kind::Array`], [`Kind::VarArray`]) and `Pad` (handled separately).
    fn scalar_width(&self) -> Option<u32> {
        Some(match self {
            Kind::U8 | Kind::I8 => 1,
            Kind::U16 | Kind::I16 => 2,
            Kind::U32
            | Kind::I32
            | Kind::F32
            | Kind::Enum
            | Kind::TemplateRef
            | Kind::RawPtr
            | Kind::SeparateIndex
            | Kind::VarIndex => 4,
            Kind::U64 => 8,
            _ => return None,
        })
    }
}

/// A named field in a [`Layout`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    /// Field name (matches the C struct member, when known).
    pub name: String,
    /// Field kind.
    pub kind: Kind,
}

impl Field {
    /// Convenience constructor.
    pub fn new(name: impl Into<String>, kind: Kind) -> Field {
        Field {
            name: name.into(),
            kind,
        }
    }
}

/// A template's full field layout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    /// Template tag (e.g. `TXMP`).
    pub tag: FourCc,
    /// Human-readable template name.
    pub name: String,
    /// Fields, in declaration order.
    pub fields: Vec<Field>,
}

/// The kind of a reference value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefKind {
    /// Reference to another instance.
    Template,
    /// Offset into `.raw`.
    Raw,
    /// Offset into `.sep`.
    Separate,
}

/// A decoded field value.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// An integer scalar (sign-extended for signed kinds).
    Int(i64),
    /// A 32-bit float.
    Float(f32),
    /// A reference (raw 32-bit value plus what it points into).
    Ref(RefKind, u32),
    /// Opaque padding of the given byte length.
    Pad(u32),
    /// An array of sub-records.
    Array(Vec<Record>),
}

impl Value {
    /// Returns the integer value, if this is an [`Value::Int`] or a numeric
    /// reference/var-index.
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(v) => Some(*v),
            Value::Ref(_, v) => Some(*v as i64),
            _ => None,
        }
    }
}

/// One decoded field: its name, absolute byte offset within the instance, and
/// value.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// Field name.
    pub name: String,
    /// Absolute offset within the decoded buffer.
    pub offset: usize,
    /// Decoded value.
    pub value: Value,
}

/// A decoded instance: its fields in order, with a name index.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Record {
    /// Fields in declaration order.
    pub entries: Vec<Entry>,
    index: BTreeMap<String, usize>,
}

impl Record {
    fn push(&mut self, name: String, offset: usize, value: Value) {
        self.index.insert(name.clone(), self.entries.len());
        self.entries.push(Entry {
            name,
            offset,
            value,
        });
    }

    /// Looks up a field by name.
    pub fn get(&self, name: &str) -> Option<&Entry> {
        self.index.get(name).map(|&i| &self.entries[i])
    }

    /// Integer value of a named field.
    pub fn int(&self, name: &str) -> Option<i64> {
        self.get(name).and_then(|e| e.value.as_int())
    }

    /// Absolute offset of a named field.
    pub fn offset_of(&self, name: &str) -> Option<usize> {
        self.get(name).map(|e| e.offset)
    }
}

/// Decode error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// Underlying read went out of bounds.
    Read(ReadError),
    /// A [`Kind::VarArray`] appeared without a preceding [`Kind::VarIndex`].
    VarArrayWithoutIndex {
        /// Name of the offending field.
        field: String,
    },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Read(e) => write!(f, "{e}"),
            Error::VarArrayWithoutIndex { field } => {
                write!(f, "var-array '{field}' has no preceding var-index")
            }
        }
    }
}

impl std::error::Error for Error {}

impl From<ReadError> for Error {
    fn from(e: ReadError) -> Self {
        Error::Read(e)
    }
}

/// Decodes `data` according to `layout`, returning a named [`Record`].
pub fn decode(layout: &Layout, data: &[u8], endian: Endian) -> Result<Record, Error> {
    let mut r = Reader::new(data, endian);
    decode_fields(&layout.fields, &mut r)
}

fn decode_fields(fields: &[Field], r: &mut Reader<'_>) -> Result<Record, Error> {
    let mut record = Record::default();
    // The count for a VarArray is the value of the preceding integer field
    // (a `tm_varindex`, of any width); track the last integer scalar read.
    let mut last_int: Option<i64> = None;

    for field in fields {
        let offset = r.pos();
        let value = match &field.kind {
            Kind::U8 => Value::Int(r.u8()? as i64),
            Kind::I8 => Value::Int(r.i8()? as i64),
            Kind::U16 => Value::Int(r.u16()? as i64),
            Kind::I16 => Value::Int(r.i16()? as i64),
            Kind::U32 | Kind::Enum => Value::Int(r.u32()? as i64),
            Kind::I32 => Value::Int(r.i32()? as i64),
            Kind::F32 => Value::Float(f32::from_bits(r.u32()?)),
            Kind::U64 => Value::Int(r.u64()? as i64),
            Kind::Pad(n) => {
                r.skip(*n as usize)?;
                Value::Pad(*n)
            }
            Kind::TemplateRef => Value::Ref(RefKind::Template, r.u32()?),
            Kind::RawPtr => Value::Ref(RefKind::Raw, r.u32()?),
            Kind::SeparateIndex => Value::Ref(RefKind::Separate, r.u32()?),
            Kind::VarIndex => Value::Int(r.u32()? as i64),
            Kind::Array { len, elem } => {
                let mut items = Vec::with_capacity(*len as usize);
                for _ in 0..*len {
                    items.push(decode_fields(elem, r)?);
                }
                Value::Array(items)
            }
            Kind::VarArray { elem } => {
                let count =
                    last_int
                        .filter(|&c| c >= 0)
                        .ok_or_else(|| Error::VarArrayWithoutIndex {
                            field: field.name.clone(),
                        })?;
                let mut items = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    items.push(decode_fields(elem, r)?);
                }
                Value::Array(items)
            }
        };
        if let Value::Int(v) = value {
            last_int = Some(v);
        }
        record.push(field.name.clone(), offset, value);
    }
    Ok(record)
}

/// Total fixed size of a layout's fields in bytes, treating a [`Kind::VarArray`]
/// as empty (its size depends on instance data). Useful for validating that a
/// layout lines up with a template's declared size.
pub fn fixed_size(fields: &[Field]) -> u32 {
    fields
        .iter()
        .map(|f| match &f.kind {
            Kind::Pad(n) => *n,
            Kind::Array { len, elem } => len * fixed_size(elem),
            Kind::VarArray { .. } => 0,
            k => k.scalar_width().unwrap_or(0),
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walks_scalars_and_tracks_offsets() {
        let layout = Layout {
            tag: FourCc::from_bytes(*b"TEST"),
            name: "test".into(),
            fields: vec![
                Field::new("pad", Kind::Pad(4)),
                Field::new("w", Kind::U16),
                Field::new("h", Kind::U16),
                Field::new("ptr", Kind::RawPtr),
            ],
        };
        // pad(4) | w=0x0040 | h=0x0020 | raw=0x11223344
        let data = [0, 0, 0, 0, 0x40, 0x00, 0x20, 0x00, 0x44, 0x33, 0x22, 0x11];
        let rec = decode(&layout, &data, Endian::Little).unwrap();
        assert_eq!(rec.int("w"), Some(64));
        assert_eq!(rec.int("h"), Some(32));
        assert_eq!(rec.offset_of("w"), Some(4));
        assert_eq!(rec.offset_of("ptr"), Some(8));
        assert_eq!(
            rec.get("ptr").unwrap().value,
            Value::Ref(RefKind::Raw, 0x11223344)
        );
        assert_eq!(fixed_size(&layout.fields), 12);
    }

    #[test]
    fn var_array_uses_preceding_index() {
        let layout = Layout {
            tag: FourCc::from_bytes(*b"VARR"),
            name: "var".into(),
            fields: vec![
                Field::new("count", Kind::VarIndex),
                Field::new(
                    "items",
                    Kind::VarArray {
                        elem: vec![Field::new("v", Kind::U16)],
                    },
                ),
            ],
        };
        // count=3, then 3 u16 values 10,20,30
        let data = [3, 0, 0, 0, 10, 0, 20, 0, 30, 0];
        let rec = decode(&layout, &data, Endian::Little).unwrap();
        let Value::Array(items) = &rec.get("items").unwrap().value else {
            panic!()
        };
        assert_eq!(items.len(), 3);
        assert_eq!(items[2].int("v"), Some(30));
    }
}
