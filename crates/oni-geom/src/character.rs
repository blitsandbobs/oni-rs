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

// `TRtAnimation` instance layout (`BFW_Totoro_Private.h`). It opens with 13
// `tm_raw` pointers (4 bytes each) whose values are byte offsets into the
// companion `.raw`; the scalars after them are all at fixed offsets (verified
// against the data — `numFrames` @356 is checked by the trbs_decode test).
//
// Pointer block, by index (`ptrs[i]` = the u32 at byte `i * 4`):
const PTR_HEIGHTS: usize = 1; // per-frame pelvis-height track
const PTR_POSITIONS: usize = 2; // root-motion track
const PTR_ATTACKS: usize = 3; // TRtAttack[]
const PTR_SELF_DAMAGE: usize = 4; // TRtTakeDamage[] (damage u16, frame u16)
const PTR_SHORTCUTS: usize = 6; // TRtShortcut[]
const PTR_THROW: usize = 7; // TRtThrowInfo (one, or none)
const PTR_FOOTSTEPS: usize = 8; // TRtFootstep[] (frame u16, type u16)
const TRAM_NUM_FOOTSTEPS: usize = 382; // `numFootsteps` u8
const PTR_BODY: usize = 10; // TRtPosition[numFrames]: per-tick body extent
const PTR_DATA: usize = 11; // keyframe data
const PTR_SOUNDS: usize = 12; // TRtSound[] (impulse name[32], frame u16)
const TRAM_NUM_PTRS: usize = 13;
const TRAM_ATTACK_NAME: usize = 312; // `attackName[16]`: its special impact (`ONIA`)
const TRAM_NUM_SOUNDS: usize = 332; // `numNewSounds` u32
const TRAM_SOUND_FRAME: usize = 340; // `soundFrame` u16
// Scalars (byte offsets in the instance): flags(4), directAnimation[2](8),
// usedParts(4), replaceParts(4), finalRotation(4), moveDirection +
// vocalizationType(4), extentInfo(232), attackName[16], hardPause + softPause,
// numNewSounds + soundName, soundFrame + fps, compressionSize, then a u16 run.
const TRAM_FLAGS: usize = 52; // `flags` u32
// `extentInfo` (80..312): where the clip's striking parts reach.
const TRAM_RING: usize = 80; // max distance, min / max height, 36 spokes (f32)
const TRAM_FIRST_STRIKE: usize = 236; // TRtAttackBound (32 bytes)
const TRAM_FARTHEST_STRIKE: usize = 268; // TRtAttackBound
const TRAM_STRIKE_DIRECTION: usize = 300; // u32 TRtDirection (when moveDirection is none)
const TRAM_NUM_STRIKES: usize = 304; // u32 numAttackExtents
const TRAM_STRIKES: usize = 308; // raw offset of TRtAttackExtent[] (12 bytes each)
const TRAM_FRAME_COUNT: usize = 356; // `numFrames` u16
/// Fixed-point steps: positions in 0.01 units, angles in 2π/65535.
const TRAM_DIRECT_ANIM: usize = 56; // `directAnimation[2]`: two instance references
const TRAM_USED_PARTS: usize = 64; // `usedParts` u32: body parts the clip moves
const TRAM_REPLACE_PARTS: usize = 68; // `replaceParts` u32: parts an overlay replaces (else layers on)
const TRAM_FINAL_ROTATION: usize = 72; // `finalRotation` f32 (radians): net yaw the clip imparts
const TRAM_MOVE_DIRECTION: usize = 76; // `moveDirection` u16 (TRtDirection)
const TRAM_VOCALIZATION: usize = 78; // `vocalizationType` u16
const TRAM_HARD_PAUSE: usize = 328; // `hardPause` u16: unblockable pause after this clip
const TRAM_SOFT_PAUSE: usize = 330; // `softPause` u16: blockable pause after this clip
const TRAM_TYPE: usize = 346; // `type` (TRtAnimType)
const TRAM_AIMING: usize = 348; // `aimingType`
const TRAM_FROM_STATE: usize = 350; // `fromState` (TRtAnimState)
const TRAM_TO_STATE: usize = 352; // `toState`
const TRAM_VARIENT: usize = 360; // `varient` (variant bits)
const TRAM_VARIENT_END: usize = 362; // `varientEnd`
const TRAM_ATOMIC_START: usize = 364; // `atomicStart`: first non-interruptible frame
const TRAM_ATOMIC_END: usize = 366; // `atomicEnd`
const TRAM_END_INTERP: usize = 368; // `endInterpolation` (frames to blend when leaving)
const TRAM_MAX_INTERP: usize = 370; // `maxInterpolation` (cap on blend into this clip)
const TRAM_ACTION_FRAME: usize = 372; // `actionFrame` i16 (weapon swap, pickup, …)
const TRAM_FIRST_LEVEL: usize = 374; // `firstLevel`: clip unavailable before this level
const TRAM_INVULN_START: usize = 376; // `invulnerableStart` u8
const TRAM_INVULN_LENGTH: usize = 377; // `invulnerableLength` u8 (0 = none)
const TRAM_NUM_SELF_DAMAGE: usize = 379; // `numTakeDamages` u8
const TRAM_NUM_SHORTCUTS: usize = 381; // `numShortcuts` u8
const ATTACK_SIZE: usize = 32; // sizeof(TRtAttack)
const MAX_ATTACKS: usize = 16;
const SHORTCUT_SIZE: usize = 8; // sizeof(TRtShortcut)
const MAX_SHORTCUTS: usize = 8; // TRcMaxShortcuts

