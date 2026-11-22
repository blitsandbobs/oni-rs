//! The bosses' own fighting (`Oni_AI2_Combat.c`, the Barabbas, super ninja
//! and Mutant Muro behaviours): each a little mode
//! machine on top of the combat layer, choosing its primary movement's
//! weights, how it aims and fires, and reacting to being hurt or knocked
//! down.

use super::goals::{Agent, Ctx};

/// The primary movements' weight slots (`AI2cPrimaryMovement_*`).
pub(super) mod pm {
    pub const HOLD: usize = 0;
    pub const ADVANCE: usize = 1;
    pub const RETREAT: usize = 2;
    pub const FIND_ALARM: usize = 3;
    pub const MELEE: usize = 4;
    pub const GETUP: usize = 5;
    pub const GUN: usize = 6;
}

/// How often the combat chooses its movement (`AI2cCombat_UpdateFrames`).
const UPDATE: u32 = 5;

// Barabbas (`AI2cBarabbas_*`).
const STATIONARY_BEAM: u32 = 180;
const ADVANCING_BEAM: u32 = 360;
const ADVANCING_BEAM_DELTA: u32 = 90;
const REGEN_HURT_DISABLE_MAX: u32 = 180;
const REGEN_HURT_DISABLE_MIN: u32 = 90;
const REGEN_DAMAGE_STOP: f32 = 5.0;
const HEALTH_CRITICAL: f32 = 15.0;
const REGEN_HEALTH: f32 = 40.0;
const REGEN_FULL: f32 = 80.0;
const REGEN_RESTART_DISABLE: u32 = 240;
const REGEN_ABORT_DISTANCE: f32 = 20.0;
const REGEN_MIN_DISTANCE: f32 = 40.0;
const REGEN_FLEE_REQ_DISTANCE: f32 = 80.0;
const REGEN_HEAL_PER_SECOND: i32 = 8;
const REGEN_CANT_FLEE_DISABLE: u32 = 600;
const REGEN_MAX_POOL: i32 = 100;
const FLEE_MAX: u32 = 240;
const FLEE_MIN_DIST: f32 = 30.0;
const FLEE_MAX_DIST: f32 = 100.0;
const GUN_MIN_DIST: f32 = 100.0;
const GUN_MAX_DIST: f32 = 350.0;
const MELEE_INSTEAD_OF_GUN: f32 = 30.0;

// The super ninja (`AI2cSuperNinja_*`).
const SN_UNSUCCESSFUL: u32 = 300;
const SN_FIREBALL: u32 = 1200;
const SN_INVIS_MIN_DISTANCE: f32 = 40.0;
const SN_INVIS_DISABLE: u32 = 1200;
const SN_INVISIBILITY: u16 = 10 * 60;
pub const SN_FALL_HEIGHT: f32 = 550.0;
const SN_FALL_FRAMES: u32 = 150;
const SN_DAMAGE_DECAY: u32 = 3;
const SN_DAMAGE_DECAY_FRAMES: u32 = 30;
const SN_DAMAGE_STAY: u32 = 240;
const SN_FLEE_DAMAGE: u32 = 35;
const SN_FLEE_RANGE: f32 = 50.0;
const SN_ADVANCE_RANGE: f32 = 80.0;
const SN_ADVANCE_CHANCE: f32 = 0.5;
const SN_BEHIND_DISTANCE: f32 = 35.0;
const SN_BEHIND_MIN_DISTANCE: f32 = 15.0;
const SN_EDGE_RANDOM_ANGLE: f32 = 30.0 * std::f32::consts::PI / 180.0;
const SN_EDGE_MOVE_DISTANCE: f32 = 40.0;
const SN_RETREAT_ANGLE_DELTA: f32 = 15.0 * std::f32::consts::PI / 180.0;
const SN_RETREAT_RANGE: f32 = 200.0;
const SN_RETREAT_RANGE_DELTA: f32 = 100.0;
const SN_EDGE_FLAG_BASE: i16 = 9000;
const SN_RETREAT_FLAG_BASE: i16 = 9500;

/// What a boss is doing (`AI2tBehaviorState_*`).
#[derive(Clone, Debug, Default)]
pub enum Boss {
    #[default]
    None,
    Barabbas(Barabbas),
    SuperNinja(SuperNinja),
    MutantMuro(MutantMuro),
}

// Mutant Muro (`AI2cMutantMuro_*`).
const MM_ZEUS_CONTINUE: u32 = 120;
const MM_ZEUS_DISABLE: u32 = 300;
const MM_ZEUS_TARGET_DISTANCE: f32 = 60.0;
const MM_ZEUS_DISABLE_DISTANCE: f32 = 40.0;
const MM_ZEUS_MAXIMUM: u32 = 720;
const MM_SHIELD_REGEN: u32 = 240;
const MM_SHIELD_ZEUS_DECAY: u32 = 90;
const MM_SHIELD_ZEUS_DISABLE: u32 = 600;
const MM_SHIELD_ATTACK_DECAY: u32 = 30;
const MM_SHIELD_ATTACK_DISABLE: u32 = 360;
const MM_SHIELD_REQUIRED: u32 = 360;
const MM_DAMAGE_STOP_ZEUS: u32 = 35;
const MM_DAMAGE_TURTLE: u32 = 11;
const MM_TURTLE_FORGET: u32 = 75;
const MM_TURTLE_DISTANCE: f32 = 30.0;

