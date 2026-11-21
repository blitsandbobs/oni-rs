//! Shield belts and invisibility at work (#60; `docs/POWERUPS.md`): the
//! shield's glow on the body (`ONrCharacter_Shield_Update`, `_HitParts`),
//! the fade in and out of invisibility (`ONrCharacter_Invis_Update`), and
//! the shield's display amount and how long a character has been
//! invisible (`WPrInventory_Update`).

use glam::Vec3;

use super::World;

/// Body parts.
pub const PARTS: usize = 19;
/// A shield belt's most (`WPcMaxShields`); invisibility that never runs
/// out (the cheat's, `(UUtUns16) -1`).
pub const MAX_SHIELD: u16 = 100;
pub const ENDLESS: u16 = u16::MAX;
/// How fast the shield's display catches up with it (`WPcShieldDisplaySpeed`).
const SHIELD_DISPLAY_SPEED: u16 = 2;
/// The shield's glow: per point of shield, and a floor while there is any
/// (`shield_base_line`, `shield_bonus`); a part struck flares to full.
const SHIELD_BASE: i32 = 69;
const SUPER_SHIELD_BASE: i32 = 39;
const SHIELD_BONUS: i32 = 20;
const SHIELD_FADE: i32 = 7;
/// Invisibility: how fast the body fades out and back in, and how long
/// before it runs out it starts coming back.
const INVIS_FADE_OUT: f32 = 0.0085;
const INVIS_FADE_IN: f32 = 0.02;
const INVIS_COMING_BACK: u16 = 120;
/// How visible a hit or a throw leaves an invisible character for a
/// moment (`ONiCharacter_BecomeTemporarilyVisible`).
const REVEALED: f32 = 0.6;
/// How fast the super amount rises and falls (`ONcSuperAmount_*PerFrame`).
const SUPER_UP: f32 = 0.03;
const SUPER_DOWN: f32 = 0.06;
/// How long a character must have been invisible before the AI can't see
/// it (`AI2cKnowledge_InvisibleFrames`).
pub const UNSEEN_AFTER: u32 = 90;

/// A character's magic effects as drawn (`ONtActiveCharacter`'s).
#[derive(Clone, Copy, Debug)]
pub struct Magic {
    /// Each body part's shield glow (0–255).
    pub shield_parts: [u8; PARTS],
    pub shield_active: bool,
    /// The glow is a super shield's (red, `DAODAN_SHIELD`).
    pub daodan: bool,
    pub invis_active: bool,
    /// How visible the body is (1 seen, 0 gone).
    pub invis_alpha: f32,
}

impl Default for Magic {
    fn default() -> Magic {
        Magic { shield_parts: [0; PARTS], shield_active: false, daodan: false, invis_active: false, invis_alpha: 1.0 }
    }
}

impl Magic {
    /// `ONrCharacter_Shield_Update`: every part's glow eases toward what the
    /// shield shown calls for — down 7 a tick from a flare, up 1 a tick;
    /// a dead character's dies away (1 two ticks in three).
    pub fn shield_update(&mut self, display: u16, super_shield: Option<f32>, dead: bool, now: u64) {
        let (amount, base) = match super_shield {
            Some(s) => {
                self.daodan = true;
                ((s * f32::from(MAX_SHIELD)).round() as u16, SUPER_SHIELD_BASE)
            }
            None => {
                self.daodan = false;
                (display, SHIELD_BASE)
            }
        };
        if !self.shield_active && amount == 0 {
            return;
        }
        let mut global = base * i32::from(amount) / i32::from(MAX_SHIELD);
        if amount > 0 {
            global += SHIELD_BONUS;
        }
        let global = global.min(255);
        let mut active = false;
        for p in &mut self.shield_parts {
            let old = i32::from(*p);
            let new = if old > global {
                (old - SHIELD_FADE).max(global)
            } else if dead {
                if !now.is_multiple_of(3) {
                    (old - 1).max(0)
                } else {
                    old
                }
            } else if old < global {
                (old + 1).min(global)
            } else {
                old
            };
            active |= new > 0;
            *p = new as u8;
        }
        self.shield_active = active;
    }

