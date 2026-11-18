//! The **world**: the set of [`Fighter`]s sharing one level, plus the shared
//! resources needed to spawn more of them. Both front-ends (play mode and the
//! Lua harness) build a `World`, drive its fighters each tick, then call
//! [`World::resolve_hits`] so melee connects between characters.
//!
//! A spawnable character is a [`CharKind`]: an uploaded render model plus the
//! shared skeleton/mesh and animation set. Spawning clones the two `Rc`s and
//! builds a fresh physics capsule + state machine — cheap, so Lua can spawn a
//! training dummy (or several) at runtime.

use std::rc::Rc;

use glam::Vec3;

use crate::anim::collection::AnimCollection;
use crate::anim::motion::ClassTuning;
use crate::anim::types::flag;
use crate::fighter::Fighter;
use crate::quirks::Quirks;
use crate::skel::Character;

/// Extra melee reach beyond the two capsules' radii (Oni units) — the distance
/// an outstretched punch/kick adds. A feel constant to tune on playtest.
const MELEE_REACH: f32 = 4.0;
/// The victim must be within this cosine of the attacker's facing to be hit
/// (`0.2` ≈ a ±78° frontal arc), so attacks only connect in front.
const FRONT_DOT: f32 = 0.2;
/// A defensive character blocks attackers within this angle of where it faces.
const BLOCK_ANGLE: f32 = 20.0 * std::f32::consts::PI / 180.0;
/// Scales an attack's data `knockback` (total shove distance). `1.0` is the
/// original's behaviour: the shove is spent over the following ticks, a share
/// per tick (see `Fighter::tick`). A feel knob to revisit at playtest.
const KNOCKBACK_SCALE: f32 = 1.0;

/// A spawnable character: its uploaded render model and the shared data a new
/// instance needs. Clone-cheap (two `Rc`s + an index).
#[derive(Clone)]
pub struct CharKind {
    pub model: usize,
    pub character: Rc<Character>,
    pub anims: Rc<AnimCollection>,
    /// The character class's tuning (fight stance, idle, jump, fall damage).
    pub tuning: ClassTuning,
}

/// All fighters in the level + the static collision mesh new capsules resolve
/// against. Fighter 0 is the player by convention.
pub struct World {
    pub fighters: Vec<Fighter>,
    /// Reproduce the original's known bugs (`--faithful`)?
    pub quirks: Quirks,
    level_positions: Vec<[f32; 3]>,
    level_indices: Vec<u32>,
}

impl World {
    /// An empty world over the given level collision mesh.
    pub fn new(level_positions: Vec<[f32; 3]>, level_indices: Vec<u32>) -> World {
        World {
            fighters: Vec::new(),
            quirks: Quirks::FIXED,
            level_positions,
            level_indices,
        }
    }

    /// Spawns a fighter of `kind` with its pelvis origin at `pos`, facing
    /// `facing_yaw` (radians). `passive` marks a training dummy (no attacks).
    /// The first fighter spawned is the player. Returns the new fighter's index.
    pub fn spawn(&mut self, kind: &CharKind, pos: Vec3, facing_yaw: f32, passive: bool) -> usize {
        let player = self.fighters.is_empty();
        let f = Fighter::new(
            kind.model,
            kind.character.clone(),
            kind.anims.clone(),
            kind.tuning,
            &self.level_positions,
            &self.level_indices,
            pos,
            facing_yaw,
            passive,
            player,
            self.quirks,
        );
        self.fighters.push(f);
        self.fighters.len() - 1
    }

    /// Switches every fighter (and future spawns) between fixed and faithful
    /// handling of the original's known bugs.
    pub fn set_quirks(&mut self, quirks: Quirks) {
        self.quirks = quirks;
        for f in &mut self.fighters {
            f.quirks = quirks;
        }
    }

    /// The player fighter (index 0), if any.
    pub fn player(&self) -> Option<&Fighter> {
        self.fighters.first()
    }

    /// Advances one tick: the player (fighter 0) from `controls`, everyone
    /// else idle; then melee hits between them are resolved and the chosen
    /// reactions played.
    pub fn step(&mut self, controls: u32) {
        let feet: Vec<(usize, Vec3)> = self
            .fighters
            .iter()
            .enumerate()
            .filter(|(_, f)| f.alive())
            .map(|(i, f)| (i, f.feet()))
            .collect();
        for (i, f) in self.fighters.iter_mut().enumerate() {
            let others: Vec<Vec3> = feet.iter().filter(|(j, _)| *j != i).map(|(_, p)| *p).collect();
            let held = if i == 0 {
                controls
            } else if f.passive {
                f.dummy_controls()
            } else {
                0
            };
            f.tick(held, &others);
        }
        self.resolve_hits();
        for f in &mut self.fighters {
            f.apply_reaction();
        }
    }

