//! Decode Oni `ONCC` (character class) — the enemy/player definition linking a
//! character to its body, textures, variant, aiming screens and **exact
//! animation set**.
//!
//! `ONtCharacterClass` (`Oni_Character.h`) is large and its middle section
//! drifts from the released source (`AI2tBehavior` holds variable-size nested
//! arrays), so computing field offsets is unreliable. But the fields we want
//! are all *template references* to other named instances, so we scan the
//! instance for reference-form words and identify each by the tag it points at:
//!
//!   * `ONCV` → `variant`         (random-variant tag, e.g. `konoko`)
//!   * `TRBS` → `body`            (the rigid-skeletal body / geometry)
//!   * `TRMA` → `textures`        (body-part materials/textures)
//!   * `TRAC` → `animations`      (the animation collection)
//!   * `TRSC` → `aimingScreens`
//!
//! The `TRAC` (`TRtAnimationCollection`) lists the character's animations and
//! may `recursiveLookup` into a parent collection it inherits from; we follow
//! that chain to produce the full set.

use oni_bytes::at;
use std::collections::BTreeSet;

use oni_dat::{Dat, FourCc, Instance};

/// The `ONCC` (character class) template tag.
pub const TAG_ONCC: FourCc = FourCc(0x4F4E_4343);
const TAG_ONCV: FourCc = FourCc(0x4F4E_4356);
const TAG_TRBS: FourCc = FourCc(0x5452_4253);
const TAG_TRMA: FourCc = FourCc(0x5452_4D41);
const TAG_TRAC: FourCc = FourCc(0x5452_4143);
const TAG_TRSC: FourCc = FourCc(0x5452_5343);

// TRtAnimationCollection layout.
const TRAC_RECURSIVE: usize = 16; // recursiveLookup ref
const TRAC_NUM: usize = 22; // numAnimations (u16)
const TRAC_ENTRY: usize = 24; // entry[] (TRtAnimationCollectionPart, 12 bytes)
const TRAC_ENTRY_SIZE: usize = 12;
const TRAC_ENTRY_ANIM: usize = 8; // animation ref within an entry

/// The 7 damage types (`P3cEnumDamageType`), matching `damageResistance[7]`.
pub const DAMAGE_TYPES: [&str; 7] = [
    "normal",
    "minorStun",
    "majorStun",
    "minorKnockdown",
    "majorKnockdown",
    "blownUp",
    "pickUp",
];

/// Gameplay scalar parameters read from the stable tail of `ONtCharacterClass`
/// (anchored on the `body` reference — the drift is all before it).
#[derive(Clone, Copy, Debug, Default)]
pub struct CharacterStats {
    /// Maximum / initial hit points.
    pub max_hit_points: u32,
    /// Random scale range `[low, high]`.
    pub scale_low: f32,
    /// Random scale range `[low, high]`.
    pub scale_high: f32,
    /// Per-damage-type resistance (0 = none, 1 = immune), see [`DAMAGE_TYPES`].
    pub damage_resistance: [f32; 7],
    /// Boss shield protection amount.
    pub boss_shield: f32,
    /// AI rate-of-fire (frames between shots).
    pub shot_time: u16,
    /// Frames before fight mode fades.
    pub fight_mode_duration: u32,
    /// Frames standing before idle.
    pub idle_delay: u32,
    /// Frames before the second idle.
    pub idle_restart_delay: u32,
    /// Frames after death before the character is deleted.
    pub death_delete_delay: u16,
    /// Bitmask marking the character's feet (for footstep effects).
    pub feet_bits: u32,
    /// Left-handed flag.
    pub left_handed: bool,
    /// Super-hypo effect.
    pub super_hypo: bool,
    /// Super-shield effect.
    pub super_shield: bool,
    /// Resists being knocked down easily.
    pub knockdown_resistant: bool,
}

/// Jump / fall tuning (`ONtAirConstants`, the first field of `ONtCharacterClass`,
/// so it sits at offset 0 and is stable). Velocities are Oni units per tick,
/// gravities units per tick² — they map directly onto the engine's jump launch
/// speed and per-tick gravity.
#[derive(Clone, Copy, Debug)]
pub struct AirConstants {
    /// Downward acceleration used by the snap-to-floor ray and melee prediction
    /// (the character itself always integrates with `jump_gravity`).
    pub fall_gravity: f32,
    /// Per-tick downward acceleration while airborne.
    pub jump_gravity: f32,
    /// Upward launch velocity added on takeoff (`jumpAmount`).
    pub jump_amount: f32,
    /// Terminal (most-negative) vertical velocity.
    pub max_velocity: f32,
    /// Extra upward velocity per tick while Jump is held early in the jump.
    pub jump_accel_amount: f32,
    /// Frames before `fall_gravity` applies (prediction only).
    pub frames_fall_gravity: u16,
    /// How many airborne frames the jump-hold acceleration may apply for.
    pub num_jump_accel_frames: u16,
    /// Fall height at which landing starts to hurt.
    pub damage_start: f32,
    /// Fall height at which landing is lethal.
    pub damage_end: f32,
}

