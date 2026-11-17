//! Decode Oni `OBAN` (object animation) — scripted keyframed motion for level
//! objects: sliding doors, lifts, driving vehicles, etc.
//!
//! `OBtAnimation` (`BFW_Object_Templates.h`), after the 8-byte pre-data:
//!
//! ```text
//! offset 0    pad[12]
//! offset 12   animFlags            u32
//! offset 16   startMatrix          M3tMatrix4x3 (48)
//! offset 64   scale                M3tMatrix4x3 (48)
//! offset 112  ticksPerFrame        u16
//! offset 114  numFrames            u16
//! offset 116  doorOpenFrames       u16
//! offset 118  numKeyFrames         u16
//! offset 120  keyFrames[] (OBtAnimationKeyFrame, 32 bytes each):
//!               quat M3tQuaternion{x,y,z,w} (16), translation M3tPoint3D (12),
//!               frame i32 (4)
//! ```
//!
//! A placed object (`OBtObjectSetup.animation`) points at its `OBAN`, so the
//! clip is matched to the object by that reference (and by the `OBAN` name).

use oni_dat::{Dat, FourCc, Instance};

/// The `OBAN` (object animation) template tag.
pub const TAG_OBAN: FourCc = FourCc(0x4F42_414E);

const OFF_FLAGS: usize = 12;
const OFF_TICKS_PER_FRAME: usize = 112;
const OFF_NUM_FRAMES: usize = 114;
const OFF_DOOR_OPEN_FRAMES: usize = 116;
const OFF_NUM_KEYFRAMES: usize = 118;
const OFF_KEYFRAMES: usize = 120;
const KEYFRAME_SIZE: usize = 32;

/// One keyframe of an object animation.
#[derive(Clone, Copy, Debug)]
pub struct ObjAnimKey {
    /// Frame index of this key.
    pub frame: i32,
    /// World-space translation at this key.
    pub translation: [f32; 3],
    /// Rotation quaternion `[x,y,z,w]` at this key.
    pub rotation: [f32; 4],
}

/// A decoded object animation clip.
#[derive(Clone, Debug)]
pub struct ObjectAnimation {
    /// The animation (owner) name.
    pub name: Option<String>,
    /// Animation flags.
    pub flags: u32,
    /// Game ticks per animation frame.
    pub ticks_per_frame: u16,
    /// Total frame count.
    pub num_frames: u16,
    /// Frames at which a door is considered open (0 if not a door).
    pub door_open_frames: u16,
    /// Keyframes.
    pub keys: Vec<ObjAnimKey>,
}

fn read_u16(d: &[u8], o: usize) -> Option<u16> {
    d.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]]))
}
fn read_u32(d: &[u8], o: usize) -> Option<u32> {
    d.get(o..o + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}
fn read_i32(d: &[u8], o: usize) -> Option<i32> {
    read_u32(d, o).map(|v| v as i32)
}
fn read_f32(d: &[u8], o: usize) -> Option<f32> {
    read_u32(d, o).map(f32::from_bits)
}

/// Decodes an [`ObjectAnimation`] from an `OBAN` instance.
pub fn decode_object_animation(dat: &Dat<'_>, inst: &Instance) -> Option<ObjectAnimation> {
    if inst.tag != TAG_OBAN {
        return None;
    }
    let d = dat.instance_data(inst)?;

    let num_keys = read_u16(d, OFF_NUM_KEYFRAMES)? as usize;
    // Sanity-bound the key count against the available data.
    if OFF_KEYFRAMES + num_keys.checked_mul(KEYFRAME_SIZE)? > d.len() {
        return None;
    }

    let mut keys = Vec::with_capacity(num_keys);
    for k in 0..num_keys {
        let o = OFF_KEYFRAMES + k * KEYFRAME_SIZE;
        let rotation = [
            read_f32(d, o)?,
            read_f32(d, o + 4)?,
            read_f32(d, o + 8)?,
            read_f32(d, o + 12)?,
        ];
        let translation = [
            read_f32(d, o + 16)?,
            read_f32(d, o + 20)?,
            read_f32(d, o + 24)?,
        ];
        let frame = read_i32(d, o + 28)?;
        keys.push(ObjAnimKey {
            frame,
            translation,
            rotation,
        });
    }

    Some(ObjectAnimation {
        name: inst.name.clone(),
        flags: read_u32(d, OFF_FLAGS)?,
        ticks_per_frame: read_u16(d, OFF_TICKS_PER_FRAME)?,
        num_frames: read_u16(d, OFF_NUM_FRAMES)?,
        door_open_frames: read_u16(d, OFF_DOOR_OPEN_FRAMES)?,
        keys,
    })
}
