//! Rigid skeletal characters: load a character `model.glb` keeping its bone
//! hierarchy (parts stay in bone-local space), load a `TRAM` animation clip,
//! and pose the skeleton at a given time.
//!
//! Oni characters are *rigid* — each bone carries a whole mesh part, and
//! animation rotates bones, so posing is `world[bone] = world[parent] · T · R`
//! with `R` taken from the clip (or the model's rest pose when a bone has no
//! track). Clips bind to bones by name, so any clip retargets to any character
//! with the same bone names.

use oni_schema as sc;
use std::path::{Path, PathBuf};

use glam::{Mat4, Quat, Vec3};

use crate::gltf::{self, Vertex};

/// One bone: its rest transform and parent.
pub struct Bone {
    pub name: String,
    pub translation: Vec3,
    pub rotation: Quat,
    pub parent: Option<usize>,
}

/// One drawable part, in **bone-local** space (posed via its bone's matrix).
pub struct Part {
    pub bone: usize,
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub texture: Option<PathBuf>,
    pub blend: bool,
    pub additive: bool,
    /// The body part's stored bounding sphere (centre in the bone's space,
    /// radius) — what Oni tests to decide whether bodies and blows connect.
    pub bound_sphere: Option<(Vec3, f32)>,
    /// Its geometry's box in the bone's space (min, max) — what shots test.
    pub bound_box: (Vec3, Vec3),
}

/// A loaded character: skeleton + drawable parts.
pub struct Character {
    pub bones: Vec<Bone>,
    pub parts: Vec<Part>,
}

/// One bone's rotation track (bound by name).
#[derive(Clone)]
pub struct Channel {
    pub bone_name: String,
    pub times: Vec<f32>,
    pub rotations: Vec<Quat>,
}

/// One attack's hit window + effects (`TRtAttack`, from the clip's `extras`,
/// decoded by `oni-convert`). The attack connects on frames
/// `[first_damage_frame, last_damage_frame]`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Attack {
    pub damage: u16,
    pub knockback: f32,
    pub first_damage_frame: u16,
    pub last_damage_frame: u16,
    /// The `TRtAnimType` the victim plays when hit (`damageAnimation`): the
    /// stagger / knockdown / stun reaction clip. 0 = no forced reaction.
    pub damage_animation: u16,
    pub hit_stun: u16,
    pub block_stun: u16,
    pub stagger: u16,
    /// Attack priority (`precedence`): higher wins when two attacks trade.
    pub precedence: u16,
    /// Attacker body-part bits that are "live weapons" this move (`damageBits`).
    pub damage_bits: u32,
    /// `ONcAttackFlag` bits (Unblockable/AttackHigh/AttackLow/SpecialMove).
    pub flags: u32,
}

