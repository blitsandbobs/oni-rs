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

/// A noise (behaviour per `docs/PERCEPTION.md`, *Hearing*): its kind, where,
/// how far it carries for a hearer of acuity 1, who made it and whom it
/// concerns.
#[derive(Clone, Copy, Debug)]
pub struct Sound {
    pub stimulus: crate::ai::perception::Stimulus,
    pub at: Vec3,
    pub volume: f32,
    pub maker: Option<usize>,
    pub other: Option<usize>,
}

/// The length of the way between two points through the level's rooms
/// (straight within a room; through portal midpoints across rooms).
fn route_length(nav: &crate::nav::Navigation, from: Vec3, to: Vec3) -> Option<f32> {
    let (a, b) = (nav.room_at(from)?, nav.room_at(to)?);
    if a == b {
        return Some(from.distance(to));
    }
    let portals = crate::nav::plan::route(nav, a, from, b, to)?;
    let mut at = from;
    let mut len = 0.0;
    for (r, k) in portals {
        let mid = nav.rooms[r].portals[k].mid;
        len += at.distance(mid);
        at = mid;
    }
    Some(len + at.distance(to))
}

/// A spawnable character: its uploaded render model and the shared data a new
/// instance needs. Clone-cheap (two `Rc`s + an index).
#[derive(Clone)]
pub struct CharKind {
    pub model: usize,
    pub character: Rc<Character>,
    pub anims: Rc<AnimCollection>,
    /// The character class's tuning (fight stance, idle, jump, fall damage).
    pub tuning: ClassTuning,
    /// How its AI fights hand to hand, and how long it lies dazed (ticks).
    pub melee: Option<Rc<crate::ai::melee::profile::Profile>>,
    pub daze: (u32, u32),
    /// How it sees, hears and remembers.
    pub perception: crate::ai::perception::Tuning,
}

/// All fighters in the level + the static collision mesh new capsules resolve
/// against. Fighter 0 is the player by convention.
pub struct World {
    pub fighters: Vec<Fighter>,
    /// Reproduce the original's known bugs (`--faithful`)?
    pub quirks: Quirks,
    /// Who is fighting whom (attack slots).
    pub fights: crate::ai::melee::fight::Fights,
    /// How sharp the AI is.
    pub difficulty: crate::ai::melee::brain::Difficulty,
    /// The level's navigation data (rooms, portals, grids), if it has any.
    pub nav: Option<Rc<crate::nav::Navigation>>,
    /// Line of sight through the level's opaque geometry.
    pub sight: Option<crate::physics::Sight>,
    /// Noises made since the last perception pass.
    pub sounds: Vec<Sound>,
    now: i64,
    rng: u32,
    level_positions: Vec<[f32; 3]>,
    level_indices: Vec<u32>,
}

