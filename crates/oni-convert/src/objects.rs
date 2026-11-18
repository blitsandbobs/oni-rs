//! Level objects: the editor-placed things (characters, patrol paths, flags,
//! triggers, doors, melee profiles…) stored per object type as a binary-data
//! blob (`BINA` instance named `CJBO` + the type's group name, e.g.
//! "CJBOCharacter"): an object file of size-prefixed chunks, each a 36-byte
//! header (type, id, flags, position, rotation in degrees) followed by the
//! type's own data.

use oni_dat::{Dat, FourCc};

use crate::{le_f32, le_u32};

const TAG_BINA: FourCc = FourCc(0x4249_4E41);
const HEADER: usize = 36;

/// One placed object.
pub struct Object<'a> {
    pub id: u32,
    pub position: [f32; 3],
    /// Euler angles, degrees.
    pub rotation: [f32; 3],
    /// The type's own data.
    pub data: &'a [u8],
}

/// The objects of group `group` in this file, and the object-file version
/// they were saved with.
pub fn read<'a>(dat: &Dat<'_>, raw: &'a [u8], group: &str) -> (u32, Vec<Object<'a>>) {
    let name = format!("CJBO{group}");
    let Some(inst) = dat.instances.iter().find(|i| i.tag == TAG_BINA && i.name.as_deref() == Some(name.as_str()))
    else {
        return (0, Vec::new());
    };
    let Some(bd) = dat.instance_data(inst) else {
        return (0, Vec::new());
    };
    // Binary data: size @0, offset into .raw @4; there, an 8-byte header
    // (class, payload size) and the payload: version, then chunks.
    let start = le_u32(bd, 4) as usize + 8;
    let Some(payload) = raw.get(start..) else {
        return (0, Vec::new());
    };
    let version = le_u32(payload, 0);
    let mut out = Vec::new();
    let mut at = 4;
    while at + 4 <= payload.len() {
        let size = le_u32(payload, at) as usize;
        if size == 0 {
            break;
        }
        if let Some(c) = payload.get(at + 4..at + 4 + size).filter(|c| c.len() >= HEADER) {
            out.push(Object {
                id: le_u32(c, 4),
                position: [le_f32(c, 12), le_f32(c, 16), le_f32(c, 20)],
                rotation: [le_f32(c, 24), le_f32(c, 28), le_f32(c, 32)],
                data: &c[HEADER..],
            });
        }
        at += 4 + size;
    }
    (version, out)
}