/// An animation clip.
#[derive(Clone)]
pub struct Clip {
    pub duration: f32,
    pub channels: Vec<Channel>,
    /// Per-frame character-local `[x, z]` root-motion displacement (from the
    /// animation's `extras.rootMotion`). Empty if the clip has none.
    pub root_motion: Vec<[f32; 2]>,
    /// Per-frame pelvis height above the ground point (`extras.rootHeight`,
    /// Oni's `heightThisFrame`). The whole character is lifted by this each
    /// frame — it carries the body's vertical bob. Empty if the clip has none.
    pub heights: Vec<f32>,
    /// Action type (`TRtAnimType`): Run, Stand, Idle, Run_Start, … Drives the
    /// locomotion state machine (see [`crate::anim_state`]).
    pub anim_type: u16,
    /// Body state this clip starts from (`TRtAnimState`).
    pub from_state: u16,
    /// Body state this clip ends in; becomes the next lookup's `from_state`.
    pub to_state: u16,
    /// Variant bits required to select this clip.
    pub varient: u16,
    /// Net yaw (radians) this clip imparts to facing (`extras.finalRotation`,
    /// Oni's `finalRotation`) — e.g. a turn-in-place clip's total turn.
    pub final_rotation: f32,
    /// Which way the clip moves the character (`extras.moveDirection`,
    /// `TRtDirection`: 1 Forwards, 2 Back, 3 Left, 4 Right). 0 = none/unset.
    pub move_direction: u16,
    /// Frames to blend out of this clip when it ends (`endInterpolation`).
    pub end_interpolation: u16,
    /// Max frames another clip may blend into this one (`maxInterpolation`).
    pub max_interpolation: u16,
    /// Attack hit windows + effects (empty for non-attack clips).
    pub attacks: Vec<Attack>,
    /// Animation flag bits (`ONcAnimFlag_*`, test with `1 << flag`).
    pub flags: u32,
    /// Alternate entry states (Oni shortcuts): selectable from `state` too.
    pub shortcuts: Vec<Shortcut>,
    /// Names of up to two direct follow-up clips (no pause / end blend).
    pub direct: Vec<String>,
    /// The frame its action happens on (a console used, a weapon swapped).
    pub action_frame: Option<u16>,
    /// Uninterruptible frame range; `atomic_start > atomic_end` = never.
    pub atomic_start: u16,
    pub atomic_end: u16,
    /// Variant bits on exit (`varientEnd`).
    pub varient_end: u16,
    /// Unavailable on levels before this one (`firstLevel`).
    pub first_level: u16,
    /// Frames of unblockable / blockable pause after this clip.
    pub hard_pause: u16,
    pub soft_pause: u16,
    /// Frames `[invulnerable_start, +invulnerable_length)` are immune to hits.
    pub invulnerable_start: u8,
    pub invulnerable_length: u8,
    /// For a throw: where the victim goes and what it plays (`extras.throw`).
    pub throw: Option<Throw>,
    /// `(frame, damage)`: damage the clip deals whoever plays it
    /// (`extras.selfDamage`; how a throw hurts its victim).
    pub self_damage: Vec<(u16, u16)>,
    /// Frames a foot comes down (`extras.footsteps`).
    pub footsteps: Vec<u16>,
    /// Where the clip's strikes reach (`extras.reach`), for attacks.
    pub reach: Option<Reach>,
    /// Body parts the clip moves (bit = part), and — played as an overlay —
    /// the ones it replaces (the rest of `used_parts` it layers on).
    pub used_parts: u32,
    pub replace_parts: u32,
    /// Which aiming screen serves it (`extras.aimingType`; 0 = none).
    pub aiming_type: u16,
    /// Per tick: root offset from the start `[x, forward]`, the body's lowest
    /// point and its height (`extras.bodyTrack`).
    pub body: Vec<[f32; 4]>,
}

/// Where an attack clip's striking parts reach (see `docs/MELEE_AI.md`,
/// *Hit prediction*). Character-local: x left, forward ahead; angles 0 =
/// ahead, growing toward the left.
#[derive(Clone, Debug, PartialEq)]
pub struct Reach {
    pub max_distance: f32,
    pub min_height: f32,
    pub max_height: f32,
    pub spokes: [f32; 36],
    pub first: StrikeBound,
    pub farthest: StrikeBound,
    /// `oni_schema::DIRECTIONS` index.
    pub direction: u16,
    pub strikes: Vec<Strike>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StrikeBound {
    pub tick: u16,
    pub offset: [f32; 2],
    pub height: f32,
    pub distance: f32,
    pub min_height: f32,
    pub max_height: f32,
    pub angle: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Strike {
    pub tick: u16,
    pub angle: f32,
    pub distance: f32,
    pub min_height: f32,
    pub max_height: f32,
}

/// How a throw clip places and animates its victim.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Throw {
    /// The victim's position in the thrower's local frame.
    pub target_offset: Vec3,
    /// The victim's facing relative to the thrower's (radians).
    pub target_facing: f32,
    /// How far away the victim may be.
    pub range: f32,
    /// The victim's animation type.
    pub target_anim_type: u16,
}

/// A clip's alternate entry state (`TRtShortcut`, from `extras.shortcuts`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shortcut {
    pub state: u16,
    /// Blend-in length when entered through this shortcut.
    pub length: u16,
    /// Flag bits; bit 0 = `Nuke` (may interrupt an atomic clip).
    pub flags: u32,
}

