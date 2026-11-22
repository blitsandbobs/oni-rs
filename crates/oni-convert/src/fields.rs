//! Named fields of a decoded template instance (`oni_template`), for the
//! decoders that turn one into a converted file. A name its layout lacks
//! panics — a bug in the decoder, found on the first conversion — where a
//! wrong hand-written offset would quietly read some other field.

use oni_dat::{Dat, FourCc, Instance};
use oni_template::{Record, Registry, Value};

/// Instance `inst` decoded by its tag's layout; `None` without a layout or
/// data, or when the data is too short for the layout.
pub(crate) fn decoded(registry: &Registry, dat: &Dat<'_>, inst: &Instance) -> Option<Record> {
    let layout = registry.get(inst.tag)?;
    oni_template::decode(layout, dat.instance_data(inst)?, dat.header.endian).ok()
}

/// Every instance of `tag` in `dat` that decodes, with its record.
pub(crate) fn all<'d>(
    registry: &'d Registry,
    dat: &'d Dat<'_>,
    tag: FourCc,
) -> impl Iterator<Item = (&'d Instance, Record)> + 'd {
    dat.instances
        .iter()
        .filter(move |i| i.tag == tag)
        .filter_map(move |i| Some((i, decoded(registry, dat, i)?)))
}

/// A decoded record's fields, by name.
#[derive(Clone, Copy)]
pub(crate) struct Fields<'a>(pub &'a Record);

impl<'a> Fields<'a> {
    fn value(&self, name: &str) -> &'a Value {
        match self.0.get(name) {
            Some(e) => &e.value,
            None => panic!("the layout has no field {name:?}"),
        }
    }

    /// An integer, enum or reference field's value.
    pub fn int(&self, name: &str) -> i64 {
        match self.value(name).as_int() {
            Some(v) => v,
            None => panic!("field {name:?} isn't an integer"),
        }
    }
    pub fn u16(&self, name: &str) -> u16 {
        self.int(name) as u16
    }
    pub fn u32(&self, name: &str) -> u32 {
        self.int(name) as u32
    }
    pub fn f32(&self, name: &str) -> f32 {
        match self.value(name) {
            Value::Float(v) => *v,
            _ => panic!("field {name:?} isn't a float"),
        }
    }
    /// The vector `name.x`, `name.y`, `name.z`.
    pub fn vec3(&self, name: &str) -> [f32; 3] {
        ["x", "y", "z"].map(|c| self.f32(&format!("{name}.{c}")))
    }
    /// A `char[n]` field: up to its first NUL, as UTF-8 (lossy).
    pub fn text(&self, name: &str) -> String {
        let bytes: Vec<u8> = self
            .list(name)
            .iter()
            .map(|e| e.int("") as u8)
            .take_while(|&b| b != 0)
            .collect();
        String::from_utf8_lossy(&bytes).into_owned()
    }
    /// A `float[n]` field.
    pub fn floats(&self, name: &str) -> Vec<f32> {
        self.list(name).iter().map(|e| e.f32("")).collect()
    }
    /// An array field's elements.
    pub fn list(&self, name: &str) -> Vec<Fields<'a>> {
        match self.value(name) {
            Value::Array(items) => items.iter().map(Fields).collect(),
            _ => panic!("field {name:?} isn't an array"),
        }
    }
}