    /// `ONrCharacter_Shield_HitParts`: the parts in `mask` flare to full
    /// (a shield already showing, or a boss's).
    pub fn shield_hit(&mut self, mask: u32, boss_shield: bool) {
        if !self.shield_active && !boss_shield {
            return;
        }
        self.shield_active = true;
        for (k, p) in self.shield_parts.iter_mut().enumerate() {
            if mask & (1 << k) != 0 {
                *p = 255;
            }
        }
    }

    /// `ONrCharacter_Invis_Update` (its alpha; the count is the caller's):
    /// turning invisible starts seen and fades out; its last 120 ticks it
    /// fades back in, and once fully back with none left it is over.
    pub fn invis_update(&mut self, remaining: u16) {
        if remaining > 0 && !self.invis_active {
            self.invis_alpha = 1.0;
            self.invis_active = true;
        } else if remaining > 0 || self.invis_active {
            let goal = if remaining > INVIS_COMING_BACK { 0.0 } else { 1.0 };
            if self.invis_alpha > goal {
                self.invis_alpha = (self.invis_alpha - INVIS_FADE_OUT).max(goal);
            } else if self.invis_alpha < goal {
                self.invis_alpha = (self.invis_alpha + INVIS_FADE_IN).min(goal);
            }
            // (SHIPPED: the test is for alpha 0 — a body faded back in to 1
            // stays "active", drawn through the blended path at full
            // alpha, until it next turns invisible. Harmless.)
            if self.invis_alpha == 0.0 && remaining == 0 {
                self.invis_active = false;
            }
        }
    }

    /// Seen for a moment: struck, striking or thrown.
    pub fn reveal(&mut self) {
        if self.invis_active {
            self.invis_alpha = self.invis_alpha.max(REVEALED);
        }
    }

    /// How visible the body is drawn (1: as usual).
    pub fn body_alpha(&self) -> f32 {
        if self.invis_active {
            self.invis_alpha
        } else {
            1.0
        }
    }
}

impl World {
    /// One tick of fighter `who`'s shield and invisibility: the shield shown
    /// catching up, the count of invisible ticks, the glow and fade, and
    /// the invisibility running down (`WPrInventory_Update`,
    /// `ONrCharacter_Shield_Update`, `ONrCharacter_Invis_Update`). A dead
    /// character's only while they still show.
    pub(super) fn magic_tick(&mut self, who: usize) {
        let now = self.now as u64;
        let Some(f) = self.fighters.get_mut(who) else { return };
        let inv = &mut f.inventory;
        if inv.shield_display < inv.shield {
            inv.shield_display += SHIELD_DISPLAY_SPEED;
        }
        inv.shield_display = inv.shield_display.min(inv.shield);
        if inv.invisibility > 0 {
            inv.invisible_frames = inv.invisible_frames.saturating_add(1);
        } else {
            inv.invisible_frames = 0;
        }
        let dead = !f.alive() || f.motion.dying;
        if dead && !f.effects.shield_active && !f.effects.invis_active {
            return;
        }
        // A super-shield class's glow follows its super amount instead.
        let super_shield = (f.motion.tuning.super_shield && f.super_amount > 0.0).then_some(f.super_amount);
        f.effects.shield_update(f.inventory.shield_display, super_shield, dead, now);
        let remaining = f.inventory.invisibility;
        f.effects.invis_update(remaining);
        if remaining > 0 && remaining != ENDLESS {
            f.inventory.invisibility = remaining - 1;
        }
    }