impl Clip {
    /// Whether flag bit `flag` (an `ONcAnimFlag_*` index) is set.
    /// How many frames (ticks) it plays.
    pub fn frames(&self) -> u16 {
        let by_track = self.root_motion.len().max(self.heights.len()) as f32;
        let by_keys = (self.duration * FPS).round() + 1.0;
        by_track.max(by_keys).max(1.0) as u16
    }

    pub fn has_flag(&self, flag: u32) -> bool {
        self.flags & (1 << flag) != 0
    }

    /// Whether `frame` falls inside the atomic range (`TRrAnimation_IsAtomic`).
    pub fn is_atomic(&self, frame: u16) -> bool {
        self.atomic_start <= frame && frame <= self.atomic_end
    }

    /// The shortcut that lets this clip be entered from `from_state`: an exact
    /// match first, then an `Anything` shortcut. None when `from_state` is the
    /// clip's own `fromState` (that is a normal entry, not a shortcut).
    pub fn shortcut_from(&self, from_state: u16) -> Option<Shortcut> {
        if from_state == self.from_state {
            return None;
        }
        self.shortcuts
            .iter()
            .find(|s| s.state == from_state)
            .or_else(|| self.shortcuts.iter().find(|s| s.state == 1))
            .copied()
    }
}

/// Animation playback rate (Oni animations run at 60 fps).
pub const FPS: f32 = 60.0;

/// Fallback pelvis height when a clip carries no height track (Oni's
/// `TRrAnimation_GetHeight` returns 6.0 when `heights` is null).
pub const DEFAULT_HEIGHT: f32 = 6.0;

impl Clip {
    /// Pelvis height at `time`, linearly interpolated between frames (Oni's
    /// `TRrAnimation_GetHeight`). Returns [`DEFAULT_HEIGHT`] when the clip has
    /// no height track.
    pub fn height_at(&self, time: f32) -> f32 {
        if self.heights.is_empty() {
            return DEFAULT_HEIGHT;
        }
        let last = self.heights.len() - 1;
        let f = (time * FPS).clamp(0.0, last as f32);
        let i = f.floor() as usize;
        if i >= last {
            return self.heights[last];
        }
        let frac = f - i as f32;
        self.heights[i] * (1.0 - frac) + self.heights[i + 1] * frac
    }
}

impl Character {
    /// The local rotation `clip` applies to each bone at `time` (rest rotation
    /// when `clip` is `None` or a bone has no track). This is the per-bone
    /// orientation posing uses — captured as the source pose for stitching.
    pub fn local_rotations(&self, clip: Option<&Clip>, time: f32) -> Vec<Quat> {
        self.bones
            .iter()
            .map(|b| {
                clip.and_then(|c| c.channels.iter().find(|ch| ch.bone_name == b.name))
                    .map(|ch| sample(ch, time))
                    .unwrap_or(b.rotation)
            })
            .collect()
    }

    /// World matrix per bone, posed by `clip` at `time` (rest pose when `clip`
    /// is `None` or a bone has no track).
    pub fn pose(&self, clip: Option<&Clip>, time: f32) -> Vec<Mat4> {
        self.pose_blended(clip, time, None)
    }

    /// Like [`pose_blended`](Self::pose_blended), with `layers` on top of
    /// the pose (overlays, aim): each replaces its `replace` parts' rotation
    /// and multiplies its rotation onto its other `used` parts, by `weight`.
    pub fn pose_layered(&self, clip: Option<&Clip>, time: f32, stitch: Option<(&[Quat], f32)>, layers: &[Layer]) -> Vec<Mat4> {
        let n = self.bones.len();
        let mut world = vec![Mat4::IDENTITY; n];
        for i in 0..n {
            let b = &self.bones[i];
            let mut rot = clip
                .and_then(|c| c.channels.iter().find(|ch| ch.bone_name == b.name))
                .map(|ch| sample(ch, time))
                .unwrap_or(b.rotation);
            if let Some((saved, t)) = stitch {
                if let Some(&s) = saved.get(i) {
                    rot = nlerp(rot, s, t);
                }
            }
            for l in layers {
                let bit = 1u32 << i;
                let Some(&q) = l.rotations.get(i) else { continue };
                if l.replace & bit != 0 {
                    rot = nlerp(rot, q, l.weight);
                } else if l.used & bit != 0 {
                    rot = nlerp(Quat::IDENTITY, q, l.weight) * rot;
                }
            }
            let local = Mat4::from_rotation_translation(rot, b.translation);
            world[i] = match b.parent {
                Some(p) => world[p] * local,
                None => local,
            };
        }
        world
    }