/// Where an attack clip's striking parts reach (`TRtExtentInfo`). Frames are
/// character-local: x to the left, `forward` ahead, heights above the floor;
/// angles 0 = ahead, growing toward the left.
#[derive(Clone, Debug, PartialEq)]
pub struct Reach {
    /// The farthest any striking point gets from where the clip starts.
    pub max_distance: f32,
    pub min_height: f32,
    pub max_height: f32,
    /// Spoke i (direction i·10°): the farthest reach within ±30° of it.
    pub spokes: [f32; 36],
    /// The first strike, and the one reaching farthest from the start.
    pub first: StrikeBound,
    pub farthest: StrikeBound,
    /// The direction the strikes reach most (meaningful when the clip has no
    /// move direction): a `TRtDirection`.
    pub direction: u16,
    /// One per damage tick, measured from that tick's root.
    pub strikes: Vec<Strike>,
}

/// A notable strike tick (`TRtAttackBound`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StrikeBound {
    pub tick: u16,
    pub attack: u8,
    pub strike: u8,
    /// Root offset from the clip start `[x, forward]` and the pelvis height.
    pub offset: [f32; 2],
    pub height: f32,
    pub distance: f32,
    pub min_height: f32,
    pub max_height: f32,
    pub angle: f32,
}

/// One damage tick's reach (`TRtAttackExtent`), from that tick's root.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Strike {
    pub tick: u16,
    pub angle: f32,
    pub distance: f32,
    pub min_height: f32,
    pub max_height: f32,
}

/// The body's extent at one tick (`TRtPosition`): root offset from the clip
/// start, the lowest point of the body and the body's height above it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BodyPoint {
    pub offset: [f32; 2],
    pub floor: f32,
    pub height: f32,
}

/// How a throw places its victim (`TRtThrowInfo`), for a clip that throws.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Throw {
    /// Where the victim stands, in the thrower's local frame (units).
    pub target_offset: [f32; 3],
    /// The victim's facing relative to the thrower's (radians).
    pub target_facing: f32,
    /// How far away the victim may be for the throw to take (units).
    pub range: f32,
    /// The animation type the victim plays (one of the "thrown" types).
    pub target_anim_type: u16,
}