/// Mutant Muro's state (`AI2tBehaviorState_MutantMuro`): his thunderbolt's
/// timers, his super shield's, and the hurt that makes him turtle.
#[derive(Clone, Debug, Default)]
pub struct MutantMuro {
    beam_maximum: u32,
    beam_continue: u32,
    beam_disable: u32,
    shield_decay_max: u32,
    shield_decay: u32,
    shield_disable: u32,
    shield_regen: u32,
    shield_regen_max: u32,
    shield_active: u32,
    total_damage: u32,
    damage_decay: u32,
    seen_clip: u32,
}

impl MutantMuro {
    /// His shield drops (`AI2iMutantMuro_DisableShield`): decaying over
    /// `decay` ticks from where it is, then off for `disable`.
    fn disable_shield(&mut self, decay: u32, disable: u32) {
        if self.shield_disable > 0 || self.shield_decay > 0 {
            self.shield_disable = self.shield_disable.max(disable);
            if self.shield_decay > 0 {
                self.shield_decay = self.shield_decay * decay / self.shield_decay_max.max(1);
                self.shield_decay_max = decay;
            }
        } else if self.shield_regen > 0 {
            self.shield_decay = self.shield_regen * decay / self.shield_regen_max.max(1);
            self.shield_decay_max = decay;
            self.shield_disable = disable;
        } else {
            self.shield_decay = decay;
            self.shield_decay_max = decay;
            self.shield_disable = disable;
        }
        self.shield_active = 0;
    }
}

/// The super ninja's modes (`AI2cSuperNinja_*`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NinjaMode {
    #[default]
    None,
    Melee,
    Fireball,
    Advance,
    Invisible,
    TeleportFlee,
    TeleportAdvance,
}

/// The super ninja's state (`AI2tBehaviorState_SuperNinja`).
#[derive(Clone, Debug, Default)]
pub struct SuperNinja {
    pub mode: NinjaMode,
    prev: NinjaMode,
    disable_fireball: u32,
    disable_invis: u32,
    disable_teleport: u32,
    fireball_started: bool,
    invis_started: bool,
    teleport_started: bool,
    medium_set: bool,
    medium_teleport: bool,
    flee_ticks: u32,
    flee_stay: u32,
    flee_damage: u32,
    fall_timer: u32,
    /// The clip it last heard begin (to notice a new one).
    seen_clip: u32,
}

/// Barabbas' modes (`AI2cBarabbas_*`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BarabbasMode {
    #[default]
    None,
    ShootBeam,
    WaitForWeapon,
    // (Never chosen, as shipped: advancing is always with grenades.)
    #[allow(dead_code)]
    Advance,
    AdvanceGrenading,
    GetGun,
    Regenerate,
    Melee,
    Flee,
}

/// Barabbas' state (`AI2tBehaviorState_Barabbas`).
#[derive(Clone, Debug, Default)]
pub struct Barabbas {
    pub mode: BarabbasMode,
    prev: BarabbasMode,
    disable_beam: u32,
    regen_damage: u32,
    regen_disable: u32,
    fleeing: u32,
}

/// What a boss's targeting says (`AI2cCombatMessage_Targeting`).
#[derive(Clone, Copy, Debug)]
pub struct BossTargeting {
    pub aim: bool,
    pub shoot: bool,
    pub alternate: bool,
}

impl Agent {
    /// The boss state for this behaviour, set up as it begins
    /// (`AI2cCombatMessage_BeginBehavior`): a Barabbas behaviour after
    /// another of his keeps his state.
    fn barabbas(&mut self) -> &mut Barabbas {
        let k = self.combat();
        if !matches!(k.boss, Boss::Barabbas(_)) {
            k.boss = Boss::Barabbas(Barabbas::default());
        }
        match &mut k.boss {
            Boss::Barabbas(b) => b,
            _ => unreachable!(),
        }
    }

    /// `AI2iBehavior_BarabbasHurtDisableRegen`: hurt, he won't start
    /// regenerating for 90–180 ticks (longer the healthier he is).
    fn barabbas_hurt_disable(&mut self, cx: &Ctx) {
        let (hp, max) = (cx.body.hp.max(0) as u32, cx.body.max_hp.max(1) as u32);
        let b = self.barabbas();
        b.regen_disable =
            REGEN_HURT_DISABLE_MIN + hp * (REGEN_HURT_DISABLE_MAX - REGEN_HURT_DISABLE_MIN) / max;
    }

