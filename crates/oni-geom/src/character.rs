//! Decode Oni `TRBS` character bodies into a rigid skeletal model.
//!
//! A `TRBS` (`TRtBodySet`) holds 5 LOD bodies. Each `TRCM` (`TRtBody`) has
//! `numParts` and references three parallel per-part arrays (`BFW_Totoro.h`):
//!   * `TRGA` — one `M3GM` geometry per part,
//!   * `TRTA` — one `M3tPoint3D` translation per part (the bone offset from its
//!     parent),
//!   * `TRIA` — one `{parent, child, sibling}` index block per part (hierarchy).
//!
//! Oni is *rigid skeletal*: each part is a separate mesh in bone-local space,
//! positioned by the bone transform. So the body maps directly onto a glTF node
//! tree — one node per part, carrying the bone translation and the part mesh —
//! which also makes it animation-ready (rotations drive node transforms).

use oni_dat::{Dat, FourCc, Instance};

use crate::{extract_mesh, Mesh};

/// The `TRBS` (body set) template tag.
pub const TAG_TRBS: FourCc = FourCc(0x5452_4253);

/// Body level-of-detail selectors (`TRtBodySelector`).
pub mod lod {
    /// Lowest detail.
    pub const SUPER_LOW: usize = 0;
    /// Low detail.
    pub const LOW: usize = 1;
    /// Medium detail.
    pub const MEDIUM: usize = 2;
    /// High detail (the usual choice).
    pub const HIGH: usize = 3;
    /// Highest detail.
    pub const SUPER_HIGH: usize = 4;
}

// TRCM (TRtBody) field offsets.
const TRCM_NUM_PARTS: usize = 4;
const TRCM_GEO_STORE: usize = 84;
const TRCM_TRANS_STORE: usize = 88;
const TRCM_IDX_STORE: usize = 92;
// Array element offsets (all tm_pad[22] + varindex u16 at 22, data at 24).
const ARR_DATA: usize = 24;

/// Names of the 19 bones of Oni's standard biped skeleton, in part order
/// (matches OniSplit's DAE rig).
pub const BONE_NAMES: [&str; 19] = [
    "pelvis",
    "left_thigh",
    "left_calf",
    "left_foot",
    "right_thigh",
    "right_calf",
    "right_foot",
    "mid",
    "chest",
    "neck",
    "head",
    "left_shoulder",
    "left_biceps",
    "left_wrist",
    "left_handfist",
    "right_shoulder",
    "right_biceps",
    "right_wrist",
    "right_handfist",
];

/// One body part: a rigid mesh at a named skeleton node.
#[derive(Clone, Debug)]
pub struct BodyPart {
    /// Bone name (e.g. `left_calf`), or `"bone<N>"` if outside the standard rig.
    pub bone: String,
    /// Bone translation relative to the parent part.
    pub translation: [f32; 3],
    /// Pose rotation quaternion `[x, y, z, w]` (identity until a pose is applied).
    pub rotation: [f32; 4],
    /// Parent part index, or `None` for the root.
    pub parent: Option<usize>,
    /// The part's mesh (in bone-local space).
    pub mesh: Mesh,
}

/// A decoded character body (one LOD).
#[derive(Clone, Debug)]
pub struct Body {
    /// Body name (from the `TRBS` instance).
    pub name: Option<String>,
    /// Parts in skeleton order.
    pub parts: Vec<BodyPart>,
}