/// A clip's alternate entry state (`TRtShortcut`): the clip may also be
/// selected from `state` (not just its own `fromState`), blending in over
/// `length` frames. Flag bit 0 is `Nuke` (may interrupt an atomic clip).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shortcut {
    pub state: u16,
    pub length: u16,
    pub flags: u32,
}

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
    let data_off = read_u32(d, PTR_DATA * 4)? as usize;

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
    /// Body parts the clip moves (bit = part index), and — as an overlay —
    /// those it replaces outright (the rest of `used_parts` it layers on).
    pub used_parts: u32,
    pub replace_parts: u32,
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
    /// Alternate entry states (`shortcuts`): the clip is also selectable from
    /// each shortcut's `state`, blending in over its `length`.
    pub shortcuts: Vec<Shortcut>,
    /// Up to two follow-up clips (`directAnimation[2]`, by instance name) that
    /// chain from this one with no pause and no end interpolation.
    pub direct: Vec<String>,
    /// `varientEnd` (variant bits on exit).
    pub varient_end: u16,
    /// Frames `[atomic_start, atomic_end]` during which the clip cannot be
    /// interrupted by an ordinary request (start > end = never atomic).
    pub atomic_start: u16,
    pub atomic_end: u16,
    /// The clip is unavailable on levels before this one (`firstLevel`).
    pub first_level: u16,
    /// For a throw clip: how the victim is placed and what it plays.
    pub throw: Option<Throw>,
    /// For an attack clip: where its strikes reach.
    pub reach: Option<Reach>,
    /// Per tick: the body's extent.
    pub body: Vec<BodyPoint>,
    /// Damage the character playing this clip takes, as `(frame, damage)`
    /// pairs — how a thrown victim gets hurt.
    pub self_damage: Vec<(u16, u16)>,
    /// Frames a foot comes down (`footsteps`): where the footstep sounds and
    /// noises come.
    pub footsteps: Vec<u16>,
    /// Each footstep's kind (1 left, 2 right, 3 single), as `footsteps`.
    pub footstep_kinds: Vec<u16>,
    /// The name its special attack's impact goes by (`attackName`), if any.
    pub attack_name: String,
    /// The frame its own animation sound plays (`soundFrame`).
    pub sound_frame: u16,
    /// Impulse sounds it plays on given frames (`newSounds`).
    pub sounds: Vec<(String, u16)>,
}

/// Resolves a reference-form word (`v & 1`, index `v >> 8`) to an instance name.
fn ref_name(dat: &Dat<'_>, v: u32) -> Option<String> {
    if v & 1 == 0 {
        return None;
    }
    dat.instances.get((v >> 8) as usize)?.name.clone()
}

