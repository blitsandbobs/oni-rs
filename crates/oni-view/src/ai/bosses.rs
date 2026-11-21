//! The bosses' own fighting (`Oni_AI2_Combat.c`, the Barabbas, super ninja
//! and Mutant Muro behaviours; `docs/AI.md`, *Bosses*): each a little mode
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

/// What a boss is doing (`AI2tBehaviorState_*`).
#[derive(Clone, Debug, Default)]
pub enum Boss {
    #[default]
    None,
    Barabbas(Barabbas),
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
            Boss::None => unreachable!(),
        }
    }

    /// `AI2iBehavior_BarabbasHurtDisableRegen`: hurt, he won't start
    /// regenerating for 90–180 ticks (longer the healthier he is).
    fn barabbas_hurt_disable(&mut self, cx: &Ctx) {
        let (hp, max) = (cx.body.hp.max(0) as u32, cx.body.max_hp.max(1) as u32);
        let b = self.barabbas();
        b.regen_disable = REGEN_HURT_DISABLE_MIN + hp * (REGEN_HURT_DISABLE_MAX - REGEN_HURT_DISABLE_MIN) / max;
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
        let required = if b.mode == BarabbasMode::Flee { REGEN_FLEE_REQ_DISTANCE } else { REGEN_MIN_DISTANCE };
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
    pub(super) fn barabbas_primary(&mut self, behavior: &str, dist: f32, w: &mut [f32; 7], cx: &mut Ctx) {
        use pm::*;
        let firing = cx.gun.is_some_and(|g| g.firing);
        let alternate = self.alt_fire;
        let b = self.barabbas();
        // The specific behaviour's mode.
        match behavior {
            "barabbasShoot" => b.mode = if b.disable_beam > 0 { BarabbasMode::WaitForWeapon } else { BarabbasMode::ShootBeam },
            "barabbasAdvance" => b.mode = if b.disable_beam > 0 { BarabbasMode::AdvanceGrenading } else { BarabbasMode::ShootBeam },
            _ => b.mode = if firing { BarabbasMode::WaitForWeapon } else { BarabbasMode::Melee },
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
                    let heal = cx.body.max_hp * REGEN_HEAL_PER_SECOND / (100 * (60 / UPDATE as i32));
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
            Some(BarabbasMode::WaitForWeapon | BarabbasMode::GetGun | BarabbasMode::Regenerate | BarabbasMode::Flee | BarabbasMode::Melee)
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
            BarabbasMode::Melee | BarabbasMode::Regenerate | BarabbasMode::Flee => BossTargeting { aim: false, shoot: false, alternate: false },
            BarabbasMode::ShootBeam => BossTargeting { aim: true, shoot: true, alternate: false },
            BarabbasMode::AdvanceGrenading => BossTargeting { aim: true, shoot: true, alternate: true },
            _ => BossTargeting { aim: true, shoot: false, alternate: false },
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