    /// `AI2iBehavior_BarabbasCheckRegen`: does he want to flee and
    /// regenerate? (Sets the mode if so.)
    fn barabbas_check_regen(&mut self, dist: f32, cx: &Ctx) -> bool {
        let health = 100.0 * cx.body.hp as f32 / cx.body.max_hp.max(1) as f32;
        let pool_used = self.regenerated >= REGEN_MAX_POOL * cx.body.max_hp / 100;
        let unkillable = cx.body.unkillable;
        let upright = cx.body.inactive_upright;
        let b = self.barabbas();
        if b.prev == BarabbasMode::Regenerate {
            // (The research lab's unkillable Barabbas has only so much.)
            if unkillable && pool_used {
                b.regen_disable = 10000;
                return false;
            }
            if 100.0 * b.regen_damage as f32 / cx.body.max_hp.max(1) as f32 >= REGEN_DAMAGE_STOP {
                b.regen_damage = 0;
                self.barabbas_hurt_disable(cx);
                return false;
            }
            if health > REGEN_FULL {
                b.regen_disable = REGEN_RESTART_DISABLE;
                return false;
            }
            if dist < REGEN_ABORT_DISTANCE {
                if health < HEALTH_CRITICAL {
                    b.mode = BarabbasMode::Flee;
                    return true;
                }
                return false;
            }
            b.mode = BarabbasMode::Regenerate;
            return true;
        }
        if unkillable && pool_used {
            return false;
        }
        if health > REGEN_HEALTH || b.regen_disable > 0 {
            return false;
        }
        let required = if b.mode == BarabbasMode::Flee {
            REGEN_FLEE_REQ_DISTANCE
        } else {
            REGEN_MIN_DISTANCE
        };
        if dist < required || !upright {
            b.mode = BarabbasMode::Flee;
            return true;
        }
        b.mode = BarabbasMode::Regenerate;
        true
    }

    /// The primary movement of a Barabbas behaviour (`AI2iBehavior_
    /// Barabbas*`, `AI2cCombatMessage_PrimaryMovement`), every 5 ticks: the
    /// specific behaviour's mode — shooting his beam or waiting for it
    /// (`barabbasShoot`), advancing with grenades between beams
    /// (`barabbasAdvance`), melee (`barabbasMelee`) — overridden by waiting
    /// for his beam to finish, fleeing to regenerate, or going for his gun;
    /// its weights into `w`.
    pub(super) fn barabbas_primary(
        &mut self,
        behavior: &str,
        dist: f32,
        w: &mut [f32; 7],
        cx: &mut Ctx,
    ) {
        use pm::*;
        let firing = cx.gun.is_some_and(|g| g.firing);
        let alternate = self.alt_fire;
        let b = self.barabbas();
        // The specific behaviour's mode.
        match behavior {
            "barabbasShoot" => {
                b.mode = if b.disable_beam > 0 {
                    BarabbasMode::WaitForWeapon
                } else {
                    BarabbasMode::ShootBeam
                }
            }
            "barabbasAdvance" => {
                b.mode = if b.disable_beam > 0 {
                    BarabbasMode::AdvanceGrenading
                } else {
                    BarabbasMode::ShootBeam
                }
            }
            _ => {
                b.mode = if firing {
                    BarabbasMode::WaitForWeapon
                } else {
                    BarabbasMode::Melee
                }
            }
        }
        b.disable_beam = b.disable_beam.saturating_sub(UPDATE);
        b.regen_disable = b.regen_disable.saturating_sub(UPDATE);
        if cx.body.inactive_upright {
            if b.fleeing > UPDATE {
                b.fleeing -= UPDATE;
            } else if b.fleeing > 0 {
                // He couldn't get away: no regenerating for a while.
                b.fleeing = 0;
                b.regen_disable = REGEN_CANT_FLEE_DISABLE;
            }
        }
        let armed = cx.gun.is_some_and(|g| g.usable);
        if firing && !alternate {
            self.barabbas().mode = BarabbasMode::WaitForWeapon;
        } else if self.barabbas_check_regen(dist, cx) {
        } else if !armed {
            self.barabbas().mode = BarabbasMode::GetGun;
        }
        let b = self.barabbas();
        let mode = b.mode;
        // (He runs — `ai2_barabbas_run`, on as shipped.)
        let leaving_regen = mode != b.prev && b.prev == BarabbasMode::Regenerate;
        if mode != b.prev {
            match mode {
                BarabbasMode::Regenerate => {
                    b.regen_damage = 0;
                    b.fleeing = 0;
                }
                BarabbasMode::Flee => b.fleeing = FLEE_MAX,
                _ => {}
            }
            eprintln!("ai: {}: barabbas {:?}", self.name, mode);
        }
        self.barabbas().prev = mode;
        self.mode = super::goals::MoveMode::Run;
        if leaving_regen {
            // Out of the powerup.
            self.anim_request = Some((crate::anim::types::ty::STAND, true));
        }
        let k = self.combat();
        match mode {
            BarabbasMode::WaitForWeapon | BarabbasMode::ShootBeam => w[HOLD] = 1.0,
            BarabbasMode::Advance | BarabbasMode::AdvanceGrenading => {
                w[HOLD] = 0.3;
                w[ADVANCE] = 1.0;
            }
            BarabbasMode::GetGun => {
                if dist > MELEE_INSTEAD_OF_GUN {
                    k.gun_search = Some((GUN_MIN_DIST, GUN_MAX_DIST));
                    w[GUN] = 1.0;
                }
                w[MELEE] = 0.8;
            }
            BarabbasMode::Regenerate => {
                w[HOLD] = 1.0;
                if cx.body.powering_up {
                    let heal =
                        cx.body.max_hp * REGEN_HEAL_PER_SECOND / (100 * (60 / UPDATE as i32));
                    self.heal += heal;
                    self.regenerated += heal;
                }
                self.fight_mode = true;
                self.anim_request = Some((cx.body.powerup_type, false));
            }
            BarabbasMode::Flee => {
                w[HOLD] = 0.3;
                w[RETREAT] = 1.0;
                k.retreat = (FLEE_MIN_DIST, FLEE_MAX_DIST);
            }
            BarabbasMode::Melee => w[MELEE] = 1.0,
            BarabbasMode::None => {}
        }
    }

