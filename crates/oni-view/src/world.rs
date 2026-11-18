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
use crate::anim::throw;
use crate::anim::types::{flag, var};
use crate::fighter::Fighter;
use crate::quirks::Quirks;
use crate::skel::Character;

/// Characters further apart than this are never tested for touching.
const CONTACT_RANGE: f32 = 25.0;
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
        // The player's strikes may become throws: tell it who it could throw.
        let sense = self.throw_sense(0);
        if let Some(p) = self.fighters.first_mut() {
            p.motion.throw_sense = sense;
        }
        let mut moves: Vec<Option<Vec3>> = Vec::with_capacity(self.fighters.len());
        for (i, f) in self.fighters.iter_mut().enumerate() {
            let others: Vec<Vec3> = feet.iter().filter(|(j, _)| *j != i).map(|(_, p)| *p).collect();
            let target = f
                .brain
                .as_ref()
                .and_then(|b| b.opponent())
                .and_then(|t| feet.iter().find(|(j, _)| *j == t && *j != i))
                .map(|(_, p)| *p);
            // The player's fire buttons become melee while unarmed (weapons
            // aren't modelled yet, #10).
            let held = if i == 0 {
                crate::anim::input::fire_as_melee(controls, false, false)
            } else {
                f.npc_controls(target)
            };
            moves.push(f.plan(held, &others));
        }
        self.collide(&mut moves);
        for (f, m) in self.fighters.iter_mut().zip(moves) {
            if let Some(m) = m {
                f.commit(m);
            }
        }
        self.start_throws();
        for f in &mut self.fighters {
            f.apply_reaction();
        }
    }

    /// What fighter `me` sees of who it could throw: everyone alive except
    /// the knockdown-resistant and anyone in a wholly invulnerable clip.
    fn throw_sense(&self, me: usize) -> Option<throw::Sense> {
        let f = self.fighters.get(me)?;
        let candidates = self
            .fighters
            .iter()
            .enumerate()
            .filter(|&(j, o)| {
                j != me
                    && o.alive()
                    && !o.motion.dying
                    && !o.motion.corpse
                    && !o.motion.tuning.knockdown_resistant
                    && !o.motion.current().has_flag(flag::INVULNERABLE)
            })
            .map(|(j, o)| throw::Candidate {
                who: j,
                offset: o.feet() - f.feet(),
                facing: o.facing.facing,
                state: o.motion.state,
                varient: o.motion.varient,
                invulnerable: o.motion.invulnerable(),
            })
            .collect();
        Some(throw::Sense {
            aim: f.facing.aim(),
            facing: f.facing.facing,
            candidates,
        })
    }

    /// Carries out the throws begun this tick: each victim starts its side of
    /// the throw, placed where the throw data puts it relative to the thrower
    /// and turned to match.
    fn start_throws(&mut self) {
        for a in 0..self.fighters.len() {
            let Some(t) = self.fighters[a].motion.throw_started.take() else {
                continue;
            };
            let fa = &self.fighters[a];
            let c = fa.motion.current();
            let Some(info) = c.throw else { continue };
            let fight = c.varient & var::FIGHT != 0;
            let dest = fa.feet() + glam::Quat::from_rotation_y(fa.facing.facing) * info.target_offset;
            let facing = fa.facing.facing + info.target_facing;
            let coll = fa.motion.coll.clone();
            let clip = coll.entries[t.target_clip].name.clone();
            let victim = &mut self.fighters[t.target];
            if victim.alive() && !victim.motion.dying {
                victim.be_thrown(a, coll, t.target_clip, fight, dest, facing);
                eprintln!("throw: fighter {a} throws {} ({clip})", t.target);
            } else {
                self.fighters[a].throwing = None;
            }
        }
    }

    /// This tick's body contact between characters (behaviour per
    /// `docs/COMBAT.md`). For each pair within `CONTACT_RANGE` — skipping
    /// bodies lying in death — the body parts are tested against each other
    /// by their bounding spheres, as Oni does. If any touch:
    ///
    /// * **contact** — each one's planned move loses whatever part of it heads
    ///   toward the other (nobody is pushed; pairs almost on top of each other
    ///   are left alone);
    /// * **blows** — an attack in its damage window connects when a part of the
    ///   other body touches one of the attack's live parts (its damage bits).
    ///   If both land blows, the higher precedence wins (both on a tie). Each
    ///   of a clip's attacks hits each victim once.
    fn collide(&mut self, moves: &mut [Option<Vec3>]) {
        let n = self.fighters.len();
        let spheres: Vec<Vec<(usize, Vec3, f32)>> =
            self.fighters.iter().map(|f| f.body_spheres()).collect();
        let mut blows: Vec<(usize, usize, usize, crate::skel::Attack)> = Vec::new();
        for i in 0..n {
            for j in i + 1..n {
                let (a, b) = (&self.fighters[i], &self.fighters[j]);
                if a.motion.corpse || b.motion.corpse {
                    continue;
                }
                // A thrower and its victim pass through each other, unhurt.
                if a.throwing == Some(j) || b.throwing == Some(i) {
                    continue;
                }
                if a.feet().distance_squared(b.feet()) > CONTACT_RANGE * CONTACT_RANGE {
                    continue;
                }
                // Which of i's parts touch any of j's, and vice versa.
                let (mut touch_i, mut touch_j) = (0u32, 0u32);
                for &(pa, ca, ra) in &spheres[i] {
                    for &(pb, cb, rb) in &spheres[j] {
                        if ca.distance_squared(cb) <= (ra + rb) * (ra + rb) {
                            touch_i |= 1 << pa;
                            touch_j |= 1 << pb;
                        }
                    }
                }
                if touch_i == 0 {
                    continue;
                }

                // Contact: no walking into each other.
                for (me, other) in [(i, j), (j, i)] {
                    let mut away = self.fighters[me].feet() - self.fighters[other].feet();
                    away.y = 0.0;
                    if away.length_squared() < 1.0 {
                        continue;
                    }
                    let away = away.normalize();
                    if let Some(m) = moves[me].as_mut() {
                        let into = m.dot(away);
                        if into < 0.0 {
                            *m -= away * into;
                        }
                    }
                }

                // Blows: each side's live parts against the other's body.
                let landed = |att: usize, vic: usize, touching: u32| {
                    let f = &self.fighters[att];
                    if f.passive || !f.alive() || !self.fighters[vic].alive() {
                        return (Vec::new(), 0);
                    }
                    let hits: Vec<_> = f
                        .active_attacks()
                        .into_iter()
                        .filter(|(k, atk)| atk.damage_bits & touching != 0 && !f.already_hit(*k, vic))
                        .collect();
                    let top = hits.iter().map(|(_, a)| a.precedence).max().unwrap_or(0);
                    (hits, top)
                };
                let (from_i, prec_i) = landed(i, j, touch_i);
                let (from_j, prec_j) = landed(j, i, touch_j);
                if !from_i.is_empty() && prec_i >= prec_j {
                    blows.extend(from_i.into_iter().map(|(k, atk)| (i, j, k, atk)));
                }
                if !from_j.is_empty() && prec_j >= prec_i {
                    blows.extend(from_j.into_iter().map(|(k, atk)| (j, i, k, atk)));
                }
            }
        }
        for (a, b, k, attack) in blows {
            self.fighters[a].mark_hit(k, b);
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
