//! Parser for Oni's template-instance container file: the `levelN_Final.dat`
//! (paired with a `.raw`, and on some builds a `.sep`).
//!
//! # On-disk format (recovered from `BFW_TM_Private.h` / `BFW_TM_Game.c`)
//!
//! A `.dat` is a flat image with a fixed 64-byte header followed by three
//! descriptor arrays and two data blocks:
//!
//! ```text
//! offset 0                      Header                      (64 bytes)
//! offset 64                     InstanceDescriptor[numInst] (20 bytes each)
//! + numInst*20                  NameDescriptor[numName]     (8 bytes each)
//! + numName*8                   TemplateDescriptor[numTmpl] (16 bytes each)
//! header.dataBlockOffset        data block                  (dataBlockLength)
//! header.nameBlockOffset        name block (NUL-terminated strings)
//! ```
//!
//! The header's `version` field is `VR31`, stored in the file's native byte
//! order; comparing it both ways tells us the endianness (PC = little, Mac =
//! big). See [`oni_fourcc::FourCc`] for how tags map to `u32`s.
//!
//! Each instance descriptor on disk is five `u32`s:
//! `templateTag, dataOffset, nameOffset, size, flags`. On load the engine turns
//! `templateTag` into a template pointer, `dataOffset` into `dataBlock +
//! offset` (unless the instance is a placeholder), and `nameOffset` into
//! `nameBlock + offset` (only when the `Unique` flag is *clear* — unique
//! instances have no name). We reproduce exactly that interpretation.
//!
//! This crate is the container layer: it enumerates every instance with its
//! tag, name, size, flags, and raw bytes. Decoding per-template fields
//! (textures, geometry, …) belongs to higher-level crates.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fmt;

use oni_bytes::{Endian, ReadError, Reader};

pub use oni_bytes::Endian as ByteOrder;
pub use oni_fourcc::FourCc;
pub use oni_fourcc::FourCc as Tag;

/// `UUm4CharToUns32('V','R','3','1')` — the instance-file version/magic.
pub const INSTANCE_FILE_VERSION: u32 = 0x5652_3331; // "VR31"

const HEADER_SIZE: u16 = 64;
const INSTANCE_DESC_SIZE: u16 = 20;
const NAME_DESC_SIZE: u16 = 8;
const TEMPLATE_DESC_SIZE: u16 = 16;

/// Persistent descriptor flags (low 16 bits; see `TMtDescriptorFlags`).
pub mod flags {
    /// Instance has no name (it is addressed only by reference from a parent).
    pub const UNIQUE: u32 = 1 << 0;
    /// Instance carries no data of its own; it stands in for another file.
    pub const PLACEHOLDER: u32 = 1 << 1;
    /// Instance points at a duplicate's data rather than its own.
    pub const DUPLICATE: u32 = 1 << 2;
    /// Instance is the source used by one or more duplicates.
    pub const DUPLICATED_SRC: u32 = 1 << 3;
    /// Mask of bits actually written to disk.
    pub const PERSISTENT_MASK: u32 = 0xFFFF;
}

/// Errors produced while parsing a `.dat`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DatError {
    /// The buffer is smaller than a header.
    TooSmall { len: usize },
    /// The `version` field matched neither endianness of `VR31`.
    BadMagic { found: u32 },
    /// A `sizeof*` header field did not match the known layout.
    BadLayout {
        field: &'static str,
        expected: u16,
        found: u16,
    },
    /// A descriptor or block referenced bytes outside the file.
    Bounds(ReadError),
}

impl fmt::Display for DatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DatError::TooSmall { len } => {
                write!(
                    f,
                    "file is {len} bytes, smaller than a {HEADER_SIZE}-byte header"
                )
            }
            DatError::BadMagic { found } => write!(
                f,
                "bad version/magic 0x{found:08X}: not 'VR31' in either byte order"
            ),
            DatError::BadLayout {
                field,
                expected,
                found,
            } => write!(f, "unexpected {field}: expected {expected}, found {found}"),
            DatError::Bounds(e) => write!(f, "out of bounds: {e}"),
        }
    }
}

impl std::error::Error for DatError {}

impl From<ReadError> for DatError {
    fn from(e: ReadError) -> Self {
        DatError::Bounds(e)
    }
}