    /// Whether Barabbas ignores being too close for his gun now
    /// (`AI2cCombatMessage_TooClose`).
    pub(super) fn barabbas_ignores_too_close(&self) -> bool {
        matches!(
            self.boss_mode(),
            Some(
                BarabbasMode::WaitForWeapon
                    | BarabbasMode::GetGun
                    | BarabbasMode::Regenerate
                    | BarabbasMode::Flee
                    | BarabbasMode::Melee
            )
        )
    }

    pub fn boss_mode(&self) -> Option<BarabbasMode> {
        match self.combat_ref().map(|k| &k.boss) {
            Some(Boss::Barabbas(b)) => Some(b.mode),
            _ => None,
        }
    }

    /// His targeting (`AI2cCombatMessage_Targeting`): no aim meleeing,
    /// regenerating or fleeing; the beam shooting, grenades (the alternate
    /// fire) advancing; held fire otherwise.
    pub(super) fn barabbas_targeting(&self) -> Option<BossTargeting> {
        let mode = self.boss_mode()?;
        Some(match mode {
            BarabbasMode::Melee | BarabbasMode::Regenerate | BarabbasMode::Flee => BossTargeting {
                aim: false,
                shoot: false,
                alternate: false,
            },
            BarabbasMode::ShootBeam => BossTargeting {
                aim: true,
                shoot: true,
                alternate: false,
            },
            BarabbasMode::AdvanceGrenading => BossTargeting {
                aim: true,
                shoot: true,
                alternate: true,
            },
            _ => BossTargeting {
                aim: true,
                shoot: false,
                alternate: false,
            },
        })
    }

    /// He pulled the trigger (`AI2cCombatMessage_Fire`): a beam shot keeps
    /// the next beam off for a while — 3 s standing, 6–7.5 s advancing.
    pub(super) fn barabbas_fired(&mut self, behavior: &str, fired: bool, alternate: bool) {
        if !fired || alternate {
            return;
        }
        let r = self.rng.next();
        let b = self.barabbas();
        match behavior {
            "barabbasShoot" => b.disable_beam = STATIONARY_BEAM,
            "barabbasAdvance" => b.disable_beam = ADVANCING_BEAM + r % ADVANCING_BEAM_DELTA,
            _ => {}
        }
    }

    /// Hurt (`AI2cCombatMessage_Hurt`): while regenerating it adds up (enough
    /// stops him); otherwise he won't start for a while.
    pub(super) fn barabbas_hurt(&mut self, damage: u32, cx: &Ctx) {
        if self.boss_mode() == Some(BarabbasMode::Regenerate) {
            self.barabbas().regen_damage += damage;
        } else {
            self.barabbas_hurt_disable(cx);
        }
    }

    /// Knocked down (`AI2cCombatMessage_Fallen`): no regenerating soon.
    pub(super) fn barabbas_fallen(&mut self, cx: &Ctx) {
        self.barabbas_hurt_disable(cx);
    }
}

/// The point near `start` toward `direction` (`AI2rFindNearbyPoint`): a walk
/// `distance` out that gets there, trying `attempts` turns of `vary` each
/// side; else the longest walk if longer than `min` — none otherwise.
fn nearby_point(
    start: glam::Vec3,
    distance: f32,
    min: f32,
    direction: f32,
    vary: f32,
    attempts: u32,
    cx: &Ctx,
) -> Option<glam::Vec3> {
    let mut best: Option<(f32, glam::Vec3)> = None;
    for attempt in 0..=attempts {
        for side in [1.0f32, -1.0] {
            if attempt == 0 && side < 0.0 {
                continue;
            }
            let d = direction + attempt as f32 * vary * side;
            let dir = glam::Vec2::new(d.sin(), d.cos());
            let probe = (cx.probe)(start, dir, distance, crate::nav::Square::Border3)?;
            if probe.stopped.is_none() {
                return Some(start + glam::Vec3::new(dir.x, 0.0, dir.y) * distance);
            }
            let stop = start + glam::Vec3::new(dir.x, 0.0, dir.y) * probe.free;
            if best.is_none_or(|(f, _)| probe.free > f) {
                best = Some((probe.free, stop));
            }
        }
    }
    best.filter(|(f, _)| *f > min).map(|(_, p)| p)
}

