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

/// The feet's bones (footsteps sound at the foot that came down).
const LEFT_FOOT: usize = 3;
const RIGHT_FOOT: usize = 6;

use glam::{Mat4, Quat, Vec3};

use crate::anim::collection::AnimCollection;
use crate::anim::decide::{self, Controls};
use crate::anim::input::{key, Buttons};
use crate::anim::motion::{ClassTuning, Motion};
use crate::anim::reaction;
use crate::anim::types::{flag, st, ty, var};
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
    /// Each body part's bounding sphere in its bone's space (bone index,
    /// centre, radius) — for detecting characters touching each other.
    part_spheres: Vec<(usize, Vec3, f32)>,
    // --- combat (see docs/COMBAT.md) ---
    pub hp: i32,
    /// Its full health.
    pub max_hp: i32,
    /// Takes no damage (`chr_invincible`).
    pub invincible: bool,
    /// Removed from the level (`chr_delete`): not simulated or drawn.
    pub gone: bool,
    /// A film it is playing (its controls come from the recording).
    pub film: Option<FilmPlay>,
    /// Using a console (its controls are the console's meanwhile).
    pub console: Option<crate::consoles::Use>,
    /// An alarm it answers went off (its alarm script is due).
    pub alarmed: bool,
    /// The alarm groups it answers (bits).
    pub alarm_groups: u32,
    /// Its class removes its body this long after death (0: never), and the
    /// countdown once dead.
    pub death_delete: u32,
    pub death_timer: Option<u32>,
    /// Unstoppable (a script's order): blows don't stun it, knock it about
    /// or make it react, and it blocks without block stun.
    pub unstoppable: bool,
    /// Death-locked (a script's order): it can die, but its body is never
    /// removed.
    pub death_lock: bool,
    /// Its class's shadow, and whether a script took it away (`chr_shadow`).
    pub shadow: Option<Rc<crate::decal::Shadow>>,
    pub no_shadow: bool,
    /// Dead and settled (`ONcCharacterFlag_Dead_3_Cosmetic`): its body has
    /// lain on the ground for over 120 ticks (or died for 15 s); it casts
    /// no shadow. With the ticks it has lain still and been dying.
    pub settled: bool,
    pub death_still: u32,
    pub death_moving: u32,
    /// Poison (a script calling `chr_poison` each tick): when it was last
    /// poisoned and its count toward the next dose.
    poison_last: u64,
    poison_count: i32,
    /// A passive fighter (training dummy): stands and takes hits, never deals
    /// damage (its swings, if any, are ignored by hit resolution).
    pub passive: bool,
    /// Pending knockback displacement, applied a share per tick.
    pub knockback: Vec3,
    /// (attack index, victim) pairs already hit by the current clip: each of
    /// a clip's attacks hits each victim once.
    hit_set: Vec<(usize, usize)>,
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
    /// The fighter this one is throwing (until its throw clip ends): the pair
    /// pass through each other and can't hit each other meanwhile.
    pub throwing: Option<usize>,
    /// Who last threw this fighter.
    pub thrown_by: Option<usize>,
    /// Its class's melee AI profile and daze range (ticks).
    pub melee_profile: Option<Rc<crate::ai::melee::profile::Profile>>,
    pub daze: (u32, u32),
    /// Its class's combat timings (for goals).
    pub class_combat: crate::ai::goals::ClassCombat,
    /// Its movement last tick (world).
    pub velocity: Vec3,
    /// A noise it made this tick (kind, volume), for the world to spread.
    pub noise: Option<(crate::ai::perception::Stimulus, f32)>,
    /// A footstep (or landing, or fall) this tick: its impact kind and
    /// where — the world plays its impact effect.
    pub step: Option<(usize, Vec3)>,
    /// The level triangle it last stood on (the floor's material).
    pub last_floor: Option<u32>,
    /// The clip and frame it played this tick (its animation sounds).
    pub played: Option<(usize, u16)>,
    /// A clip it started this tick (its vocalization).
    pub started: Option<usize>,
    /// Hurts this tick: damage, the least pain it calls for (0 light, 1
    /// medium, 2 heavy), and whether it killed.
    pub hurts: Vec<(u32, u32, bool)>,
    /// What it is saying, and how it cries out (`docs/AUDIO.md`, *Voices*).
    pub speech: crate::world::Speech,
    pub pain: crate::world::PainState,
    /// Its class's say in its impacts.
    pub sounds: Rc<crate::impacts::ClassSounds>,
    /// What it sees, hears and remembers (AI characters).
    pub perception: Option<crate::ai::perception::Perception>,
    /// Glancing toward a point until a tick (no brain to steer it).
    pub glance: Option<(Vec3, u64)>,
    /// Startled: the facing it started from and the turn it makes over the
    /// startle clip (nothing else happens meanwhile).
    pub startling: Option<(f32, f32)>,
    /// The gun in its hand and the one holstered (indices into the world's
    /// weapons; `docs/WEAPONS.md`).
    pub hand: Option<usize>,
    pub holster: Option<usize>,
    /// Where a cutscene or console puts the gun in hand away.
    pub magic: Option<usize>,
    /// What it carries: ammo clips, energy cells, hypos.
    pub inventory: Inventory,
    /// Aim pitch (radians, up positive) and the recoil kick on top of it.
    pub aiming_ud: f32,
    pub recoil: f32,
    pub recoil_speed: f32,
    /// The last particle that damaged it (a shot hits once).
    pub last_particle: Option<u64>,
    /// Its last hit's direction × strength (knocks a dropped gun away).
    pub last_impact: Vec3,
    /// Never runs out of ammo (`chr_inv_reset`-style script orders aside).
    pub infinite_ammo: bool,
    /// Shots don't hurt it.
    pub weapon_immune: bool,
    /// Its gun is empty and it has nothing to reload with (fire = punch).
    pub weapon_empty: bool,
    /// Its class's aiming screens.
    pub aiming: Rc<Vec<crate::anim::aim::Screen>>,
    /// Its AI's shooting skills and targeting settings, and where its aim
    /// points now (absolute yaw, pitch) while it aims a gun.
    pub shooting: Rc<Vec<crate::ai::ranged::Skill>>,
    pub targeting: crate::ai::ranged::Params,
    pub ai_aim: Option<(f32, f32)>,
    /// Partial-body clips playing over the body: the recoil, and the
    /// inventory one (holster, draw, reload).
    pub overlays: [Option<Overlay>; 2],
    /// An item it is bending to pick up, and whether the pickup clip has
    /// started.
    pub pickup: Option<(crate::world::Item, bool, u16)>,
}