/// The 64-byte instance-file header.
#[derive(Clone, Debug)]
pub struct Header {
    /// Byte order the file was written in.
    pub endian: Endian,
    /// Combined checksum of every template present at build time.
    pub total_template_checksum: u64,
    /// File version/magic (always [`INSTANCE_FILE_VERSION`] once parsed).
    pub version: u32,
    /// Number of instance descriptors (total instances in the file).
    pub num_instances: u32,
    /// Number of name descriptors (name → instance lookups).
    pub num_names: u32,
    /// Number of distinct templates (tags) used.
    pub num_templates: u32,
    /// Absolute offset of the instance data block.
    pub data_block_offset: u32,
    /// Length of the instance data block.
    pub data_block_length: u32,
    /// Absolute offset of the name string block.
    pub name_block_offset: u32,
    /// Length of the name string block.
    pub name_block_length: u32,
}

/// One instance: a tagged, optionally-named blob of data.
#[derive(Clone, Debug)]
pub struct Instance {
    /// Index of this instance in the file (its stable id; cross-references use it).
    pub index: u32,
    /// Template tag, e.g. `TXMP`, `SNDD`.
    pub tag: FourCc,
    /// Name, present only for non-[`UNIQUE`](flags::UNIQUE) instances.
    pub name: Option<String>,
    /// Total on-disk size of the instance's data, including trailing var-arrays.
    pub size: u32,
    /// Persistent flags (see [`flags`]).
    pub flags: u32,
    /// Offset of the instance's data within the data block, or `None` for a
    /// placeholder (which has no data).
    pub data_offset: Option<u32>,
}

impl Instance {
    /// True when the instance has no name of its own.
    pub fn is_unique(&self) -> bool {
        self.flags & flags::UNIQUE != 0
    }
    /// True when the instance is a placeholder (no data).
    pub fn is_placeholder(&self) -> bool {
        self.flags & flags::PLACEHOLDER != 0
    }
    /// True when the instance points at a duplicate's data.
    pub fn is_duplicate(&self) -> bool {
        self.flags & flags::DUPLICATE != 0
    }
    /// True when the instance carries a name (the inverse of [`is_unique`](Self::is_unique)).
    pub fn is_named(&self) -> bool {
        !self.is_unique()
    }
}

/// One template-descriptor entry.
#[derive(Clone, Debug)]
pub struct TemplateEntry {
    /// The template's tag.
    pub tag: FourCc,
    /// Checksum of the template layout at build time.
    pub checksum: u64,
    /// Number of instances that use this template.
    pub num_used: u32,
}

/// A fully-parsed `.dat` container.
pub struct Dat<'a> {
    data: &'a [u8],
    /// Parsed header.
    pub header: Header,
    /// Every instance, in file order (so `instances[i].index == i`).
    pub instances: Vec<Instance>,
    /// Template descriptors, in file order.
    pub templates: Vec<TemplateEntry>,
}