/// The level's edge (`AI2iBehavior_FindEdgeFlag`): flags `base`, `base + 1`, …
/// make a closed outline; the nearest point on it to `at`, and the way in.
fn edge_flag(at: glam::Vec3, base: i16, cx: &Ctx) -> Option<(glam::Vec3, f32)> {
    let mut points = Vec::new();
    let mut id = base;
    while let Some(f) = cx.level.flags.get(&id) {
        points.push(f.at);
        id += 1;
    }
    let first = *points.first()?;
    points.push(first);
    let mut best: Option<(f32, glam::Vec3, f32)> = None;
    for w in points.windows(2) {
        let (a, b) = (w[0], w[1]);
        let (dx, dz) = (b.x - a.x, b.z - a.z);
        let t = ((at.x - a.x) * dx + (at.z - a.z) * dz) / (dx * dx + dz * dz);
        let p = if t <= 0.0 {
            a
        } else if t >= 1.0 {
            b
        } else {
            a * (1.0 - t) + b * t
        };
        let d = (p.x - at.x).powi(2) + (p.z - at.z).powi(2);
        if best.is_none_or(|(bd, _, _)| d < bd) {
            best = Some((d, p, dz.atan2(-dx).rem_euclid(std::f32::consts::TAU)));
        }
    }
    best.map(|(_, p, f)| (p, f))
}

/// A retreat flag (`AI2iBehavior_FindRetreatFlag`): of flags `base`, … the
/// one best placed `range` (± `range_delta`) from the enemy along `facing`
/// (± `facing_delta`).
fn retreat_flag(
    enemy: glam::Vec3,
    facing: f32,
    facing_delta: f32,
    range: f32,
    range_delta: f32,
    base: i16,
    cx: &Ctx,
) -> Option<glam::Vec3> {
    use std::f32::consts::PI;
    let mut best: Option<(f32, glam::Vec3)> = None;
    let mut id = base;
    while let Some(f) = cx.level.flags.get(&id) {
        let d = f.at - enemy;
        let angle = (crate::angle::yaw_of(d) - facing + PI).rem_euclid(2.0 * PI) - PI;
        let rd = (d.length() - range).abs();
        let mut score = if rd > range_delta {
            0.2
        } else {
            1.0 - 0.8 * rd / range_delta
        };
        score *= if angle.abs() > facing_delta {
            0.2
        } else {
            1.0 - 0.8 * angle.abs() / facing_delta
        };
        if best.is_none_or(|(s, _)| score > s) {
            best = Some((score, f.at));
        }
        id += 1;
    }
    best.map(|(_, p)| p)
}

impl Agent {
    fn ninja(&mut self) -> &mut SuperNinja {
        let k = self.combat();
        if !matches!(k.boss, Boss::SuperNinja(_)) {
            k.boss = Boss::SuperNinja(SuperNinja::default());
        }
        match &mut k.boss {
            Boss::SuperNinja(n) => n,
            _ => unreachable!(),
        }
    }

    pub fn ninja_mode(&self) -> Option<NinjaMode> {
        match self.combat_ref().map(|k| &k.boss) {
            Some(Boss::SuperNinja(n)) => Some(n.mode),
            _ => None,
        }
    }

    /// Whether the super ninja's medium-range move is a teleport (rolled
    /// once until he next closes in: half the time) or invisibility.
    fn ninja_medium_teleport(&mut self) -> bool {
        let roll = (self.rng.next() % 65536) as f32 / 65536.0;
        let n = self.ninja();
        if !n.medium_set {
            n.medium_teleport = roll < SN_ADVANCE_CHANCE;
            n.medium_set = true;
        }
        n.medium_teleport
    }

    /// Each tick of a super ninja behaviour: his clips' news
    /// (`AI2cCombatMessage_NewAnimation`, `_ActionFrame`).
    pub(super) fn ninja_tick(&mut self, cx: &Ctx) {
        use crate::anim::types::ty;
        let b = cx.body;
        let n = self.ninja();
        if b.started != n.seen_clip {
            n.seen_clip = b.started;
            let kind = b.anim_type;
            if n.fireball_started {
                if kind != ty::NINJA_FIREBALL {
                    n.disable_fireball = SN_UNSUCCESSFUL;
                    n.fireball_started = false;
                }
            } else if n.invis_started {
                if kind != ty::NINJA_INVISIBLE {
                    n.disable_invis = SN_UNSUCCESSFUL;
                    n.invis_started = false;
                }
            } else if n.teleport_started || kind == ty::TELEPORT_OUT {
                // Teleported (or not).
                n.mode = NinjaMode::None;
                n.prev = NinjaMode::None;
                n.teleport_started = false;
                if kind == ty::TELEPORT_OUT {
                    n.flee_ticks = 0;
                    n.flee_damage = 0;
                    n.medium_set = false;
                } else {
                    n.disable_teleport = SN_UNSUCCESSFUL;
                }
            }
        }
        if b.action_frame {
            if b.anim_type == ty::NINJA_FIREBALL {
                n.disable_fireball = SN_FIREBALL;
                n.fireball_started = false;
            } else if b.anim_type == ty::NINJA_INVISIBLE {
                n.disable_invis = SN_INVIS_DISABLE;
                n.invis_started = false;
                self.give_invisibility = SN_INVISIBILITY;
            }
        }
    }