fn read_u32(d: &[u8], o: usize) -> Option<u32> {
    d.get(o..o + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}
fn read_f32(d: &[u8], o: usize) -> Option<f32> {
    read_u32(d, o).map(f32::from_bits)
}
fn ref_index(v: u32) -> Option<usize> {
    if v & 1 != 0 {
        Some((v >> 8) as usize)
    } else {
        None
    }
}
fn resolve<'a>(dat: &'a Dat<'a>, ref_val: u32) -> Option<&'a [u8]> {
    dat.instance_data(dat.instances.get(ref_index(ref_val)?)?)
}

/// Extracts a character [`Body`] from a `TRBS` instance at the given LOD.
pub fn extract_body(dat: &Dat<'_>, trbs: &Instance, lod: usize) -> Option<Body> {
    if trbs.tag != TAG_TRBS {
        return None;
    }
    let bs = dat.instance_data(trbs)?;
    let body_ref = read_u32(bs, lod * 4)?;
    let body = resolve(dat, body_ref)?;

    let num_parts =
        u16::from_le_bytes([*body.get(TRCM_NUM_PARTS)?, body[TRCM_NUM_PARTS + 1]]) as usize;

    let geo = resolve(dat, read_u32(body, TRCM_GEO_STORE)?)?;
    let trans = resolve(dat, read_u32(body, TRCM_TRANS_STORE)?)?;
    let idx = resolve(dat, read_u32(body, TRCM_IDX_STORE)?)?;

    let mut parts = Vec::with_capacity(num_parts);
    for i in 0..num_parts {
        // geometry (M3GM) for this part
        let geo_ref = read_u32(geo, ARR_DATA + i * 4)?;
        let m3gm_inst = dat.instances.get(ref_index(geo_ref)?)?;
        let mesh = extract_mesh(dat, m3gm_inst)?;

        // translation (M3tPoint3D, 12 bytes)
        let to = ARR_DATA + i * 12;
        let translation = [
            read_f32(trans, to)?,
            read_f32(trans, to + 4)?,
            read_f32(trans, to + 8)?,
        ];

        // hierarchy (TRtIndexBlock: parent, child, sibling, zero)
        let parent_byte = *idx.get(ARR_DATA + i * 4)? as usize;
        let parent = if i == 0 || parent_byte == i {
            None
        } else {
            Some(parent_byte)
        };

        let bone = BONE_NAMES
            .get(i)
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("bone{i}"));
        parts.push(BodyPart {
            bone,
            translation,
            rotation: [0.0, 0.0, 0.0, 1.0],
            parent,
            mesh,
        });
    }

    Some(Body {
        name: trbs.name.clone(),
        parts,
    })
}

/// The `TRAM` (animation) template tag.
pub const TAG_TRAM: FourCc = FourCc(0x5452_414D);

// The only reliable TRAM struct offset: `data` is field 12 (twelve `tm_raw`
// pointers), *before* the variable-size `extentInfo`, so its offset is fixed.
// `numParts` and `compressionSize` sit past `extentInfo` where the released
// source drifted from the data, so we derive them from the frame data itself.
const TRAM_DATA: usize = 44; // tm_raw `data` -> byte offset into .raw
const TRAM_HEIGHTS: usize = 4; // tm_raw `heights` -> per-frame pelvis-height track
const TRAM_POSITIONS: usize = 8; // tm_raw `positions` -> root-motion track in .raw
const TRAM_ATTACKS: usize = 3; // `attacks` is the 4th tm_raw pointer (index 3)
const TRAM_NUM_PTRS: usize = 13; // 13 leading tm_raw pointers
const TRAM_FLAGS: usize = 52; // `flags` u32, right after the pointer block
const TRAM_FINAL_ROTATION: usize = 72; // `finalRotation` f32 (radians): net yaw the clip imparts
const TRAM_MOVE_DIRECTION: usize = 76; // `moveDirection` u16 (TRtDirection): which way the clip moves
const TRAM_VOCALIZATION: usize = 78; // `vocalizationType` u16: voice/grunt sound this clip triggers
                              // Scalar state-machine fields. Offsets: 52-byte pointer block, then flags(4),
                              // directAnimation[2](8), usedParts(4), replaceParts(4), finalRotation(4),
                              // moveDirection+vocalizationType(4), extentInfo(232), attackName[16],
                              // hardPause+softPause(4), numNewSounds+soundName(8), soundFrame+fps(4),
                              // compressionSize(2), then the contiguous u16 run below.
