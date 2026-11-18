//! A **fighter**: one character instance — its controls, animation state
//! ([`Motion`]), facing, collision capsule and combat state. The player and
//! every spawned character (e.g. a training dummy) are fighters; the
//! [`World`](crate::world::World) holds them and steps them together.
//!
//! Each tick (`docs/MOVEMENT.md`): the controls feed the decision step
//! ([`decide::think`]), which requests animations and turns the body; then the
//! character moves — by the current clip's root motion on the ground, or by
//! its own airborne velocity under gravity — collides with the level, and
//! detects takeoff, walking off a ledge, and landing (with fall damage).

use std::rc::Rc;

use glam::{Mat4, Quat, Vec3};

use crate::anim::collection::AnimCollection;
use crate::anim::decide::{self, Controls};
use crate::anim::input::{key, Buttons};
use crate::anim::motion::{ClassTuning, Motion};
use crate::anim::reaction;
use crate::anim::types::{ty, var};
use crate::control::{Facing, MED_TURN, SLOW_TURN};
use crate::physics::Physics;
use crate::quirks::Quirks;
use crate::render::CharDraw;
use crate::skel::Character;

/// Downward pull each grounded tick, so the feet stay on the floor.
const GROUND_STICK: f32 = 0.5;
/// The pull grows to meet a floor this far below (stepping down slopes/stairs).
const MAX_GROUND_STICK: f32 = 4.0;
/// Vertical speed a character walks off a ledge with.
const LEDGE_DROP_SPEED: f32 = 0.5;
/// Grounded ticks with no floor beneath before the character falls.
const LEDGE_GRACE: u16 = 11;
/// Ticks airborne after which the fall is fatal.
const MAX_AIR_FRAMES: u16 = 300;
/// Share of the pending knockback applied each tick, and what remains.
const KNOCKBACK_PER_TICK: f32 = 0.07;
const KNOCKBACK_KEEP: f32 = 1.0 - KNOCKBACK_PER_TICK;
/// A second forward press within this many ticks starts a sprint.
const SPRINT_TAP_WINDOW: u64 = 15;
/// Starting an attack turns the attacker toward an opponent this close…
const AUTO_FACE_RANGE: f32 = 40.0;
/// …within this angle of where the attack is aimed.
const AUTO_FACE_ARC: f32 = std::f32::consts::FRAC_PI_2;
/// A corpse slides at most this fast, and comes to rest after this many
/// ticks on the ground.
const CORPSE_MAX_SLIDE: f32 = 0.5;
const CORPSE_SETTLE_TICKS: u16 = 120;
/// A knocked-down training dummy lies this long before getting up.
const DUMMY_DAZE_TICKS: u16 = 90;

fn is_landing(t: u16) -> bool {
    matches!(
        t,
        ty::LAND
            | ty::LAND_FORWARD
            | ty::LAND_RIGHT
            | ty::LAND_LEFT
            | ty::LAND_BACK
            | ty::LAND_HARD
            | ty::LAND_FORWARD_HARD
            | ty::LAND_RIGHT_HARD
            | ty::LAND_LEFT_HARD
            | ty::LAND_BACK_HARD
            | ty::LAND_DEAD
    )
}