    /// Falling below 550 (`AI2cCombatMessage_FallDetected`): he hangs there
    /// for 150 ticks, then teleports back up to the level's edge (its edge
    /// flags), visible, straight into his teleport-out clip. True while it
    /// takes over his update.
    pub(super) fn ninja_fell(&mut self, cx: &Ctx) -> bool {
        if cx.feet.y >= SN_FALL_HEIGHT
            || !matches!(
                self.combat_ref().map(|k| &k.boss),
                Some(Boss::SuperNinja(_))
            )
        {
            return false;
        }
        let n = self.ninja();
        n.fall_timer += 1;
        if n.fall_timer < SN_FALL_FRAMES {
            self.hold_height = Some(SN_FALL_HEIGHT);
            return true;
        }
        let Some((edge, facing)) = edge_flag(cx.feet, SN_EDGE_FLAG_BASE, cx) else {
            return false;
        };
        let roll = (self.rng.next() % 65536) as f32 / 65536.0;
        let theta = facing + (roll * 2.0 - 1.0) * SN_EDGE_RANDOM_ANGLE;
        let dest = nearby_point(
            edge,
            SN_EDGE_MOVE_DISTANCE,
            0.0,
            theta,
            std::f32::consts::FRAC_PI_4 / 4.0,
            4,
            cx,
        )
        .unwrap_or(
            edge + glam::Vec3::new(facing.sin(), 0.0, facing.cos()) * (2.0 * SN_EDGE_MOVE_DISTANCE),
        );
        self.teleport_now = Some((dest, facing));
        self.ninja().fall_timer = 0;
        true
    }

    /// The primary movement of a super ninja behaviour (`AI2iBehavior_
    /// SuperNinja*`): fireballs from afar (`superNinjaFireball`), closing in
    /// (`superNinjaAdvance`), melee (`superNinjaMelee`) — overridden by a
    /// teleport under way, a teleport away when beaten about close in, a
    /// teleport in behind his enemy (or invisibility) when advancing.
    pub(super) fn ninja_primary(
        &mut self,
        behavior: &str,
        dist: f32,
        w: &mut [f32; 7],
        cx: &mut Ctx,
    ) {
        use pm::*;
        let b = cx.body;
        let n = self.ninja();
        match behavior {
            "superNinjaFireball" => {
                n.mode = if n.disable_fireball == 0 && b.inactive_upright {
                    NinjaMode::Fireball
                } else {
                    NinjaMode::Advance
                }
            }
            "superNinjaAdvance" => n.mode = NinjaMode::Advance,
            _ => n.mode = NinjaMode::Melee,
        }
        n.disable_fireball = n.disable_fireball.saturating_sub(UPDATE);
        n.disable_invis = n.disable_invis.saturating_sub(UPDATE);
        n.disable_teleport = n.disable_teleport.saturating_sub(UPDATE);
        // His hurt decays 3 every 30 ticks, once 4 s have passed.
        if n.flee_ticks == 0 {
            n.flee_ticks = SN_DAMAGE_DECAY_FRAMES / UPDATE;
            if n.flee_stay > SN_DAMAGE_DECAY_FRAMES {
                n.flee_stay -= SN_DAMAGE_DECAY_FRAMES;
            } else {
                n.flee_damage = n.flee_damage.saturating_sub(SN_DAMAGE_DECAY);
            }
        }
        n.flee_ticks -= 1;
        let invisible = b.invisibility > 0;
        // (As shipped this tests the mode just set by the behaviour above —
        // never a teleport — so a teleport under way is never "continued";
        // the teleport clips see it through.)
        let teleporting = matches!(n.mode, NinjaMode::TeleportFlee | NinjaMode::TeleportAdvance)
            && !b.fallen
            && !b.victim;
        let flee = !invisible
            && n.flee_damage >= SN_FLEE_DAMAGE
            && dist < SN_FLEE_RANGE
            && b.inactive_upright;
        let advancing = n.mode == NinjaMode::Advance;
        let fireball = n.mode == NinjaMode::Fireball;
        let invis_ok = !invisible
            && n.disable_invis == 0
            && b.inactive_upright
            && !fireball
            && dist >= SN_INVIS_MIN_DISTANCE;
        if teleporting {
            let n = self.ninja();
            n.mode = n.prev;
        } else if flee {
            self.ninja().mode = NinjaMode::TeleportFlee;
        } else if !invisible
            && advancing
            && dist >= SN_ADVANCE_RANGE
            && b.inactive_upright
            && self.ninja_medium_teleport()
        {
            self.ninja().mode = NinjaMode::TeleportAdvance;
        } else if invis_ok && !self.ninja_medium_teleport() {
            self.ninja().mode = NinjaMode::Invisible;
        }
        let target = self.combat().target;
        let enemy = cx.others.get(target).map(|o| (o.feet, o.facing));
        let name = self.name.clone();
        let n = self.ninja();
        if n.mode != n.prev {
            eprintln!("ai: {name}: super ninja {:?}", n.mode);
            match n.prev {
                NinjaMode::Fireball => n.fireball_started = false,
                NinjaMode::Invisible => n.invis_started = false,
                _ => {}
            }
            match (n.mode, enemy) {
                (NinjaMode::TeleportFlee, Some((at, facing))) => {
                    match retreat_flag(
                        at,
                        facing,
                        SN_RETREAT_ANGLE_DELTA,
                        SN_RETREAT_RANGE,
                        SN_RETREAT_RANGE_DELTA,
                        SN_RETREAT_FLAG_BASE,
                        cx,
                    ) {
                        Some(dest) => {
                            self.teleport_to = Some((
                                dest,
                                (at.x - dest.x)
                                    .atan2(at.z - dest.z)
                                    .rem_euclid(std::f32::consts::TAU),
                            ))
                        }
                        None => self.ninja().mode = NinjaMode::Melee,
                    }
                }
                (NinjaMode::TeleportAdvance, Some((at, facing))) => {
                    let behind = facing + std::f32::consts::PI;
                    match nearby_point(
                        at,
                        SN_BEHIND_DISTANCE,
                        SN_BEHIND_MIN_DISTANCE,
                        behind,
                        std::f32::consts::FRAC_PI_2 / 6.0,
                        6,
                        cx,
                    ) {
                        Some(dest) => {
                            self.teleport_to = Some((
                                dest,
                                (at.x - dest.x)
                                    .atan2(at.z - dest.z)
                                    .rem_euclid(std::f32::consts::TAU),
                            ))
                        }
                        None => self.ninja().mode = NinjaMode::Advance,
                    }
                }
                _ => {}
            }
        }
        let n = self.ninja();
        n.prev = n.mode;
        let mode = n.mode;
        use crate::anim::types::ty;
        match mode {
            NinjaMode::Fireball
            | NinjaMode::Invisible
            | NinjaMode::TeleportFlee
            | NinjaMode::TeleportAdvance => {
                w[HOLD] = 1.0;
                let (started, kind) = match mode {
                    NinjaMode::Fireball => (n.fireball_started, ty::NINJA_FIREBALL),
                    NinjaMode::Invisible => (n.invis_started, ty::NINJA_INVISIBLE),
                    _ => (n.teleport_started, ty::TELEPORT_IN),
                };
                if !started {
                    if b.standing_still {
                        self.fight_mode = true;
                        self.anim_request = Some((kind, false));
                        let n = self.ninja();
                        match mode {
                            NinjaMode::Fireball => n.fireball_started = true,
                            NinjaMode::Invisible => n.invis_started = true,
                            _ => n.teleport_started = true,
                        }
                    } else if b.fallen || b.victim {
                        let n = self.ninja();
                        match mode {
                            NinjaMode::Fireball => n.disable_fireball = SN_UNSUCCESSFUL,
                            NinjaMode::Invisible => n.disable_invis = SN_UNSUCCESSFUL,
                            _ => {
                                n.disable_teleport = SN_UNSUCCESSFUL;
                                n.mode = NinjaMode::Melee;
                                n.prev = NinjaMode::None;
                                w[MELEE] = 1.2;
                            }
                        }
                    }
                }
            }
            NinjaMode::Advance => w[ADVANCE] = 1.0,
            NinjaMode::Melee => {
                n.medium_set = false;
                w[MELEE] = 1.0;
            }
            NinjaMode::None => {}
        }
    }