const TRAM_HARD_PAUSE: usize = 328; // `hardPause` u16: frames of unblockable pause (stun) after this clip
const TRAM_SOFT_PAUSE: usize = 330; // `softPause` u16: frames of blockable pause after this clip
const TRAM_TYPE: usize = 346; // `type` (TRtAnimType)
const TRAM_AIMING: usize = 348; // `aimingType`
const TRAM_FROM_STATE: usize = 350; // `fromState` (TRtAnimState)
const TRAM_TO_STATE: usize = 352; // `toState`
                                  // (`numFrames` sits at 356 — the trbs_decode test reads it to validate this math.)
const TRAM_VARIENT: usize = 360; // `varient` (variant bits)
                                 // ...varientEnd(362), atomicStart(364), atomicEnd(366), then:
const TRAM_END_INTERP: usize = 368; // `endInterpolation` (frames to blend when leaving)
const TRAM_MAX_INTERP: usize = 370; // `maxInterpolation` (cap on blend into this clip)
const TRAM_ACTION_FRAME: usize = 372; // `actionFrame` i16: frame an action fires (weapon swap, pickup, …)
                                      // ...firstLevel(374), then two u8s:
const TRAM_INVULN_START: usize = 376; // `invulnerableStart` u8: first i-frame
const TRAM_INVULN_LENGTH: usize = 377; // `invulnerableLength` u8: i-frame count (0 = none)
const ATTACK_SIZE: usize = 32; // sizeof(TRtAttack)
const MAX_ATTACKS: usize = 16;

/// One attack's hit window + effects (`TRtAttack`, `BFW_Totoro.h`). Frames are
/// animation frames; the attack connects during `[first_damage_frame,
/// last_damage_frame]`.
#[derive(Clone, Copy, Debug)]
pub struct Attack {
    pub damage: u16,
    pub knockback: f32,
    pub first_damage_frame: u16,
    pub last_damage_frame: u16,
    pub hit_stun: u16,
    pub block_stun: u16,
    pub stagger: u16,
    /// The `TRtAnimType` the victim plays when hit by this attack (`damageAnimation`):
    /// the stagger / knockdown / stun reaction clip. 0 = no forced reaction.
    pub damage_animation: u16,
    /// Attack priority (`precedence`): higher wins when two attacks trade.
    pub precedence: u16,
    pub damage_bits: u32,
    pub flags: u32,
}

fn read_u16(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*b.get(o)?, b[o + 1]]))
}

fn read_i16(b: &[u8], o: usize) -> Option<i16> {
    Some(i16::from_le_bytes([*b.get(o)?, b[o + 1]]))
}

fn read_u8(b: &[u8], o: usize) -> Option<u8> {
    b.get(o).copied()
}

/// Decodes frame 0's per-part rotation quaternions `[x,y,z,w]` from a `TRAM`
/// animation (the first keyframe of each part's stream — no interpolation). The
/// frame data lives in the companion `.raw` and is self-describing:
/// `[numParts × u16 offset table][per-part keyframe streams]`, so `numParts =
/// offsets[0] / 2` and `compressionSize` follows from the stream lengths.
pub fn frame0_pose(dat: &Dat<'_>, anim: &Instance, raw: &[u8]) -> Option<Vec<[f32; 4]>> {
    if anim.tag != TAG_TRAM {
        return None;
    }
    let d = dat.instance_data(anim)?;
    let data_off = read_u32(d, TRAM_DATA)? as usize;

    // numParts = size of the u16 offset table = offsets[0] / 2.
    let off0 = read_u16(raw, data_off)? as usize;
    if off0 < 2 || !off0.is_multiple_of(2) {
        return None;
    }
    let num_parts = off0 / 2;
    if num_parts == 0 || num_parts > 64 {
        return None;
    }

    let offsets: Vec<usize> = (0..num_parts)
        .map(|i| read_u16(raw, data_off + i * 2).map(|v| v as usize))
        .collect::<Option<_>>()?;
    let comp = derive_compression(&offsets)?;

    let mut out = Vec::with_capacity(num_parts);
    for &off_i in &offsets {
        out.push(decode_quat(comp, raw.get(data_off + off_i..)?)?);
    }
    Some(out)
}

