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

use serde_json::{json, Map, Value};

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
const CLASS_FLAGS_MORE: [(u32, &str); 3] = [(21, "hasContrailData"), (22, "hasLensFrames"), (24, "hasEnvironmentCache")];

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
const POSITIONS: [&str; 8] = ["point", "line", "circle", "sphere", "offset", "cylinder", "bodySurface", "bodyBones"];
const DIRECTIONS: [&str; 7] = ["straight", "random", "cone", "ring", "offset", "inaccurate", "attractor"];
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

fn name<const N: usize>(table: &[&str; N], i: u32) -> Value {
    table.get(i as usize).map_or(json!(i), |s| json!(s))
}

fn flag_names(bits: u32, table: &[(u32, &'static str)]) -> Vec<&'static str> {
    table.iter().filter(|(b, _)| bits & (1 << b) != 0).map(|(_, n)| *n).collect()
}

fn shade(c: u32) -> Value {
    json!(format!("#{c:08x}"))
}

/// A `P3tValue` at `o` (see the module docs for the encoding).
fn value(d: &[u8], o: usize) -> Value {
    let p = o + 4;
    match le_u32(d, o) {
        0 => {
            let n = cstr(d, p, 16);
            if n.is_empty() {
                Value::Null
            } else {
                json!({ "var": n })
            }
        }
        1 => json!(le_u16(d, p) as f32),
        2 => json!({ "range": [le_u16(d, p) as f32, le_u16(d, p + 2) as f32] }),
        3 => json!(le_f32(d, p)),
        4 => json!({ "range": [le_f32(d, p), le_f32(d, p + 4)] }),
        5 => json!({ "bell": [le_f32(d, p), le_f32(d, p + 4)] }),
        6 => json!(cstr(d, p, 16)),
        7 => json!({ "shade": shade(le_u32(d, p)) }),
        8 => json!({ "shadeRange": [shade(le_u32(d, p)), shade(le_u32(d, p + 4))] }),
        9 => json!({ "shadeBell": [shade(le_u32(d, p)), shade(le_u32(d, p + 4))] }),
        10 => json!({ "enum": le_u32(d, p) }),
        11 => json!({ "cycle": [le_f32(d, p), le_f32(d, p + 4)] }),
        t => json!({ "unknown": t }),
    }
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

/// Drops trailing `null`s (unset slots) from a list.
fn trim(mut v: Vec<Value>) -> Vec<Value> {
    while v.last().is_some_and(Value::is_null) {
        v.pop();
    }
    v
}

/// One class definition as JSON; `None` when it is not a well-formed v18 one.
pub fn decode(d: &[u8]) -> Option<Value> {
    if d.len() < FIXED || le_u16(d, 2) != 18 {
        return None;
    }
    let (nv, na, ne) = (le_u16(d, 16) as usize, le_u16(d, 18) as usize, le_u16(d, 20) as usize);
    if d.len() < FIXED + nv * VAR + na * ACTION + ne * EMITTER {
        return None;
    }
    let mut c = Map::new();
    let flags = le_u32(d, 4);
    let mut names = flag_names(flags, &CLASS_FLAGS);
    names.extend(flag_names(flags, &CLASS_FLAGS_MORE));
    if !names.is_empty() {
        c.insert("flags".into(), json!(names));
    }
    let sprite = (flags >> 5) & 7;
    if sprite != 0 {
        c.insert("spriteType".into(), json!(sprite));
    }
    let names2 = flag_names(le_u32(d, 8), &CLASS_FLAGS2);
    if !names2.is_empty() {
        c.insert("flags2".into(), json!(names2));
    }
    let mask = le_u32(d, 12);
    if mask != 0 {
        c.insert("actionMask".into(), json!(mask));
    }
    for (key, off) in [("lifetime", 88), ("collisionRadius", 116)] {
        let v = value(d, off);
        if !v.is_null() && v != json!(0.0) {
            c.insert(key.into(), v);
        }
    }
    for (key, off) in [("dodgeRadius", 144), ("alertRadius", 148)] {
        let v = le_f32(d, off);
        if v != 0.0 {
            c.insert(key.into(), json!(v));
        }
    }
    let flyby = cstr(d, 152, 16);
    if !flyby.is_empty() {
        c.insert("flybySound".into(), json!(flyby));
    }
    let mut app = Map::new();
    for (key, off) in [("scale", 168), ("yScale", 196), ("rotation", 224), ("alpha", 252), ("tint", 368)] {
        let v = value(d, off);
        if !v.is_null() {
            app.insert(key.into(), v);
        }
    }
    // Lens flares: how far behind something one can still be seen, and the
    // frames it fades in and out over as it is seen or hidden.
    let dist = value(d, 480);
    if !dist.is_null() && dist != json!(0.0) {
        app.insert("lensflareDist".into(), dist);
    }
    for (key, off) in [("lensInFrames", 508), ("lensOutFrames", 510)] {
        let v = le_u16(d, off);
        if v != 0 {
            app.insert(key.into(), json!(v));
        }
    }
    // Decals: how many may live at once, frames to fade out, how far (in
    // degrees) one wraps round a corner.
    for (key, off) in [("maxDecals", 512), ("decalFadeFrames", 514)] {
        app.insert(key.into(), json!(le_u16(d, off)));
    }
    let wrap = value(d, 516);
    if !wrap.is_null() {
        app.insert("decalWrap".into(), wrap);
    }
    let inst = cstr(d, 280, 32);
    if !inst.is_empty() {
        app.insert("instance".into(), json!(crate::sanitize(&inst)));
    }
    c.insert("appearance".into(), Value::Object(app));
    let iterator = le_u32(d, 544);
    if iterator != 0 {
        let mut a = Map::new();
        a.insert("iterator".into(), name(&ITERATORS, iterator));
        a.insert("selector".into(), name(&SELECTORS, le_u32(d, 548)));
        let n = cstr(d, 556, 64);
        if !n.is_empty() {
            a.insert("name".into(), json!(n));
        }
        for (key, off) in [
            ("maxDistance", 620),
            ("maxAngle", 648),
            ("thetaMin", 676),
            ("thetaMax", 704),
            ("thetaWeight", 732),
        ] {
            let v = value(d, off);
            if !v.is_null() {
                a.insert(key.into(), v);
            }
        }
        c.insert("attractor".into(), Value::Object(a));
    }
    let vars: Vec<Value> = (0..nv)
        .map(|i| {
            let o = FIXED + i * VAR;
            json!({ "name": cstr(d, o, 16), "type": data_type(le_u32(d, o + 16)), "init": value(d, o + 24) })
        })
        .collect();
    if !vars.is_empty() {
        c.insert("variables".into(), json!(vars));
    }
    let mut events = Map::new();
    for (e, ev) in EVENTS.iter().enumerate() {
        let (s, t) = (le_u16(d, 24 + e * 4), le_u16(d, 26 + e * 4));
        if t > s {
            events.insert((*ev).into(), json!([s, t]));
        }
    }
    if !events.is_empty() {
        c.insert("events".into(), Value::Object(events));
    }
    let base = FIXED + nv * VAR;
    let actions: Vec<Value> = (0..na)
        .map(|i| {
            let o = base + i * ACTION;
            let t = le_u32(d, o);
            let mut a = Map::new();
            a.insert(
                "type".into(),
                ACTIONS.iter().find(|x| x.0 == t).map_or(json!(t), |x| json!(x.1)),
            );
            let vars = trim(
                (0..8)
                    .map(|k| {
                        let n = cstr(d, o + 8 + k * 24, 16);
                        if n.is_empty() {
                            Value::Null
                        } else {
                            json!(n)
                        }
                    })
                    .collect(),
            );
            if !vars.is_empty() {
                a.insert("vars".into(), json!(vars));
            }
            let params = trim((0..8).map(|k| value(d, o + 200 + k * VALUE)).collect());
            if !params.is_empty() {
                a.insert("params".into(), json!(params));
            }
            Value::Object(a)
        })
        .collect();
    if !actions.is_empty() {
        c.insert("actions".into(), json!(actions));
    }
    let base = base + na * ACTION;
    let emitters: Vec<Value> = (0..ne)
        .map(|i| {
            let o = base + i * EMITTER;
            let vals = |from: usize, n: usize| trim((from..from + n).map(|k| value(d, o + 108 + k * VALUE)).collect());
            let mut e = Map::new();
            e.insert("class".into(), json!(cstr(d, o, 64)));
            let f = le_u32(d, o + 68);
            let fl: Vec<&str> = EMITTER_FLAGS.iter().enumerate().filter(|(b, _)| f & (1 << b) != 0).map(|(_, n)| *n).collect();
            if !fl.is_empty() {
                e.insert("flags".into(), json!(fl));
            }
            e.insert("limit".into(), json!(le_u16(d, o + 72)));
            e.insert("chance".into(), json!(le_u16(d, o + 74)));
            e.insert("burst".into(), json!(le_f32(d, o + 76)));
            e.insert("link".into(), name(&LINKS, le_u32(d, o + 80)));
            e.insert("rate".into(), json!({ "type": name(&RATES, le_u32(d, o + 84)), "values": vals(0, 2) }));
            e.insert("position".into(), json!({ "type": name(&POSITIONS, le_u32(d, o + 88)), "values": vals(2, 3) }));
            e.insert("direction".into(), json!({ "type": name(&DIRECTIONS, le_u32(d, o + 92)), "values": vals(5, 3) }));
            e.insert("speed".into(), json!({ "type": name(&SPEEDS, le_u32(d, o + 96)), "values": vals(8, 3) }));
            e.insert("orientDir".into(), name(&ORIENTS, le_u32(d, o + 100)));
            e.insert("orientUp".into(), name(&ORIENTS, le_u32(d, o + 104)));
            Value::Object(e)
        })
        .collect();
    if !emitters.is_empty() {
        c.insert("emitters".into(), json!(emitters));
    }
    Some(Value::Object(c))
}

/// Writes every class in `classes` (raw definitions by name) to
/// `<common>/particle_classes.json`; returns how many were written.
pub fn write(classes: &BTreeMap<String, &[u8]>, common: &std::path::Path) -> std::io::Result<usize> {
    let mut out = Map::new();
    for (n, d) in classes {
        if let Some(v) = decode(d) {
            out.insert(n.clone(), v);
        }
    }
    std::fs::create_dir_all(common)?;
    let count = out.len();
    std::fs::write(
        common.join("particle_classes.json"),
        serde_json::to_string(&json!({ "count": count, "classes": out }))?,
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

        let c = decode(&d).unwrap();
        assert_eq!(c["flags"], json!(["collideEnv", "hasVelocity"]));
        assert_eq!(c["lifetime"], json!(30.0));
        assert_eq!(c["variables"][0], json!({ "name": "speed", "type": "float", "init": { "range": [1.0, 2.0] } }));
        assert_eq!(c["events"], json!({ "update": [0, 2] }));
        assert_eq!(c["actions"][0], json!({ "type": "moveLine" }));
        assert_eq!(c["actions"][1], json!({ "type": "moveGravity", "params": [0.5] }));
        assert_eq!(c["emitters"][0]["class"], json!("kid"));
        assert_eq!(c["emitters"][0]["rate"]["type"], json!("instant"));
        assert_eq!(
            c["emitters"][0]["flags"],
            json!(["initiallyActive", "increaseEmitCount", "deactivateWhenEmitCountThreshold"])
        );
    }

    #[test]
    fn rejects_short_or_old_definitions() {
        assert!(decode(&[0u8; 100]).is_none());
        let d = vec![0u8; FIXED];
        assert!(decode(&d).is_none()); // version 0
    }
}