    /// Like [`pose`](Self::pose), but when `stitch` is `Some((saved, t))` each
    /// bone's rotation is blended toward the `saved` pose by weight `t`
    /// (`t = 1` → fully `saved`, `t = 0` → fully `clip`) — Oni's animation
    /// stitching (`ONrCharacter_BuildQuaternions`, nlerp of new vs saved).
    pub fn pose_blended(
        &self,
        clip: Option<&Clip>,
        time: f32,
        stitch: Option<(&[Quat], f32)>,
    ) -> Vec<Mat4> {
        let n = self.bones.len();
        let mut world = vec![Mat4::IDENTITY; n];
        for i in 0..n {
            let b = &self.bones[i];
            let mut rot = clip
                .and_then(|c| c.channels.iter().find(|ch| ch.bone_name == b.name))
                .map(|ch| sample(ch, time))
                .unwrap_or(b.rotation);
            if let Some((saved, t)) = stitch {
                if let Some(&s) = saved.get(i) {
                    rot = nlerp(rot, s, t);
                }
            }
            let local = Mat4::from_rotation_translation(rot, b.translation);
            world[i] = match b.parent {
                Some(p) => world[p] * local,
                None => local,
            };
        }
        world
    }
}

/// A pose layer: an overlay clip's or an aiming screen's rotations per bone.
pub struct Layer {
    pub rotations: Vec<Quat>,
    pub used: u32,
    pub replace: u32,
    pub weight: f32,
}

/// Normalized lerp from `a` (t=0) to `b` (t=1), flipping `b` into `a`'s
/// hemisphere first so the short arc is taken (matches `MUrQuat_Lerp`).
fn nlerp(a: Quat, b: Quat, t: f32) -> Quat {
    let b = if a.dot(b) < 0.0 { -b } else { b };
    a.lerp(b, t)
}

/// Samples a rotation channel at `time` (linear/slerp between keys).
fn sample(ch: &Channel, time: f32) -> Quat {
    if ch.times.is_empty() {
        return Quat::IDENTITY;
    }
    if time <= ch.times[0] {
        return ch.rotations[0];
    }
    let last = ch.times.len() - 1;
    if time >= ch.times[last] {
        return ch.rotations[last];
    }
    let i = ch.times.partition_point(|&t| t <= time).max(1) - 1;
    let (t0, t1) = (ch.times[i], ch.times[i + 1]);
    let f = if t1 > t0 {
        (time - t0) / (t1 - t0)
    } else {
        0.0
    };
    ch.rotations[i].slerp(ch.rotations[i + 1], f)
}

fn f(a: &[serde_json::Value], i: usize) -> f32 {
    a.get(i).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32
}