/// Decodes the `shortcuts` array: `numShortcuts` entries (capped at
/// `TRcMaxShortcuts`), also bounded by the gap to the next `tm_raw` pointer.
fn decode_shortcuts(raw: &[u8], ptrs: &[u32], count: usize) -> Vec<Shortcut> {
    let off = ptrs[PTR_SHORTCUTS] as usize;
    if off == 0 || count == 0 {
        return Vec::new();
    }
    let bound = ptrs
        .iter()
        .map(|&p| p as usize)
        .filter(|&p| p > off)
        .min()
        .unwrap_or(off + count * SHORTCUT_SIZE);
    let n = count
        .min(MAX_SHORTCUTS)
        .min((bound - off) / SHORTCUT_SIZE);
    (0..n)
        .filter_map(|s| {
            let o = off + s * SHORTCUT_SIZE;
            Some(Shortcut {
                state: read_u16(raw, o)?,
                length: read_u16(raw, o + 2)?,
                flags: read_u32(raw, o + 4)?,
            })
        })
        .collect()
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
    let data_off = read_u32(d, PTR_DATA * 4)? as usize;
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
    let root_motion = match read_u32(d, PTR_POSITIONS * 4) {
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
    let heights = match read_u32(d, PTR_HEIGHTS * 4) {
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
    let num_shortcuts = read_u8(d, TRAM_NUM_SHORTCUTS).unwrap_or(0) as usize;
    let shortcuts = decode_shortcuts(raw, &ptrs, num_shortcuts);
    let throw = (ptrs.get(PTR_THROW).copied().unwrap_or(0) != 0)
        .then(|| {
            let o = ptrs[PTR_THROW] as usize;
            Some(Throw {
                target_offset: [read_f32(raw, o)?, read_f32(raw, o + 4)?, read_f32(raw, o + 8)?],
                target_facing: read_f32(raw, o + 12)?,
                range: read_f32(raw, o + 16)?,
                target_anim_type: read_u16(raw, o + 20)?,
            })
        })
        .flatten();
    let self_damage = {
        let off = ptrs.get(PTR_SELF_DAMAGE).copied().unwrap_or(0) as usize;
        let count = read_u8(d, TRAM_NUM_SELF_DAMAGE).unwrap_or(0) as usize;
        let bound = ptrs
            .iter()
            .map(|&p| p as usize)
            .filter(|&p| p > off)
            .min()
            .unwrap_or(off + count * 4);
        if off == 0 {
            Vec::new()
        } else {
            (0..count.min((bound - off) / 4))
                .filter_map(|k| {
                    let o = off + k * 4;
                    Some((read_u16(raw, o + 2)?, read_u16(raw, o)?))
                })
                .collect()
        }
    };
    let footsteps = {
        let off = ptrs.get(PTR_FOOTSTEPS).copied().unwrap_or(0) as usize;
        let count = read_u8(d, TRAM_NUM_FOOTSTEPS).unwrap_or(0) as usize;
        let bound = ptrs
            .iter()
            .map(|&p| p as usize)
            .filter(|&p| p > off)
            .min()
            .unwrap_or(off + count * 4);
        if off == 0 {
            Vec::new()
        } else {
            (0..count.min((bound - off) / 4))
                .filter_map(|k| {
                    let o = off + k * 4;
                    let kind = read_u16(raw, o + 2)?;
                    (kind != 0).then_some(read_u16(raw, o)?)
                })
                .collect()
        }
    };
    let footstep_kinds = {
        let off = ptrs.get(PTR_FOOTSTEPS).copied().unwrap_or(0) as usize;
        let count = read_u8(d, TRAM_NUM_FOOTSTEPS).unwrap_or(0) as usize;
        (0..if off == 0 { 0 } else { count })
            .filter_map(|k| read_u16(raw, off + k * 4 + 2))
            .filter(|&kind| kind != 0)
            .take(footsteps.len())
            .collect()
    };
    let sounds = {
        let off = ptrs.get(PTR_SOUNDS).copied().unwrap_or(0) as usize;
        let count = read_u32(d, TRAM_NUM_SOUNDS).unwrap_or(0) as usize;
        (0..if off == 0 { 0 } else { count.min(64) })
            .filter_map(|k| {
                let o = off + k * 34;
                let name = raw.get(o..o + 32)?;
                let end = name.iter().position(|&c| c == 0).unwrap_or(32);
                Some((String::from_utf8_lossy(&name[..end]).into_owned(), read_u16(raw, o + 32)?))
            })
            .filter(|(n, _)| !n.is_empty())
            .collect()
    };
    let attack_name = d
        .get(TRAM_ATTACK_NAME..TRAM_ATTACK_NAME + 16)
        .map(|b| String::from_utf8_lossy(&b[..b.iter().position(|&c| c == 0).unwrap_or(16)]).into_owned())
        .unwrap_or_default();
    let reach = decode_reach(d, raw);
    let body = {
        let off = ptrs.get(PTR_BODY).copied().unwrap_or(0) as usize;
        let n = read_u16(d, TRAM_FRAME_COUNT).unwrap_or(0) as usize;
        if off == 0 {
            Vec::new()
        } else {
            (0..n)
                .map_while(|k| {
                    let o = off + k * 8;
                    let c = |at: usize| read_i16(raw, o + at).map(|v| f32::from(v) * 0.01);
                    Some(BodyPoint {
                        offset: [c(0)?, c(2)?],
                        height: f32::from(read_u16(raw, o + 4)?) * 0.01,
                        floor: c(6)?,
                    })
                })
                .collect()
        }
    };
    let direct = (0..2)
        .filter_map(|k| read_u32(d, TRAM_DIRECT_ANIM + k * 4))
        .filter_map(|v| ref_name(dat, v))
        .collect();

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
        used_parts: read_u32(d, TRAM_USED_PARTS).unwrap_or(0),
        replace_parts: read_u32(d, TRAM_REPLACE_PARTS).unwrap_or(0),
        move_direction: read_u16(d, TRAM_MOVE_DIRECTION).unwrap_or(0),
        end_interpolation: read_u16(d, TRAM_END_INTERP).unwrap_or(0),
        max_interpolation: read_u16(d, TRAM_MAX_INTERP).unwrap_or(0),
        vocalization_type: read_u16(d, TRAM_VOCALIZATION).unwrap_or(0),
        hard_pause: read_u16(d, TRAM_HARD_PAUSE).unwrap_or(0),
        soft_pause: read_u16(d, TRAM_SOFT_PAUSE).unwrap_or(0),
        action_frame: read_i16(d, TRAM_ACTION_FRAME).unwrap_or(0),
        invulnerable_start: read_u8(d, TRAM_INVULN_START).unwrap_or(0),
        invulnerable_length: read_u8(d, TRAM_INVULN_LENGTH).unwrap_or(0),
        shortcuts,
        direct,
        varient_end: read_u16(d, TRAM_VARIENT_END).unwrap_or(0),
        atomic_start: read_u16(d, TRAM_ATOMIC_START).unwrap_or(1),
        atomic_end: read_u16(d, TRAM_ATOMIC_END).unwrap_or(0),
        first_level: read_u16(d, TRAM_FIRST_LEVEL).unwrap_or(0),
        throw,
        self_damage,
        footsteps,
        footstep_kinds,
        attack_name,
        sound_frame: read_u16(d, TRAM_SOUND_FRAME).unwrap_or(0),
        sounds,
        reach,
        body,
    })
}