/// One `TRtAnimationCollectionPart`: an animation and its authored selection
/// weight (`weight` @0; `virtualFromState`/`lookupValue` are rebuilt at load
/// time by the engine, so they are not read here).
#[derive(Clone, Debug)]
pub struct CollectionEntry {
    /// Animation (`TRAM`) instance name.
    pub animation: String,
    /// Authored weight among clips sharing (type, from-state, varient).
    pub weight: u16,
}

/// One `TRAC` animation collection, its entries in on-disk order, and the
/// parent collection it falls back to (`recursiveLookup`).
#[derive(Clone, Debug)]
pub struct AnimationCollection {
    /// Collection instance name.
    pub name: String,
    /// Parent collection searched when this one has no exact match.
    pub recursive: Option<String>,
    /// Entries in on-disk order.
    pub entries: Vec<CollectionEntry>,
}

/// A decoded character class.
#[derive(Clone, Debug, Default)]
pub struct CharacterClass {
    /// Class name (e.g. `konoko_generic`, `striker_easy`).
    pub name: Option<String>,
    /// Variant tag used for random-variant selection (e.g. `konoko`).
    pub variant: Option<String>,
    /// Body (`TRBS`) name — links to the character model.
    pub body: Option<String>,
    /// Body textures (`TRMA`) name.
    pub textures: Option<String>,
    /// Aiming-screen collection (`TRSC`) name.
    pub aiming_screens: Option<String>,
    /// Animation collection (`TRAC`) name.
    pub animation_collection: Option<String>,
    /// The full, exact list of animation names this class uses (following
    /// `recursiveLookup` inheritance).
    pub animations: Vec<String>,
    /// Gameplay scalars (present when the `body` anchor was found).
    pub stats: Option<CharacterStats>,
    /// Jump / fall tuning (`airConstants`, at struct offset 0).
    pub air: Option<AirConstants>,
}

/// Resolves a reference-form word to the referenced instance index.
fn ref_index(v: u32) -> Option<usize> {
    (v & 1 != 0).then_some((v >> 8) as usize)
}

/// Decodes a [`CharacterClass`] from an `ONCC` instance by scanning for the
/// template references it holds.
pub fn decode_character_class(dat: &Dat<'_>, inst: &Instance) -> Option<CharacterClass> {
    if inst.tag != TAG_ONCC {
        return None;
    }
    let d = dat.instance_data(inst)?;

    // Scan for the last reference to each tag of interest (the real field sits
    // near the struct's end; incidental matches, if any, come earlier). Record
    // the `body` ref's offset — the struct tail (scalars) is anchored on it.
    let (mut variant_i, mut body_i, mut tex_i, mut trac_i, mut screen_i) =
        (None, None, None, None, None);
    let mut body_off = None;
    let mut off = 0;
    while off + 4 <= d.len() {
        if let Some(idx) = at::u32(d, off).and_then(ref_index) {
            if let Some(t) = dat.instances.get(idx) {
                match t.tag {
                    TAG_ONCV => variant_i = Some(idx),
                    TAG_TRBS => {
                        body_i = Some(idx);
                        body_off = Some(off);
                    }
                    TAG_TRMA => tex_i = Some(idx),
                    TAG_TRAC => trac_i = Some(idx),
                    TAG_TRSC => screen_i = Some(idx),
                    _ => {}
                }
            }
        }
        off += 4;
    }

    let name_of = |i: Option<usize>| -> Option<String> {
        i.and_then(|idx| dat.instances.get(idx))
            .and_then(|x| x.name.clone())
    };

    let animations = trac_i
        .map(|i| collect_animations(dat, i))
        .unwrap_or_default();

    // Read the stable scalar tail, anchored on the body reference. The field
    // order after `body` is fixed (the drift is all in the earlier AI section),
    // and we sanity-check it: `animations` must sit at body+76.
    let stats = body_off
        .filter(|&b| trac_i.is_none() || at::u32(d, b + 76).and_then(ref_index) == trac_i)
        .and_then(|b| read_stats(d, b));

    // Air constants are the first struct field (offset 0) — stable regardless of
    // the AI-section drift that forces the body-anchored read above.
    let air = Some(AirConstants {
        fall_gravity: at::f32(d, 0)?,
        jump_gravity: at::f32(d, 4)?,
        jump_amount: at::f32(d, 8)?,
        max_velocity: at::f32(d, 12)?,
        jump_accel_amount: at::f32(d, 16)?,
        frames_fall_gravity: at::u16(d, 20)?,
        num_jump_accel_frames: at::u16(d, 22)?,
        damage_start: at::f32(d, 24)?,
        damage_end: at::f32(d, 28)?,
    });

    Some(CharacterClass {
        name: inst.name.clone(),
        variant: name_of(variant_i),
        body: name_of(body_i),
        textures: name_of(tex_i),
        aiming_screens: name_of(screen_i),
        animation_collection: name_of(trac_i),
        animations,
        stats,
        air,
    })
}