/// One character instance in the world.
pub struct Fighter {
    /// Uploaded render-model index (see [`crate::render::Renderer::add_character`]).
    pub model: usize,
    /// Shared skeleton/mesh for posing, bounds, and blend capture.
    pub character: Rc<Character>,
    pub physics: Physics,
    pub motion: Motion,
    /// Body yaw slew (`facing` / `desired_facing` / modifier).
    pub facing: Facing,
    /// Controls this tick (held / pressed / released).
    pub buttons: Buttons,
    pub quirks: Quirks,
    /// A player character (vs. an AI / dummy).
    pub player: bool,
    /// World placement matrix (rotation + pelvis origin), rebuilt each tick.
    pub placement: Mat4,
    /// Capsule radius (for melee reach tests).
    pub radius: f32,
    // --- combat (see docs/COMBAT.md) ---
    pub hp: i32,
    /// A passive fighter (training dummy): stands and takes hits, never deals
    /// damage (its swings, if any, are ignored by hit resolution).
    pub passive: bool,
    /// Pending knockback displacement, applied a share per tick.
    knockback: Vec3,
    /// Victims already hit by the current clip (one hit per swing).
    hit_set: Vec<usize>,
    last_forward_tap: Option<u64>,
    sprinting: bool,
    /// A reaction to force at the end of this tick's hit resolution.
    pub pending_reaction: u16,
    /// A hit was blocked this tick: force the block clip.
    pub please_block: bool,
    /// A corpse's drift, and how long it has rested on the ground.
    corpse_velocity: Vec3,
    corpse_rest: u16,
    /// Ticks spent lying on the ground (for the dummy's get-up stand-in).
    down_ticks: u16,
    /// The AI driving this (non-player) character, if any.
    pub brain: Option<crate::ai::Brain>,
}