    /// One tick of fighter `who`'s Daodan power (`ONrCharacter_Update`):
    /// its super amount eases toward what a script set — or, for a
    /// super-hypo class, its health over full, as a share of full — up 0.03
    /// a tick, down 0.06; while it has any, its super particles (the class's
    /// `super…` ones, each on its body part) run. Dead, they go.
    pub(super) fn super_tick(&mut self, who: usize) {
        use crate::p3::{event, owner};
        let Some(f) = self.fighters.get_mut(who) else { return };
        if !f.alive() || f.motion.dying {
            if let Some(hs) = f.super_handles.take() {
                let now = self.now as u64;
                let crate::world::weapons::Armory { particles, .. } = &mut self.armory;
                let mut host = crate::world::weapons::ShotHost::new(&mut self.fighters, Some(&self.quads), now);
                for h in hs {
                    particles.kill(h, &mut host);
                }
            }
            return;
        }
        let mut desired = f.external_super;
        if f.motion.tuning.super_hypo && f.hp > f.max_hp {
            desired = desired.max((f.hp - f.max_hp) as f32 / f.max_hp.max(1) as f32);
        }
        let desired = desired.max(0.0);
        f.super_amount = if f.super_amount < desired { (f.super_amount + SUPER_UP).min(desired) } else { (f.super_amount - SUPER_DOWN).max(desired) };
        let on = f.super_amount > 0.0;
        if on == f.super_running && (!on || f.super_handles.is_some()) {
            return;
        }
        // (Made the first time it has any.)
        if on && f.super_handles.is_none() {
            let wanted: Vec<(String, usize)> = f.super_particles.iter().cloned().collect();
            let centres: Vec<Option<Vec3>> = wanted
                .iter()
                .map(|(_, part)| f.character.parts.iter().find(|p| p.bone == *part).map(|p| (p.bound_box.0 + p.bound_box.1) / 2.0))
                .collect();
            let mut handles = Vec::new();
            for ((class, part), centre) in wanted.iter().zip(centres) {
                let Some(id) = self.armory.particles.classes.get(class) else { continue };
                let Some(attach) = self.fighters[who].part_matrix(*part) else { continue };
                let spec = crate::p3::effect::Spec { class: id, orientation: crate::p3::effect::Orientation::Unchanged, location: crate::p3::effect::Location::Default };
                let mut data = crate::p3::effect::Data::new(self.armory.particles.tick, centre.unwrap_or(Vec3::ZERO));
                data.owner = owner::CHARACTER_MELEE | who as u32;
                data.parent_orientation = Some(glam::Mat3::IDENTITY);
                data.attach = Some(attach);
                if let Some(h) = self.create_effect(&spec, &data) {
                    handles.push(h);
                }
            }
            self.fighters[who].super_handles = Some(handles);
        }
        // Started or stopped with it.
        let ev = if on { event::START } else { event::STOP };
        let handles = self.fighters[who].super_handles.clone().unwrap_or_default();
        let t = f64::from(self.armory.particles.tick) / 60.0;
        let now = self.now as u64;
        let crate::world::weapons::Armory { particles, .. } = &mut self.armory;
        let mut host = crate::world::weapons::ShotHost::new(&mut self.fighters, Some(&self.quads), now);
        for h in handles {
            particles.send(h, ev, t, &mut host);
        }
        self.fighters[who].super_running = on;
    }

    /// Whether the AI can see fighter `who` at all
    /// (`AI2rKnowledge_CharacterCanBeSeen`): not once it has been
    /// invisible 90 ticks.
    pub fn can_be_seen(&self, who: usize) -> bool {
        self.fighters.get(who).is_none_or(|f| f.inventory.invisibility == 0 || f.inventory.invisible_frames < UNSEEN_AFTER)
    }

    /// `ONiCharacter_BecomeTemporarilyVisible`: a blow between `a` and `b`
    /// (or a throw) shows them both for a moment, and the AI's count
    /// starts again.
    pub(crate) fn reveal(&mut self, a: usize, b: usize) {
        for i in [a, b] {
            if let Some(f) = self.fighters.get_mut(i) {
                f.inventory.invisible_frames = 0;
                f.effects.reveal();
            }
        }
    }
}

/// How bright invisibility's shimmer is at most (`alpha_adjustment`).
const INVIS_AURA: f32 = 0.22;
/// The magic shells' size over the body (`TRrBody_DrawMagic`'s scale).
const SHELL_SCALE: f32 = 1.4;

