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
use crate::fighter::Fighter;
use crate::quirks::Quirks;
use crate::skel::Character;

/// Extra melee reach beyond the two capsules' radii (Oni units) — the distance
/// an outstretched punch/kick adds. A feel constant to tune on playtest.
const MELEE_REACH: f32 = 4.0;
/// The victim must be within this cosine of the attacker's facing to be hit
/// (`0.2` ≈ a ±78° frontal arc), so attacks only connect in front.
const FRONT_DOT: f32 = 0.2;
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

    /// Resolves melee hits between fighters for this tick (`docs/COMBAT.md` §3–4,
    /// minimal slice). For every fighter whose current attack is in its damage
    /// window, any live fighter in front and within reach that this swing has not
    /// already hit takes the attack's damage + a knockback impulse. One swing
    /// hits each victim once (the attacker's per-attack `hit_set`).
    ///
    /// The reach test is a capsule-distance + frontal-arc approximation (the kind
    /// Oni uses for AI reach), not yet the full per-bone geometry sweep; blocking
    /// and reaction-clip selection (stagger/knockdown) are follow-ups.
    pub fn resolve_hits(&mut self) {
        let n = self.fighters.len();
        // Pass 1 (reads only): collect (attacker, victim, damage, knockback).
        let mut events: Vec<(usize, usize, i32, Vec3)> = Vec::new();
        for a in 0..n {
            // A passive training dummy never deals damage, even mid-swing.
            if self.fighters[a].passive {
                continue;
            }
            let Some(atk) = self.fighters[a].active_attack() else {
                continue;
            };
            let fa = &self.fighters[a];
            let afeet = fa.feet();
            let ayaw = fa.facing.facing;
            let arad = fa.radius;
            let fwd = Vec3::new(ayaw.sin(), 0.0, ayaw.cos());
            for b in 0..n {
                if b == a || !self.fighters[b].alive() || self.fighters[a].already_hit(b) {
                    continue;
                }
                let fb = &self.fighters[b];
                let mut to = fb.feet() - afeet;
                to.y = 0.0;
                let dist = to.length();
                let reach = arad + fb.radius + MELEE_REACH;
                if dist < 1e-3 || dist > reach {
                    continue;
                }
                let dir = to / dist;
                if dir.dot(fwd) < FRONT_DOT {
                    continue;
                }
                events.push((a, b, atk.damage as i32, dir * atk.knockback * KNOCKBACK_SCALE));
            }
        }
        // Pass 2 (mutations): mark the hit on the attacker, damage the victim.
        for (a, b, damage, knockback) in events {
            self.fighters[a].mark_hit(b);
            self.fighters[b].hurt(damage, knockback);
            eprintln!(
                "hit: fighter {a} → {b} for {damage} (hp {})",
                self.fighters[b].hp
            );
        }
    }
}