/// Reads the scalar tail of `ONtCharacterClass`, offsets relative to the `body`
/// reference at `b` (see the struct in `Oni_Character.h`).
fn read_stats(d: &[u8], b: usize) -> Option<CharacterStats> {
    // Need everything through knockdownResistant @ body+91.
    if b + 92 > d.len() {
        return None;
    }
    let mut damage_resistance = [0.0f32; 7];
    for (i, r) in damage_resistance.iter_mut().enumerate() {
        *r = at::f32(d, b + 44 + i * 4)?;
    }
    Some(CharacterStats {
        fight_mode_duration: at::u32(d, b + 16)?,
        idle_delay: at::u32(d, b + 20)?,
        idle_restart_delay: at::u32(d, b + 24)?,
        max_hit_points: at::u32(d, b + 28)?,
        feet_bits: at::u32(d, b + 32)?,
        scale_low: at::f32(d, b + 36)?,
        scale_high: at::f32(d, b + 40)?,
        damage_resistance,
        boss_shield: at::f32(d, b + 72)?,
        shot_time: at::u16(d, b + 84)?,
        death_delete_delay: at::u16(d, b + 86)?,
        left_handed: d[b + 88] != 0,
        super_hypo: d[b + 89] != 0,
        super_shield: d[b + 90] != 0,
        knockdown_resistant: d[b + 91] != 0,
    })
}

/// Collects all animation names from a `TRAC`, following `recursiveLookup`
/// inheritance (cycle-guarded, deduplicated, preserving first-seen order).
fn collect_animations(dat: &Dat<'_>, trac_idx: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    let mut visited = BTreeSet::new();
    let mut cur = Some(trac_idx);

    while let Some(idx) = cur {
        if !visited.insert(idx) {
            break; // cycle
        }
        let Some(inst) = dat.instances.get(idx) else {
            break;
        };
        let Some(d) = dat.instance_data(inst) else {
            break;
        };
        let num = at::u16(d, TRAC_NUM).unwrap_or(0) as usize;
        for e in 0..num {
            let ao = TRAC_ENTRY + e * TRAC_ENTRY_SIZE + TRAC_ENTRY_ANIM;
            if let Some(ai) = at::u32(d, ao).and_then(ref_index) {
                if let Some(name) = dat.instances.get(ai).and_then(|x| x.name.clone()) {
                    if seen.insert(name.clone()) {
                        out.push(name);
                    }
                }
            }
        }
        cur = at::u32(d, TRAC_RECURSIVE).and_then(ref_index);
    }
    out
}

/// Decodes one `TRAC` instance: its entries (animation + weight) and the name
/// of the collection it recursively falls back to.
pub fn decode_animation_collection(dat: &Dat<'_>, inst: &Instance) -> Option<AnimationCollection> {
    if inst.tag != TAG_TRAC {
        return None;
    }
    let d = dat.instance_data(inst)?;
    let num = at::u16(d, TRAC_NUM)? as usize;
    let entries = (0..num)
        .filter_map(|e| {
            let base = TRAC_ENTRY + e * TRAC_ENTRY_SIZE;
            let weight = at::u16(d, base)?;
            let ai = at::u32(d, base + TRAC_ENTRY_ANIM).and_then(ref_index)?;
            let animation = dat.instances.get(ai)?.name.clone()?;
            Some(CollectionEntry { animation, weight })
        })
        .collect();
    let recursive = at::u32(d, TRAC_RECURSIVE)
        .and_then(ref_index)
        .and_then(|i| dat.instances.get(i))
        .and_then(|x| x.name.clone());
    Some(AnimationCollection {
        name: inst.name.clone()?,
        recursive,
        entries,
    })
}

/// Iterates every `TRAC` animation collection in `dat`.
pub fn animation_collections<'a>(
    dat: &'a Dat<'a>,
) -> impl Iterator<Item = AnimationCollection> + 'a {
    dat.instances
        .iter()
        .filter(|i| i.tag == TAG_TRAC)
        .filter_map(move |i| decode_animation_collection(dat, i))
}

/// Iterates every `ONCC` character class in `dat`.
pub fn character_classes<'a>(dat: &'a Dat<'a>) -> impl Iterator<Item = CharacterClass> + 'a {
    dat.instances
        .iter()
        .filter(|i| i.tag == TAG_ONCC)
        .filter_map(move |i| decode_character_class(dat, i))
}