/// Decodes the clip's reach (`extentInfo`), if it has strikes.
fn decode_reach(d: &[u8], raw: &[u8]) -> Option<Reach> {
    let n = read_u32(d, TRAM_NUM_STRIKES)? as usize;
    if n == 0 {
        return None;
    }
    let f = |o: usize| read_f32(d, o);
    let mut spokes = [0.0; 36];
    for (i, s) in spokes.iter_mut().enumerate() {
        *s = f(TRAM_RING + 12 + i * 4)?;
    }
    let bound = |o: usize| -> Option<StrikeBound> {
        Some(StrikeBound {
            tick: read_u16(d, o)?,
            attack: read_u8(d, o + 2)?,
            strike: read_u8(d, o + 3)?,
            // The importer stores this y with the opposite sign to the
            // body track's: negated, it is the distance ahead.
            offset: [f(o + 4)?, -f(o + 8)?],
            height: f(o + 12)?,
            distance: f(o + 16)?,
            min_height: f(o + 20)?,
            max_height: f(o + 24)?,
            angle: f(o + 28)?,
        })
    };
    let at = read_u32(d, TRAM_STRIKES)? as usize;
    let angle = std::f32::consts::TAU / 65535.0;
    let strikes = (0..n.min(255))
        .map_while(|k| {
            let o = at + k * 12;
            Some(Strike {
                tick: read_u16(raw, o)?,
                angle: f32::from(read_u16(raw, o + 2)?) * angle,
                distance: f32::from(read_u16(raw, o + 4)?) * 0.01,
                min_height: f32::from(read_i16(raw, o + 8)?) * 0.01,
                max_height: f32::from(read_i16(raw, o + 10)?) * 0.01,
            })
        })
        .collect();
    Some(Reach {
        max_distance: f(TRAM_RING)?,
        min_height: f(TRAM_RING + 4)?,
        max_height: f(TRAM_RING + 8)?,
        spokes,
        first: bound(TRAM_FIRST_STRIKE)?,
        farthest: bound(TRAM_FARTHEST_STRIKE)?,
        direction: read_u32(d, TRAM_STRIKE_DIRECTION)? as u16,
        strikes,
    })
}

/// Decodes the `attacks` array, bounding its length by the gap to the next
/// `tm_raw` pointer in `.raw` (avoids the drifted `numAttacks` count).
fn decode_attacks(raw: &[u8], ptrs: &[u32]) -> Vec<Attack> {
    let off = ptrs[PTR_ATTACKS] as usize;
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

/// One keyframe's rotation, in the standard convention (glTF / glam).
///
/// Oni's own quaternions turn the other way: its `MUrQuatToMatrix` is the
/// transpose of the usual matrix, so its `q` is our `conjugate(q)`. The Euler
/// packing (`MUrXYZrEulerToQuat`) comes out conjugated by the standard
/// product below on its own; the stored ones (8, 16) are conjugated here — so
/// every clip shares one convention, and composing them reverses the
/// original's order (`docs/WEAPONS.md`, *Overlays*).
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
                -(i16le(6)? as f32 * s),
            ])
        }
        16 => {
            let f = |o: usize| read_f32(b, o);
            normalize([f(0)?, f(4)?, f(8)?, -f(12)?])
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