/// Infers `compressionSize` from the per-part keyframe stream lengths. Each
/// stream is `comp + k·(1 + comp)` bytes (one quaternion plus gap-prefixed
/// keyframes), so the true `comp` makes every stream satisfy
/// `(len - comp) % (comp + 1) == 0`.
fn derive_compression(offsets: &[usize]) -> Option<u16> {
    let streams: Vec<usize> = offsets
        .windows(2)
        .map(|w| w[1].saturating_sub(w[0]))
        .filter(|&s| s > 0)
        .collect();
    for comp in [6u16, 16, 8, 4, 3] {
        let c = comp as usize;
        if !streams.is_empty()
            && streams
                .iter()
                .all(|&s| s >= c && (s - c).is_multiple_of(c + 1))
        {
            return Some(comp);
        }
    }
    None
}

/// Oni animations run at a fixed 60 frames per second (per the engine comment
/// in `TRrQuatArray_SetAnimationInternal`).
pub const ANIM_FPS: f32 = 60.0;

/// One bone's rotation keyframes within a [`Clip`].
#[derive(Clone, Debug)]
pub struct BoneChannel {
    /// Part/bone index (0..numParts).
    pub part: usize,
    /// Keyframe times in seconds.
    pub times: Vec<f32>,
    /// Quaternion `[x,y,z,w]` at each keyframe.
    pub rotations: Vec<[f32; 4]>,
}

/// A decoded animation clip: per-bone rotation keyframes.
#[derive(Clone, Debug)]
pub struct Clip {
    /// Frames per second (always [`ANIM_FPS`]).
    pub fps: f32,
    /// Total frame count.
    pub num_frames: u32,
    /// One channel per bone.
    pub channels: Vec<BoneChannel>,
    /// Root motion: per-frame character-local `[x, z]` displacement (the
    /// animation's `positions` track; `y` is always 0 — vertical is gravity).
    /// Empty if the animation has no position track (e.g. a static idle).
    pub root_motion: Vec<[f32; 2]>,
    /// Per-frame pelvis height above the ground point (the animation's
    /// `heights` track, used as `heightThisFrame` — the whole character is
    /// lifted by this each frame, giving the body its vertical bob). Empty if
    /// the animation has no height track (the engine then uses a flat 6.0).
    pub heights: Vec<f32>,
    /// Animation flags (`TRtAnimation.flags`).
    pub flags: u32,
    /// Attacks: hit windows + effects (empty for non-attack animations).
    pub attacks: Vec<Attack>,
    /// The action this clip performs (`TRtAnimation.type`, a `TRtAnimType`):
    /// stand, run, walk, turn, etc. Drives the locomotion state machine.
    pub anim_type: u16,
    /// Aiming screen / weapon-holding variant (`TRtAnimation.aimingType`).
    pub aiming_type: u16,
    /// Body state this clip starts from (`TRtAnimation.fromState`, a `TRtAnimState`).
    pub from_state: u16,
    /// Body state this clip ends in (`TRtAnimation.toState`).
    pub to_state: u16,
    /// Variant bits required to select this clip (`TRtAnimation.varient`).
    pub varient: u16,
    /// Net yaw (radians) this clip imparts to the character's facing
    /// (`TRtAnimation.finalRotation`) — e.g. a turn-in-place clip's total turn.
    pub final_rotation: f32,
    /// Which way the clip moves the character (`TRtAnimation.moveDirection`, a
    /// `TRtDirection`: 0 None, 1 Forwards, 2 Back, 3 Left, 4 Right, 5 360).
    pub move_direction: u16,
    /// Frames to blend out of this clip when it ends (`endInterpolation`).
    pub end_interpolation: u16,
    /// Max frames another clip may blend into this one (`maxInterpolation`).
    pub max_interpolation: u16,
    /// Voice / grunt sound this clip triggers (`vocalizationType`). 0 = none.
    pub vocalization_type: u16,
    /// Unblockable pause (stun) in frames after this clip (`hardPause`). 0 = none.
    pub hard_pause: u16,
    /// Blockable pause in frames after this clip (`softPause`). 0 = none.
    pub soft_pause: u16,
    /// Frame an action fires — weapon swap, pickup, etc. (`actionFrame`). 0 = none.
    pub action_frame: i16,
    /// First invulnerable (i-frame) frame (`invulnerableStart`).
    pub invulnerable_start: u8,
    /// Number of invulnerable frames (`invulnerableLength`); 0 = never invulnerable.
    pub invulnerable_length: u8,
}