impl Fighter {
    /// Builds a fighter from a shared character + animation set, placed with its
    /// pelvis origin at `spawn_pos` facing `facing_yaw` (radians). `level_*` is
    /// the static collision mesh the capsule resolves against.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        model: usize,
        character: Rc<Character>,
        anims: Rc<AnimCollection>,
        tuning: ClassTuning,
        level_positions: &[[f32; 3]],
        level_indices: &[u32],
        spawn_pos: Vec3,
        facing_yaw: f32,
        passive: bool,
        player: bool,
        quirks: Quirks,
    ) -> Fighter {
        let (foot_offset, height, radius) = capsule_dims(&character);
        let feet = Vec3::new(spawn_pos.x, spawn_pos.y + foot_offset, spawn_pos.z);
        let physics = Physics::new(level_positions, level_indices, feet, height, radius);
        let mut motion = Motion::new(anims, tuning);
        motion.idle_enabled = !passive;
        motion.feet_y = feet.y;
        let facing = Facing::new(facing_yaw);
        let origin = Vec3::new(feet.x, feet.y + motion.height(), feet.z);
        let placement = Mat4::from_rotation_translation(Quat::from_rotation_y(facing_yaw), origin);
        Fighter {
            model,
            character,
            physics,
            motion,
            facing,
            buttons: Buttons::default(),
            quirks,
            player,
            placement,
            radius,
            hp: tuning.max_hit_points,
            passive,
            knockback: Vec3::ZERO,
            hit_set: Vec::new(),
            last_forward_tap: None,
            sprinting: false,
            pending_reaction: ty::NONE,
            please_block: false,
            corpse_velocity: Vec3::ZERO,
            corpse_rest: 0,
            down_ticks: 0,
            brain: None,
        }
    }

    /// Advances one 60 Hz tick with `held` the controls (`anim::input::key`
    /// bits) currently down; `opponents` are the feet of the other living
    /// fighters (for auto-facing). Nudge `self.facing` (mouse / `turn`) before
    /// calling to turn.
    pub fn tick(&mut self, held: u32, opponents: &[Vec3]) {
        let ch = self.character.clone();
        self.motion.now += 1;
        if self.motion.corpse {
            self.drift_as_corpse();
            return;
        }
        let held = if self.motion.dying { 0 } else { held };
        self.buttons.advance(held);
        self.update_sprint();
        self.motion.feet_y = self.physics.feet.y;

        let turn_cap = if self.buttons.holding(key::WALK) {
            SLOW_TURN
        } else {
            MED_TURN
        };
        let controls = Controls {
            buttons: &self.buttons,
            quirks: self.quirks,
            player: self.player,
            restricted: false,
            turn_cap,
        };
        decide::think(&mut self.motion, &ch, &mut self.facing, &controls);
        self.facing.rotate(self.motion.take_facing_change());
        if std::mem::take(&mut self.motion.attack_began) && self.player && self.motion.air.frames == 0 {
            self.face_opponent(opponents);
        }
        self.motion.count_down(&ch);

        // This tick's character-local movement.
        self.take_off();
        let local = if self.motion.air.frames > 0 {
            self.fly()
        } else {
            let mut d = self.motion.root_delta();
            d.y -= self.ground_stick();
            d
        };
        let mut world = Quat::from_rotation_y(self.facing.facing) * local;

        // Knockback: spent a share per tick on the ground; dropped in the air.
        if self.motion.air.frames > 0 {
            self.knockback = Vec3::ZERO;
        } else {
            world += self.knockback * KNOCKBACK_PER_TICK;
            self.knockback *= KNOCKBACK_KEEP;
            if self.knockback.length_squared() < 0.01 {
                self.knockback = Vec3::ZERO;
            }
        }

        self.motion.advance(&ch);
        self.facing.rotate(self.motion.take_facing_change());

        let grounded = self.physics.slide(world);
        if world.y < 0.0 {
            self.touch_ground(grounded);
        }

        self.replace();
        if std::mem::take(&mut self.motion.new_clip) {
            self.hit_set.clear();
        }
    }

    /// A double-tapped forward sprints until forward is released.
    fn update_sprint(&mut self) {
        let now = self.motion.now;
        if self.buttons.just_pressed(key::FORWARD) {
            if self
                .last_forward_tap
                .is_some_and(|t| now.saturating_sub(t) <= SPRINT_TAP_WINDOW)
            {
                self.sprinting = true;
            }
            self.last_forward_tap = Some(now);
        }
        if !self.buttons.holding(key::FORWARD) {
            self.sprinting = false;
        }
        if self.sprinting {
            self.motion.varient |= var::SPRINT;
        } else {
            self.motion.varient &= !var::SPRINT;
        }
    }

    /// Leaves the ground when a launching clip asked to: a jump uses the
    /// velocity prepared at the jump request plus the class's jump strength
    /// (and a push in the held direction); walking off a ledge carries the
    /// current momentum with a small downward speed.
    fn take_off(&mut self) {
        if !self.motion.please_jump {
            return;
        }
        self.motion.please_jump = false;
        let air = &mut self.motion.air;
        air.off_ground = 0;
        if air.frames > 0 {
            return;
        }
        let velocity = match air.build.take() {
            Some(build) => {
                let amount = self.motion.tuning.jump_amount;
                let push = 0.5 * amount;
                let mut v = build + Vec3::new(0.0, amount, 0.0);
                let b = &self.buttons;
                if b.holding(key::FORWARD) {
                    v.z = v.z.max(push);
                } else if b.holding(key::BACKWARD) {
                    v.z = v.z.min(-push);
                } else if b.holding(key::STEP_LEFT) {
                    v.x = v.x.max(push);
                } else if b.holding(key::STEP_RIGHT) {
                    v.x = v.x.min(-push);
                }
                v
            }
            None => {
                let mut v = self.motion.velocity_estimate();
                v.y = -LEDGE_DROP_SPEED;
                v
            }
        };
        let air = &mut self.motion.air;
        air.velocity = velocity;
        air.frames = 1;
    }

    /// One airborne tick: returns this tick's (character-local) movement and
    /// applies gravity for the next.
    fn fly(&mut self) -> Vec3 {
        let t = self.motion.tuning;
        let air = &mut self.motion.air;
        let moved = air.velocity;
        air.frames = air.frames.saturating_add(1);
        if air.frames > MAX_AIR_FRAMES {
            self.hp = 0; // a fatal fall (death itself is not modelled yet)
        }
        air.velocity.y = (air.velocity.y - t.jump_gravity).max(t.max_velocity);
        moved
    }

    /// The downward pull that keeps a grounded character on the floor: the
    /// gap to a floor within reach (so stepping down a slope stays planted),
    /// at least the base stick. Attacks always use the base stick.
    fn ground_stick(&self) -> f32 {
        if !self.motion.current().attacks.is_empty() {
            return GROUND_STICK;
        }
        match self.physics.ground_below(MAX_GROUND_STICK) {
            Some(d) if d < MAX_GROUND_STICK => d.max(GROUND_STICK),
            _ => GROUND_STICK,
        }
    }

    /// After a downward move: land if airborne and now on the floor; count
    /// floorless ticks if grounded, falling after a short grace.
    fn touch_ground(&mut self, grounded: bool) {
        let feet_y = self.physics.feet.y;
        if self.motion.air.frames > 0 {
            self.motion.air.off_ground = 0;
            if grounded && !is_landing(self.motion.kind) {
                self.land(feet_y);
            }
            return;
        }
        let air = &mut self.motion.air;
        if grounded {
            air.off_ground = 0;
        } else {
            if air.off_ground == 0 {
                air.jump_height = feet_y;
            }
            air.off_ground += 1;
            if air.off_ground > LEDGE_GRACE {
                self.motion.please_jump = true;
            }
        }
    }

    /// Touchdown: fall damage by the height fallen since takeoff, then ask for
    /// a landing (a hard one if it hurt).
    fn land(&mut self, impact_y: f32) {
        let t = self.motion.tuning;
        let fall = self.motion.air.jump_height - impact_y;
        let damage = if fall > t.damage_end {
            1000.0f32.max(self.hp as f32)
        } else if fall < t.damage_start {
            0.0
        } else {
            (fall - t.damage_start) / (t.damage_end - t.damage_start) * t.max_hit_points as f32
        };
        let damage = damage.round() as i32;
        if damage > 0 {
            self.hp = (self.hp - damage).max(0);
            eprintln!("fall: {fall:.1} units, {damage} damage (hp {})", self.hp);
        }
        let air = &mut self.motion.air;
        air.velocity = Vec3::ZERO;
        air.frames = 0;
        self.motion.please_land = true;
        self.motion.please_land_hard = damage > 0;
    }

    /// Starting a ground attack turns the attacker so the attack points at
    /// the nearest-in-angle opponent in range and in front.
    fn face_opponent(&mut self, opponents: &[Vec3]) {
        use std::f32::consts::{FRAC_PI_2, PI, TAU};
        let offset = match self.motion.current().move_direction {
            2 => PI,         // backwards attack
            3 => FRAC_PI_2,  // to the left
            4 => -FRAC_PI_2, // to the right
            _ => 0.0,
        };
        let aim = self.facing.aim() + offset;
        let me = self.physics.feet;
        let best = opponents
            .iter()
            .filter(|p| p.distance(me) <= AUTO_FACE_RANGE)
            .map(|p| {
                let d = *p - me;
                let toward = d.x.atan2(d.z);
                (toward - aim + PI).rem_euclid(TAU) - PI
            })
            .filter(|a| a.abs() <= AUTO_FACE_ARC)
            .min_by(|a, b| a.abs().total_cmp(&b.abs()));
        if let Some(delta) = best {
            self.facing.rotate(delta);
            self.facing.adjust_desired(delta);
        }
    }

    /// Takes damage and a shove. Stuns end (a new hit sets its own); at zero
    /// health the character dies: its death reaction plays (a knockdown that
    /// was already chosen, else a frontal knockdown) and it stops acting.
    pub fn take_damage(&mut self, damage: i32, knockback: Vec3) {
        let m = &mut self.motion;
        m.hit_stun = 0;
        m.block_stun = 0;
        m.stagger_stun = 0;
        m.dizzy_stun = 0;
        if m.dying {
            return;
        }
        self.hp = (self.hp - damage).max(0);
        self.knockback += knockback;
        if self.hp == 0 {
            self.motion.dying = true;
            let kind = if reaction::is_knockdown(self.pending_reaction) {
                self.pending_reaction
            } else {
                ty::KNOCKDOWN_HEAD
            };
            self.pending_reaction = ty::NONE;
            let ch = self.character.clone();
            // Force it past the "dead make no moves" rule.
            self.motion.dying = false;
            self.motion.knock(&ch, kind, self.quirks);
            self.motion.dying = true;
            eprintln!("fighter died (reaction type {kind})");
        }
    }

    /// Plays the reaction or block chosen for this tick's hits.
    pub fn apply_reaction(&mut self) {
        let ch = self.character.clone();
        let pending = std::mem::replace(&mut self.pending_reaction, ty::NONE);
        if pending != ty::NONE {
            if !self.motion.dying {
                self.motion.knock(&ch, pending, self.quirks);
            }
        } else if self.please_block {
            self.motion.block(&ch);
        }
        self.please_block = false;
    }

    /// A finished death reaction: the body keeps its pose and slides to rest.
    fn drift_as_corpse(&mut self) {
        if self.corpse_rest >= CORPSE_SETTLE_TICKS {
            return;
        }
        if self.corpse_rest == 0 && self.corpse_velocity == Vec3::ZERO {
            // Start from the leftover motion: flight, or the last shove.
            self.corpse_velocity = if self.motion.air.frames > 0 {
                Quat::from_rotation_y(self.facing.facing) * self.motion.air.velocity
            } else {
                self.knockback
            };
        }
        let mut v = self.corpse_velocity;
        let flat = Vec3::new(v.x, 0.0, v.z);
        if flat.length() > CORPSE_MAX_SLIDE {
            let capped = flat.normalize() * CORPSE_MAX_SLIDE;
            v.x = capped.x;
            v.z = capped.z;
        }
        if self.physics.grounded {
            v.x *= 0.5;
            v.z *= 0.5;
            v.y = -0.5;
            self.corpse_rest += 1;
        } else {
            v.x *= 0.8;
            v.z *= 0.8;
            v.y = (v.y - 0.5).clamp(-10.0, 2.0);
        }
        self.corpse_velocity = v;
        self.physics.slide(v);
        self.replace();
    }

    /// This tick's controls for a non-player character: its AI's, if it has
    /// one (which also turns the body); otherwise a training dummy's.
    pub fn npc_controls(&mut self) -> u32 {
        if self.motion.dying {
            return 0;
        }
        let feet = self.physics.feet;
        match self.brain.as_mut() {
            Some(brain) => brain.think(feet, &mut self.facing),
            None if self.passive => self.dummy_controls(),
            None => 0,
        }
    }

    /// Controls for a passive training dummy: none — except that after lying
    /// on the ground for a while it presses forward once to get up. This
    /// stands in for the AI's daze timer until AI drives characters.
    pub fn dummy_controls(&mut self) -> u32 {
        use crate::anim::types::st;
        let lying = matches!(self.motion.state, st::FALLEN_BACK | st::FALLEN_FRONT)
            && !reaction::is_victim(self.motion.kind);
        if !lying {
            self.down_ticks = 0;
            return 0;
        }
        self.down_ticks += 1;
        if self.down_ticks >= DUMMY_DAZE_TICKS {
            self.down_ticks = 0;
            key::FORWARD
        } else {
            0
        }
    }

    /// Whether the fighter is in the air.
    pub fn airborne(&self) -> bool {
        self.motion.air.frames > 0
    }

    /// The attack whose damage window contains the current frame (hit detection).
    pub fn active_attack(&self) -> Option<crate::skel::Attack> {
        self.motion.active_attack()
    }

    /// Whether this fighter has already landed the current attack on `victim`.
    pub fn already_hit(&self, victim: usize) -> bool {
        self.hit_set.contains(&victim)
    }

    /// Records that the current attack has now hit `victim`.
    pub fn mark_hit(&mut self, victim: usize) {
        self.hit_set.push(victim);
    }

    /// Rebuilds the world placement from the feet + pelvis height + facing.
    fn replace(&mut self) {
        let feet = self.physics.feet;
        let origin = Vec3::new(feet.x, feet.y + self.motion.height(), feet.z);
        self.placement =
            Mat4::from_rotation_translation(Quat::from_rotation_y(self.facing.facing), origin);
    }

    /// Posed bone world-matrices (placement baked in) for rendering.
    pub fn bones(&self) -> Vec<Mat4> {
        self.character
            .pose_blended(
                Some(self.motion.current()),
                self.motion.frame_time(),
                self.motion.blend_pose(),
            )
            .into_iter()
            .map(|m| self.placement * m)
            .collect()
    }

    /// A render draw referencing `bones` (kept alive by the caller).
    pub fn draw<'a>(&self, bones: &'a [Mat4]) -> CharDraw<'a> {
        CharDraw {
            model: self.model,
            bones,
        }
    }

    /// Feet (ground) position in world space.
    pub fn feet(&self) -> Vec3 {
        self.physics.feet
    }

    pub fn alive(&self) -> bool {
        self.hp > 0 && !self.motion.dying
    }
}

