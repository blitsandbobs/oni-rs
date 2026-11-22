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

use oni_bytes::at;
use oni_dat::{Dat, FourCc, Instance};

/// The `OBAN` (object animation) template tag.
pub const TAG_OBAN: FourCc = FourCc(0x4F42_414E);

const OFF_FLAGS: usize = 12;
const OFF_SCALE: usize = 64;
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
    /// The `scale` matrix (4×3, column-major: three axes, then translation)
    /// applied after each key's rotation.
    pub scale: [f32; 12],
}

impl ObjectAnimation {
    /// The matrix at integer `frame` (4×3, column-major), as the original
    /// builds it: translate × rotate × scale, the translation and quaternion
    /// lerped between keys (Oni's quaternion-to-matrix is the transpose of
    /// the usual one). Localized clips (from Max space) are rotated a
    /// quarter-turn about X.
    pub fn matrix(&self, frame: i32) -> [f32; 12] {
        let k = self
            .keys
            .iter()
            .rposition(|k| k.frame <= frame)
            .unwrap_or(0);
        let a = self.keys.get(k).copied().unwrap_or(ObjAnimKey {
            frame: 0,
            translation: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
        });
        let (t, q) = match self.keys.get(k + 1) {
            Some(b) if a.frame != frame && b.frame > a.frame => {
                let u = (frame - a.frame) as f32 / (b.frame - a.frame) as f32;
                let lerp = |x: f32, y: f32| x + (y - x) * u;
                // The short way round (`MUrQuat_Lerp`): the second key
                // negated when the two point apart — else the turn between
                // them swings the long way (the brain's lasers jumped).
                let dot: f32 = (0..4).map(|i| a.rotation[i] * b.rotation[i]).sum();
                let sign = if dot < 0.0 { -1.0 } else { 1.0 };
                (
                    std::array::from_fn::<f32, 3, _>(|i| lerp(a.translation[i], b.translation[i])),
                    std::array::from_fn::<f32, 4, _>(|i| lerp(a.rotation[i], sign * b.rotation[i])),
                )
            }
            _ => (a.translation, a.rotation),
        };
        let [x, y, z, w] = q;
        let s = 2.0 / (x * x + y * y + z * z + w * w).max(1e-12);
        // Columns (m[col][row]) as Oni writes them.
        let r = [
            [
                1.0 - s * (y * y + z * z),
                s * (x * y) - s * (w * z),
                s * (x * z) + s * (w * y),
            ],
            [
                s * (x * y) + s * (w * z),
                1.0 - s * (x * x + z * z),
                s * (y * z) - s * (w * x),
            ],
            [
                s * (x * z) - s * (w * y),
                s * (y * z) + s * (w * x),
                1.0 - s * (x * x + y * y),
            ],
        ];
        let col = |m: &[f32; 12], c: usize| [m[c * 3], m[c * 3 + 1], m[c * 3 + 2]];
        let rot = |v: [f32; 3]| -> [f32; 3] {
            std::array::from_fn(|i| r[0][i] * v[0] + r[1][i] * v[1] + r[2][i] * v[2])
        };
        // (T·R)·S: S's axes rotated; its translation rotated and moved.
        let mut out = [0.0; 12];
        for c in 0..3 {
            out[c * 3..c * 3 + 3].copy_from_slice(&rot(col(&self.scale, c)));
        }
        let st = rot(col(&self.scale, 3));
        out[9..12].copy_from_slice(&[st[0] + t[0], st[1] + t[1], st[2] + t[2]]);
        if self.flags & 0x10 != 0 {
            // Rotate X by +90°: (x, y, z) → (x, -z, y).
            for c in 0..4 {
                let (yy, zz) = (out[c * 3 + 1], out[c * 3 + 2]);
                out[c * 3 + 1] = -zz;
                out[c * 3 + 2] = yy;
            }
        }
        out
    }
}

/// Decodes an [`ObjectAnimation`] from an `OBAN` instance.
pub fn decode_object_animation(dat: &Dat<'_>, inst: &Instance) -> Option<ObjectAnimation> {
    if inst.tag != TAG_OBAN {
        return None;
    }
    let d = dat.instance_data(inst)?;

    let num_keys = at::u16(d, OFF_NUM_KEYFRAMES)? as usize;
    // Sanity-bound the key count against the available data.
    if OFF_KEYFRAMES + num_keys.checked_mul(KEYFRAME_SIZE)? > d.len() {
        return None;
    }

    let mut keys = Vec::with_capacity(num_keys);
    for k in 0..num_keys {
        let o = OFF_KEYFRAMES + k * KEYFRAME_SIZE;
        let rotation = [
            at::f32(d, o)?,
            at::f32(d, o + 4)?,
            at::f32(d, o + 8)?,
            at::f32(d, o + 12)?,
        ];
        let translation = [
            at::f32(d, o + 16)?,
            at::f32(d, o + 20)?,
            at::f32(d, o + 24)?,
        ];
        let frame = at::i32(d, o + 28)?;
        keys.push(ObjAnimKey {
            frame,
            translation,
            rotation,
        });
    }

    Some(ObjectAnimation {
        name: inst.name.clone(),
        flags: at::u32(d, OFF_FLAGS)?,
        ticks_per_frame: at::u16(d, OFF_TICKS_PER_FRAME)?,
        num_frames: at::u16(d, OFF_NUM_FRAMES)?,
        door_open_frames: at::u16(d, OFF_DOOR_OPEN_FRAMES)?,
        keys,
        scale: std::array::from_fn(|i| at::f32(d, OFF_SCALE + i * 4).unwrap_or(0.0)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two keys a quarter-turn apart about Y, the second stored with its
    /// quaternion negated (the same rotation): halfway is an eighth of a
    /// turn — the short way, as `MUrQuat_Lerp` takes it.
    #[test]
    fn keys_turn_the_short_way() {
        let h = std::f32::consts::FRAC_PI_4; // half of a quarter-turn
        let key = |frame, q: [f32; 4]| ObjAnimKey {
            frame,
            translation: [0.0; 3],
            rotation: q,
        };
        let anim = ObjectAnimation {
            name: None,
            flags: 0,
            ticks_per_frame: 1,
            num_frames: 11,
            door_open_frames: 0,
            keys: vec![
                key(0, [0.0, 0.0, 0.0, 1.0]),
                key(10, [0.0, -h.sin(), 0.0, -h.cos()]),
            ],
            scale: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0],
        };
        // The X axis's turn at each frame: steps of about 9° (a lerp, not a
        // slerp: within a degree). The long way round would be far off.
        let angle = |f| {
            let m = anim.matrix(f);
            m[2].atan2(m[0]).to_degrees().abs()
        };
        for f in 0..=10 {
            assert!(
                (angle(f) - 9.0 * f as f32).abs() < 1.0,
                "frame {f}: {}°",
                angle(f)
            );
        }
    }
}