/// Decodes a full `TRAM` animation: every keyframe of every bone.
///
/// Each part's stream is `[quat][gap,quat][gap,quat]…`; a keyframe's frame
/// number accumulates the 1-byte gaps. The frame data is self-describing
/// (`numParts = offsets[0]/2`, `compressionSize` from stream lengths), and
/// `numFrames` is the furthest keyframe reached by the length-bounded parts.
pub fn decode_animation(dat: &Dat<'_>, anim: &Instance, raw: &[u8]) -> Option<Clip> {
    if anim.tag != TAG_TRAM {
        return None;
    }
    let d = dat.instance_data(anim)?;
    let data_off = read_u32(d, TRAM_DATA)? as usize;
    let off0 = read_u16(raw, data_off)? as usize;
    if off0 < 2 || !off0.is_multiple_of(2) {
        return None;
    }
    let num_parts = off0 / 2;
    if num_parts == 0 || num_parts > 64 {
        return None;
    }
    let offsets: Vec<usize> = (0..num_parts)
        .map(|i| read_u16(raw, data_off + i * 2).map(|v| v as usize))
        .collect::<Option<_>>()?;
    let comp = derive_compression(&offsets)? as usize;

    // numFrames: the furthest keyframe reached by length-bounded parts
    // (all but the last, whose stream has no following offset to bound it).
    let mut num_frames: u32 = 0;
    for w in offsets.windows(2) {
        let (start, end) = (data_off + w[0], data_off + w[1]);
        let mut pos = start + comp;
        let mut frame: u32 = 0;
        while pos + 1 + comp <= end {
            frame += raw.get(pos).copied()? as u32;
            pos += 1 + comp;
        }
        num_frames = num_frames.max(frame);
    }

    let mut channels = Vec::with_capacity(num_parts);
    for (i, &off_i) in offsets.iter().enumerate() {
        let start = data_off + off_i;
        let end = offsets
            .get(i + 1)
            .map(|o| data_off + o)
            .unwrap_or(raw.len());
        let mut times = vec![0.0];
        let mut rotations = vec![decode_quat(comp as u16, raw.get(start..)?)?];
        let mut pos = start + comp;
        let mut frame: u32 = 0;
        while pos + 1 + comp <= end && frame < num_frames {
            frame += raw.get(pos).copied()? as u32;
            let q = decode_quat(comp as u16, raw.get(pos + 1..)?)?;
            pos += 1 + comp;
            times.push(frame as f32 / ANIM_FPS);
            rotations.push(q);
        }
        channels.push(BoneChannel {
            part: i,
            times,
            rotations,
        });
    }

    // Root motion: the `positions` track = `numFrames` pairs of local (x, z).
    let total_frames = (num_frames + 1) as usize;
    let root_motion = match read_u32(d, TRAM_POSITIONS) {
        Some(pos_off) if pos_off != 0 => (0..total_frames)
            .map(|f| {
                let o = pos_off as usize + f * 8;
                [
                    read_f32(raw, o).unwrap_or(0.0),
                    read_f32(raw, o + 4).unwrap_or(0.0),
                ]
            })
            .collect(),
        _ => Vec::new(),
    };

    // Height track: the `heights` array = `numFrames` floats, the pelvis
    // elevation above the ground point for each frame (`heightThisFrame`).
    let heights = match read_u32(d, TRAM_HEIGHTS) {
        Some(h_off) if h_off != 0 => (0..total_frames)
            .map(|f| read_f32(raw, h_off as usize + f * 4).unwrap_or(0.0))
            .collect(),
        _ => Vec::new(),
    };

    // Combat: flags (stable, right after the pointer block) + the attacks array
    // (bounded by the next-larger tm_raw pointer, so we never read the drifted
    // `numAttacks` scalar).
    let flags = read_u32(d, TRAM_FLAGS).unwrap_or(0);
    let ptrs: Vec<u32> = (0..TRAM_NUM_PTRS)
        .filter_map(|k| read_u32(d, k * 4))
        .collect();
    let attacks = decode_attacks(raw, &ptrs);

    Some(Clip {
        fps: ANIM_FPS,
        num_frames: num_frames + 1,
        channels,
        root_motion,
        heights,
        flags,
        attacks,
        anim_type: read_u16(d, TRAM_TYPE).unwrap_or(0),
        aiming_type: read_u16(d, TRAM_AIMING).unwrap_or(0),
        from_state: read_u16(d, TRAM_FROM_STATE).unwrap_or(0),
        to_state: read_u16(d, TRAM_TO_STATE).unwrap_or(0),
        varient: read_u16(d, TRAM_VARIENT).unwrap_or(0),
        final_rotation: read_f32(d, TRAM_FINAL_ROTATION).unwrap_or(0.0),
        move_direction: read_u16(d, TRAM_MOVE_DIRECTION).unwrap_or(0),
        end_interpolation: read_u16(d, TRAM_END_INTERP).unwrap_or(0),
        max_interpolation: read_u16(d, TRAM_MAX_INTERP).unwrap_or(0),
        vocalization_type: read_u16(d, TRAM_VOCALIZATION).unwrap_or(0),
        hard_pause: read_u16(d, TRAM_HARD_PAUSE).unwrap_or(0),
        soft_pause: read_u16(d, TRAM_SOFT_PAUSE).unwrap_or(0),
        action_frame: read_i16(d, TRAM_ACTION_FRAME).unwrap_or(0),
        invulnerable_start: read_u8(d, TRAM_INVULN_START).unwrap_or(0),
        invulnerable_length: read_u8(d, TRAM_INVULN_LENGTH).unwrap_or(0),
    })
}