/// Capsule dimensions from a character's rest pose: `(foot_offset, height,
/// radius)` where `foot_offset` is the feet's Y relative to the pelvis origin
/// (negative).
fn capsule_dims(ch: &Character) -> (f32, f32, f32) {
    let world = ch.pose(None, 0.0);
    let (mut mn, mut mx) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
    for part in &ch.parts {
        let m = world.get(part.bone).copied().unwrap_or(Mat4::IDENTITY);
        for v in &part.vertices {
            let p = m.transform_point3(Vec3::from_array(v.pos));
            mn = mn.min(p);
            mx = mx.max(p);
        }
    }
    let foot_offset = mn.y;
    let height = (mx.y - mn.y).max(2.0);
    let radius = ((mx.x - mn.x).max(mx.z - mn.z) * 0.5).clamp(0.5, 3.0);
    (foot_offset, height, radius)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim::collection::tests::konoko;

    /// A big flat floor at y = 0.
    fn floor() -> (Vec<[f32; 3]>, Vec<u32>) {
        let e = 500.0;
        (
            vec![[-e, 0.0, -e], [e, 0.0, -e], [e, 0.0, e], [-e, 0.0, e]],
            vec![0, 1, 2, 0, 2, 3],
        )
    }

    fn konoko_at(y: f32) -> Option<Fighter> {
        let coll = konoko()?;
        let model = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../generated/oni/common/characters/konoko_body_high/model.glb");
        let ch = Rc::new(crate::skel::load_character(&model).ok()?);
        let (v, i) = floor();
        Some(Fighter::new(
            0,
            ch,
            coll,
            ClassTuning::default(),
            &v,
            &i,
            Vec3::new(0.0, y, 0.0),
            0.0,
            false,
            true,
            Quirks::FIXED,
        ))
    }

    /// Steps until the fighter has been airborne and is back on the ground,
    /// returning the animation type it touched down with.
    fn fall_and_land(f: &mut Fighter) -> Option<u16> {
        let mut was_airborne = false;
        for _ in 0..600 {
            f.tick(0, &[]);
            if f.airborne() {
                was_airborne = true;
            } else if was_airborne {
                f.tick(0, &[]); // the landing request is taken on the next tick
                return Some(f.motion.kind);
            }
        }
        None
    }

    #[test]
    fn a_long_fall_hurts_and_lands_hard() {
        let Some(mut f) = konoko_at(70.0) else {
            eprintln!("SKIP: generated assets not found");
            return;
        };
        let hp0 = f.hp;
        let landed = fall_and_land(&mut f).expect("falls and lands");
        assert!(f.hp < hp0, "a ~70 unit fall costs health ({hp0} -> {})", f.hp);
        assert!(
            (ty::LAND_HARD..=ty::LAND_BACK_HARD).contains(&landed),
            "lands with a hard-landing clip, got type {landed}"
        );
        assert!(f.feet().y.abs() < 0.5, "ends on the floor");
    }

    #[test]
    fn a_jump_lands_softly() {
        let Some(mut f) = konoko_at(0.5) else {
            eprintln!("SKIP: generated assets not found");
            return;
        };
        for _ in 0..30 {
            f.tick(0, &[]); // settle onto the floor
        }
        let hp0 = f.hp;
        f.tick(key::JUMP, &[]); // press jump for one tick
        let landed = fall_and_land(&mut f).expect("takes off and lands");
        assert_eq!(f.hp, hp0, "a jump on flat ground is harmless");
        assert!(
            matches!(landed, ty::LAND | ty::LAND_FORWARD | ty::LAND_LEFT | ty::LAND_RIGHT | ty::LAND_BACK),
            "lands with an ordinary landing clip, got type {landed} ({})",
            f.motion.name()
        );
    }
}
