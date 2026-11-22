//! Particle-class definitions → `common/particle_classes.json`.
//!
//! Every `PAR3` class (version 18 `P3tParticleDefinition`, `BFW_Particle3.h`)
//! decoded in full — flags, variables, the action list with its per-event
//! ranges, emitters and the attractor — so the engine can run Oni's particle
//! system (weapon projectiles first; `docs/PARTICLES.md`).
//!
//! ## Layout (little-endian, all enums 4 bytes)
//!
//! A 772-byte fixed part, then `num_variables` × 52-byte variables, then
//! `num_actions` × 424-byte actions, then `num_emitters` × 444-byte emitters.
//! Parse by those counts: one shipped class carries 4 bytes of slack.
//!
//! Values (`P3tValue`, 28 bytes: a type word and a 24-byte union) become:
//! a number (float constant), a string, `null` (an unset variable reference),
//! or an object `{"var"}`, `{"range"}`, `{"bell"}`, `{"shade"}`,
//! `{"shadeRange"}`, `{"shadeBell"}`, `{"enum"}`, `{"cycle"}`.

use std::collections::BTreeMap;

use oni_schema::files::{self, Named, ParticleValue as V};
#[cfg(test)]
use serde_json::json;

use crate::{cstr, le_f32, le_u16, le_u32};

const FIXED: usize = 772;
const VAR: usize = 52;
const ACTION: usize = 424;
const EMITTER: usize = 444;
const VALUE: usize = 28;

/// Class flag bits (`P3cParticleClassFlag_*`), bit index → name. Bits 1–2 and
/// 5–7 are the visibility and sprite-type fields, kept separately.
const CLASS_FLAGS: [(u32, &str); 17] = [
    (0, "decorative"),
    (3, "separateYScale"),
    (4, "needsTextureInstance"),
    (8, "geometry"),
    (9, "collideEnv"),
    (10, "collideChar"),
    (11, "scaleToVelocity"),
    (12, "hasVelocity"),
    (13, "hasOrientation"),
    (14, "hasOffsetPos"),
    (15, "hasDynamicMatrix"),
    (16, "hasPrevPosition"),
    (17, "hasDecalData"),
    (18, "hasTextureIndex"),
    (19, "hasTextureTime"),
    (20, "hasOwner"),
    (23, "hasAttractor"),
];
/// The remaining optional-field bits.
const CLASS_FLAGS_MORE: [(u32, &str); 3] = [
    (21, "hasContrailData"),
    (22, "hasLensFrames"),
    (24, "hasEnvironmentCache"),
];

/// `flags2` bits (`P3cParticleClassFlag2_*`).
const CLASS_FLAGS2: [(u32, &str); 21] = [
    (0, "useGlobalTint"),
    (1, "attractorCheckEnvironment"),
    (2, "attractorAvoidFriendly"),
    (3, "expireOnCutscene"),
    (4, "dieOnCutscene"),
    (5, "lowDetailDisable"),
    (6, "mediumDetailDisable"),
    (20, "sky"),
    (21, "decalFullBright"),
    (22, "decalCullBackfacing"),
    (23, "decal"),
    (24, "initiallyHidden"),
    (25, "invisible"),
    (26, "edgeFade"),
    (27, "vector"),
    (28, "lockToLinkedParticle"),
    (29, "contrailEmitter"),
    (30, "lensFlare"),
    (31, "edgeFadeOneSided"),
    (7, "lowDetailOnly"),
    (8, "mediumDetailOnly"),
];

/// Event lists (`P3tEventType`) in on-disk order.
pub const EVENTS: [&str; 16] = [
    "update",
    "pulse",
    "start",
    "stop",
    "bgStart",
    "bgStop",
    "collideEnv",
    "collideChar",
    "lifetime",
    "explode",
    "brokenLink",
    "create",
    "die",
    "newAttractor",
    "delayStart",
    "delayStop",
];