/// Loads a character `model.glb` (bones + bone-local parts).
pub fn load_character(path: &Path) -> std::io::Result<Character> {
    let bytes = std::fs::read(path)?;
    let dir = path.parent().unwrap_or(Path::new("."));
    let (json, bin) = gltf::glb_json_bin(&bytes);
    let nodes = json["nodes"].as_array().cloned().unwrap_or_default();

    // Parent links from `children`.
    let mut parent = vec![None; nodes.len()];
    for (i, n) in nodes.iter().enumerate() {
        if let Some(children) = n["children"].as_array() {
            for c in children {
                parent[c.as_u64().unwrap() as usize] = Some(i);
            }
        }
    }

    let mut bones = Vec::with_capacity(nodes.len());
    let mut parts = Vec::new();
    for (i, n) in nodes.iter().enumerate() {
        let t = n["translation"]
            .as_array()
            .map(|a| Vec3::new(f(a, 0), f(a, 1), f(a, 2)))
            .unwrap_or(Vec3::ZERO);
        let r = n["rotation"]
            .as_array()
            .map(|a| Quat::from_xyzw(f(a, 0), f(a, 1), f(a, 2), f(a, 3)))
            .unwrap_or(Quat::IDENTITY);
        bones.push(Bone {
            name: n["name"].as_str().unwrap_or("").to_string(),
            translation: t,
            rotation: r,
            parent: parent[i],
        });

        let bound_sphere = n["extras"]["boundSphere"].as_array().and_then(|a| {
            let r = a.get(3)?.as_f64()? as f32;
            Some((Vec3::new(f(a, 0), f(a, 1), f(a, 2)), r))
        });
        if let Some(mesh) = n["mesh"].as_u64() {
            for prim in json["meshes"][mesh as usize]["primitives"]
                .as_array()
                .unwrap()
            {
                let at = &prim["attributes"];
                let Some(pos_acc) = at["POSITION"].as_u64() else {
                    continue;
                };
                let pos = gltf::read_floats(&json, bin, pos_acc as usize);
                let nrm = at["NORMAL"]
                    .as_u64()
                    .map(|a| gltf::read_floats(&json, bin, a as usize));
                let uv = at["TEXCOORD_0"]
                    .as_u64()
                    .map(|a| gltf::read_floats(&json, bin, a as usize));
                let idx =
                    gltf::read_indices(&json, bin, prim["indices"].as_u64().unwrap() as usize);
                let (texture, blend, additive) = prim["material"]
                    .as_u64()
                    .map(|m| material(&json, dir, m as usize))
                    .unwrap_or((None, false, false));

                let vcount = pos.len() / 3;
                let mut verts = Vec::with_capacity(vcount);
                for v in 0..vcount {
                    let normal = nrm
                        .as_ref()
                        .map(|a| [a[v * 3], a[v * 3 + 1], a[v * 3 + 2]])
                        .unwrap_or([0.0, 1.0, 0.0]);
                    let uvv = uv
                        .as_ref()
                        .map(|a| [a[v * 2], a[v * 2 + 1]])
                        .unwrap_or([0.0, 0.0]);
                    verts.push(Vertex {
                        pos: [pos[v * 3], pos[v * 3 + 1], pos[v * 3 + 2]],
                        normal,
                        uv: uvv,
                        color: [1.0, 1.0, 1.0],
                    });
                }
                let bound_box = verts.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(lo, hi), v| {
                    let p = Vec3::from(v.pos);
                    (lo.min(p), hi.max(p))
                });
                parts.push(Part {
                    bone: i,
                    bound_box,
                    vertices: verts,
                    indices: idx,
                    texture,
                    blend,
                    additive,
                    bound_sphere,
                });
            }
        }
    }
    Ok(Character { bones, parts })
}

