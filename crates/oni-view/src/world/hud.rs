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

/// The overall volume while the pause screen is up, of what it was
/// (`ONcPauseScreen_VolumeCut`).
const PAUSE_VOLUME_CUT: f32 = 0.5;

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
            shield_display: i.shield_display,
            invisibility: i.invisibility,
            lsi: i.lsi,
            timer: self.timer_text(),
            timer_on: self.timer.is_some(),
            feet: p.feet(),
            facing: p.facing.aim(),
            gun: p
                .hand
                .map(|w| gun(w, true))
                .or_else(|| p.holster.map(|w| gun(w, false))),
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
        let player = self.hud_player().unwrap_or_default();
        let (letterbox, cutscene) = (self.cutscene.letterbox, self.cutscene.on || self.now < 30);
        if let Some(sound) = self.hud.tick(&player, letterbox, cutscene) {
            self.event_sound(sound, None);
        }
        self.hud.cinematics.tick();
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
        let Some(p) = self.fighters.first().filter(|p| p.player && p.alive()) else {
            return out;
        };
        if self
            .weapon_in_reach(0)
            .is_some_and(|w| self.armory.weapons[w].ammo > 0)
        {
            out.push("weapon");
        }
        if let Some(k) = self.powerup_in_reach(0) {
            out.push(super::weapons::POWERUPS[self.armory.powerups[k].kind]);
        }
        // (Neutral characters to talk to: above pickups, below doors.)
        if self.neutral_prompt() {
            out.push("nci");
        }
        if self.door_prompt(0) {
            out.push("usedoor");
        }
        let big = p.hand.is_some_and(|w| {
            self.armory.weapons[w]
                .class
                .has(crate::weapons::flag::UNSTORABLE)
        });
        if !big {
            if let Some(c) = self
                .console_in_reach(0, false)
                .map(|c| &self.consoles[c])
                .filter(|c| c.active && !c.triggered)
            {
                out.push(if c.is_alarm { "usealarm" } else { "useconsole" });
            }
        }
        if p.inventory.hypos > 0 && p.hp <= p.max_hp * 30 / 100 {
            out.push("usehypo");
        }
        out
    }

    /// The HUD's overlay for a frame, on a screen `w` × `h` (HUD pixels).
    /// F1: the pause screen opened (with what the player carries) or —
    /// once it has been up a second — closed.
    pub fn pause_toggle(&mut self) {
        let was = self.hud.pause.is_some();
        // (Not in a cutscene, nor dead: `ONiGameState_ProcessMiscActions`.)
        if !was && (self.cutscene.on || self.fighters.first().is_some_and(|p| !p.alive())) {
            return;
        }
        let (lsi, weapon) = match self.fighters.first() {
            Some(p) => (
                p.inventory.lsi,
                p.hand
                    .or(p.holster)
                    .map(|w| self.armory.weapons[w].class.name.clone()),
            ),
            None => (false, None),
        };
        self.hud.pause_toggle(lsi, weapon.as_deref());
        self.pause_sounds(was);
    }

    /// A pause screen button pressed (by id; `clicked`: with the mouse,
    /// which clicks — `WMcButton_ClickSound`).
    pub fn pause_press(&mut self, id: i64, clicked: bool) {
        let was = self.hud.pause.is_some();
        if clicked {
            self.audio.impulse_play("button_click", None, None, None);
        }
        self.hud.pause_press(id);
        self.pause_sounds(was);
    }

    /// The console text's button clicked (`ONiTC_HandleCommand`): it clicks
    /// (`WMcButton_ClickSound`); Next shows the next page, Close (on the
    /// last) closes.
    pub fn text_console_click(&mut self) {
        self.audio.impulse_play("button_click", None, None, None);
        self.hud.text_console_next();
    }

    /// The pause screen's sounds (`ONrPauseScreen_Display`): opening, its
    /// sound and the overall volume halved; closing, the volume back and its
    /// sound.
    fn pause_sounds(&mut self, was: bool) {
        match (was, self.hud.pause.is_some()) {
            (false, true) => {
                self.audio
                    .impulse_play("pause_screen_open", None, None, None);
                self.pause_volume = self.audio.volume();
                self.audio.set_volume(self.pause_volume * PAUSE_VOLUME_CUT);
            }
            (true, false) => {
                self.audio.set_volume(self.pause_volume);
                self.audio
                    .impulse_play("pause_screen_close", None, None, None);
            }
            _ => {}
        }
    }

    pub fn hud_draw(&mut self, w: f32, h: f32) -> Vec<OverlayDraw> {
        let player = self.hud_player();
        let letterboxed = self.cutscene.letterbox;
        self.hud.draw(player.as_ref(), w, h, letterboxed)
    }
}
