//! The world's side of the HUD (`docs/HUD.md`): what it shows of the player,
//! its tick (meters, prompts, the hit ring fading, autoprompts) and its
//! overlay for a frame.

use glam::Vec3;

use super::World;
use crate::hud::{Gun, Player};
use crate::render::OverlayDraw;

/// The hit ring's quarter for a blow along `dir` (the shove, attacker to
/// victim) on someone aiming at `facing` (`ONrCharacter_DamageCompass`): 0
/// right, 1 front, 2 left, 3 back of the ring's own naming.
pub fn hit_quadrant(facing: f32, dir: Vec3) -> usize {
    let (s, c) = facing.sin_cos();
    let fwd = s * dir.x + c * dir.z;
    let right = c * dir.x - s * dir.z;
    match (fwd < right, fwd < -right) {
        (true, true) => 3,
        (true, false) => 0,
        (false, true) => 2,
        (false, false) => 1,
    }
}

impl World {
    /// What the HUD shows of the player now.
    pub fn hud_player(&self) -> Option<Player> {
        let p = self.fighters.first().filter(|p| p.player && !p.gone)?;
        let gun = |w: usize, in_hand: bool| {
            let g = &self.armory.weapons[w];
            Gun {
                rounds: g.ammo,
                max: g.class.max_ammo,
                in_hand,
                icon: g.class.icon.clone(),
                empty: g.class.hud_empty.clone(),
                fill: g.class.hud_fill.clone(),
            }
        };
        let i = &p.inventory;
        Some(Player {
            hp: p.hp,
            hypo_remaining: p.inventory.hypo_remaining,
            max_hp: p.max_hp,
            boosted: self.game_sounds.boosted_health,
            health_shade: self.health_shade(p.hp, p.max_hp),
            shadow_shade: self.health_shade(self.hud.health_shadow().round() as i32, p.max_hp),
            hits: p.hits,
            ammo: i.ammo,
            cells: i.cells,
            hypos: i.hypos,
            shield: i.shield,
            invisibility: i.invisibility,
            lsi: i.lsi,
            feet: p.feet(),
            facing: p.facing.aim(),
            gun: p.hand.map(|w| gun(w, true)).or_else(|| p.holster.map(|w| gun(w, false))),
        })
    }

    /// One tick of the HUD (`ONrInGameUI_Update`, the autoprompt every 12
    /// ticks): the player's hit ring fades, the meters follow, an unread
    /// objective is prompted with its sound.
    pub(super) fn hud_tick(&mut self) {
        if let Some(p) = self.fighters.first_mut() {
            for h in &mut p.hits {
                *h = h.saturating_sub(crate::hud::HITS_FADE);
            }
        }
        // A dialogue line's subtitle stays while it plays.
        if !self.audio.dialog_playing() {
            self.hud.subtitles_done();
        }
        let player = self.hud_player().unwrap_or_default();
        let (letterbox, cutscene) = (self.letterbox, self.in_cutscene || self.now < 30);
        if let Some(sound) = self.hud.tick(&player, letterbox, cutscene) {
            self.event_sound(sound, None);
        }
        if self.hud.autoprompt_due() {
            let events = self.autoprompt_events();
            self.hud.autoprompt(&events);
        }
    }

    /// What the player could do now, in rising priority
    /// (`ONrGameState_UpdateAutoPrompt`): pick up a gun with rounds, a
    /// powerup, open a door, use a console (or an alarm), use a hypo when
    /// low on health.
    fn autoprompt_events(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        let Some(p) = self.fighters.first().filter(|p| p.player && p.alive()) else { return out };
        if self.weapon_in_reach(0).is_some_and(|w| self.armory.weapons[w].ammo > 0) {
            out.push("weapon");
        }
        if let Some(k) = self.powerup_in_reach(0) {
            out.push(super::weapons::POWERUPS[self.armory.powerups[k].kind]);
        }
        if self.door_prompt(0) {
            out.push("usedoor");
        }
        let big = p.hand.is_some_and(|w| self.armory.weapons[w].class.has(crate::weapons::flag::UNSTORABLE));
        if !big {
            if let Some(c) = self.console_in_reach(0, false).map(|c| &self.consoles[c]).filter(|c| c.active && !c.triggered) {
                out.push(if c.is_alarm { "usealarm" } else { "useconsole" });
            }
        }
        if p.inventory.hypos > 0 && p.hp <= p.max_hp * 30 / 100 {
            out.push("usehypo");
        }
        out
    }

    /// The HUD's overlay for a frame, on a screen `w` × `h` (HUD pixels).
    pub fn hud_draw(&mut self, w: f32, h: f32) -> Vec<OverlayDraw> {
        let player = self.hud_player();
        let letterboxed = self.letterbox;
        self.hud.draw(player.as_ref(), w, h, letterboxed)
    }
}