/// Loads an animation `.glb` clip (rotation tracks, bound by bone name).
pub fn load_clip(path: &Path) -> std::io::Result<Clip> {
    let bytes = std::fs::read(path)?;
    let (json, bin) = gltf::glb_json_bin(&bytes);
    let nodes = json["nodes"].as_array().cloned().unwrap_or_default();
    let anim = &json["animations"][0];
    let samplers = anim["samplers"].as_array().cloned().unwrap_or_default();

    let mut channels = Vec::new();
    let mut duration = 0.0f32;
    for ch in anim["channels"].as_array().cloned().unwrap_or_default() {
        if ch["target"]["path"].as_str() != Some("rotation") {
            continue;
        }
        let node = ch["target"]["node"].as_u64().unwrap() as usize;
        let bone_name = nodes[node]["name"].as_str().unwrap_or("").to_string();
        let s = &samplers[ch["sampler"].as_u64().unwrap() as usize];
        let times = gltf::read_floats(&json, bin, s["input"].as_u64().unwrap() as usize);
        let rot = gltf::read_floats(&json, bin, s["output"].as_u64().unwrap() as usize);
        let rotations: Vec<Quat> = rot
            .chunks_exact(4)
            .map(|q| Quat::from_xyzw(q[0], q[1], q[2], q[3]))
            .collect();
        if let Some(&t) = times.last() {
            duration = duration.max(t);
        }
        channels.push(Channel {
            bone_name,
            times,
            rotations,
        });
    }
    // Root motion from the animation's extras.
    let root_motion: Vec<[f32; 2]> = anim["extras"]["rootMotion"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|p| {
                    let x = p.get(0).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                    let z = p.get(1).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                    [x, z]
                })
                .collect()
        })
        .unwrap_or_default();

    // Per-frame pelvis height (Oni's `heights` track).
    let heights: Vec<f32> = anim["extras"]["rootHeight"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|v| v.as_f64().unwrap_or(0.0) as f32)
                .collect()
        })
        .unwrap_or_default();

    let ex = &anim["extras"];
    let u16_field = |k: &str| ex[k].as_u64().unwrap_or(0) as u16;
    // Enumerations are names (`docs/ASSET_FORMAT.md`, `oni-schema`).
    let named = |v: &serde_json::Value, table: &[&str]| {
        v.as_str().and_then(|n| sc::value(table, n)).unwrap_or(0)
    };
    let named_bits = |v: &serde_json::Value, table: &[&str]| {
        v.as_array()
            .map(|a| sc::bits(table, a.iter().filter_map(|n| n.as_str())))
            .unwrap_or(0)
    };

    // Attack hit windows.
    let attacks: Vec<Attack> = ex["attacks"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|a| {
                    let u16k = |k: &str| a[k].as_u64().unwrap_or(0) as u16;
                    let window = |k: &str| a["damageWindow"][k].as_u64().unwrap_or(0) as u16;
                    Attack {
                        damage: u16k("damage"),
                        knockback: a["knockback"].as_f64().unwrap_or(0.0) as f32,
                        first_damage_frame: window("start"),
                        last_damage_frame: window("end"),
                        damage_animation: named(&a["reaction"], &sc::ANIM_TYPES),
                        hit_stun: u16k("hitStun"),
                        block_stun: u16k("blockStun"),
                        stagger: u16k("stagger"),
                        precedence: u16k("precedence"),
                        damage_bits: named_bits(&a["strikingParts"], &sc::BODY_PARTS),
                        flags: named_bits(&a["flags"], &sc::ATTACK_FLAGS),
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    // Absent: never atomic (start > end). Present without an end: to the end.
    let (atomic_start, atomic_end) = match ex["atomic"].as_object() {
        Some(r) => (
            r.get("start").and_then(|v| v.as_u64()).unwrap_or(0) as u16,
            r.get("end").and_then(|v| v.as_u64()).map_or(u16::MAX, |v| v as u16),
        ),
        None => (1, 0),
    };

    Ok(Clip {
        duration: duration.max(0.001),
        channels,
        root_motion,
        heights,
        anim_type: named(&ex["animType"], &sc::ANIM_TYPES),
        from_state: named(&ex["fromState"], &sc::ANIM_STATES),
        to_state: named(&ex["toState"], &sc::ANIM_STATES),
        varient: named_bits(&ex["variant"], &sc::VARIANTS) as u16,
        used_parts: ex["usedParts"].as_u64().unwrap_or(0) as u32,
        replace_parts: ex["replaceParts"].as_u64().unwrap_or(0) as u32,
        aiming_type: named(&ex["aimingType"], &sc::ANIM_TYPES),
        final_rotation: ex["finalRotation"].as_f64().unwrap_or(0.0) as f32,
        move_direction: named(&ex["moveDirection"], &sc::DIRECTIONS),
        end_interpolation: u16_field("endBlend"),
        max_interpolation: ex["maxBlendIn"].as_u64().map_or(u16::MAX, |v| v as u16),
        attacks,
        flags: named_bits(&ex["flags"], &sc::ANIM_FLAGS),
        shortcuts: ex["shortcuts"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .map(|s| Shortcut {
                        state: named(&s["fromState"], &sc::ANIM_STATES),
                        length: s["blendLength"].as_u64().unwrap_or(0) as u16,
                        flags: u32::from(s["interruptsAtomic"].as_bool().unwrap_or(false)),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        direct: ex["direct"]
            .as_array()
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default(),
        action_frame: ex["actionFrame"].as_i64().filter(|&v| v >= 0).map(|v| v as u16),
        atomic_start,
        atomic_end,
        varient_end: named_bits(&ex["variantEnd"], &sc::VARIANTS) as u16,
        first_level: u16_field("firstLevel"),
        hard_pause: u16_field("hardPause"),
        soft_pause: u16_field("softPause"),
        invulnerable_start: ex["invulnerable"]["start"].as_u64().unwrap_or(0) as u8,
        invulnerable_length: ex["invulnerable"]["length"].as_u64().unwrap_or(0) as u8,
        throw: ex["throw"].is_object().then(|| {
            let t = &ex["throw"];
            let o = |i: usize| t["targetOffset"][i].as_f64().unwrap_or(0.0) as f32;
            Throw {
                target_offset: Vec3::new(o(0), o(1), o(2)),
                target_facing: t["targetFacing"].as_f64().unwrap_or(0.0) as f32,
                range: t["range"].as_f64().unwrap_or(0.0) as f32,
                target_anim_type: named(&t["targetAnimType"], &sc::ANIM_TYPES),
            }
        }),
        reach: ex["reach"].is_object().then(|| {
            let r = &ex["reach"];
            let g = |v: &serde_json::Value| v.as_f64().unwrap_or(0.0) as f32;
            let bound = |b: &serde_json::Value| StrikeBound {
                tick: b["tick"].as_u64().unwrap_or(0) as u16,
                offset: [g(&b["offset"][0]), g(&b["offset"][1])],
                height: g(&b["height"]),
                distance: g(&b["distance"]),
                min_height: g(&b["minHeight"]),
                max_height: g(&b["maxHeight"]),
                angle: g(&b["angle"]),
            };
            let mut spokes = [0.0; 36];
            for (i, s) in spokes.iter_mut().enumerate() {
                *s = g(&r["spokes"][i]);
            }
            Reach {
                max_distance: g(&r["maxDistance"]),
                min_height: g(&r["minHeight"]),
                max_height: g(&r["maxHeight"]),
                spokes,
                first: bound(&r["first"]),
                farthest: bound(&r["farthest"]),
                direction: named(&r["direction"], &sc::DIRECTIONS),
                strikes: r["strikes"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|s| Strike {
                        tick: s["tick"].as_u64().unwrap_or(0) as u16,
                        angle: g(&s["angle"]),
                        distance: g(&s["distance"]),
                        min_height: g(&s["minHeight"]),
                        max_height: g(&s["maxHeight"]),
                    })
                    .collect(),
            }
        }),
        body: ex["bodyTrack"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .map(|p| {
                        let c = |i: usize| p[i].as_f64().unwrap_or(0.0) as f32;
                        [c(0), c(1), c(2), c(3)]
                    })
                    .collect()
            })
            .unwrap_or_default(),
        footsteps: ex["footsteps"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_u64().map(|v| v as u16)).collect())
            .unwrap_or_default(),
        self_damage: ex["selfDamage"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .map(|d| {
                        let k = |n: &str| d[n].as_u64().unwrap_or(0) as u16;
                        (k("tick"), k("damage"))
                    })
                    .collect()
            })
            .unwrap_or_default(),
    })
}

/// Resolves a material's texture path + blend/additive flags.
fn material(json: &serde_json::Value, dir: &Path, idx: usize) -> (Option<PathBuf>, bool, bool) {
    let m = &json["materials"][idx];
    let blend = m["alphaMode"].as_str() == Some("BLEND");
    let additive = m["extras"]["additive"].as_bool().unwrap_or(false);
    let Some(ti) = m["pbrMetallicRoughness"]["baseColorTexture"]["index"].as_u64() else {
        return (None, blend, additive);
    };
    let src = json["textures"][ti as usize]["source"].as_u64().unwrap() as usize;
    let uri = json["images"][src]["uri"].as_str().unwrap_or("");
    (Some(gltf::normalize(dir, uri)), blend, additive)
}