/// Action types (`P3tActionType`), type code → name.
const ACTIONS: [(u32, &str); 70] = [
    (0, "animateLinear"),
    (1, "animateAccel"),
    (2, "animateRandom"),
    (3, "animatePingpong"),
    (4, "animateLoop"),
    (5, "animateToValue"),
    (6, "colorInterpolate"),
    (8, "fadeOut"),
    (9, "enableAtTime"),
    (10, "disableAtTime"),
    (11, "die"),
    (12, "setLifetime"),
    (13, "emitActivate"),
    (14, "emitDeactivate"),
    (15, "emitParticles"),
    (16, "changeClass"),
    (17, "killLastEmitted"),
    (18, "explodeLastEmitted"),
    (20, "soundStart"),
    (21, "soundEnd"),
    (22, "soundVolume"),
    (23, "impulseSound"),
    (26, "damageHitChar"),
    (27, "damageBlast"),
    (28, "explode"),
    (29, "damageEnvironment"),
    (30, "glassCharge"),
    (31, "stop"),
    (33, "rotateX"),
    (34, "rotateY"),
    (35, "rotateZ"),
    (37, "findAttractor"),
    (38, "attractGravity"),
    (39, "attractHoming"),
    (40, "attractSpring"),
    (47, "moveLine"),
    (48, "moveGravity"),
    (49, "moveSpiral"),
    (50, "moveResistance"),
    (51, "moveDrift"),
    (52, "setVelocity"),
    (53, "spiralTangent"),
    (54, "killBeyondPoint"),
    (55, "collisionEffect"),
    (56, "stickToWall"),
    (57, "bounce"),
    (58, "createDecal"),
    (59, "chop"),
    (60, "impactEffect"),
    (62, "show"),
    (63, "hide"),
    (64, "setTextureTick"),
    (65, "randomTextureFrame"),
    (70, "setVariable"),
    (71, "recalculateAll"),
    (72, "enableAbove"),
    (73, "enableBelow"),
    (74, "enableNow"),
    (75, "disableNow"),
    (77, "superballTrigger"),
    (78, "stopIfBreakable"),
    (79, "avoidWalls"),
    (80, "randomSwirl"),
    (81, "floatAbovePlayer"),
    (82, "stopIfSlow"),
    (83, "superParticle"),
    (84, "stopLink"),
    (85, "checkLink"),
    (86, "breakLink"),
    (7, "unused7"),
];

const EMITTER_FLAGS: [&str; 11] = [
    "initiallyActive",
    "increaseEmitCount",
    "deactivateWhenEmitCountThreshold",
    "activateWhenEmitCountThreshold",
    "addParentVelocity",
    "sameDynamicMatrix",
    "orientToVelocity",
    "inheritTint",
    "onePerAttractor",
    "atLeastOne",
    "rotateAttractors",
];
const LINKS: [&str; 11] = [
    "none",
    "emitter",
    "lastEmission0",
    "lastEmission1",
    "lastEmission2",
    "lastEmission3",
    "lastEmission4",
    "lastEmission5",
    "lastEmission6",
    "lastEmission7",
    "ourLink",
];
const RATES: [&str; 5] = ["continuous", "random", "instant", "distance", "attractor"];
const POSITIONS: [&str; 8] = [
    "point",
    "line",
    "circle",
    "sphere",
    "offset",
    "cylinder",
    "bodySurface",
    "bodyBones",
];
const DIRECTIONS: [&str; 7] = [
    "straight",
    "random",
    "cone",
    "ring",
    "offset",
    "inaccurate",
    "attractor",
];
const SPEEDS: [&str; 2] = ["uniform", "stratified"];
const ORIENTS: [&str; 16] = [
    "parentPosX",
    "parentNegX",
    "parentPosY",
    "parentNegY",
    "parentPosZ",
    "parentNegZ",
    "worldPosX",
    "worldNegX",
    "worldPosY",
    "worldNegY",
    "worldPosZ",
    "worldNegZ",
    "velocity",
    "reverseVelocity",
    "towardsEmitter",
    "awayFromEmitter",
];
const ITERATORS: [&str; 9] = [
    "none",
    "link",
    "particleClass",
    "particleTag",
    "character",
    "hostileCharacter",
    "emittedTowards",
    "parent",
    "allCharacters",
];
const SELECTORS: [&str; 2] = ["distance", "conical"];