/// A clip playing as an overlay: which, and its frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Overlay {
    pub clip: usize,
    pub frame: u16,
    /// A holster forced by a cutscene or console (into the hidden slot).
    pub magic: bool,
}

/// Overlay slots.
pub const RECOIL: usize = 0;
pub const INVENTORY: usize = 1;

/// What a character carries besides guns (`docs/WEAPONS.md`, *Powerups*).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Inventory {
    pub ammo: u16,
    pub cells: u16,
    pub hypos: u16,
    pub shield: u16,
    pub invisibility: u16,
    pub lsi: bool,
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
        let part_spheres = part_spheres(&character);
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
            part_spheres,
            hp: tuning.max_hit_points,
            max_hp: tuning.max_hit_points,
            invincible: false,
            gone: false,
            film: None,
            console: None,
            alarmed: false,
            alarm_groups: 0,
            death_delete: 0,
            death_timer: None,
            unstoppable: false,
            death_lock: false,
            shadow: None,
            no_shadow: false,
            settled: false,
            death_still: 0,
            death_moving: 0,
            poison_last: 0,
            poison_count: 0,
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
            throwing: None,
            thrown_by: None,
            melee_profile: None,
            daze: (60, 100),
            class_combat: Default::default(),
            velocity: Vec3::ZERO,
            noise: None,
            step: None,
            last_floor: None,
            played: None,
            started: None,
            hurts: Vec::new(),
            speech: Default::default(),
            pain: Default::default(),
            sounds: Rc::default(),
            perception: None,
            glance: None,
            startling: None,
            hand: None,
            holster: None,
            magic: None,
            inventory: Inventory::default(),
            aiming_ud: 0.0,
            recoil: 0.0,
            recoil_speed: 0.0,
            last_particle: None,
            last_impact: Vec3::ZERO,
            infinite_ammo: false,
            weapon_immune: false,
            weapon_empty: false,
            overlays: [None; 2],
            pickup: None,
            aiming: Rc::new(Vec::new()),
            shooting: Rc::new(Vec::new()),
            targeting: Default::default(),
            ai_aim: None,
        }
    }

    /// Advances one 60 Hz tick with `held` the controls (`anim::input::key`
    /// bits) currently down; `opponents` are the feet of the other living
    /// fighters (for auto-facing). Nudge `self.facing` (mouse / `turn`) before
    /// calling to turn.
    #[cfg(test)]
    pub fn tick(&mut self, held: u32, opponents: &[Opponent]) {
        if let Some(world) = self.plan(held, 0, opponents) {
            self.commit(world);
        }
    }

    /// The first half of a tick: decide, animate, and work out this tick's
    /// world-space move — without applying it, so the world can stop
    /// characters walking into each other first. Returns None for a corpse
    /// (which moves itself).
    pub fn plan(&mut self, held: u32, pressed: u32, opponents: &[Opponent]) -> Option<Vec3> {
        let ch = self.character.clone();
        self.motion.now += 1;
        if self.motion.corpse {
            self.drift_as_corpse();
            return None;
        }
        // The dead, and anyone being thrown, have no control.
        let held = if self.motion.dying || ty::is_thrown(self.motion.kind) {
            0
        } else {
            held
        };
        self.buttons.advance(held);
        // An AI's fresh presses (it may press a key it already holds).
        self.buttons.pressed |= pressed & self.buttons.held;
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
        self.track_throw();
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

        // The frame played this tick (its sounds and footsteps, as the
        // original checks them before moving on to the next frame).
        let played = (self.motion.clip, self.motion.frame);
        self.motion.advance(&ch);
        self.facing.rotate(self.motion.take_facing_change());
        self.track_throw();

        // Noises: rolling or sliding (faint) and taunting (interesting), at
        // the clip's half-way point. (Crouched walking is silent.)
        if self.motion.frame == self.motion.length() / 2 {
            use crate::ai::perception::Stimulus;
            self.noise = match self.motion.kind {
                ty::CROUCH_FORWARD | ty::CROUCH_BACK | ty::CROUCH_LEFT | ty::CROUCH_RIGHT | ty::SLIDE => {
                    Some((Stimulus::SoundIgnore, 60.0))
                }
                ty::TAUNT => Some((Stimulus::SoundInteresting, 150.0)),
                _ => None,
            };
        }

        self.played = Some(played);

        // Footfalls (on the ground): an impact effect of the kind its clip
        // makes — the sound, and the noise the AI hears (`docs/AUDIO.md`,
        // *Impact effects*) — at the foot that came down.
        let clip = self.motion.clip_of(played.0);
        if let Some(k) = clip.footsteps.iter().position(|&f| f == played.1).filter(|_| self.motion.air.frames == 0) {
            let kind_name = oni_schema::name(&oni_schema::ANIM_TYPES, clip.anim_type).unwrap_or("");
            if let Some(mut kind) = crate::impacts::anim_impact(kind_name) {
                if kind == crate::impacts::anim::RUN && clip.varient & crate::anim::types::var::SPRINT != 0 {
                    kind = crate::impacts::anim::SPRINT;
                }
                let feet = self.feet();
                let foot = match clip.footstep_kinds.get(k) {
                    Some(1) => Some(LEFT_FOOT),
                    Some(2) => Some(RIGHT_FOOT),
                    _ => None,
                };
                let at = match foot.and_then(|b| self.bones().get(b).map(|m| m.w_axis.truncate())) {
                    Some(p) => Vec3::new(p.x, feet.y + 0.5, p.z),
                    None => feet,
                };
                self.step = Some((kind, at));
            }
        }

        // A clip can hurt whoever plays it at set frames (a throw's victim
        // hitting the floor).
        let wound = self.motion.self_damage();
        if wound > 0 {
            self.take_damage(wound, Vec3::ZERO);
            eprintln!("thrown: {wound} damage (hp {})", self.hp);
        }
        Some(world)
    }

    /// Its team (the player's is Konoko).
    pub fn team(&self) -> crate::ai::perception::Team {
        match &self.perception {
            Some(p) => p.team,
            None if self.player => crate::ai::perception::Team::Konoko,
            None => crate::ai::perception::Team::Syndicate,
        }
    }

    /// Where others look for it: pelvis, head, feet (+1).
    pub fn sight_points(&self) -> [Vec3; 3] {
        let spheres = self.body_spheres();
        let part = |k: usize, or: Vec3| spheres.iter().find(|s| s.0 == k).map_or(or, |s| s.1);
        let feet = self.feet();
        [part(0, feet + Vec3::Y * 8.0), part(10, feet + Vec3::Y * 15.0), feet + Vec3::Y]
    }

    /// A startled jump toward something at `at`: the startle animation for
    /// its quadrant (forward, left, back, right of the facing).
    pub fn startle(&mut self, at: Vec3) {
        use crate::ai::melee::tables::ty;
        let d = at - self.feet();
        let off = (d.x.atan2(d.z) - self.facing.facing + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
            - std::f32::consts::PI;
        let q = std::f32::consts::FRAC_PI_4;
        let kind = if off.abs() <= q {
            "startleForward"
        } else if off.abs() >= 3.0 * q {
            "startleBack"
        } else if off > 0.0 {
            "startleLeft"
        } else {
            "startleRight"
        };
        // Into the fight stance (the startle clips are fight-stance ones).
        self.motion.fight_mode();
        let ch = self.character.clone();
        if let Some(clip) = self.motion.request(&ch, crate::anim::motion::Urge::Force, false, ty(kind)) {
            // Over the clip the body turns to face the contact.
            let rotation = self.motion.clip_of(clip).final_rotation;
            let want = d.x.atan2(d.z);
            let delta = (want - self.facing.facing - rotation + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
                - std::f32::consts::PI;
            self.startling = Some((self.facing.facing, delta));
            eprintln!("perception: startled ({kind})");
        }
    }

    /// While startled: turn by the startle's share for this frame. Returns
    /// whether it is still startled.
    pub fn startle_turn(&mut self) -> bool {
        let Some((start, delta)) = self.startling else { return false };
        let startle = matches!(
            oni_schema::name(&oni_schema::ANIM_TYPES, self.motion.kind),
            Some("startleForward" | "startleLeft" | "startleBack" | "startleRight")
        );
        if !startle {
            self.startling = None;
            return false;
        }
        let t = f32::from(self.motion.frame) / f32::from(self.motion.length().max(1));
        self.facing.facing = (start + delta * t).rem_euclid(std::f32::consts::TAU);
        self.facing.desired_facing = self.facing.facing;
        true
    }

    /// Applies a melee AI's decision: turns toward where it wants to face (at
    /// the AI turn rate, not mid-reaction or mid-throw) and, for a fresh
    /// attack, forgets the combo in progress.
    pub fn follow(&mut self, d: &crate::ai::melee::brain::Output) {
        let c = self.motion.current();
        let locked = reaction::is_victim(self.motion.kind)
            || c.has_flag(flag::THROW_SOURCE)
            || c.has_flag(flag::THROW_TARGET);
        if let (Some(yaw), false) = (d.face, locked) {
            let cap = crate::ai::MAX_TURN;
            let delta = (yaw - self.facing.facing + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
                - std::f32::consts::PI;
            self.facing.rotate(delta.clamp(-cap, cap));
            self.facing.desired_facing = self.facing.facing;
        }
        if d.fresh_attack {
            self.motion.last_attack = ty::NONE;
        }
    }

    /// Keeps `throwing` in step with the clips: any new clip ends a throw this
    /// fighter was carrying out, unless it is itself the start of one — which
    /// also turns the thrower to face its victim squarely.
    fn track_throw(&mut self) {
        if self.motion.new_clip {
            self.throwing = self.motion.throw_started.map(|t| t.target);
        }
        if let Some(t) = self.motion.throw_started.as_mut() {
            let turn = std::mem::take(&mut t.turn);
            if turn != 0.0 {
                self.facing.rotate(turn);
                self.facing.adjust_desired(turn);
            }
        }
    }

    /// Being thrown by fighter `by`: plays the victim clip `clip` of the
    /// thrower's collection `coll`, moved to `dest` (sliding against walls on
    /// the way) and turned to `facing`.
    pub fn be_thrown(
        &mut self,
        by: usize,
        coll: Rc<AnimCollection>,
        clip: usize,
        fight: bool,
        dest: Vec3,
        facing: f32,
    ) {
        let ch = self.character.clone();
        self.motion.thrown(&ch, coll, clip, fight);
        self.knockback = Vec3::ZERO;
        self.physics.slide(dest - self.physics.feet);
        self.facing = Facing::new(facing);
        self.thrown_by = Some(by);
        self.throwing = None;
        self.replace();
    }

    /// The second half of a tick: apply the (possibly adjusted) move against
    /// the level, detect landing / ledges, and update the placement.
    pub fn commit(&mut self, world: Vec3) {
        let before = self.physics.feet;
        let grounded = self.physics.slide(world);
        if grounded {
            if let Some(t) = self.physics.floor_triangle(2.0) {
                self.last_floor = Some(t);
            }
        }
        self.velocity = self.physics.feet - before;
        if world.y < 0.0 {
            self.touch_ground(grounded);
        }

        self.replace();
        if std::mem::take(&mut self.motion.new_clip) {
            self.hit_set.clear();
            self.started = Some(self.motion.clip);
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
            if self.invincible {
            return;
        }
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
    /// the nearest-in-angle non-friend in range and in front — if that one
    /// is an enemy (a better-centred neutral stops it).
    fn face_opponent(&mut self, opponents: &[Opponent]) {
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
            .filter(|o| o.feet.distance(me) <= AUTO_FACE_RANGE)
            .map(|o| {
                let d = o.feet - me;
                let toward = d.x.atan2(d.z);
                ((toward - aim + PI).rem_euclid(TAU) - PI, o.hostile)
            })
            .filter(|(a, _)| a.abs() <= AUTO_FACE_ARC)
            .min_by(|a, b| a.0.abs().total_cmp(&b.0.abs()));
        if let Some((delta, true)) = best {
            self.facing.rotate(delta);
            self.facing.adjust_desired(delta);
        }
    }

    /// One poisoning at tick `now` (behaviour per `docs/COMBAT.md`, *Poison*):
    /// every call counts; at `initial` it takes `amount` and counts again
    /// from `initial - every`; 30 ticks without one and the count restarts.
    pub fn poison(&mut self, now: u64, amount: i32, every: i32, initial: i32) {
        const FORGET: u64 = 30;
        if self.poison_last + FORGET < now {
            self.poison_count = 0;
        }
        self.poison_last = now;
        self.poison_count += 1;
        if self.poison_count >= initial {
            if !self.invincible {
                self.take_damage(amount, Vec3::ZERO);
            }
            self.poison_count = initial - every;
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
        if damage > 0 {
            // The least pain the blow calls for: heavy blown up, medium
            // knocked down or landing dead.
            let least = match oni_schema::name(&oni_schema::ANIM_TYPES, m.kind).unwrap_or("") {
                "blownup" | "blownupBehind" => 2,
                "landDead" | "knockdownHead" | "knockdownBody" | "knockdownFoot" | "knockdownHeadBehind" | "knockdownBodyBehind"
                | "knockdownFootBehind" => 1,
                _ => 0,
            };
            self.hurts.push((damage as u32, least, self.hp <= damage));
        }
        self.hp = (self.hp - damage).max(0);
        if !self.unstoppable {
            self.knockback += knockback;
        }
        if self.hp == 0 {
            self.motion.dying = true;
            // Killed mid-throw on the way to the floor: the throw plays out.
            let c = self.motion.current();
            if c.has_flag(flag::THROW_TARGET) && matches!(c.to_state, st::FALLEN_BACK | st::FALLEN_FRONT) {
                self.pending_reaction = ty::NONE;
                eprintln!("fighter died (thrown)");
                return;
            }
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
    /// `target` is where its opponent stands, if it has a living one.
    pub fn npc_controls(&mut self, target: Option<Vec3>) -> u32 {
        use crate::anim::types::st;
        if self.motion.dying {
            return 0;
        }
        // Startled: only the turn plays out (no goals meanwhile).
        if self.startle_turn() {
            return 0;
        }
        let m = &self.motion;
        let senses = crate::ai::Senses {
            feet: self.physics.feet,
            lying: matches!(m.state, st::FALLEN_BACK | st::FALLEN_FRONT)
                && !reaction::is_victim(m.kind),
            reeling: reaction::is_victim(m.kind),
            attacking: !m.current().attacks.is_empty(),
            attacks_started: m.attacks_started,
            target,
            now: m.now,
        };
        match self.brain.as_mut() {
            Some(brain) => brain.think(&senses, &mut self.facing),
            None if self.passive => self.dummy_controls(),
            None => {
                // Glancing toward something it noticed: turn the body there.
                if let Some((at, until)) = self.glance {
                    if self.motion.now >= until {
                        self.glance = None;
                    } else {
                        let d = at - self.feet();
                        let want = d.x.atan2(d.z);
                        let cap = crate::ai::MAX_TURN;
                        let delta = (want - self.facing.facing + std::f32::consts::PI)
                            .rem_euclid(std::f32::consts::TAU)
                            - std::f32::consts::PI;
                        self.facing.rotate(delta.clamp(-cap, cap));
                        self.facing.desired_facing = self.facing.facing;
                    }
                }
                0
            }
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

    /// World-space bounding spheres of the body parts in the current pose:
    /// `(part index, centre, radius)`.
    pub fn body_spheres(&self) -> Vec<(usize, Vec3, f32)> {
        let bones = self.bones();
        self.part_spheres
            .iter()
            .map(|&(bone, c, r)| {
                let m = bones.get(bone).copied().unwrap_or(Mat4::IDENTITY);
                (bone, m.transform_point3(c), r)
            })
            .collect()
    }

    /// Whether the fighter is in the air.
    pub fn airborne(&self) -> bool {
        self.motion.air.frames > 0
    }

    /// The attacks in their damage window this frame (index within the clip).
    pub fn active_attacks(&self) -> Vec<(usize, crate::skel::Attack)> {
        self.motion.active_attacks()
    }

    /// Whether attack `k` of the current clip has already hit `victim`.
    pub fn already_hit(&self, k: usize, victim: usize) -> bool {
        self.hit_set.contains(&(k, victim))
    }

    /// Records that attack `k` of the current clip has hit `victim`.
    pub fn mark_hit(&mut self, k: usize, victim: usize) {
        self.hit_set.push((k, victim));
    }

    /// Rebuilds the world placement from the feet + pelvis height + facing.
    fn replace(&mut self) {
        let feet = self.physics.feet;
        let origin = Vec3::new(feet.x, feet.y + self.motion.height(), feet.z);
        self.placement =
            Mat4::from_rotation_translation(Quat::from_rotation_y(self.facing.facing), origin);
    }

    /// Posed bone world-matrices (placement baked in) for rendering: the
    /// body's clip, the overlays on it, and — holding a gun, when the clip
    /// allows — the aim pose (`docs/WEAPONS.md`, *Aim poses*).
    pub fn bones(&self) -> Vec<Mat4> {
        let mut layers = Vec::new();
        let mut dont_aim = false;
        for o in self.overlays.iter().flatten() {
            let c = self.motion.clip_of(o.clip);
            let frames = c.frames();
            // A six-frame ramp in and out.
            let weight = (f32::from(o.frame) / 6.0).min(f32::from(frames.saturating_sub(o.frame)) / 6.0).clamp(0.0, 1.0);
            dont_aim |= c.has_flag(flag::DONT_AIM);
            layers.push(crate::skel::Layer {
                rotations: self.character.local_rotations(Some(c), f32::from(o.frame) / crate::skel::FPS),
                used: c.used_parts,
                replace: c.replace_parts,
                weight,
            });
        }
        if let Some(l) = self.aim_layer(dont_aim) {
            layers.push(l);
        }
        self.character
            .pose_layered(Some(self.motion.current()), self.motion.frame_time(), self.motion.blend_pose(), &layers)
            .into_iter()
            .map(|m| self.placement * m)
            .collect()
    }

    /// The aim pose: holding a gun, in a clip that aims (not blocking, not
    /// reeling), no overlay forbidding it — the class's screen for the clip,
    /// bent toward the aim (yaw off the body, pitch with recoil).
    fn aim_layer(&self, dont_aim: bool) -> Option<crate::skel::Layer> {
        let c = self.motion.current();
        if self.hand.is_none() || dont_aim || !c.has_flag(flag::DO_AIM) || reaction::is_victim(c.anim_type) || !self.alive() {
            return None;
        }
        let s = crate::anim::aim::find(&self.aiming, c.aiming_type, c.varient)?;
        let wrap = |a: f32| (a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
        let yaw = wrap(self.facing.aim() - self.facing.facing).clamp(-std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2);
        let pitch = (self.aiming_ud + self.recoil).clamp(-std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2);
        if yaw == 0.0 && pitch == 0.0 {
            return None;
        }
        Some(crate::anim::aim::layer(&self.character, s, yaw, pitch))
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

    /// The animation types it is playing: its body's, and its overlays'.
    pub fn anim_types(&self) -> Vec<u16> {
        let mut v = vec![self.motion.kind];
        v.extend(self.overlays.iter().flatten().map(|o| self.motion.clip_of(o.clip).anim_type));
        v
    }

    /// Reloading (a reload clip in the inventory overlay).
    pub fn reloading(&self) -> bool {
        self.overlays[INVENTORY].is_some_and(|o| (164..=171).contains(&self.motion.clip_of(o.clip).anim_type))
    }

    pub fn alive(&self) -> bool {
        self.hp > 0 && !self.motion.dying && !self.gone
    }
}

/// Someone an attack may turn toward: not a friend (`hostile`: an enemy —
/// only an enemy is turned to).
#[derive(Clone, Copy, Debug)]
pub struct Opponent {
    pub feet: Vec3,
    pub hostile: bool,
}

/// A film being played on a fighter.
#[derive(Clone, Debug)]
pub struct FilmPlay {
    pub film: std::rc::Rc<crate::ai::level::Film>,
    pub frame: u32,
    /// Easing into the film's start: a step per tick, ticks left.
    pub interp: Option<(Vec3, u32)>,
}

impl FilmPlay {
    /// This tick's controls and turn (the original's `ONrFilm_GetFrame`):
    /// the buttons of the last key at or before this tick; its turn only on
    /// the key's own tick. None once the film is over.
    pub fn next(&mut self) -> Option<(u32, f32)> {
        if self.frame >= self.film.length {
            return None;
        }
        let keys = &self.film.keys;
        let k = keys.iter().rposition(|k| k.0 <= self.frame).unwrap_or(0);
        let out = keys.get(k).map(|&(t, b, turn)| {
            (crate::anim::input::from_original_buttons(b), if t == self.frame { turn } else { 0.0 })
        });
        self.frame += 1;
        Some(out.unwrap_or((0, 0.0)))
    }
}

/// One bounding sphere per body part (bone), in the bone's own space: the
/// sphere stored with the part's geometry when the model carries it (what Oni
/// tests), else one computed around the part's vertices.
fn part_spheres(ch: &Character) -> Vec<(usize, Vec3, f32)> {
    let mut out: Vec<(usize, Vec3, f32)> = Vec::new();
    for bone in 0..ch.bones.len() {
        let parts: Vec<_> = ch.parts.iter().filter(|p| p.bone == bone).collect();
        if parts.is_empty() {
            continue;
        }
        if let Some((c, r)) = parts.iter().find_map(|p| p.bound_sphere) {
            out.push((bone, c, r));
            continue;
        }
        let pts: Vec<Vec3> = parts
            .iter()
            .flat_map(|p| p.vertices.iter().map(|v| Vec3::from_array(v.pos)))
            .collect();
        if pts.is_empty() {
            continue;
        }
        let (mn, mx) = pts.iter().fold(
            (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
            |(a, b), p| (a.min(*p), b.max(*p)),
        );
        let c = (mn + mx) * 0.5;
        let r = pts.iter().map(|p| p.distance(c)).fold(0.0, f32::max);
        out.push((bone, c, r));
    }
    out
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