impl World {
    /// The magic effects to draw this frame (`ONrCharacter_Draw`), given
    /// each fighter's body matrices: over its low body, each part scaled up
    /// 1.4 about its origin — the shield's glow part by part, then
    /// invisibility's shimmer, as bright as the body is faded out (at most
    /// 22%).
    pub fn magic_draws(&self, bones: &[Vec<glam::Mat4>]) -> Vec<crate::render::MagicDraw> {
        use crate::render::MagicKind;
        let time = self.now as u32;
        let mut out = Vec::new();
        for (f, b) in self.fighters.iter().zip(bones) {
            let Some(model) = f.magic_model else { continue };
            if f.gone || (!f.effects.shield_active && !f.effects.invis_active) {
                continue;
            }
            let scaled: Vec<glam::Mat4> = b.iter().map(|m| *m * glam::Mat4::from_scale(Vec3::splat(SHELL_SCALE))).collect();
            if f.effects.shield_active {
                // (A super shield's and a boss's red.)
                let (kind, tex) = if f.effects.daodan || f.boss_shield { (MagicKind::Daodan, 1) } else { (MagicKind::Shield, 0) };
                if let Some(texture) = self.magic_textures[tex].clone() {
                    let mut alpha = [0.0; PARTS];
                    for (a, p) in alpha.iter_mut().zip(f.effects.shield_parts) {
                        *a = f32::from(p) / 255.0;
                    }
                    out.push(crate::render::MagicDraw { model, bones: scaled.clone(), texture, kind, alpha, time });
                }
            }
            if f.effects.invis_active {
                if let Some(texture) = self.magic_textures[2].clone() {
                    let a = ((1.0 - f.effects.invis_alpha) * 255.0 * INVIS_AURA).round() / 255.0;
                    out.push(crate::render::MagicDraw { model, bones: scaled, texture, kind: MagicKind::Invis, alpha: [a; PARTS], time });
                }
            }
        }
        out
    }
}

/// `ONrCharacter_FindParts`: the body parts whose bounds come within
/// `radius` of `at` (a mask), given each part's world matrix and bounding
/// sphere (centre in the part's frame, radius).
pub fn parts_near(bones: &[glam::Mat4], spheres: &[Option<(Vec3, f32)>], at: Vec3, radius: f32) -> u32 {
    let mut mask = 0;
    for (k, (m, s)) in bones.iter().zip(spheres).enumerate().take(PARTS) {
        let Some((centre, r)) = s else { continue };
        let c = m.transform_point3(*centre);
        if c.distance_squared(at) < (radius + r) * (radius + r) {
            mask |= 1 << k;
        }
    }
    mask
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shield_glows_up_flares_where_struck_and_dies_away() {
        let mut m = Magic::default();
        // A full shield: 69 + 20 = 89, climbing a step a tick.
        for t in 0..100 {
            m.shield_update(100, None, false, t);
        }
        assert!(m.shield_active && m.shield_parts.iter().all(|&p| p == 89));
        m.shield_hit(1 << 3, false);
        assert_eq!(m.shield_parts[3], 255);
        m.shield_update(100, None, false, 0);
        assert_eq!(m.shield_parts[3], 248, "down 7 a tick from the flare");
        // Gone: it fades out and is over.
        for t in 0..200 {
            m.shield_update(0, None, false, t);
        }
        assert!(!m.shield_active && m.shield_parts.iter().all(|&p| p == 0));
    }

    #[test]
    fn invisibility_fades_out_then_back_in_before_it_runs_out() {
        let mut m = Magic::default();
        m.invis_update(crate::hud::MAX_INVISIBILITY);
        assert!(m.invis_active && m.invis_alpha == 1.0, "it starts seen");
        for _ in 0..200 {
            m.invis_update(crate::hud::MAX_INVISIBILITY);
        }
        assert_eq!(m.invis_alpha, 0.0, "faded out in 1 / 0.0085 ticks");
        m.reveal();
        assert_eq!(m.invis_alpha, REVEALED);
        for _ in 0..100 {
            m.invis_update(100);
        }
        assert_eq!(m.invis_alpha, 1.0, "its last 120 ticks: back");
    }

    #[test]
    fn a_hit_meets_the_parts_within_reach() {
        let bones = [glam::Mat4::IDENTITY, glam::Mat4::from_translation(Vec3::new(10.0, 0.0, 0.0))];
        let spheres = [Some((Vec3::ZERO, 1.0)), Some((Vec3::ZERO, 1.0))];
        assert_eq!(parts_near(&bones, &spheres, Vec3::new(9.0, 0.0, 0.0), 4.0), 0b10);
        assert_eq!(parts_near(&bones, &spheres, Vec3::new(5.0, 0.0, 0.0), 4.5), 0b11);
    }
}