fn name<const N: usize>(table: &[&str; N], i: u32) -> Named {
    table
        .get(i as usize)
        .map_or(Named::Number(i), |s| Named::Name(s.to_string()))
}

fn flag_names(bits: u32, table: &[(u32, &'static str)]) -> Vec<String> {
    table
        .iter()
        .filter(|(b, _)| bits & (1 << b) != 0)
        .map(|(_, n)| n.to_string())
        .collect()
}

fn shade(c: u32) -> String {
    format!("#{c:08x}")
}

/// A `P3tValue` at `o` (see the module docs for the encoding); `None`: unset.
fn value(d: &[u8], o: usize) -> Option<V> {
    let p = o + 4;
    Some(match le_u32(d, o) {
        0 => {
            let n = cstr(d, p, 16);
            if n.is_empty() {
                return None;
            }
            V::Var { var: n }
        }
        1 => V::Number(le_u16(d, p) as f32),
        2 => V::Range {
            range: [le_u16(d, p) as f32, le_u16(d, p + 2) as f32],
        },
        3 => V::Number(le_f32(d, p)),
        4 => V::Range {
            range: [le_f32(d, p), le_f32(d, p + 4)],
        },
        5 => V::Bell {
            bell: [le_f32(d, p), le_f32(d, p + 4)],
        },
        6 => V::Text(cstr(d, p, 16)),
        7 => V::Shade {
            shade: shade(le_u32(d, p)),
        },
        8 => V::ShadeRange {
            shade_range: [shade(le_u32(d, p)), shade(le_u32(d, p + 4))],
        },
        9 => V::ShadeBell {
            shade_bell: [shade(le_u32(d, p)), shade(le_u32(d, p + 4))],
        },
        10 => V::Enum {
            value: le_u32(d, p),
        },
        11 => V::Cycle {
            cycle: [le_f32(d, p), le_f32(d, p + 4)],
        },
        t => V::Unknown { unknown: t },
    })
}

/// A value set to something other than 0.
fn nonzero(v: Option<V>) -> Option<V> {
    v.filter(|v| *v != V::Number(0.0))
}

/// A variable's data type (`P3tDataType`).
fn data_type(t: u32) -> &'static str {
    match t {
        2 => "float",
        4 => "string",
        8 => "shade",
        0x2000 => "ambientSound",
        0x4000 => "impulseSound",
        t if t & 0x1000 != 0 => "enum",
        _ => "unknown",
    }
}

/// Drops trailing unset slots from a list.
fn trim<T>(mut v: Vec<Option<T>>) -> Vec<Option<T>> {
    while v.last().is_some_and(Option::is_none) {
        v.pop();
    }
    v
}

/// One class definition; `None` when it is not a well-formed v18 one.
pub fn decode(d: &[u8]) -> Option<files::ParticleClass> {
    if d.len() < FIXED || le_u16(d, 2) != 18 {
        return None;
    }
    let (nv, na, ne) = (
        le_u16(d, 16) as usize,
        le_u16(d, 18) as usize,
        le_u16(d, 20) as usize,
    );
    if d.len() < FIXED + nv * VAR + na * ACTION + ne * EMITTER {
        return None;
    }
    let mut c = files::ParticleClass::default();
    let flags = le_u32(d, 4);
    c.flags = flag_names(flags, &CLASS_FLAGS);
    c.flags.extend(flag_names(flags, &CLASS_FLAGS_MORE));
    let sprite = (flags >> 5) & 7;
    c.sprite_type = (sprite != 0).then_some(sprite);
    c.flags2 = flag_names(le_u32(d, 8), &CLASS_FLAGS2);
    let mask = le_u32(d, 12);
    c.action_mask = (mask != 0).then_some(mask);
    c.lifetime = nonzero(value(d, 88));
    c.collision_radius = nonzero(value(d, 116));
    let nonzero_f32 = |off: usize| Some(le_f32(d, off)).filter(|&v| v != 0.0);
    c.dodge_radius = nonzero_f32(144);
    c.alert_radius = nonzero_f32(148);
    let flyby = cstr(d, 152, 16);
    c.flyby_sound = (!flyby.is_empty()).then_some(flyby);
    let inst = cstr(d, 280, 32);
    let nonzero_u16 = |off: usize| Some(le_u16(d, off)).filter(|&v| v != 0);
    c.appearance = files::Appearance {
        scale: value(d, 168),
        y_scale: value(d, 196),
        rotation: value(d, 224),
        alpha: value(d, 252),
        tint: value(d, 368),
        // Contrails: fading out edge-on (between these, when the class
        // fades at its edges) and the longest link drawn (0: any).
        edge_fade_min: nonzero(value(d, 396)),
        edge_fade_max: nonzero(value(d, 424)),
        max_contrail: nonzero(value(d, 452)),
        // Lens flares: how far behind something one can still be seen, and
        // the frames it fades in and out over as it is seen or hidden.
        lensflare_dist: nonzero(value(d, 480)),
        lens_in_frames: nonzero_u16(508),
        lens_out_frames: nonzero_u16(510),
        // Decals: how many may live at once, frames to fade out, how far
        // (in degrees) one wraps round a corner.
        max_decals: Some(le_u16(d, 512)),
        decal_fade_frames: Some(le_u16(d, 514)),
        decal_wrap: value(d, 516),
        instance: (!inst.is_empty()).then(|| crate::sanitize(&inst)),
    };
    let iterator = le_u32(d, 544);
    if iterator != 0 {
        let n = cstr(d, 556, 64);
        c.attractor = Some(files::Attractor {
            iterator: name(&ITERATORS, iterator),
            selector: name(&SELECTORS, le_u32(d, 548)),
            name: (!n.is_empty()).then_some(n),
            max_distance: value(d, 620),
            max_angle: value(d, 648),
            theta_min: value(d, 676),
            theta_max: value(d, 704),
            theta_weight: value(d, 732),
        });
    }
    c.variables = (0..nv)
        .map(|i| {
            let o = FIXED + i * VAR;
            files::ParticleVariable {
                name: cstr(d, o, 16),
                kind: data_type(le_u32(d, o + 16)).to_string(),
                init: value(d, o + 24),
            }
        })
        .collect();
    for (e, ev) in EVENTS.iter().enumerate() {
        let (s, t) = (le_u16(d, 24 + e * 4), le_u16(d, 26 + e * 4));
        if t > s {
            c.events.insert(ev.to_string(), [s, t]);
        }
    }
    let base = FIXED + nv * VAR;
    c.actions = (0..na)
        .map(|i| {
            let o = base + i * ACTION;
            let t = le_u32(d, o);
            files::ParticleAction {
                kind: ACTIONS
                    .iter()
                    .find(|x| x.0 == t)
                    .map_or(Named::Number(t), |x| Named::Name(x.1.to_string())),
                vars: trim(
                    (0..8)
                        .map(|k| Some(cstr(d, o + 8 + k * 24, 16)).filter(|n| !n.is_empty()))
                        .collect(),
                ),
                params: trim((0..8).map(|k| value(d, o + 200 + k * VALUE)).collect()),
            }
        })
        .collect();
    let base = base + na * ACTION;
    c.emitters = (0..ne)
        .map(|i| {
            let o = base + i * EMITTER;
            let vals = |from: usize, n: usize| {
                trim(
                    (from..from + n)
                        .map(|k| value(d, o + 108 + k * VALUE))
                        .collect(),
                )
            };
            let spec = |kind: Named, values| files::EmitterSpec { kind, values };
            let f = le_u32(d, o + 68);
            files::ParticleEmitter {
                class: cstr(d, o, 64),
                flags: EMITTER_FLAGS
                    .iter()
                    .enumerate()
                    .filter(|(b, _)| f & (1 << b) != 0)
                    .map(|(_, n)| n.to_string())
                    .collect(),
                limit: le_u16(d, o + 72),
                chance: le_u16(d, o + 74),
                burst: le_f32(d, o + 76),
                link: name(&LINKS, le_u32(d, o + 80)),
                rate: spec(name(&RATES, le_u32(d, o + 84)), vals(0, 2)),
                position: spec(name(&POSITIONS, le_u32(d, o + 88)), vals(2, 3)),
                direction: spec(name(&DIRECTIONS, le_u32(d, o + 92)), vals(5, 3)),
                speed: spec(name(&SPEEDS, le_u32(d, o + 96)), vals(8, 3)),
                orient_dir: name(&ORIENTS, le_u32(d, o + 100)),
                orient_up: name(&ORIENTS, le_u32(d, o + 104)),
            }
        })
        .collect();
    Some(c)
}

/// Writes every class in `classes` (raw definitions by name) to
/// `<common>/particle_classes.json`; returns how many were written.
pub fn write(
    classes: &BTreeMap<String, &[u8]>,
    common: &std::path::Path,
) -> std::io::Result<usize> {
    let out: BTreeMap<String, files::ParticleClass> = classes
        .iter()
        .filter_map(|(n, d)| Some((n.clone(), decode(d)?)))
        .collect();
    std::fs::create_dir_all(common)?;
    let count = out.len();
    std::fs::write(
        common.join("particle_classes.json"),
        serde_json::to_string(&files::ParticleClassesFile {
            count,
            classes: out,
        })?,
    )?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put_u16(d: &mut [u8], o: usize, v: u16) {
        d[o..o + 2].copy_from_slice(&v.to_le_bytes());
    }
    fn put_u32(d: &mut [u8], o: usize, v: u32) {
        d[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }

    /// A hand-built class: one float variable, a two-action update list and
    /// one emitter.
    #[test]
    fn decodes_a_minimal_class() {
        let mut d = vec![0u8; FIXED + VAR + 2 * ACTION + EMITTER];
        put_u16(&mut d, 2, 18);
        put_u32(&mut d, 4, (1 << 9) | (1 << 12));
        put_u16(&mut d, 16, 1);
        put_u16(&mut d, 18, 2);
        put_u16(&mut d, 20, 1);
        put_u16(&mut d, 26, 2); // update: [0, 2)
        put_u32(&mut d, 88, 3); // lifetime: float 30
        d[92..96].copy_from_slice(&30f32.to_le_bytes());
        d[FIXED..FIXED + 5].copy_from_slice(b"speed");
        put_u32(&mut d, FIXED + 16, 2);
        put_u32(&mut d, FIXED + 24, 4); // init: range 1..2
        d[FIXED + 28..FIXED + 32].copy_from_slice(&1f32.to_le_bytes());
        d[FIXED + 32..FIXED + 36].copy_from_slice(&2f32.to_le_bytes());
        let a0 = FIXED + VAR;
        put_u32(&mut d, a0, 47);
        put_u32(&mut d, a0 + ACTION, 48);
        put_u32(&mut d, a0 + ACTION + 200, 3);
        d[a0 + ACTION + 204..a0 + ACTION + 208].copy_from_slice(&0.5f32.to_le_bytes());
        let e = a0 + 2 * ACTION;
        d[e..e + 3].copy_from_slice(b"kid");
        put_u32(&mut d, e + 68, 0x7);
        put_u32(&mut d, e + 84, 2);

        let c = serde_json::to_value(decode(&d).unwrap()).unwrap();
        assert_eq!(c["flags"], json!(["collideEnv", "hasVelocity"]));
        assert_eq!(c["lifetime"], json!(30.0));
        assert_eq!(
            c["variables"][0],
            json!({ "name": "speed", "type": "float", "init": { "range": [1.0, 2.0] } })
        );
        assert_eq!(c["events"], json!({ "update": [0, 2] }));
        assert_eq!(c["actions"][0], json!({ "type": "moveLine" }));
        assert_eq!(
            c["actions"][1],
            json!({ "type": "moveGravity", "params": [0.5] })
        );
        assert_eq!(c["emitters"][0]["class"], json!("kid"));
        assert_eq!(c["emitters"][0]["rate"]["type"], json!("instant"));
        assert_eq!(
            c["emitters"][0]["flags"],
            json!([
                "initiallyActive",
                "increaseEmitCount",
                "deactivateWhenEmitCountThreshold"
            ])
        );
    }

    #[test]
    fn rejects_short_or_old_definitions() {
        assert!(decode(&[0u8; 100]).is_none());
        let d = vec![0u8; FIXED];
        assert!(decode(&d).is_none()); // version 0
    }
}