/// Decodes the `attacks` array, bounding its length by the gap to the next
/// `tm_raw` pointer in `.raw` (avoids the drifted `numAttacks` count).
fn decode_attacks(raw: &[u8], ptrs: &[u32]) -> Vec<Attack> {
    let off = ptrs[TRAM_ATTACKS] as usize;
    if off == 0 {
        return Vec::new();
    }
    let bound = ptrs
        .iter()
        .map(|&p| p as usize)
        .filter(|&p| p > off)
        .min()
        .unwrap_or(off + ATTACK_SIZE);
    let n = ((bound - off) / ATTACK_SIZE).min(MAX_ATTACKS);
    (0..n)
        .filter_map(|a| {
            let o = off + a * ATTACK_SIZE;
            Some(Attack {
                damage_bits: read_u32(raw, o)?,
                knockback: read_f32(raw, o + 4)?,
                flags: read_u32(raw, o + 8)?,
                damage: read_u16(raw, o + 12)?,
                first_damage_frame: read_u16(raw, o + 14)?,
                last_damage_frame: read_u16(raw, o + 16)?,
                damage_animation: read_u16(raw, o + 18)?,
                hit_stun: read_u16(raw, o + 20)?,
                block_stun: read_u16(raw, o + 22)?,
                stagger: read_u16(raw, o + 24)?,
                precedence: read_u16(raw, o + 26)?,
                // attackExtentIndex @28, pad @30 (skipped)
            })
        })
        .collect()
}