    /// Hurt (`AI2cCombatMessage_Hurt`): it counts toward fleeing, and stays
    /// counted 4 s.
    pub(super) fn ninja_hurt(&mut self, damage: u32) {
        let n = self.ninja();
        n.flee_damage += damage;
        n.flee_stay = SN_DAMAGE_STAY;
    }
}

impl Agent {
    fn muro(&mut self) -> &mut MutantMuro {
        let k = self.combat();
        if !matches!(k.boss, Boss::MutantMuro(_)) {
            k.boss = Boss::MutantMuro(MutantMuro::default());
        }
        match &mut k.boss {
            Boss::MutantMuro(m) => m,
            _ => unreachable!(),
        }
    }

    pub(super) fn is_muro(&self) -> bool {
        matches!(
            self.combat_ref().map(|k| &k.boss),
            Some(Boss::MutantMuro(_))
        )
    }

    /// His thunderbolt holds him in it (`AI2cCombatMessage_SwitchBehavior`):
    /// no change of range band while it goes on.
    pub(super) fn muro_locked(&self, behavior: &str) -> bool {
        match self.combat_ref().map(|k| &k.boss) {
            Some(Boss::MutantMuro(m)) => {
                behavior == "mutantMuroThunderbolt" && m.beam_continue > 0 && m.beam_disable == 0
            }
            _ => false,
        }
    }

    /// His super moves wait on a shield up 6 s
    /// (`AI2cCombatMessage_ShieldCharging`).
    pub(super) fn muro_shield_charging(&self) -> bool {
        match self.combat_ref().map(|k| &k.boss) {
            Some(Boss::MutantMuro(m)) => m.shield_active < MM_SHIELD_REQUIRED,
            _ => false,
        }
    }