impl World {
    /// An empty world over the given level collision mesh.
    pub fn new(level_positions: Vec<[f32; 3]>, level_indices: Vec<u32>) -> World {
        World {
            fighters: Vec::new(),
            quirks: Quirks::FIXED,
            fights: Default::default(),
            difficulty: Default::default(),
            nav: None,
            sight: None,
            sounds: Vec::new(),
            now: 0,
            rng: 0x9E37_79B9,
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
        let mut f = f;
        f.melee_profile = kind.melee.clone();
        f.daze = kind.daze;
        if !player {
            f.perception = Some(crate::ai::perception::Perception::new(
                kind.perception,
                crate::ai::perception::Team::Syndicate,
            ));
        }
        self.fighters.push(f);
        self.fighters.len() - 1
    }

    /// Makes fighter `id` fight fighter `target` with its class's melee AI.
    /// Returns false when its class has no melee profile.
    pub fn fight(&mut self, id: usize, target: usize) -> bool {
        let Some(f) = self.fighters.get_mut(id) else { return false };
        let Some(profile) = f.melee_profile.clone() else { return false };
        let m = crate::ai::melee::brain::Melee::new(profile, target, f.daze, id as u32 * 7919 + 1);
        f.brain = Some(crate::ai::Brain::Melee(Box::new(m)));
        // It knows exactly where its target is (the original's "magic
        // awareness" of an ordered attack).
        let (team, now) = (self.fighters.get(target).map(|t| (t.team(), t.feet())), self.now as u64);
        if let (Some((team, feet)), Some(p)) = (team, self.fighters[id].perception.as_mut()) {
            p.force_target(target, team, feet, now);
        }
        true
    }

    /// A roll in `[0, 100)`.
    fn percent(&mut self) -> f32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x % 100) as f32
    }

    /// A noise someone made: who hears it is worked out at the next
    /// perception pass.
    pub fn sound(&mut self, stimulus: crate::ai::perception::Stimulus, at: Vec3, volume: f32, maker: Option<usize>, other: Option<usize>) {
        self.sounds.push(Sound { stimulus, at, volume, maker, other });
    }

    /// Perception (behaviour per `docs/PERCEPTION.md`): noises, sight, memory
    /// fading and alert easing for every perceiving character; then each
    /// acts on what it noticed.
    fn perceive(&mut self) {
        use crate::ai::perception::{forward, Notice, Strength, Stimulus};
        let now = self.now as u64;
        let n = self.fighters.len();
        let alive: Vec<bool> = self.fighters.iter().map(|f| f.alive() && !f.motion.dying).collect();
        let teams: Vec<_> = self.fighters.iter().map(|f| f.team()).collect();
        let feet: Vec<Vec3> = self.fighters.iter().map(|f| f.feet()).collect();
        let points: Vec<[Vec3; 3]> = self.fighters.iter().map(|f| f.sight_points()).collect();
        let faithful = self.quirks.faithful;

        // Deaths.
        for (d, &living) in alive.iter().enumerate() {
            if !living {
                for f in &mut self.fighters {
                    if let Some(p) = f.perception.as_mut() {
                        if p.about(d).is_some_and(|c| c.strength > Strength::Dead) {
                            p.died(d);
                        }
                    }
                }
            }
        }

        // Noises.
        let sounds = std::mem::take(&mut self.sounds);
        for s in &sounds {
            for i in 0..n {
                if !alive[i] || self.fighters[i].passive {
                    continue;
                }
                let Some(p) = self.fighters[i].perception.as_ref() else { continue };
                let combat = s.stimulus.combat_sound();
                let ignored = |w: Option<usize>| {
                    w.is_some_and(|w| !alive[w] || p.ignores(i, w, teams[w], self.fighters[w].player, combat))
                };
                let (ig1, ig2) = (ignored(s.maker), s.other.is_none() || ignored(s.other));
                if s.maker.is_some() && ig1 && ig2 {
                    continue;
                }
                let range = s.volume * p.tuning.hearing;
                if s.at.distance(feet[i]) > range {
                    continue;
                }
                let eye = points[i][1];
                let heard = self.sight.as_ref().is_none_or(|sg| sg.clear(s.at, eye))
                    || self.nav.as_ref().is_some_and(|nav| route_length(nav, s.at, feet[i]).is_some_and(|d| d <= range));
                if !heard {
                    continue;
                }
                let p = self.fighters[i].perception.as_mut().expect("perceiving");
                match s.maker {
                    Some(m) if !ig1 => {
                        let strength = if s.stimulus as u8 >= Stimulus::SoundMelee as u8 && s.at.distance(eye) <= 40.0 {
                            Strength::Definite
                        } else {
                            Strength::Strong
                        };
                        p.perceive(Some(m), Some(teams[m]), s.stimulus, strength, feet[m], 0, now);
                    }
                    None => p.perceive(None, None, s.stimulus, Strength::Strong, s.at, 0, now),
                    _ => {}
                }
                if let Some(o) = s.other.filter(|_| !ig2) {
                    let p = self.fighters[i].perception.as_mut().expect("perceiving");
                    p.perceive(Some(o), Some(teams[o]), s.stimulus, Strength::Strong, feet[o], 0, now);
                }
            }
        }

        // Sight.
        for i in 0..n {
            if !alive[i] || self.fighters[i].passive || self.fighters[i].perception.is_none() {
                continue;
            }
            let every = self.fighters[i].perception.as_ref().map_or(59, |p| p.look_every(now));
            if (i as u64 * 13) % every != now % every {
                continue;
            }
            let eye = points[i][1];
            let look = forward(self.fighters[i].facing.facing);
            for j in 0..n {
                if j == i || !alive[j] {
                    continue;
                }
                let p = self.fighters[i].perception.as_ref().expect("perceiving");
                if p.ignores(i, j, teams[j], self.fighters[j].player, false) {
                    continue;
                }
                let known = p.about(j).is_some_and(|c| c.strength == Strength::Definite);
                let aim = feet[j] + Vec3::Y * self.fighters[j].motion.height();
                let Some(stimulus) = p.tuning.sees(aim - eye, look, known, faithful) else { continue };
                let seen = points[j].iter().find(|&&pt| self.sight.as_ref().is_none_or(|s| s.clear(eye, pt)));
                let Some(&at) = seen else { continue };
                let strength = if stimulus == Stimulus::SightDefinite { Strength::Definite } else { Strength::Weak };
                let p = self.fighters[i].perception.as_mut().expect("perceiving");
                p.perceive(Some(j), Some(teams[j]), stimulus, strength, at, 0, now);
            }
        }

        // Memory fades (every 60 ticks); the alert eases when not fighting.
        for f in &mut self.fighters {
            let fighting = matches!(f.brain, Some(crate::ai::Brain::Melee(_)));
            if let Some(p) = f.perception.as_mut() {
                if now.is_multiple_of(60) {
                    p.decay(now, &|_| true);
                }
                if !fighting {
                    p.calm();
                }
            }
        }

        // Acting on it.
        for i in 0..n {
            let notices = match self.fighters[i].perception.as_mut() {
                Some(p) => std::mem::take(&mut p.notices),
                None => continue,
            };
            for notice in notices {
                match notice {
                    Notice::Combat(who) => {
                        // (Switching targets mid-fight is the combat layer's
                        // call, #4: a fighter keeps its fight.)
                        let fighting = matches!(&self.fighters[i].brain, Some(crate::ai::Brain::Melee(_)));
                        if !fighting && !self.fighters[i].passive && alive[who] {
                            eprintln!("perception: fighter {i} engages {who}");
                            self.fight(i, who);
                        }
                    }
                    Notice::Startle(at) => self.fighters[i].startle(at),
                    Notice::Glance(at, ticks) => self.fighters[i].glance = Some((at, now + u64::from(ticks))),
                }
            }
        }
    }

    /// Plans a path for every navigator that needs one. Other characters'
    /// squares count as obstructions (a 5×5 block minus its corners).
    fn plan_paths(&mut self) {
        let Some(nav) = self.nav.clone() else { return };
        let now = self.now as u64;
        let feet: Vec<Vec3> = self.fighters.iter().map(|f| f.feet()).collect();
        for i in 0..self.fighters.len() {
            let wants = matches!(&self.fighters[i].brain, Some(crate::ai::Brain::GoTo(n)) if n.wants_plan(now));
            if !wants {
                continue;
            }
            let (from, careful, goal) = match &self.fighters[i].brain {
                Some(crate::ai::Brain::GoTo(n)) => (feet[i], n.careful, n.goal),
                _ => continue,
            };
            let others: Vec<Vec3> = feet.iter().enumerate().filter(|&(j, _)| j != i).map(|(_, p)| *p).collect();
            let obstructed = |room: usize| {
                let mut set = std::collections::HashSet::new();
                let Some(g) = nav.rooms[room].grid.as_ref() else { return set };
                for p in &others {
                    if nav.room_at(*p) != Some(room) {
                        continue;
                    }
                    if let Some((ci, cj)) = g.cell_of(p.x, p.z) {
                        for dj in -2..=2i32 {
                            for di in -2..=2i32 {
                                if di.abs() == 2 && dj.abs() == 2 {
                                    continue;
                                }
                                set.insert((ci + di, cj + dj));
                            }
                        }
                    }
                }
                set
            };
            let plan = crate::nav::plan::plan(&nav, from, goal, &obstructed, careful);
            if let Some(crate::ai::Brain::GoTo(n)) = self.fighters[i].brain.as_mut() {
                n.take_plan(plan, now);
            }
        }
    }

    /// The melee AIs decide (all against the same snapshot of the world).
    fn melee_decisions(&mut self) -> Vec<Option<crate::ai::melee::brain::Output>> {
        use crate::ai::melee::brain::view;
        let n = self.fighters.len();
        let positions: Vec<Vec3> = self.fighters.iter().map(|f| f.feet()).collect();
        let mut outs = vec![None; n];
        for (i, out) in outs.iter_mut().enumerate() {
            let target = match &self.fighters[i].brain {
                Some(crate::ai::Brain::Melee(m)) if m.target < n && m.target != i => m.target,
                _ => continue,
            };
            if self.fighters[i].startle_turn() {
                *out = Some(Default::default()); // startled: just the turn
                continue;
            }
            let Some(crate::ai::Brain::Melee(mut m)) = self.fighters[i].brain.take() else {
                continue;
            };
            let nav = self.nav.as_deref();
            let me = view(&self.fighters, nav, i, target);
            let them = view(&self.fighters, nav, target, i);
            *out = Some(m.think(&me, &them, &mut self.fights, self.now, &positions, self.difficulty, self.quirks));
            self.fighters[i].brain = Some(crate::ai::Brain::Melee(m));
        }
        outs
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
        self.now += 1;
        self.perceive();
        self.plan_paths();
        let decisions = self.melee_decisions();
        // A melee AI ordering a throw may throw its target.
        for (i, d) in decisions.iter().enumerate() {
            let order = d.and_then(|d| d.throw);
            let target = match &self.fighters[i].brain {
                Some(crate::ai::Brain::Melee(m)) => m.target,
                _ => continue,
            };
            let sense = order.and_then(|kind| {
                let mut s = self.throw_sense(i)?;
                s.candidates.retain(|c| c.who == target);
                s.only = Some(kind);
                Some(s)
            });
            self.fighters[i].motion.throw_sense = sense;
        }
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
            let (held, pressed) = if i == 0 {
                (crate::anim::input::fire_as_melee(controls, false, false), 0)
            } else if let Some(d) = decisions[i] {
                f.follow(&d);
                (d.held | d.pressed, d.pressed)
            } else {
                (f.npc_controls(target), 0)
            };
            moves.push(f.plan(held, pressed, &others));
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
        // Noises made this tick.
        for i in 0..self.fighters.len() {
            if let Some((stimulus, volume)) = self.fighters[i].noise.take() {
                let at = self.fighters[i].feet();
                self.sound(stimulus, at, volume, Some(i), None);
            }
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
            only: None,
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
                let at = self.fighters[a].feet();
                self.sound(crate::ai::perception::Stimulus::SoundMelee, at, 150.0, Some(a), Some(t.target));
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

    /// A melee AI's chance (percent) to make a block it is in position for:
    /// its block skill, shading toward its group skill the more attackers
    /// share its fight. None for anyone else (the shared rule alone decides).
    fn block_skill(&self, b: usize) -> Option<f32> {
        let f = &self.fighters[b];
        let Some(crate::ai::Brain::Melee(m)) = &f.brain else { return None };
        let p = &m.profile;
        let n = self.fights.attackers(m.target).len().max(1) as f32;
        let skill = if n >= 4.0 {
            p.block_group
        } else {
            p.block_skill + (p.block_group - p.block_skill) * (n - 1.0) / 3.0
        };
        Some(skill * self.difficulty.block)
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

        // Hit while lying dazed: the AI gets up at once.
        if let Some(crate::ai::Brain::Melee(m)) = fb.brain.as_mut() {
            if fb.motion.state == crate::anim::types::st::FALLEN_BACK
                || fb.motion.state == crate::anim::types::st::FALLEN_FRONT
            {
                m.daze = m.daze.min(1);
            }
        }
        if fb.motion.invulnerable() {
            fb.take_damage(0, attack_dir);
            eprintln!("hit: fighter {a} → {b}: invulnerable");
            return;
        }

        let (attacker_team, attacker_feet) = (self.fighters[a].team(), self.fighters[a].feet());
        // Blocking: automatic for a defensive character facing the attacker
        // (an AI also has to pass its block-skill roll).
        let skill = self.block_skill(b);
        let roll = self.percent();
        let fb = &mut self.fighters[b];
        let blocks = skill.is_none_or(|p| roll < p)
            && attack.flags & strike::UNBLOCKABLE == 0
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
        // The victim knows who hit it.
        let now = self.now as u64;
        if let Some(p) = fb.perception.as_mut() {
            if p.team.regards(attacker_team) != crate::ai::perception::Relation::Friend || p.about(a).is_some() {
                use crate::ai::perception::{Stimulus, Strength};
                p.perceive(Some(a), Some(attacker_team), Stimulus::HitMelee, Strength::Definite, attacker_feet, damage, now);
            }
        }
        self.fights.damage(a, damage, true);
        self.fights.damage(b, damage, false);
        let fb = &mut self.fighters[b];
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