/// Applies a pose (per-part quaternions) to a body's parts. When `zero_root` is
/// set, the root bone's rotation is left identity (dropping the animation's
/// facing) for a neutral upright pose.
pub fn apply_pose(body: &mut Body, quats: &[[f32; 4]], zero_root: bool) {
    for (i, part) in body.parts.iter_mut().enumerate() {
        if let Some(q) = quats.get(i) {
            part.rotation = if i == 0 && zero_root {
                [0.0, 0.0, 0.0, 1.0]
            } else {
                *q
            };
        }
    }
}

fn decode_quat(comp: u16, b: &[u8]) -> Option<[f32; 4]> {
    let u16le = |o: usize| -> Option<u16> { Some(u16::from_le_bytes([*b.get(o)?, b[o + 1]])) };
    let i16le = |o: usize| -> Option<i16> { Some(i16::from_le_bytes([*b.get(o)?, b[o + 1]])) };
    Some(match comp {
        6 => {
            // Euler XYZ: each u16 maps to [0, 2*pi).
            let f = std::f32::consts::TAU / 65535.0;
            euler_xyz_to_quat(
                u16le(0)? as f32 * f,
                u16le(2)? as f32 * f,
                u16le(4)? as f32 * f,
            )
        }
        8 => {
            // Direct i16 quaternion, /32767.
            let s = 1.0 / 32767.0;
            normalize([
                i16le(0)? as f32 * s,
                i16le(2)? as f32 * s,
                i16le(4)? as f32 * s,
                i16le(6)? as f32 * s,
            ])
        }
        16 => {
            let f = |o: usize| read_f32(b, o);
            normalize([f(0)?, f(4)?, f(8)?, f(12)?])
        }
        // 3/4-byte packings are rare for bipeds; fall back to identity.
        _ => [0.0, 0.0, 0.0, 1.0],
    })
}

/// Euler XYZ (radians) to quaternion `[x,y,z,w]`, matching `MUrXYZrEulerToQuat`
/// (q = qx * qy * qz).
fn euler_xyz_to_quat(x: f32, y: f32, z: f32) -> [f32; 4] {
    let (sx, cx) = (x * 0.5).sin_cos();
    let (sy, cy) = (y * 0.5).sin_cos();
    let (sz, cz) = (z * 0.5).sin_cos();
    let qx = [sx, 0.0, 0.0, cx];
    let qy = [0.0, sy, 0.0, cy];
    let qz = [0.0, 0.0, sz, cz];
    quat_mul(quat_mul(qx, qy), qz)
}

fn quat_mul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let [ax, ay, az, aw] = a;
    let [bx, by, bz, bw] = b;
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

fn normalize(q: [f32; 4]) -> [f32; 4] {
    let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if n > 0.0 {
        [q[0] / n, q[1] / n, q[2] / n, q[3] / n]
    } else {
        [0.0, 0.0, 0.0, 1.0]
    }
}

/// Finds a named animation (`TRAM`) in `dat` by name.
pub fn find_animation<'a>(dat: &'a Dat<'a>, name: &str) -> Option<&'a Instance> {
    dat.instances
        .iter()
        .find(|i| i.tag == TAG_TRAM && i.name.as_deref() == Some(name))
}

/// Iterates every `TRBS` body in `dat` (at the given LOD) that resolves.
pub fn bodies<'a>(dat: &'a Dat<'a>, lod: usize) -> impl Iterator<Item = (usize, Body)> + 'a {
    dat.instances
        .iter()
        .enumerate()
        .filter(|(_, i)| i.tag == TAG_TRBS)
        .filter_map(move |(idx, i)| extract_body(dat, i, lod).map(|b| (idx, b)))
}