    /// Hurt: toward turtling (and out of the thunderbolt), forgotten 75
    /// ticks on.
    pub(super) fn muro_hurt(&mut self, damage: u32) {
        let m = self.muro();
        m.total_damage += damage;
        m.damage_decay = MM_TURTLE_FORGET;
    }

    /// Each tick: a clip that drops the shield as it begins
    /// (`AI2cCombatMessage_NewAnimation`, `ONcAnimFlag_DisableShield`).
    pub(super) fn muro_tick(&mut self, cx: &Ctx) {
        let b = cx.body;
        let m = self.muro();
        if b.started != m.seen_clip {
            m.seen_clip = b.started;
            if b.disables_shield {
                m.disable_shield(MM_SHIELD_ATTACK_DECAY, MM_SHIELD_ATTACK_DISABLE);
            }
        }
    }

    /// The primary movement of Mutant Muro's behaviours
    /// (`AI2iBehavior_MutantMuro{,Zeus,Melee}`): his shield's course, then
    /// the thunderbolt from afar (`mutantMuroThunderbolt`) or melee — turtling
    /// when hurt close in without his shield (`mutantMuroMelee`).
    pub(super) fn muro_primary(
        &mut self,
        behavior: &str,
        dist: f32,
        w: &mut [f32; 7],
        cx: &mut Ctx,
    ) {
        use crate::anim::types::ty;
        use pm::*;
        let b = cx.body;
        let have_los = self.combat().have_los;
        let m = self.muro();
        // His shield.
        if m.total_damage > 0 {
            if m.damage_decay > UPDATE {
                m.damage_decay -= UPDATE;
            } else {
                m.damage_decay = 0;
                m.total_damage = 0;
            }
        }
        let tick = |t: &mut u32| *t = t.saturating_sub(UPDATE);
        let amount = if m.shield_decay > 0 {
            let a = m.shield_decay as f32 / m.shield_decay_max.max(1) as f32;
            tick(&mut m.shield_decay);
            m.shield_active = 0;
            a
        } else if m.shield_disable > 0 {
            if m.shield_disable > UPDATE {
                m.shield_disable -= UPDATE;
            } else {
                m.shield_disable = 0;
                m.shield_regen = MM_SHIELD_REGEN;
                m.shield_regen_max = MM_SHIELD_REGEN;
            }
            m.shield_active = 0;
            0.0
        } else if m.shield_regen > 0 {
            let a = 1.0 - m.shield_regen as f32 / m.shield_regen_max.max(1) as f32;
            tick(&mut m.shield_regen);
            m.shield_active = 0;
            a
        } else {
            m.shield_active += UPDATE;
            1.0
        };
        tick(&mut m.beam_maximum);
        tick(&mut m.beam_continue);
        tick(&mut m.beam_disable);
        self.set_super = Some(amount);
        let m = self.muro();
        if behavior == "mutantMuroThunderbolt" {
            if b.fallen {
                w[GETUP] = 1.0;
                return;
            }
            let enable = dist > MM_ZEUS_TARGET_DISTANCE || have_los;
            let disable = dist < MM_ZEUS_DISABLE_DISTANCE || m.total_damage > MM_DAMAGE_STOP_ZEUS;
            if disable {
                m.beam_disable = MM_ZEUS_DISABLE;
            } else if enable {
                m.beam_continue = MM_ZEUS_CONTINUE;
            }
            if m.beam_maximum == 0 || m.beam_continue == 0 || m.beam_disable > 0 {
                if m.beam_maximum == 0 && m.beam_continue > 0 && m.beam_disable == 0 {
                    // Out of it by running too long: not again at once.
                    m.beam_disable = MM_ZEUS_DISABLE;
                }
                m.beam_maximum = MM_ZEUS_MAXIMUM;
                m.beam_continue = 0;
                m.total_damage = 0;
                m.damage_decay = 0;
                w[ADVANCE] = 1.0;
                return;
            }
            w[HOLD] = 1.0;
            if b.queued == ty::MURO_THUNDERBOLT
                || (!b.standing_still && b.anim_type != ty::MURO_THUNDERBOLT)
            {
                return;
            }
            if !b.has_thunderbolt {
                m.beam_disable = MM_ZEUS_DISABLE;
                return;
            }
            m.disable_shield(MM_SHIELD_ZEUS_DECAY, MM_SHIELD_ZEUS_DISABLE);
            self.fight_mode = true;
            self.anim_request = Some((ty::MURO_THUNDERBOLT, false));
            return;
        }
        m.beam_maximum = MM_ZEUS_MAXIMUM;
        m.beam_continue = 0;
        if amount > 0.5 {
            m.total_damage = 0;
            m.damage_decay = 0;
        } else if m.total_damage > MM_DAMAGE_TURTLE && dist < MM_TURTLE_DISTANCE {
            if b.fallen {
                w[GETUP] = 1.0;
            } else {
                w[HOLD] = 1.0;
                if b.standing_still {
                    // Turtle: out of any stun, into the crouch back.
                    m.total_damage = 0;
                    m.damage_decay = 0;
                    self.clear_stun = true;
                    self.fight_mode = true;
                    self.anim_request = Some((ty::CROUCH_BACK, false));
                }
            }
            return;
        }
        w[MELEE] = 1.0;
    }
}
