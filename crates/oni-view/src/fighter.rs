//! A **fighter**: one character instance driven by physics + the locomotion
//! state machine, with a facing slew, an airborne FSM, and (minimal) combat
//! state. The player and every spawned character (e.g. a training dummy) are
//! fighters; [`World`](crate::world::World) holds them and steps them together.
//!
//! This folds the per-tick driving that `app.rs` (play mode) and `script.rs`
//! (the Lua harness) each used to run inline — takeoff → turn-in-place intent →
//! state-machine tick → root-motion → physics → landing → facing slew → replace
//! — into one place, so the two front-ends and spawned characters all share it.

use std::rc::Rc;

use glam::{Mat4, Quat, Vec3};

use crate::anim_state::{AnimCollection, Input, Locomotion};
use crate::control::{diagonal_strafe_offset, Facing, JUMP_SPEED, MED_TURN, SLOW_TURN, TURN_DEADZONE};
use crate::physics::Physics;
use crate::render::CharDraw;
use crate::skel::Character;

/// How fast the knockback velocity accumulator bleeds off each tick (Oni
/// consumes `activeChar.knockback` through physics; this is the decay).
const KNOCKBACK_DECAY: f32 = 0.80;

/// One character instance in the world.
pub struct Fighter {
    /// Uploaded render-model index (see [`crate::render::Renderer::add_character`]).
    pub model: usize,
    /// Shared skeleton/mesh for posing, bounds, and stitch capture.
    pub character: Rc<Character>,
    pub physics: Physics,
    pub loco: Locomotion,
    /// Body-yaw slew (`facing`/`desiredFacing`); nudge `desired_facing` to turn.
    pub facing: Facing,
    /// Off the ground: horizontal motion is takeoff momentum, not root motion.
    pub airborne: bool,
    air_vel: Vec3,
    air_ticks: u32,
    /// Last grounded horizontal world-move (captured as takeoff momentum).
    last_horiz: Vec3,
    /// World placement matrix (rotation + pelvis origin), rebuilt each tick.
    pub placement: Mat4,
    /// Capsule radius (for melee reach tests).
    pub radius: f32,
    // --- combat (minimal slice; see docs/COMBAT.md) ---
    pub hp: i32,
    /// A passive fighter (training dummy): stands and takes hits, never deals
    /// damage (its swings, if any, are ignored by hit resolution).
    pub passive: bool,
    /// Knockback velocity accumulator, added to the move and decayed each tick.
    knockback: Vec3,
    /// Victim fighter-ids already hit by the current attack window (so one swing
    /// hits each victim once); cleared when the attack window ends.
    hit_set: Vec<usize>,
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
        level_positions: &[[f32; 3]],
        level_indices: &[u32],
        spawn_pos: Vec3,
        facing_yaw: f32,
        passive: bool,
    ) -> Fighter {
        let (foot_offset, height, radius) = capsule_dims(&character);
        let feet = Vec3::new(spawn_pos.x, spawn_pos.y + foot_offset, spawn_pos.z);
        let physics = Physics::new(level_positions, level_indices, feet, height, radius);
        let loco = Locomotion::new(anims);
        let facing = Facing::new(facing_yaw);
        let origin = Vec3::new(feet.x, feet.y + loco.height(), feet.z);
        let placement = Mat4::from_rotation_translation(Quat::from_rotation_y(facing_yaw), origin);
        Fighter {
            model,
            character,
            physics,
            loco,
            facing,
            airborne: false,
            air_vel: Vec3::ZERO,
            air_ticks: 0,
            last_horiz: Vec3::ZERO,
            placement,
            radius,
            hp: 100,
            passive,
            knockback: Vec3::ZERO,
            hit_set: Vec::new(),
        }
    }

    /// Advances one 60 Hz tick. `mv` carries movement/attack intent (forward,
    /// strafe, walk, punch, kick — `turn_*`/`airborne` are filled in here from
    /// the fighter's own facing/air state); `jump` is a jump press edge. Nudge
    /// `self.facing` (mouse / `turn`) before calling for turning.
    pub fn tick(&mut self, mut mv: Input, jump: bool) {
        let turn_cap = if mv.walk { SLOW_TURN } else { MED_TURN };
        let diag =
            diagonal_strafe_offset(mv.forward, mv.backward, mv.step_left, mv.step_right);
        let moving = mv.forward || mv.backward || mv.step_left || mv.step_right;

        // Takeoff: launch the physics arc + force the jump clip, carrying the
        // run momentum captured on the ground.
        if jump && !self.airborne && self.physics.grounded && self.physics.jump(JUMP_SPEED) {
            self.loco.start_jump(Some(&self.character));
            self.airborne = true;
            self.air_ticks = 0;
            self.air_vel = self.last_horiz;
        }

        // Turn-in-place intent (large pending turn while standing) + airborne hold.
        let offset = self.facing.desired_offset();
        mv.turn_left = !self.airborne && !moving && offset > TURN_DEADZONE;
        mv.turn_right = !self.airborne && !moving && offset < -TURN_DEADZONE;
        mv.airborne = self.airborne;

        // State machine → character-local root motion; airborne uses momentum.
        let local = self.loco.tick(mv, Some(&self.character));
        let mut world_move = if self.airborne {
            self.air_vel
        } else {
            let m = Quat::from_rotation_y(self.facing.facing + diag)
                * Vec3::new(local.x, 0.0, local.y);
            self.last_horiz = Vec3::new(m.x, 0.0, m.z);
            m
        };

        // Knockback accumulator (combat): add to the move, bleed off.
        if self.knockback.length_squared() > 1e-4 {
            world_move += self.knockback;
            self.knockback *= KNOCKBACK_DECAY;
            if self.knockback.length_squared() < 1e-3 {
                self.knockback = Vec3::ZERO;
            }
        }

        self.physics.step(world_move);

        // Landing after a couple airborne ticks.
        if self.airborne {
            self.air_ticks += 1;
            if self.air_ticks > 2 && self.physics.grounded {
                self.airborne = false;
                self.air_vel = Vec3::ZERO;
                self.loco.land(Some(&self.character));
            }
        }

        self.facing.turn_toward(turn_cap);
        self.replace();

        // The hit-dedup set belongs to one attack window: once the window ends
        // (or the clip is not an attack), a later swing may hit again.
        if self.loco.active_attack().is_none() {
            self.hit_set.clear();
        }
    }

    /// The attack whose damage window contains the current frame (hit detection).
    pub fn active_attack(&self) -> Option<crate::skel::Attack> {
        self.loco.active_attack()
    }

    /// Whether this fighter has already landed the current attack on `victim`.
    pub fn already_hit(&self, victim: usize) -> bool {
        self.hit_set.contains(&victim)
    }

    /// Records that the current attack has now hit `victim`.
    pub fn mark_hit(&mut self, victim: usize) {
        self.hit_set.push(victim);
    }

    /// Rebuilds the world placement from the settled feet + pelvis bob + facing.
    fn replace(&mut self) {
        let feet = self.physics.feet;
        let origin = Vec3::new(feet.x, feet.y + self.loco.height(), feet.z);
        self.placement =
            Mat4::from_rotation_translation(Quat::from_rotation_y(self.facing.facing), origin);
    }

    /// Posed bone world-matrices (placement baked in) for rendering.
    pub fn bones(&self) -> Vec<Mat4> {
        self.character
            .pose_blended(
                Some(self.loco.clip()),
                self.loco.frame_time(),
                self.loco.blend(),
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

    /// Applies a hit: subtract HP (floored at 0) and add a knockback impulse.
    /// Reaction-clip selection is driven by the caller (see hit detection).
    pub fn hurt(&mut self, damage: i32, knockback: Vec3) {
        self.hp = (self.hp - damage).max(0);
        self.knockback += knockback;
    }

    pub fn alive(&self) -> bool {
        self.hp > 0
    }
}

/// Capsule dimensions from a character's rest pose: `(foot_offset, height,
/// radius)` where `foot_offset` is the feet's Y relative to the pelvis origin
/// (negative). Mirrors the capsule sizing play mode used from the AABB.
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