impl<'a> Dat<'a> {
    /// Parses a `.dat` image. Does not require the companion `.raw`/`.sep`.
    pub fn parse(data: &'a [u8]) -> Result<Dat<'a>, DatError> {
        if data.len() < HEADER_SIZE as usize {
            return Err(DatError::TooSmall { len: data.len() });
        }

        let endian = detect_endian(data)?;
        let mut r = Reader::new(data, endian);

        // --- Header ---
        let total_template_checksum = r.u64()?;
        let version = r.u32()?;
        let sizeof_header = r.u16()?;
        let sizeof_instance = r.u16()?;
        let sizeof_template = r.u16()?;
        let sizeof_name = r.u16()?;
        let num_instances = r.u32()?;
        let num_names = r.u32()?;
        let num_templates = r.u32()?;
        let data_block_offset = r.u32()?;
        let data_block_length = r.u32()?;
        let name_block_offset = r.u32()?;
        let name_block_length = r.u32()?;
        // pad2[4] follows; we stop reading the header here.

        check_layout("sizeofHeader", HEADER_SIZE, sizeof_header)?;
        check_layout(
            "sizeofInstanceDescriptor",
            INSTANCE_DESC_SIZE,
            sizeof_instance,
        )?;
        check_layout(
            "sizeofTemplateDescriptor",
            TEMPLATE_DESC_SIZE,
            sizeof_template,
        )?;
        check_layout("sizeofNameDescriptor", NAME_DESC_SIZE, sizeof_name)?;

        let header = Header {
            endian,
            total_template_checksum,
            version,
            num_instances,
            num_names,
            num_templates,
            data_block_offset,
            data_block_length,
            name_block_offset,
            name_block_length,
        };

        // --- Instance descriptors (each: tag, dataOffset, nameOffset, size, flags) ---
        let mut instances = Vec::with_capacity(num_instances as usize);
        r.seek(HEADER_SIZE as usize)?;
        for index in 0..num_instances {
            let tag = FourCc(r.u32()?);
            let data_offset_raw = r.u32()?;
            let name_offset_raw = r.u32()?;
            let size = r.u32()?;
            let flags = r.u32()? & flags::PERSISTENT_MASK;

            let data_offset = if flags & flags::PLACEHOLDER != 0 {
                None
            } else {
                Some(data_offset_raw)
            };

            // Non-unique instances carry a name at name_block + name_offset.
            // The stored string is prefixed with the 4-byte template tag; the
            // engine's name accessor returns `namePtr + 4`, so we skip it too.
            let name = if flags & flags::UNIQUE == 0 {
                Some(r.cstr_at(name_block_offset as usize + name_offset_raw as usize + 4)?)
            } else {
                None
            };

            instances.push(Instance {
                index,
                tag,
                name,
                size,
                flags,
                data_offset,
            });
        }

        // --- Template descriptors (each: checksum u64, tag u32, numUsed u32) ---
        let templates_offset = HEADER_SIZE as usize
            + num_instances as usize * INSTANCE_DESC_SIZE as usize
            + num_names as usize * NAME_DESC_SIZE as usize;
        let mut templates = Vec::with_capacity(num_templates as usize);
        r.seek(templates_offset)?;
        for _ in 0..num_templates {
            let checksum = r.u64()?;
            let tag = FourCc(r.u32()?);
            let num_used = r.u32()?;
            templates.push(TemplateEntry {
                tag,
                checksum,
                num_used,
            });
        }

        Ok(Dat {
            data,
            header,
            instances,
            templates,
        })
    }

    /// The raw data bytes of an instance (`None` for a placeholder, or if the
    /// recorded range falls outside the data block).
    ///
    /// Note: the engine keeps 8 bytes of "pre-data" immediately *before* this
    /// slice (`TMcPreDataSize`); it is not part of the returned bytes.
    pub fn instance_data(&self, inst: &Instance) -> Option<&'a [u8]> {
        let off = inst.data_offset?;
        let start = self.header.data_block_offset as usize + off as usize;
        let end = start.checked_add(inst.size as usize)?;
        self.data.get(start..end)
    }

    /// Count of named (non-unique) instances.
    pub fn named_count(&self) -> usize {
        self.instances.iter().filter(|i| i.is_named()).count()
    }

    /// A `tag -> count` histogram over all instances.
    pub fn tag_histogram(&self) -> BTreeMap<FourCc, usize> {
        let mut h = BTreeMap::new();
        for i in &self.instances {
            *h.entry(i.tag).or_insert(0) += 1;
        }
        h
    }

    /// A `tag -> count` histogram over named (non-unique) instances only.
    pub fn named_tag_histogram(&self) -> BTreeMap<FourCc, usize> {
        let mut h = BTreeMap::new();
        for i in self.instances.iter().filter(|i| i.is_named()) {
            *h.entry(i.tag).or_insert(0) += 1;
        }
        h
    }
}

/// Reads the `version` field (offset 8) in both byte orders and returns the one
/// that equals `VR31`.
fn detect_endian(data: &[u8]) -> Result<Endian, DatError> {
    let mut v = [0u8; 4];
    v.copy_from_slice(&data[8..12]);
    let le = u32::from_le_bytes(v);
    let be = u32::from_be_bytes(v);
    if le == INSTANCE_FILE_VERSION {
        Ok(Endian::Little)
    } else if be == INSTANCE_FILE_VERSION {
        Ok(Endian::Big)
    } else {
        Err(DatError::BadMagic { found: le })
    }
}

fn check_layout(field: &'static str, expected: u16, found: u16) -> Result<(), DatError> {
    if expected == found {
        Ok(())
    } else {
        Err(DatError::BadLayout {
            field,
            expected,
            found,
        })
    }
}