    /// Resolves this tick's melee hits (`docs/COMBAT.md` §4). For every
    /// attacker in an attack's damage window, each live fighter in front and
    /// within reach that this swing hasn't touched yet is either:
    ///
    /// * **untouchable** — its clip is invulnerable now: only a small shove;
    /// * **blocking** — it stands (or crouches, or backs off) facing the
    ///   attacker within 20° and has a block that covers the attack's height:
    ///   no damage; it blocks (or staggers, for attacks that stagger through a
    ///   block). Attacks marked special break through at half strength;
    /// * **hit** — it takes the damage and the attack's shove (along the
    ///   attack's direction) and reacts: the attack's named reaction, a stagger
    ///   for heavy hits, a knockdown if already reeling, adapted to whether it
    ///   is airborne, crouched or down, and the behind variant when struck
    ///   from behind — turning to face (or away from) the attacker.
    ///
    /// The reach test is a capsule-distance + frontal-arc approximation, not
    /// the original's per-limb geometry test.
    pub fn resolve_hits(&mut self) {
        let n = self.fighters.len();
        let mut hits = Vec::new();
        for a in 0..n {
            let fa = &self.fighters[a];
            if fa.passive || !fa.alive() {
                continue;
            }
            let Some(attack) = fa.active_attack() else {
                continue;
            };
            let afeet = fa.feet();
            let fwd = Vec3::new(fa.facing.facing.sin(), 0.0, fa.facing.facing.cos());
            for b in 0..n {
                if b == a || !self.fighters[b].alive() || self.fighters[a].already_hit(b) {
                    continue;
                }
                let fb = &self.fighters[b];
                let mut to = fb.feet() - afeet;
                to.y = 0.0;
                let dist = to.length();
                if dist < 1e-3 || dist > fa.radius + fb.radius + MELEE_REACH {
                    continue;
                }
                if (to / dist).dot(fwd) < FRONT_DOT {
                    continue;
                }
                hits.push((a, b, attack));
            }
        }
        for (a, b, attack) in hits {
            self.fighters[a].mark_hit(b);
            self.land_hit(a, b, attack);
        }
    }

    /// Applies one landed attack from fighter `a` to fighter `b`.
    fn land_hit(&mut self, a: usize, b: usize, attack: crate::skel::Attack) {
        use crate::anim::reaction::{self, Victim};
        use crate::anim::types::{strike, ty};
        use std::f32::consts::{FRAC_PI_2, PI, TAU};

        let wrap = |x: f32| (x + PI).rem_euclid(TAU) - PI;
        let (attacker_feet, attack_dir) = {
            let fa = &self.fighters[a];
            let offset = match fa.motion.current().move_direction {
                2 => PI,
                3 => FRAC_PI_2,
                4 => -FRAC_PI_2,
                _ => 0.0,
            };
            let yaw = fa.facing.facing + offset;
            (fa.feet(), Vec3::new(yaw.sin(), 0.0, yaw.cos()))
        };
        let quirks = self.quirks;
        let fb = &mut self.fighters[b];
        let to_attacker = {
            let d = attacker_feet - fb.feet();
            d.x.atan2(d.z)
        };
        let mut damage = attack.damage as i32;
        let mut shove = attack.knockback * KNOCKBACK_SCALE;

        if fb.motion.invulnerable() {
            fb.take_damage(0, attack_dir);
            eprintln!("hit: fighter {a} → {b}: invulnerable");
            return;
        }

        // Blocking: automatic for a defensive character facing the attacker.
        let blocks = attack.flags & strike::UNBLOCKABLE == 0
            && fb.motion.hit_stun == 0
            && fb.motion.defensive()
            && wrap(to_attacker - fb.facing.aim()).abs() <= BLOCK_ANGLE
            && fb.motion.block_clip().is_some_and(|c| {
                let clip = fb.motion.clip_of(c);
                (attack.flags & strike::HIGH == 0 || clip.has_flag(flag::BLOCK_HIGH))
                    && (attack.flags & strike::LOW == 0 || clip.has_flag(flag::BLOCK_LOW))
            });
        if blocks {
            fb.facing.facing = to_attacker.rem_euclid(TAU);
            if attack.flags & strike::SPECIAL == 0 {
                fb.take_damage(0, attack_dir);
                if attack.stagger > 0 {
                    fb.pending_reaction = ty::STAGGER;
                    fb.motion.stagger_stun = attack.stagger;
                } else {
                    fb.please_block = true;
                    fb.motion.block_stun = attack.block_stun;
                }
                eprintln!("hit: fighter {a} → {b}: blocked");
                return;
            }
            // A special move breaks through at half strength.
            damage = (damage / 2).max(1);
            shove *= 0.5;
        }

        // A clean hit: choose the reaction.
        let reeling = fb.motion.stagger_stun > 0 || fb.motion.dizzy_stun > 0;
        let heavy = attack.hit_stun > 20;
        let mut stun = attack.hit_stun;
        let mut as_stagger = false;
        let mut kind = attack.damage_animation;
        if reeling && heavy {
            kind = reaction::to_knockdown(kind, quirks);
        } else if heavy {
            kind = ty::STAGGER;
            as_stagger = true;
        }
        if kind != ty::NONE {
            let victim = Victim {
                airborne: fb.airborne(),
                to_state: fb.motion.current().to_state,
                resistant: fb.motion.tuning.knockdown_resistant,
            };
            let mut chosen = reaction::adapt(kind, victim, quirks);
            if chosen != ty::HIT_OVERLAY {
                // (An overlay flinch — a resistant character shrugging off a
                // light hit — has no full-body clip; partial-body overlays
                // are not modelled yet.)
                if victim.resistant && chosen == ty::STAGGER && kind != ty::STAGGER {
                    as_stagger = true;
                    stun = stun.max(30);
                }
                let behind = wrap(to_attacker - fb.facing.facing).abs() > FRAC_PI_2;
                if behind {
                    chosen = reaction::from_behind(chosen);
                }
                fb.pending_reaction = chosen;
                if !matches!(chosen, ty::STUN | ty::HIT_FALLEN) {
                    let face = if reaction::is_behind(chosen) { to_attacker + PI } else { to_attacker };
                    fb.facing.facing = face.rem_euclid(TAU);
                }
            }
        }
        fb.take_damage(damage, attack_dir * shove);
        if as_stagger {
            fb.motion.stagger_stun = stun;
        } else {
            fb.motion.hit_stun = stun;
        }
        eprintln!(
            "hit: fighter {a} → {b} for {damage} (hp {}) reaction {}",
            fb.hp, fb.pending_reaction
        );
    }
}
