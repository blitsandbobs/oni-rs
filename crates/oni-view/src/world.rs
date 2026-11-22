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

/// The head's bone (the listener's ear height).
const HEAD_BONE: usize = 10;

pub mod anim_particles;
pub mod camera;
pub mod cheats;
pub mod corpses;
pub mod cutscene;
mod env_particles;
mod flow;
pub mod fog;
mod hud;
mod hypo;
mod lens;
pub mod magic;
pub mod neutral;
pub mod skip;
pub mod timer;
pub use hud::hit_quadrant;
mod objects;
mod objects_sound;
mod particle_sounds;
mod turrets;
mod voices;
mod weapons;
pub use voices::{speech, vocal, Line, PainState, Speech};
pub use weapons::{Armory, Item, SightTarget, POWERUPS};

/// Characters further apart than this are never tested for touching.
const CONTACT_RANGE: f32 = 25.0;
/// A defensive character blocks attackers within this angle of where it faces.
const BLOCK_ANGLE: f32 = 20.0 * std::f32::consts::PI / 180.0;
/// Scales an attack's data `knockback` (total shove distance). `1.0` is the
/// original's behaviour: the shove is spent over the following ticks, a share
/// per tick (see `Fighter::tick`). A feel knob to revisit at playtest.
const KNOCKBACK_SCALE: f32 = 1.0;

/// A film eased into ("interp") must start within this of the character.
const FILM_MAX_EASE: f32 = 80.0;

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
/// How far a sound travels from `from` to `to` through the rooms, past the
/// portals `gate` lets it through (`crate::nav::plan::sound_route`).
fn sound_length(
    nav: &crate::nav::Navigation,
    from: Vec3,
    to: Vec3,
    gate: &dyn Fn(usize, usize) -> Option<f32>,
) -> Option<f32> {
    crate::nav::plan::sound_route(nav, nav.room_at(from)?, from, nav.room_at(to)?, to, gate)
}

fn route_length(nav: &crate::nav::Navigation, from: Vec3, to: Vec3) -> Option<f32> {
    let (a, b) = (nav.room_at(from)?, nav.room_at(to)?);
    if a == b {
        return Some(from.distance(to));
    }
    let portals = crate::nav::plan::route(nav, a, from, b, to, &Default::default())?;
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
    /// Its super particles: class and body part (`ONtCharacterParticle`).
    pub super_particles: Rc<Vec<(String, usize)>>,
    /// All its class's particles (what its animations attach).
    pub class_particles: Rc<Vec<anim_particles::ClassParticle>>,
    /// Its low body's model, for the magic effects (if converted).
    pub magic_model: Option<usize>,
    pub character: Rc<Character>,
    pub anims: Rc<AnimCollection>,
    /// The character class's tuning (fight stance, idle, jump, fall damage).
    pub tuning: ClassTuning,
    /// How its AI fights hand to hand, and how long it lies dazed (ticks).
    pub melee: Option<Rc<crate::ai::melee::profile::Profile>>,
    pub daze: (u32, u32),
    /// How it sees, hears and remembers.
    pub perception: crate::ai::perception::Tuning,
    /// Its combat timings (for goals).
    pub class_combat: crate::ai::goals::ClassCombat,
    /// A non-player body is removed this long after death (0: it stays).
    pub death_delete: u32,
    /// Its aiming screens (upper-body aim poses).
    pub aiming: Rc<Vec<crate::anim::aim::Screen>>,
    /// How its AI shoots: skill per weapon slot, and targeting settings.
    pub shooting: Rc<Vec<crate::ai::ranged::Skill>>,
    pub targeting: crate::ai::ranged::Params,
    /// Its say in its impacts: steps, blows (`docs/AUDIO.md`).
    pub sounds: Rc<crate::impacts::ClassSounds>,
    /// The class's shadow.
    pub shadow: Option<Rc<crate::decal::Shadow>>,
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
    /// Noises made since the last perception pass.
    pub sounds: Vec<Sound>,
    /// The level's folder (`levels/NN`), if known.
    pub level_dir: std::path::PathBuf,
    /// The level's consoles.
    pub consoles: Vec<crate::consoles::Console>,
    /// The level's neutral behaviours, by id, and the interactions under
    /// way, by character (`world/neutral.rs`).
    pub neutrals: std::collections::HashMap<u16, Rc<crate::neutrals::Behavior>>,
    /// The level's pre-placed corpses.
    pub corpses: corpses::Corpses,
    /// Level parts shaded flat by script reference (`env_shade`).
    pub env_shades: std::collections::HashMap<u32, [f32; 3]>,
    /// The frame clock (`docs/GAME_FLOW.md`, *Frames*): a cutscene keeps
    /// up with the clock (`sync_enabled`, ticks owed); slow motion's ticks
    /// left (`slowmo`).
    pub sync_enabled: bool,
    pub sync_behind: u32,
    pub slowmo_timer: u32,
    /// Whether the player killed Griffin, once a script has said
    /// (`killed_griffen`).
    pub killed_griffin: Option<bool>,
    /// A boss battle on (`ai2_boss_battle`): bosses go after bosses.
    pub boss_battle: bool,
    /// The cheat codes typed and their toggles.
    pub cheats: cheats::Cheats,
    /// The magic effects' textures: shield, super shield, invisibility.
    pub magic_textures: [Option<std::path::PathBuf>; 3],
    pub(crate) neutral_runs: std::collections::HashMap<usize, neutral::Run>,
    /// The level's laser triggers, and their beams out this tick (from, to,
    /// colour — for drawing).
    pub lasers: Vec<crate::lasers::Laser>,
    pub beams: Vec<(Vec3, Vec3, [f32; 3])>,
    /// The level's doors (`doors.rs`).
    pub doors: crate::doors::Doors,
    /// The level parts scripts have hidden (`env_show`).
    pub env_hidden: std::collections::HashSet<u32>,
    /// The game camera (`world/camera.rs`).
    pub camera: camera::GameCamera,
    /// The HUD (`hud/`, `world/hud.rs`).
    pub hud: crate::hud::Hud,
    /// The level's outcome (`win`: true, `lose`: false), a splash screen a
    /// script asked for, and the save point kept (`world/flow.rs`).
    pub victory: Option<bool>,
    pub pending_splash: Option<String>,
    pub saved: Option<crate::game::Continue>,
    /// Save points write their continue to disk (play mode; `game.rs`).
    pub persist_continues: bool,
    /// The overall volume the pause screen will restore.
    pub pause_volume: f32,
    /// The game settings' hypo values (`world/hypo.rs`), and the hypo key
    /// last tick (it acts as pressed).
    pub hypo: hypo::HypoSettings,
    last_hypo_key: u32,
    /// The lens flares' fades (`world/lens.rs`): each's state and opacity.
    pub lens: std::collections::HashMap<crate::p3::Handle, (lens::LensState, f32)>,
    /// The countdown timer (`world/timer.rs`), and the script of one just
    /// run out (the mission runs it).
    pub timer: Option<timer::Timer>,
    pub timer_expired: Option<String>,
    /// The quads see-through this tick (the camera's jello).
    pub jello: Vec<u32>,
    /// The placed objects see-through this tick (the camera's jello on
    /// objects, `OBcFlags_JelloObjects`).
    pub jello_objects: Vec<usize>,
    /// The door leaves (object id, leaf) see-through this tick: open or
    /// moving, their leaves are objects too (the camera's jello on them).
    pub jello_doors: Vec<(u32, u8)>,
    /// The fog (`world/fog.rs`).
    pub fog: fog::FogState,
    /// The level's placed objects as authored, and those scripts have made.
    pub object_setups: Vec<objects::Setup>,
    pub objects: Vec<objects::Object>,
    /// The object animations scripts name (`OBAN`, the level's and the
    /// common ones), by name.
    pub object_anims: std::collections::HashMap<String, Rc<oni_geom::objanim::ObjectAnimation>>,
    /// Script functions the world asks to run (consoles' events), with
    /// their argument.
    pub script_events: Vec<(String, Vec<String>)>,
    /// Which of the player's buttons work (`lock_keys`).
    pub key_mask: u32,
    /// Whether the player's input works at all (`input`).
    pub input_enabled: bool,
    /// The AI movement system drives the player (`ai2_takecontrol`): her
    /// keys come from her [`crate::ai::Brain::Steer`], not the input.
    pub ai_control: bool,
    last_controls: u32,
    /// The player's name (its level placement's).
    pub player_name: String,
    /// The level's AI data: placed characters, patrol paths, flags, combat
    /// profiles.
    pub level_ai: Rc<crate::ai::level::LevelAi>,
    now: i64,
    rng: u32,
    /// Guns and the particles their shots are (`docs/WEAPONS.md`).
    pub armory: Armory,
    /// The level's breakable glass panes (`glass.rs`).
    pub glass: crate::glass::Glass,
    /// The level's turrets (`docs/TURRETS.md`).
    pub turrets: Vec<turrets::Turret>,
    /// Every AI made passive (`ai2_allpassive`): turrets hold fire too.
    pub all_passive: bool,
    /// Sound (`docs/AUDIO.md`).
    pub audio: crate::audio::Audio,
    /// What impacts sound like (`common/impacts.json`), and moves
    /// (`common/anim_sounds.json`).
    pub impacts: Option<Rc<crate::impacts::Impacts>>,
    pub anim_sounds: Option<Rc<crate::anim_sounds::AnimSounds>>,
    /// The level's quads (decals are laid on them; `levels/NN/quads.bin`).
    pub quads: crate::quads::Quads,
    /// Health → colour (`game_settings.json`): melee flashes' tint.
    health_color: Vec<(f32, u32)>,
    /// The textures drawn glowing (`texture_flags.json`).
    additive_textures: std::collections::HashSet<String>,
    /// The animated textures, by file stem (`texanim`).
    texture_anims: std::collections::HashMap<String, crate::texanim::TexAnim>,
    /// Particles' ambient sounds (their audio handles), and projectiles
    /// followed for flybys (where each was last).
    particle_ambients: std::collections::HashMap<crate::p3::Handle, usize>,
    flybys: Vec<(crate::p3::Handle, Vec3)>,
    /// The game's event and condition sounds, and the object classes'
    /// (`docs/AUDIO.md`, *Objects and the game*).
    pub(crate) game_sounds: objects_sound::GameSounds,
    /// A cutscene or letterbox is on (no compass / objective sounds; the
    /// condition loops hushed).
    pub in_cutscene: bool,
    /// Ticks into the cutscene (or since a splash over it went).
    pub cutscene_ticks: u32,
    /// A cutscene skip under way (`world/skip.rs`).
    pub skip: skip::Skip,
    /// The sound turned off (a skip): the overall volume to restore.
    sound_off_volume: Option<f32>,
    pub letterbox: bool,
    /// The level's environmental particles (`docs/PARTICLES.md`).
    pub(crate) env_particles: Vec<env_particles::EnvParticle>,
    env_particles_started: bool,
}

impl World {
    /// An empty world over the given level collision mesh.
    pub fn new() -> World {
        World {
            fighters: Vec::new(),
            quirks: Quirks::FIXED,
            fights: Default::default(),
            difficulty: Default::default(),
            nav: None,
            sounds: Vec::new(),
            level_ai: Default::default(),
            player_name: "player".into(),
            level_dir: Default::default(),
            consoles: Vec::new(),
            neutrals: Default::default(),
            corpses: Default::default(),
            magic_textures: Default::default(),
            cheats: Default::default(),
            boss_battle: false,
            env_shades: Default::default(),
            sync_enabled: false,
            sync_behind: 0,
            slowmo_timer: 0,
            killed_griffin: None,
            neutral_runs: Default::default(),
            doors: Default::default(),
            lasers: Vec::new(),
            beams: Vec::new(),
            env_hidden: Default::default(),
            camera: Default::default(),
            hud: Default::default(),
            victory: None,
            pending_splash: None,
            saved: None,
            hypo: Default::default(),
            last_hypo_key: 0,
            lens: Default::default(),
            timer: None,
            timer_expired: None,
            jello: Vec::new(),
            jello_objects: Vec::new(),
            jello_doors: Vec::new(),
            persist_continues: false,
            pause_volume: 1.0,
            fog: Default::default(),
            object_anims: Default::default(),
            object_setups: Vec::new(),
            objects: Vec::new(),
            script_events: Vec::new(),
            key_mask: u32::MAX,
            input_enabled: true,
            ai_control: false,
            last_controls: 0,
            now: 0,
            rng: 0x9E37_79B9,
            armory: Armory::default(),
            glass: Default::default(),
            turrets: Vec::new(),
            all_passive: false,
            audio: crate::audio::Audio::empty(),
            impacts: None,
            anim_sounds: None,
            quads: Default::default(),
            health_color: Vec::new(),
            additive_textures: Default::default(),
            texture_anims: Default::default(),
            particle_ambients: Default::default(),
            flybys: Vec::new(),
            game_sounds: Default::default(),
            in_cutscene: false,
            cutscene_ticks: 0,
            skip: skip::Skip::Idle,
            sound_off_volume: None,
            letterbox: false,
            env_particles: Vec::new(),
            env_particles_started: false,
        }
    }

    /// Spawns a fighter of `kind` with its pelvis origin at `pos`, facing
    /// `facing_yaw` (radians). `passive` marks a training dummy (no attacks).
    /// The first fighter spawned is the player. Returns the new fighter's index.
    pub fn spawn(&mut self, kind: &CharKind, pos: Vec3, facing_yaw: f32, passive: bool) -> usize {
        let player = self.fighters.is_empty();
        let mut f = Fighter::new(
            kind.model,
            kind.character.clone(),
            kind.anims.clone(),
            kind.tuning,
            pos,
            facing_yaw,
            passive,
            player,
            self.quirks,
        );
        // Placed on the floor under it.
        if !self.quads.quads.is_empty() {
            f.snap_to_floor(&self.quads);
        }
        f.magic_model = kind.magic_model;
        f.super_particles = kind.super_particles.clone();
        f.class_particles = kind.class_particles.clone();
        f.melee_profile = kind.melee.clone();
        f.daze = kind.daze;
        f.class_combat = kind.class_combat;
        f.death_delete = kind.death_delete;
        f.aiming = kind.aiming.clone();
        f.shooting = kind.shooting.clone();
        f.targeting = kind.targeting;
        f.sounds = kind.sounds.clone();
        f.shadow = kind.shadow.clone();
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
        let Some(f) = self.fighters.get_mut(id) else {
            return false;
        };
        let Some(profile) = f.melee_profile.clone() else {
            return false;
        };
        let m = crate::ai::melee::brain::Melee::new(profile, target, f.daze, id as u32 * 7919 + 1);
        f.brain = Some(crate::ai::Brain::Melee(Box::new(m)));
        // It knows exactly where its target is (the original's "magic
        // awareness" of an ordered attack).
        let (team, now) = (
            self.fighters.get(target).map(|t| (t.team(), t.feet())),
            self.now as u64,
        );
        if let (Some((team, feet)), Some(p)) = (team, self.fighters[id].perception.as_mut()) {
            p.force_target(target, team, feet, now);
        }
        true
    }

    /// How many ticks to run this frame (`iComputeDeltaTicks`), with
    /// `available` passed on the clock: at most 6 — the rest dropped (the
    /// game slows) unless a cutscene keeps up with the clock, owing them to
    /// the next frame (more than 5 s owed is given up). A `cutscene_sync`
    /// mark clears what is owed.
    pub fn frame_ticks(&mut self, available: u32) -> u32 {
        const MAX_TICKS_PER_FRAME: u32 = 6;
        let mut timing = available;
        if self.sync_enabled && self.sync_behind > 0 {
            if self.sync_behind > 60 * 5 {
                eprintln!("frames: {} ticks behind, out of sync", self.sync_behind);
            } else {
                timing += self.sync_behind;
            }
            self.sync_behind = 0;
        }
        let engine = timing.min(MAX_TICKS_PER_FRAME);
        if self.sync_enabled {
            self.sync_behind = timing - engine;
        }
        engine
    }

    /// How much slower than the clock the game runs: 2 in slow motion
    /// (`slowmo`, the `carousel` cheat), else 1.
    pub fn time_scale(&self) -> f32 {
        if self.slowmo_timer > 0 || self.cheats.slow_motion {
            2.0
        } else {
            1.0
        }
    }

    /// `env_shade a b r g b`: the level parts with script references `a`–`b`
    /// shaded flat in this colour (each 0–1, kept to 8 bits).
    pub fn env_shade(&mut self, a: u32, b: u32, rgb: [f32; 3]) {
        let (lo, hi) = (a.min(b), a.max(b));
        let shade = rgb.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() / 255.0);
        let refs: std::collections::BTreeSet<u32> = self
            .quads
            .quads
            .iter()
            .filter_map(|q| q.env_ref)
            .filter(|r| (lo..=hi).contains(r))
            .collect();
        for r in refs {
            self.env_shades.insert(r, shade);
        }
    }

    /// `particle_temp_kill`: the temporary particles go.
    pub fn kill_temporary_particles(&mut self) {
        let now = self.now as u64;
        let weapons::Armory { particles, .. } = &mut self.armory;
        let mut host = weapons::ShotHost::new(&mut self.fighters, Some(&self.quads), now);
        particles.kill_temporary(&mut host);
    }

    /// A roll in `[0, 1)` for others (`UUrLocalRandom`'s uses).
    pub fn roll(&mut self) -> f32 {
        self.random()
    }

    /// A roll in `[0, 1)`.
    fn random(&mut self) -> f32 {
        self.percent() / 100.0 + (self.percent() / 10_000.0)
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
    pub fn sound(
        &mut self,
        stimulus: crate::ai::perception::Stimulus,
        at: Vec3,
        volume: f32,
        maker: Option<usize>,
        other: Option<usize>,
    ) {
        self.sounds.push(Sound {
            stimulus,
            at,
            volume,
            maker,
            other,
        });
    }

    /// Perception (behaviour per `docs/PERCEPTION.md`): noises, sight, memory
    /// fading and alert easing for every perceiving character; then each
    /// acts on what it noticed.
    fn perceive(&mut self) {
        use crate::ai::perception::{forward, Notice, Stimulus, Strength};
        let now = self.now as u64;
        let n = self.fighters.len();
        let alive: Vec<bool> = self
            .fighters
            .iter()
            .map(|f| f.alive() && !f.motion.dying)
            .collect();
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

        // Noises. (Through the rooms, doors that aren't open pass only what
        // their class lets through, muffled.)
        let portal_doors: std::collections::HashMap<(usize, usize), usize> = match self.nav.clone()
        {
            Some(nav) => self
                .doors
                .portals(&nav)
                .iter()
                .enumerate()
                .flat_map(|(d, ps)| ps.iter().map(move |&rk| (rk, d)))
                .collect(),
            None => Default::default(),
        };
        let sounds = std::mem::take(&mut self.sounds);
        for s in &sounds {
            for i in 0..n {
                if !alive[i]
                    || self.fighters[i].passive
                    || self.fighters[i].perception.as_ref().is_some_and(|p| p.deaf)
                {
                    continue;
                }
                let Some(p) = self.fighters[i].perception.as_ref() else {
                    continue;
                };
                let combat = s.stimulus.combat_sound();
                let ignored = |w: Option<usize>| {
                    w.is_some_and(|w| {
                        !alive[w] || p.ignores(i, w, teams[w], self.fighters[w].player, combat)
                    })
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
                let gate = |r: usize, k: usize| {
                    portal_doors
                        .get(&(r, k))
                        .map_or(Some(1.0), |&d| self.doors.list[d].sound_through(s.stimulus))
                };
                let heard = self.quads.sees(s.at, eye)
                    || self.nav.as_ref().is_some_and(|nav| {
                        sound_length(nav, s.at, feet[i], &gate).is_some_and(|d| d <= range)
                    });
                if !heard {
                    continue;
                }
                let p = self.fighters[i].perception.as_mut().expect("perceiving");
                match s.maker {
                    Some(m) if !ig1 => {
                        let strength = if s.stimulus as u8 >= Stimulus::SoundMelee as u8
                            && s.at.distance(eye) <= 40.0
                        {
                            Strength::Definite
                        } else {
                            Strength::Strong
                        };
                        p.perceive(
                            Some(m),
                            Some(teams[m]),
                            s.stimulus,
                            strength,
                            feet[m],
                            0,
                            now,
                        );
                    }
                    None => p.perceive(None, None, s.stimulus, Strength::Strong, s.at, 0, now),
                    _ => {}
                }
                if let Some(o) = s.other.filter(|_| !ig2) {
                    let p = self.fighters[i].perception.as_mut().expect("perceiving");
                    p.perceive(
                        Some(o),
                        Some(teams[o]),
                        s.stimulus,
                        Strength::Strong,
                        feet[o],
                        0,
                        now,
                    );
                }
            }
        }

        // In a boss battle, who the bosses are (the player always is).
        let bosses = self.boss_battle.then(|| {
            Rc::new(
                self.fighters
                    .iter()
                    .map(|f| f.boss || f.player)
                    .collect::<Vec<bool>>(),
            )
        });
        for f in &mut self.fighters {
            if let Some(p) = f.perception.as_mut() {
                p.boss_battle = bosses.clone();
            }
        }
        // Sight (none of one invisible long enough).
        let seeable: Vec<bool> = (0..n).map(|j| self.can_be_seen(j)).collect();
        for i in 0..n {
            if !alive[i]
                || self.fighters[i].passive
                || self.fighters[i].perception.as_ref().is_none_or(|p| p.blind)
            {
                continue;
            }
            let every = self.fighters[i]
                .perception
                .as_ref()
                .map_or(59, |p| p.look_every(now));
            if (i as u64 * 13) % every != now % every {
                continue;
            }
            let eye = points[i][1];
            // Along its aim while it aims (`aimingVector`: the head's turn,
            // pitched — "up" is square to it), else its body's facing.
            let (look, up) = match self.fighters[i].ai_aim {
                Some((yaw, pitch)) => {
                    let dir = Vec3::new(
                        yaw.sin() * pitch.cos(),
                        pitch.sin(),
                        yaw.cos() * pitch.cos(),
                    );
                    let right = Vec3::new(-dir.z, 0.0, dir.x);
                    (dir, right.cross(dir).normalize_or(Vec3::Y))
                }
                None => (forward(self.fighters[i].facing.facing), Vec3::Y),
            };
            for j in 0..n {
                if j == i || !alive[j] || !seeable[j] {
                    continue;
                }
                let p = self.fighters[i].perception.as_ref().expect("perceiving");
                if p.ignores(i, j, teams[j], self.fighters[j].player, false) {
                    continue;
                }
                let known = p.about(j).is_some_and(|c| c.strength == Strength::Definite);
                let aim = feet[j] + Vec3::Y * self.fighters[j].motion.height();
                let Some(stimulus) = p.tuning.sees(aim - eye, look, up, known, faithful) else {
                    continue;
                };
                let seen = points[j].iter().find(|&&pt| self.quads.sees(eye, pt));
                let Some(&at) = seen else { continue };
                let strength = if stimulus == Stimulus::SightDefinite {
                    Strength::Definite
                } else {
                    Strength::Weak
                };
                let p = self.fighters[i].perception.as_mut().expect("perceiving");
                p.perceive(Some(j), Some(teams[j]), stimulus, strength, at, 0, now);
            }
        }

        // Memory fades (every 60 ticks); the alert eases when not fighting
        // (an agent: only while idle or patrolling).
        for f in &mut self.fighters {
            let calm = match &f.brain {
                Some(crate::ai::Brain::Agent(a)) => a.calming(),
                b => b.as_ref().and_then(|b| b.melee()).is_none(),
            };
            if let Some(p) = f.perception.as_mut() {
                if now.is_multiple_of(60) {
                    p.decay(now, &|w| seeable.get(w).copied().unwrap_or(true));
                }
                if calm {
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
            if matches!(self.fighters[i].brain, Some(crate::ai::Brain::Agent(_))) {
                // Goals act on it (`docs/GOALS.md`).
                for notice in notices {
                    if let Notice::Startle(at) = notice {
                        let fighting = matches!(&self.fighters[i].brain,
                            Some(crate::ai::Brain::Agent(a)) if matches!(a.goal, crate::ai::goals::Goal::Combat(_)));
                        if !fighting {
                            self.fighters[i].startle(at);
                        }
                        continue;
                    }
                    self.with_agent(i, |a, cx| a.notice(notice, cx));
                }
                continue;
            }
            for notice in notices {
                match notice {
                    Notice::Combat(who) => {
                        // (A fighter without goals keeps its fight.)
                        let fighting = self.fighters[i]
                            .brain
                            .as_ref()
                            .and_then(|b| b.melee())
                            .is_some();
                        if !fighting && !self.fighters[i].passive && alive[who] {
                            eprintln!("perception: fighter {i} engages {who}");
                            self.fight(i, who);
                        }
                    }
                    Notice::Startle(at) => self.fighters[i].startle(at),
                    Notice::Glance(at, ticks) => {
                        self.fighters[i].glance = Some((at, now + u64::from(ticks)))
                    }
                    // Without goals, a threat it would pursue it glances at.
                    Notice::Pursue(id) => {
                        let p = self.fighters[i].perception.as_ref().expect("perceiving");
                        if let Some(c) = p.contact(id) {
                            if p.alert < crate::ai::perception::Alert::Combat {
                                let (at, ticks) = (c.location, c.stimulus.glance());
                                self.fighters[i].glance = Some((at, now + u64::from(ticks)));
                            }
                        }
                    }
                    Notice::Knowledge { .. } => {}
                }
            }
        }
    }

    /// Runs `run` on fighter `i`'s goals (if it has them) with a view of the
    /// world.
    fn with_agent(
        &mut self,
        i: usize,
        run: impl FnOnce(&mut crate::ai::goals::Agent, &mut crate::ai::goals::Ctx),
    ) {
        use crate::ai::goals::{Ctx, Other};
        use crate::anim::types::st;
        let me_team = self.fighters[i].team();
        let others: Vec<Other> = self
            .fighters
            .iter()
            .map(|f| Other {
                feet: f.feet(),
                alive: f.alive() && !f.motion.dying,
                pelvis: f.feet() + Vec3::Y * f.motion.height(),
                height: f.motion.height() * 2.0,
                friend: f.team().regards(me_team) != crate::ai::perception::Relation::Hostile,
                dizzy: f.motion.dizzy_stun > 0 || f.motion.stagger_stun > 0,
                fallen: matches!(f.motion.state, st::FALLEN_BACK | st::FALLEN_FRONT),
                invisible: f.inventory.invisibility > 0,
                facing: f.facing.facing,
                gone: f.gone,
            })
            .collect();
        let gun = self.gun_view(i);
        let guns = self.free_guns(i);
        let (spreads, projectiles) = (self.spreads(), self.projectiles());
        let Some(crate::ai::Brain::Agent(mut agent)) = self.fighters[i].brain.take() else {
            return;
        };
        let level = self.level_ai.clone();
        let nav = self.nav.clone();
        let quads = &self.quads;
        let sight_fn = |a: Vec3, b: Vec3, skip: u32| quads.clear(a, b, skip);
        let path_fn = |a: Vec3, b: Vec3| match nav.as_deref() {
            Some(n) => route_length(n, a, b),
            None => Some(a.distance(b)),
        };
        let probe_fn = |from: Vec3, dir: glam::Vec2, dist: f32, stop: crate::nav::Square| {
            nav.as_deref().map(|n| n.probe(from, dir, dist, stop))
        };
        let now = self.now as u64;
        let quirks = self.quirks;
        let f = &mut self.fighters[i];
        let (feet, eye, facing) = (f.feet(), f.sight_points()[1], f.facing.facing);
        let fallen = matches!(f.motion.state, st::FALLEN_BACK | st::FALLEN_FRONT);
        let crouching = f.motion.crouching();
        let f_state_standing = f.motion.state == st::STANDING;
        let powerup_type = oni_schema::value(&oni_schema::ANIM_TYPES, "powerup").unwrap_or(0);
        let powerup_state = oni_schema::value(&oni_schema::ANIM_STATES, "powerup").unwrap_or(0);
        let body = crate::ai::goals::Body {
            hp: f.hp,
            max_hp: f.max_hp,
            unkillable: f.unkillable,
            inactive_upright: f.motion.inactive_upright(),
            powering_up: f.motion.kind == powerup_type
                && f.motion.current().from_state == powerup_state,
            powerup_type,
            anim_type: f.motion.kind,
            started: f.motion.started,
            action_frame: f
                .motion
                .current()
                .action_frame
                .is_some_and(|a| f.motion.frame == a),
            standing_still: f.motion.kind == crate::anim::types::ty::STAND
                && f.motion.state == st::STANDING
                && f.motion.entered_from == st::STANDING,
            fallen,
            victim: crate::anim::reaction::is_victim(f.motion.kind),
            invisibility: f.inventory.invisibility,
            queued: f.motion.queued,
            disables_shield: f
                .motion
                .current()
                .has_flag(crate::anim::types::flag::DISABLE_SHIELD),
            has_thunderbolt: f
                .motion
                .coll
                .lookup(
                    crate::anim::types::ty::MURO_THUNDERBOLT,
                    f.motion.state,
                    f.motion.varient | crate::anim::types::var::FIGHT,
                )
                .is_some(),
            can_pickup: picks_up(f.motion.current()),
            picking_up: matches!(f.motion.kind, 157..=159 | 227..=229),
        };
        let (pelvis, using_console) = (f.sight_points()[0], f.console.is_some());
        // (The roll that way, else the slide: one that can pick up.)
        let (motion, body_facing) = (&f.motion, f.facing.facing);
        let escape_fn = |way: usize| {
            use crate::anim::types::ty;
            let kind = [
                ty::CROUCH_FORWARD,
                ty::CROUCH_BACK,
                ty::CROUCH_LEFT,
                ty::CROUCH_RIGHT,
            ][way];
            [kind, ty::SLIDE].into_iter().find_map(|k| {
                let c = motion.coll.lookup(k, motion.state, motion.varient)?;
                let clip = &motion.coll.entries[c].clip;
                picks_up(clip).then(|| {
                    let n = clip.root_motion.len().saturating_sub(1);
                    let [x, z] = clip.root_motion[..n]
                        .iter()
                        .fold([0.0, 0.0], |a, m| [a[0] + m[0], a[1] + m[1]]);
                    (
                        k,
                        glam::Quat::from_rotation_y(body_facing) * Vec3::new(x, 0.0, z),
                    )
                })
            })
        };
        let (melee_profile, daze) = (f.melee_profile.clone(), f.daze);
        if let Some(perception) = f.perception.as_mut() {
            let mut cx = Ctx {
                me: i,
                body,
                now,
                feet,
                eye,
                facing,
                fallen,
                standing: !fallen && f_state_standing,
                crouching,
                pelvis,
                consoles: &self.consoles,
                using_console,
                perception,
                level: &level,
                sight: &sight_fn,
                escape: &escape_fn,
                path_length: &path_fn,
                probe: &probe_fn,
                others: &others,
                fights: &mut self.fights,
                melee_profile,
                daze,
                quirks,
                gun,
                guns: &guns,
                spreads: &spreads,
                projectiles: &projectiles,
            };
            run(&mut agent, &mut cx);
        }
        self.fighters[i].brain = Some(crate::ai::Brain::Agent(agent));
    }

    /// Every agent's goals take a step (`docs/GOALS.md`) — except while
    /// startled (only the turn plays) or dead.
    fn run_agents(&mut self) {
        for i in 0..self.fighters.len() {
            let f = &self.fighters[i];
            if !matches!(f.brain, Some(crate::ai::Brain::Agent(_)))
                || !f.alive()
                || f.motion.dying
                || f.passive
            {
                continue;
            }
            if f.startling.is_some() || f.film.is_some() || f.console.is_some() {
                continue;
            }
            self.with_agent(i, |a, cx| a.update(cx));
            // What it asks of its body: an animation, health, the stance.
            let asked = match self.fighters[i].brain.as_mut() {
                Some(crate::ai::Brain::Agent(a)) => Some((
                    a.anim_request.take(),
                    std::mem::take(&mut a.heal),
                    std::mem::take(&mut a.fight_mode),
                )),
                _ => None,
            };
            // A super ninja's powers: invisibility, teleports, hanging on.
            let powers = match self.fighters[i].brain.as_mut() {
                Some(crate::ai::Brain::Agent(a)) => Some((
                    std::mem::take(&mut a.give_invisibility),
                    a.teleport_to.take(),
                    a.teleport_now.take(),
                    a.hold_height.take(),
                    a.set_super.take(),
                    a.clear_stun,
                )),
                _ => None,
            };
            if let Some((invis, to, now, hold, set_super, clear_stun)) = powers {
                let f = &mut self.fighters[i];
                if let Some(v) = set_super {
                    f.super_amount = v;
                }
                if clear_stun {
                    let m = &mut f.motion;
                    (
                        m.hit_stun,
                        m.block_stun,
                        m.stagger_stun,
                        m.soft_pause,
                        m.hard_pause,
                    ) = (0, 0, 0, 0, 0);
                }
                if invis > 0 {
                    self.give_powerup(i, "invis", Some(invis), true);
                }
                if let Some(t) = to {
                    self.fighters[i].teleport_dest = Some(t);
                }
                let f = &mut self.fighters[i];
                if let Some(y) = hold {
                    let mut at = f.feet();
                    at.y = y;
                    f.teleport(at);
                }
                if let Some(t) = now {
                    // Straight into the teleport-out, seen.
                    f.teleport_dest = Some(t);
                    f.inventory.invisibility = 0;
                    let ch = f.character.clone();
                    f.motion.force_stand(&ch);
                    f.motion.request(
                        &ch,
                        crate::anim::motion::Urge::Force,
                        false,
                        crate::anim::types::ty::TELEPORT_OUT,
                    );
                }
            }
            if let Some((anim, heal, fight)) = asked {
                let f = &mut self.fighters[i];
                // (The stance first: the special moves are its clips.)
                if fight {
                    f.motion.fight_mode();
                }
                if let Some((kind, force)) = anim {
                    let ch = f.character.clone();
                    let urge = if force {
                        crate::anim::motion::Urge::Force
                    } else {
                        crate::anim::motion::Urge::Appropriate
                    };
                    f.motion.request(&ch, urge, false, kind);
                }
                if heal > 0 && f.alive() {
                    f.hp = (f.hp + heal).min(f.max_hp);
                }
            }
            // A gun it bends for: its own (useless) one goes to the floor.
            let (want, drop) = match self.fighters[i].brain.as_mut() {
                Some(crate::ai::Brain::Agent(a)) => {
                    (a.want_pickup.take(), std::mem::take(&mut a.drop_gun))
                }
                _ => (None, false),
            };
            if drop {
                self.drop_weapon(i, false);
            }
            if let Some(w) = want.filter(|_| self.fighters[i].pickup.is_none()) {
                self.drop_weapon(i, false);
                if self.start_pickup(i, Item::Weapon(w)) {
                    eprintln!(
                        "ai: fighter {i} bends for {}",
                        self.armory.weapons[w].class.name
                    );
                }
            }
            // A console it wants to use.
            if let Some(crate::ai::Brain::Agent(a)) = self.fighters[i].brain.as_mut() {
                if let Some(c) = a.use_console.take() {
                    self.fighters[i].console = Some(crate::consoles::Use::Stopping(c));
                }
            }
            // A panicking character cowers.
            let f = &mut self.fighters[i];
            if let Some(crate::ai::Brain::Agent(a)) = &f.brain {
                let panic = a.panicking();
                if panic != (f.motion.varient & crate::anim::types::var::PANIC != 0) {
                    f.motion.set_panic(panic);
                }
            }
        }
    }

    /// Gives fighter `id` goals: a placed character's job (idle, or its
    /// patrol path), alerts, pursuit settings and combat / melee profiles —
    /// or, unplaced, the defaults (idle, its class's combat profile).
    pub fn give_goals(
        &mut self,
        id: usize,
        placed: Option<&crate::ai::level::Placed>,
        melee: Option<Rc<crate::ai::melee::profile::Profile>>,
    ) {
        use crate::ai::goals::{Agent, Job, Patrol};
        let level = self.level_ai.clone();
        let player_team = self
            .fighters
            .first()
            .map_or(crate::ai::perception::Team::Konoko, |p| p.team());
        let enemy_hp = self.difficulty.enemy_hp;
        let Some(f) = self.fighters.get_mut(id) else {
            return;
        };
        let class = f.class_combat;
        let profile_of = |cid: u16| level.combat.get(&cid).cloned().unwrap_or_default();
        let mut agent = match placed {
            Some(p) => {
                let job = match (p.job.as_str(), p.job_id) {
                    ("patrol", Some(pid)) => match level.patrols.get(&pid) {
                        Some(path) => Job::Patrol(Box::new(Patrol::new(path.clone()))),
                        None => {
                            eprintln!("ai: {}: no patrol path {pid}: idle", p.name);
                            Job::Idle
                        }
                    },
                    _ => Job::Idle,
                };
                let mut a = Agent::new(
                    p.name.clone(),
                    job,
                    profile_of(p.combat_id),
                    class,
                    id as u32 * 7919 + 3,
                );
                a.job_alert = p.alert[2];
                a.investigate_alert = p.alert[3];
                a.pursue = p.pursue.clone();
                a
            }
            None => Agent::new(
                format!("fighter {id}"),
                Job::Idle,
                profile_of(class.default_profile),
                class,
                id as u32 * 7919 + 3,
            ),
        };
        if placed.is_some() {
            f.melee_profile = melee.or_else(|| f.melee_profile.clone());
        }
        if let Some(p) = f.perception.as_mut() {
            if let Some(pl) = placed {
                p.alert = pl.alert[0];
                p.minimum_alert = pl.alert[1];
                p.team = pl.team;
                p.non_combatant = pl.has("nonCombatant");
                p.omniscient = pl.has("omniscient");
                p.boss = pl.has("boss");
            }
        }
        if let Some(pl) = placed {
            f.boss = pl.has("boss");
            f.unkillable = pl.has("unkillable");
            f.infinite_ammo = pl.has("infiniteAmmo");
            f.max_hp += pl.hp_offset;
            // Enemies of the player are as tough as the difficulty says.
            if pl.team.regards(player_team) == crate::ai::perception::Relation::Hostile {
                f.max_hp = (f.max_hp as f32 * enemy_hp) as i32;
            }
            f.hp = f.max_hp;
            f.alarm_groups = pl.alarm_groups;
        }
        f.passive = false;
        agent.goal = crate::ai::goals::Goal::None;
        f.brain = Some(crate::ai::Brain::Agent(Box::new(agent)));
        if let Some(p) = placed {
            self.arm(id, p);
        }
        // (Its neutral behaviour, from the start: `AI2rNeutral_ChangeBehavior`.)
        self.neutral_behavior(id, placed.and_then(|p| p.neutral_id), true);
    }

    /// Arms fighter `id` as `p` was placed: its gun (as full as placed) and
    /// what it carries.
    pub fn arm(&mut self, id: usize, p: &crate::ai::level::Placed) {
        {
            let inv = &mut self.fighters[id].inventory;
            inv.ammo = p.carries[0];
            inv.cells = p.carries[1];
            inv.hypos = p.carries[2];
            inv.shield = p.carries[3];
            inv.invisibility = p.carries[4];
            inv.drops = p.drops;
            inv.no_auto_drop = p.flags.iter().any(|f| f == "noAutoDrop");
            inv.lsi |= p.flags.iter().any(|f| f == "hasLsi");
            if let Some(w) = p.weapon.as_deref() {
                if self.give_weapon(id, w) {
                    if let (Some(pc), Some(g)) = (p.ammo_percent, self.fighters[id].hand) {
                        let gun = &mut self.armory.weapons[g];
                        gun.ammo = (u32::from(gun.class.max_ammo) * pc.min(100) / 100) as u16;
                    }
                } else {
                    eprintln!("ai: {}: no weapon class {w}", p.name);
                }
            }
        }
    }

    /// Loads a level's data for its AI and scripts: placed characters,
    /// paths, flags, films, combat profiles, consoles.
    pub fn load_level(&mut self, level_dir: &std::path::Path) {
        self.level_dir = level_dir.to_path_buf();
        // (Each level starts from the draw engine's fog.)
        self.fog = Default::default();
        self.level_ai = Rc::new(crate::ai::level::LevelAi::load(
            level_dir,
            &level_dir.join("../../common"),
        ));
        self.consoles = crate::consoles::load(level_dir);
        // (Cheats: once the game is won — or the developers' switch.)
        self.cheats.allowed = crate::game::won_game() || std::env::var_os("ONI_CHEATS").is_some();
        self.neutrals = crate::neutrals::load(level_dir);
        self.corpses = corpses::load(level_dir);
        // (Q21: the shipped array holds 20 bodies in all.)
        if !self.quirks.corpse_limit_20() {
            self.corpses.max = self.corpses.max.max(corpses::FIXED_MAX);
        }
        self.doors = crate::doors::Doors::load(level_dir);
        self.lasers = crate::lasers::load(level_dir);
        self.object_setups = objects::load(level_dir);
        self.objects.clear();
        let level = level_dir
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.parse().ok())
            .unwrap_or(0);
        self.hud = crate::hud::Hud::load(&level_dir.join("../../common"), level);
    }

    /// One tick of every laser trigger (behaviour per `docs/LASERS.md`): its
    /// emitters move and it blinks; each beam out reaches the level (worked
    /// out once per track frame) and trips if it meets the player's body
    /// first — firing the event list once, until a beam misses again.
    fn lasers_tick(&mut self) {
        self.beams.clear();
        for k in 0..self.lasers.len() {
            self.lasers[k].tick();
            self.laser_sounds(k, false);
            if !self.lasers[k].beaming() {
                continue;
            }
            for e in 0..self.lasers[k].emitters.len() {
                let (from, dir, frame) = self.lasers[k].beam(e);
                let env = crate::chrphys::Env::new(&self.quads, &self.doors.boxes);
                let reach = self.lasers[k].reach(frame, || {
                    let len = crate::lasers::BEAM_LENGTH;
                    crate::chrphys::level_ray(&env, from, dir * len)
                        .map_or(len, |h| h.fraction * len)
                });
                let hit = self
                    .fighters
                    .first()
                    .filter(|p| p.player && !p.gone)
                    .and_then(|p| {
                        p.body_spheres()
                            .iter()
                            .filter_map(|&(_, c, r)| {
                                crate::lasers::ray_sphere(from, dir, reach, c, r)
                            })
                            .min_by(f32::total_cmp)
                    });
                let len = hit.unwrap_or(reach);
                let l = &mut self.lasers[k];
                self.beams
                    .push((from, from + dir * crate::lasers::drawn(len), l.color));
                if hit.is_some() {
                    if !l.triggered {
                        l.triggered = true;
                        eprintln!("laser {}: tripped", l.id);
                        let events = l.events.clone();
                        self.laser_sounds(k, true);
                        self.run_events(&events, Some(0));
                    }
                } else {
                    // (Any beam missing re-arms it — with several emitters,
                    // even while another beam is on the player.)
                    l.triggered = false;
                }
            }
        }
    }

    /// Poisons fighter `i` once (`chr_poison`, called each tick).
    pub fn poison(&mut self, i: usize, amount: i32, every: i32, initial: i32) {
        let now = self.now as u64;
        if let Some(f) = self.fighters.get_mut(i) {
            f.poison(now, amount, every, initial);
        }
    }

    /// Script / event orders to laser trigger `id` (`-1`: every one, except
    /// for activate / deactivate, as in the original): "activate",
    /// "deactivate", "reset", "hide", "show", "speed" (by `factor`).
    pub fn laser_set(&mut self, id: i64, what: &str, factor: f32) {
        let every = id == crate::lasers::ALL && !matches!(what, "activate" | "deactivate");
        for l in self
            .lasers
            .iter_mut()
            .filter(|l| every || i64::from(l.id) == id)
        {
            match what {
                "activate" => l.activate(),
                "deactivate" => l.active = false,
                "reset" => l.reset(),
                "hide" => l.hidden = true,
                "show" => l.hidden = false,
                "speed" => l.set_speed(factor),
                _ => {}
            }
        }
    }

    /// An object animation by name (exactly, else ignoring case, as the
    /// original's template names go).
    pub fn object_anim(&self, name: &str) -> Option<Rc<oni_geom::objanim::ObjectAnimation>> {
        self.object_anims
            .get(name)
            .or_else(|| {
                self.object_anims
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case(name))
                    .map(|(_, a)| a)
            })
            .cloned()
    }

    /// Whether the segment from `a` to `b` meets an animating object
    /// (`AMrRayToObject`): a placed object's sphere, or the leaf of a door
    /// that isn't closed (its leaves are objects then, met by their boxes).
    pub fn segment_meets_object(&self, a: Vec3, b: Vec3) -> bool {
        self.doors
            .boxes
            .iter()
            .any(|d| d.solid && d.meets_segment(a, b))
            || (0..self.objects.len()).any(|k| {
                crate::geom::segment_meets_sphere(
                    a,
                    b,
                    self.object_matrix(k).w_axis.truncate(),
                    self.object_setups[self.objects[k].setup].radius,
                )
            })
    }

    /// The nearest placed object along a ray (`AMrRayToObject`, unit `dir`,
    /// out to `max`): the distance to where it meets the first one's sphere
    /// — 0 when it starts inside one, as shipped. (Door leaves are met as
    /// the level's door boxes.)
    pub fn ray_to_object(&self, from: Vec3, dir: Vec3, max: f32) -> Option<f32> {
        (0..self.objects.len())
            .filter_map(|k| {
                let c = self.object_matrix(k).w_axis.truncate();
                let r = self.object_setups[self.objects[k].setup].radius;
                match crate::geom::ray_sphere(from, dir, c, r)? {
                    crate::geom::Entry::Inside => Some(0.0),
                    crate::geom::Entry::At(t) => Some(t),
                }
            })
            .filter(|&t| t <= max)
            .min_by(f32::total_cmp)
    }

    /// A level quad's material (its texture's).
    pub(crate) fn quad_material(&self, q: u32) -> Option<u16> {
        let quad = self.quads.quads.get(q as usize)?;
        self.impacts
            .as_ref()?
            .texture_material(self.quads.textures.get(quad.texture as usize)?)
    }

    /// The level's glass panes (`crate::glass_parts`): solid until broken.
    pub fn attach_glass(&mut self, panes: Vec<(u32, Option<u32>, crate::Mesh)>) {
        self.glass = crate::glass::Glass::new(panes);
    }

    /// Pane `k` struck hard enough: broken, and burst into shards
    /// (`p3::glass`) — at the original's highest detail (graphics quality
    /// isn't applied yet, #37).
    pub(crate) fn shatter(
        &mut self,
        k: usize,
        damage: f32,
        blast: Option<crate::p3::glass::Blast>,
    ) {
        let Some(quad) = self.glass.panes.get(k).map(|p| p.quad) else {
            return;
        };
        if !self.glass.set(k, true, &mut self.quads) {
            return;
        }
        let Some(class) = self.armory.particles.classes.get("glass_shard") else {
            return;
        };
        let Some(q) = self.quads.quads.get(quad as usize) else {
            return;
        };
        let (normal, d) = self.quads.plane(q.plane);
        let pane = crate::p3::glass::Pane {
            corners: std::array::from_fn(|i| self.quads.corner(q, i)),
            normal,
            d,
        };
        let now = self.now as u64;
        let weapons::Armory { particles, .. } = &mut self.armory;
        let mut host = weapons::ShotHost::new(&mut self.fighters, Some(&self.quads), now);
        host.quads = Some(&self.quads);
        particles.break_glass(class, &pane, damage, blast, 2, &mut host);
    }

    /// How many of the script references `from..=to` have no unbroken
    /// pane left (`env_broken`).
    pub fn env_broken(&self, from: u32, to: u32) -> u32 {
        self.glass.env_broken(from, to)
    }

    /// `env_show ref on`: shows or hides a level part — drawn, solid and
    /// blocking sight only while shown (the original flags its quads as
    /// broken glass).
    pub fn env_show(&mut self, r: u32, on: bool) {
        if on {
            self.env_hidden.remove(&r);
        } else {
            self.env_hidden.insert(r);
        }
        for q in self.quads.quads.iter_mut().filter(|q| q.env_ref == Some(r)) {
            if on {
                q.flags &= !crate::quads::flag::BROKEN_GLASS;
            } else {
                q.flags |= crate::quads::flag::BROKEN_GLASS;
            }
        }
        // (Hiding glass is breaking it, as in the original; showing mends it.)
        self.glass.show_part(r, on, &mut self.quads);
    }

    /// One tick of every door (behaviour per `docs/DOORS.md`): it opens for
    /// anyone allowed through within its radius (an AI walking somewhere
    /// opens a manual one too) and runs its events; its leaf blocks only
    /// while closed.
    fn doors_tick(&mut self) {
        for k in 0..self.doors.list.len() {
            let d = &self.doors.list[k];
            let closed_manual = d.manual && d.state == crate::doors::State::Closed;
            let opener = self.fighters.iter().position(|f| {
                let near = f.alive() && !f.gone && f.feet().distance(d.position) < d.radius && d.may_open(f.feet(), false);
                let walking = matches!(&f.brain, Some(crate::ai::Brain::Agent(a))
                    if a.nav.as_ref().is_some_and(|n| n.state == crate::ai::navigate::State::Following));
                near && (!closed_manual || walking)
            });
            let leaf0_only = self.quirks.door_first_leaf_only();
            let d = &mut self.doors.list[k];
            let (tick, mut opener) = if closed_manual && opener.is_some() {
                d.open();
                (
                    crate::doors::Tick {
                        started: true,
                        reopen: false,
                    },
                    opener,
                )
            } else {
                (d.tick(opener.is_some(), leaf0_only), opener)
            };
            let mut started = tick.started;
            // Closing and blocked too long: it opens for whoever is in the way
            // (`OBJrDoor_CharacterOpen`), if they may open it.
            if tick.reopen {
                let blocker = self.doors.list[k].blocker;
                let feet = blocker.and_then(|b| self.fighters.get(b)).map(|f| f.feet());
                if feet.is_some_and(|p| self.doors.list[k].may_open(p, false)) {
                    self.doors.list[k].open();
                    started = true;
                    opener = blocker;
                }
            }
            let d = &mut self.doors.list[k];
            if started {
                eprintln!("door {} ({}): opens", d.id, d.class);
                let events = d.events.clone();
                self.run_events(&events, opener);
            }
            // (Its sound and its leaf follow its state — whoever changed
            // it: this tick, a script, the Action key.)
            if self.doors.quads_stale(k) {
                self.doors.mark_quads(&mut self.quads);
            }
        }
        self.doors.place_boxes();
        self.door_sounds();
    }

    /// The closed manual door fighter `i` faces in reach, and whether they
    /// may open it (`OBJrDoor_TryDoorAction`).
    fn door_in_reach(&self, i: usize) -> Option<(usize, bool)> {
        let f = self.fighters.get(i)?;
        self.doors
            .manual_in_reach(f.sight_points()[0], f.facing.facing, f.feet())
    }

    /// Whether fighter `i` could open a manual door with Action now (the
    /// autoprompt's `OBJrDoor_TryDoorAction`).
    pub(super) fn door_prompt(&self, i: usize) -> bool {
        self.door_in_reach(i).is_some_and(|(_, may)| may)
    }

    /// The player's Action at a manual door: the closed one in reach it
    /// faces most (preferring one it may open). True if there was one.
    fn door_action(&mut self, i: usize) -> bool {
        let Some((k, may)) = self.door_in_reach(i) else {
            return false;
        };
        if may {
            self.doors.list[k].open();
            eprintln!("door {}: opened (Action)", self.doors.list[k].id);
            let events = self.doors.list[k].events.clone();
            self.run_events(&events, Some(i));
        } else {
            eprintln!("door {}: locked", self.doors.list[k].id);
            let at = self.doors.list[k].position;
            self.event_sound("doorFail", Some(at));
        }
        true
    }

    /// Locks / unlocks / jams / unjams door `id` (`ALL` / -1: every door).
    pub fn door_set(&mut self, id: i64, lock: Option<bool>, jam: Option<bool>) {
        let sounds = self.doors.set(id, lock, jam);
        for (event, at) in sounds {
            self.event_sound(event, Some(at));
        }
    }

    /// The name scripts know fighter `i` by.
    pub fn name_of(&self, i: usize) -> String {
        match self.fighters.get(i).and_then(|f| f.brain.as_ref()) {
            Some(crate::ai::Brain::Agent(a)) => a.name.clone(),
            _ if i == 0 => self.player_name.clone(),
            _ => format!("fighter{i}"),
        }
    }

    /// Fighter `user` uses console `c` (its action frame came): if it is
    /// active and not yet used, it fires its events.
    pub fn activate_console(&mut self, c: usize, user: Option<usize>) {
        let Some(k) = self.consoles.get_mut(c) else {
            return;
        };
        if !k.active || k.triggered {
            eprintln!(
                "console {}: {}",
                k.id,
                if k.active { "already used" } else { "locked" }
            );
            let what = if k.active {
                "console_already_used"
            } else {
                "console_locked"
            };
            self.console_sound(c, user, what);
            return;
        }
        k.triggered = true;
        self.console_sound(c, user, "console_activate");
        let k = &mut self.consoles[c];
        eprintln!("console {} ({}): used", k.id, k.class);
        let events = k.events.clone();
        self.run_events(&events, user);
    }

    /// Carries out an event list (`Oni_Event`), with `user` as its
    /// character.
    pub fn run_events(&mut self, events: &[crate::consoles::Event], user: Option<usize>) {
        use crate::consoles::{Event, ALL};
        let who = user.map_or_else(|| "none".to_string(), |u| self.name_of(u));
        for e in events {
            match e {
                Event::Script(s) => self.script_events.push((s.clone(), vec![who.clone()])),
                Event::ActivateConsole(id) | Event::DeactivateConsole(id) => {
                    let on = matches!(e, Event::ActivateConsole(_));
                    for k in self
                        .consoles
                        .iter_mut()
                        .filter(|k| *id == ALL || k.id == *id)
                    {
                        k.active = on;
                    }
                }
                Event::ActivateAlarm(id) => self.trip_alarm(u32::from(*id), user.unwrap_or(0)),
                Event::LockDoor(id) => self.door_set(i64::from(*id), Some(true), None),
                Event::UnlockDoor(id) => self.door_set(i64::from(*id), Some(false), None),
                // (By id only: the original's "every laser" doesn't apply here.)
                Event::ActivateLaser(id) => self.laser_set(i64::from(*id), "activate", 1.0),
                Event::DeactivateLaser(id) => self.laser_set(i64::from(*id), "deactivate", 1.0),
                Event::ActivateTurret(id) => self.turret_set(i64::from(*id), "activate"),
                Event::DeactivateTurret(id) => self.turret_set(i64::from(*id), "deactivate"),
                Event::Other(kind, id) => eprintln!("event {kind} {id} (not carried out yet)"),
            }
        }
    }

    /// An alarm of group `group` goes off, caused by fighter `cause`: every
    /// AI answering that group (not passive, not ignoring the cause) learns
    /// where the cause is, and its alarm script is due.
    pub fn trip_alarm(&mut self, group: u32, cause: usize) {
        if group >= 32 {
            return;
        }
        let Some(c) = self.fighters.get(cause) else {
            return;
        };
        let (team, at, player, now) = (c.team(), c.feet(), c.player, self.now as u64);
        for (i, f) in self.fighters.iter_mut().enumerate() {
            if i == cause
                || f.player
                || f.passive
                || !f.alive()
                || f.alarm_groups & (1 << group) == 0
            {
                continue;
            }
            let Some(p) = f.perception.as_mut() else {
                continue;
            };
            if p.ignores(i, cause, team, player, false) {
                continue;
            }
            p.perceive(
                Some(cause),
                Some(team),
                crate::ai::perception::Stimulus::Alarm,
                crate::ai::perception::Strength::Strong,
                at,
                0,
                now,
            );
            f.alarmed = true;
        }
        eprintln!("alarm {group} tripped");
    }

    /// The console fighter `i` could use now (Action pressed): the one whose
    /// marker is nearest its pelvis, if it is in reach.
    fn console_in_reach(&self, i: usize, generous: bool) -> Option<usize> {
        let f = self.fighters.get(i)?;
        let pelvis = f.sight_points()[0];
        let (c, k) = self.consoles.iter().enumerate().min_by(|a, b| {
            a.1.marker
                .distance_squared(pelvis)
                .total_cmp(&b.1.marker.distance_squared(pelvis))
        })?;
        k.in_reach(pelvis, f.facing.facing, generous).then_some(c)
    }

    /// One tick of fighter `i` using a console (behaviour per
    /// `scripting/BSL.md`, *Consoles*): come to a stop, walk onto the
    /// marker, play the console animation, and use it on its action frame.
    fn console_tick(&mut self, i: usize) {
        use crate::anim::motion::Urge;
        use crate::anim::types::st;
        use crate::consoles::Use;
        let Some(u) = self.fighters[i].console else {
            return;
        };
        let now = self.now as u64;
        let ty = |n: &str| oni_schema::value(&oni_schema::ANIM_TYPES, n).unwrap_or(0);
        let Some(k) = self.consoles.get(u.console()).cloned() else {
            self.fighters[i].console = None;
            return;
        };
        let f = &mut self.fighters[i];
        let ch = f.character.clone();
        let kind = oni_schema::name(&oni_schema::ANIM_TYPES, f.motion.kind).unwrap_or("");
        if crate::anim::reaction::is_victim(f.motion.kind) || kind.starts_with("block") {
            f.console = None; // knocked out of it
            return;
        }
        match u {
            Use::Stopping(c) => {
                if f.motion.state == st::STANDING {
                    f.console = Some(Use::Walking(c, now + crate::consoles::walk_in_ticks()));
                }
            }
            Use::Walking(c, deadline) => {
                if now > deadline {
                    f.console = None;
                    return;
                }
                f.facing.desired_facing = k.facing;
                let (step, there) = crate::consoles::walk_in_step(f.feet(), k.stand());
                if there {
                    f.facing.facing = k.facing;
                    f.facing.desired_facing = k.facing;
                    f.teleport(k.stand());
                    f.motion.force_stand(&ch);
                    let anim = if k.punch {
                        ty("consolePunch")
                    } else {
                        ty("console")
                    };
                    if f.motion.request(&ch, Urge::Force, false, anim).is_none() {
                        f.motion.request(&ch, Urge::Force, false, ty("console"));
                    }
                    f.console = Some(Use::Using(c));
                } else {
                    f.physics.feet += step;
                    if f.motion.kind != ty("consoleWalk") {
                        f.motion.request(&ch, Urge::Force, false, ty("consoleWalk"));
                    }
                }
            }
            Use::Using(c) => {
                if !matches!(kind, "console" | "consolePunch") {
                    f.console = None;
                    return;
                }
                let len = f.motion.length().saturating_sub(1);
                let fire = f.motion.current().action_frame.unwrap_or(len).min(len);
                if f.motion.frame >= fire {
                    f.console = None;
                    self.activate_console(c, Some(i));
                    if matches!(self.fighters[i].brain, Some(crate::ai::Brain::Agent(_))) {
                        self.with_agent(i, |a, cx| a.console_used(cx));
                    }
                }
            }
        }
    }

    /// Plays film `name` on fighter `id` (`mode`: "" from the film's start,
    /// "fromhere" where it stands, "interp" easing there over `interp`
    /// ticks). Returns the film's length, or None if it can't play.
    pub fn play_film(&mut self, id: usize, name: &str, mode: &str, interp: u32) -> Option<u32> {
        let film = self.level_ai.films.get(name).cloned()?;
        let f = self.fighters.get_mut(id)?;
        // (A film ends any environment animation.)
        f.env_anim = None;
        let mut play = crate::fighter::FilmPlay {
            film: film.clone(),
            frame: 0,
            interp: None,
        };
        // (Its facing, the facing it wants and its aim snap to the film's,
        // `ONrFilm_Start`.)
        f.facing.facing = film.facing;
        f.facing.desired_facing = film.desired_facing;
        f.facing.facing_modifier = 0.0;
        (f.aiming_lr, f.aiming_ud) = film.aim;
        match mode {
            "fromhere" => {}
            "interp" => {
                let delta = film.start - f.feet();
                if delta.length() > FILM_MAX_EASE {
                    return None; // too far to ease into
                }
                let n = interp.max(1);
                play.interp = Some((delta / n as f32, n));
            }
            _ => f.teleport(film.start),
        }
        f.film = Some(play);
        Some(film.length)
    }

    /// Removes fighter `id` from the level (it stays in the list, gone).
    pub fn remove(&mut self, id: usize) {
        if let Some(f) = self.fighters.get_mut(id) {
            f.gone = true;
            f.brain = None;
            f.perception = None;
            f.physics.feet = Vec3::new(0.0, -100_000.0, 0.0);
        }
        self.fights.leave(id);
    }

    /// Puts fighter `id` at `at` (feet), facing `facing` if given.
    pub fn teleport(&mut self, id: usize, at: Vec3, facing: Option<f32>) {
        if let Some(f) = self.fighters.get_mut(id) {
            f.teleport(at);
            if let Some(y) = facing {
                f.facing.facing = y;
                f.facing.desired_facing = y;
            }
        }
    }

    /// Orders fighter `id` along the level's patrol path named `path`.
    pub fn order_path(&mut self, id: usize, path: &str) {
        let Some(p) = self
            .level_ai
            .patrols
            .values()
            .find(|p| p.name.eq_ignore_ascii_case(path))
            .cloned()
        else {
            eprintln!("ai: no patrol path {path:?}");
            return;
        };
        self.with_agent(id, |a, cx| a.do_path(p, cx));
    }

    /// What fighter `id` is doing becomes its job.
    pub fn make_job(&mut self, id: usize) {
        if let Some(crate::ai::Brain::Agent(a)) =
            self.fighters.get_mut(id).and_then(|f| f.brain.as_mut())
        {
            a.make_job();
        }
    }

    /// Every AI passive or not (`ai2_allpassive`, a cutscene): those there
    /// now, and — while on — those spawned later.
    pub fn set_all_passive(&mut self, on: bool) {
        self.all_passive = on;
        for id in 1..self.fighters.len() {
            self.set_passive(id, on);
        }
    }

    /// Makes fighter `id` passive (doing nothing, noticing nothing) or not.
    pub fn set_passive(&mut self, id: usize, on: bool) {
        if let Some(f) = self.fighters.get_mut(id) {
            if f.player {
                return;
            }
            f.passive = on;
        }
        if on
            && matches!(
                self.fighters.get(id).and_then(|f| f.brain.as_ref()),
                Some(crate::ai::Brain::Agent(_))
            )
        {
            self.with_agent(id, |a, cx| a.stop(cx));
        }
    }

    /// Sends fighter `id` to raise the alarm at console `console_id` (None
    /// or 0: the nearest alarm console). A given console only has to be
    /// active and unused.
    pub fn order_alarm(&mut self, id: usize, console_id: Option<u16>) {
        if self.fighters.get(id).is_none_or(|f| f.passive) {
            return;
        }
        let pick = match console_id.filter(|&c| c != 0) {
            Some(cid) => match self
                .consoles
                .iter()
                .position(|k| k.id == cid && k.active && !k.triggered)
            {
                Some(c) => Some(c),
                None => return,
            },
            None => None,
        };
        self.with_agent(id, |a, cx| {
            let c = pick.or_else(|| a.nearest_alarm(cx));
            if c.is_some() {
                a.alarm_setup(c, cx);
            }
        });
    }

    /// Makes fighter `id` stand idle now (`ai2_idle`).
    pub fn order_idle(&mut self, id: usize) {
        self.with_agent(id, |a, cx| a.idle_now(cx));
    }

    /// Orders fighter `id` to attack fighter `target` (it knows where they
    /// are).
    /// (`force`: a forced target, else just a definite sighting.)
    pub fn order_attack(&mut self, id: usize, target: usize, force: bool) {
        let Some(t) = self.fighters.get(target) else {
            return;
        };
        let (team, feet, now) = (t.team(), t.feet(), self.now as u64);
        if let Some(p) = self
            .fighters
            .get_mut(id)
            .and_then(|f| f.perception.as_mut())
        {
            if force {
                p.force_target(target, team, feet, now);
            } else {
                p.perceive(
                    Some(target),
                    Some(team),
                    crate::ai::perception::Stimulus::SightDefinite,
                    crate::ai::perception::Strength::Definite,
                    feet,
                    0,
                    now,
                );
                p.notices.clear();
            }
        }
        if matches!(
            self.fighters.get(id).and_then(|f| f.brain.as_ref()),
            Some(crate::ai::Brain::Agent(_))
        ) {
            self.with_agent(id, |a, cx| {
                a.notice(crate::ai::perception::Notice::Combat(target), cx)
            });
        } else {
            self.fight(id, target);
        }
    }

    /// Plans a path for every navigator that needs one. Other characters'
    /// squares count as obstructions (a 5×5 block minus its corners).
    fn plan_paths(&mut self) {
        let Some(nav) = self.nav.clone() else { return };
        let now = self.now as u64;
        let feet: Vec<Vec3> = self.fighters.iter().map(|f| f.feet()).collect();
        for i in 0..self.fighters.len() {
            let Some(n) = self.fighters[i]
                .brain
                .as_mut()
                .and_then(|b| b.navigator_mut())
            else {
                continue;
            };
            if !n.wants_plan(now) {
                continue;
            }
            let (from, careful, goal) = (feet[i], n.careful, n.goal);
            let others: Vec<Vec3> = feet
                .iter()
                .enumerate()
                .filter(|&(j, _)| j != i)
                .map(|(_, p)| *p)
                .collect();
            let obstructed = |room: usize| {
                let mut set = std::collections::HashSet::new();
                let Some(g) = nav.rooms[room].grid.as_ref() else {
                    return set;
                };
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
            // Doors it may not open shut their portals to it.
            let links = self.doors.portals(&nav).clone();
            let shut: std::collections::HashSet<(usize, usize)> = links
                .iter()
                .enumerate()
                .filter(|&(k, _)| !self.doors.list[k].may_open(from, false))
                .flat_map(|(_, ps)| ps.iter().copied())
                .collect();
            let plan = crate::nav::plan::plan(&nav, from, goal, &obstructed, careful, &shut);
            if let Err(e) = &plan {
                eprintln!("nav: fighter {i}: no path {from:?} → {goal:?}: {e:?}");
            }
            if let Some(n) = self.fighters[i]
                .brain
                .as_mut()
                .and_then(|b| b.navigator_mut())
            {
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
            let target = match self.fighters[i].brain.as_ref().and_then(|b| b.melee()) {
                Some(m) if m.target < n && m.target != i => m.target,
                _ => continue,
            };
            if self.fighters[i].startle_turn() {
                *out = Some(Default::default()); // startled: just the turn
                continue;
            }
            let Some(mut brain) = self.fighters[i].brain.take() else {
                continue;
            };
            let nav = self.nav.as_deref();
            let env = crate::chrphys::Env::new(&self.quads, &self.doors.boxes);
            let me = view(&self.fighters, nav, &env, i, target);
            let them = view(&self.fighters, nav, &env, target, i);
            if let Some(m) = brain.melee_mut() {
                *out = Some(m.think(
                    &me,
                    &them,
                    &mut self.fights,
                    self.now,
                    &positions,
                    self.difficulty,
                    self.quirks,
                ));
            }
            self.fighters[i].brain = Some(brain);
        }
        outs
    }

    /// Switches every fighter (and future spawns) between fixed and faithful
    /// handling of the original's known bugs.
    pub fn set_quirks(&mut self, quirks: Quirks) {
        self.quirks = quirks;
        self.hud.stretch_portraits = quirks.portraits_stretched();
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
        if self.in_cutscene {
            self.cutscene_ticks += 1;
        }
        self.fog.tick();
        // The player's input: unless disabled, what the locked keys allow;
        // Action pressed by a console in reach starts using it.
        let controls = if self.input_enabled {
            controls & self.key_mask
        } else {
            0
        };
        // (Talking to a neutral character: what would end it, and what
        // would act, held back.)
        let controls = self.neutral_keys(controls);
        let action = controls & crate::anim::input::key::ACTION != 0
            && self.last_controls & crate::anim::input::key::ACTION == 0;
        self.last_controls = controls;
        let mut controls = controls;
        if let Some(p) = self.fighters.first_mut() {
            p.look_tick(controls & crate::anim::input::key::LOOK != 0);
        }
        // Tab pressed: a hypo (`LIc_BitMask_Hypo`).
        if controls & crate::anim::input::key::HYPO != 0
            && self.last_hypo_key == 0
            && self.fighters.first().is_some_and(|p| p.alive())
        {
            self.use_hypo(0);
        }
        self.last_hypo_key = controls & crate::anim::input::key::HYPO;
        if action
            && self
                .fighters
                .first()
                .is_some_and(|p| p.console.is_none() && p.alive())
        {
            if let Some(c) = self.console_in_reach(0, false) {
                self.fighters[0].console = Some(crate::consoles::Use::Stopping(c));
                controls &= !crate::anim::input::key::ACTION;
            } else if self.door_action(0) || self.neutral_action(0) {
                controls &= !crate::anim::input::key::ACTION;
            }
        }
        self.doors_tick();
        self.objects_tick();
        self.lasers_tick();
        for i in 0..self.fighters.len() {
            self.console_tick(i);
        }
        if self.fighters.first().is_some_and(|p| p.console.is_some()) {
            controls = 0;
        }
        self.perceive();
        self.run_agents();
        self.neutral_tick();
        self.plan_paths();
        // Someone lifted by a weapon is pulled toward the lifter (its
        // place: the pelvis over the feet, `actual_position`).
        let places: Vec<Vec3> = self
            .fighters
            .iter()
            .map(|f| {
                let p = f.sight_points()[0];
                Vec3::new(p.x, f.feet().y, p.z)
            })
            .collect();
        for f in &mut self.fighters {
            f.lift.owner_at = f.lift.owner.and_then(|o| places.get(o).copied());
        }
        let decisions = self.melee_decisions();
        // A melee AI ordering a throw may throw its target.
        for (i, d) in decisions.iter().enumerate() {
            let order = d.and_then(|d| d.throw.map(|t| (t, d.throw_victim)));
            let Some(target) = self.fighters[i]
                .brain
                .as_ref()
                .and_then(|b| b.melee())
                .map(|m| m.target)
            else {
                continue;
            };
            let sense = order.and_then(|(kind, needs)| {
                let mut s = self.throw_sense(i)?;
                s.candidates.retain(|c| c.who == target);
                s.only = Some(kind);
                s.victim_needs = needs;
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
        use crate::ai::perception::Relation;
        // With a gun, the fire buttons shoot instead of striking.
        self.check_empty(0, controls);
        let armed = self.armed(0);
        let alternate = self.fighters.first().and_then(|f| f.hand).is_some_and(|w| {
            self.armory.weapons[w]
                .class
                .has(crate::weapons::flag::HAS_ALTERNATE)
        });
        let teams: Vec<_> = self.fighters.iter().map(|f| f.team()).collect();
        let ai_control = self.ai_control;
        let mut moves: Vec<Option<Vec3>> = Vec::with_capacity(self.fighters.len());
        for (i, f) in self.fighters.iter_mut().enumerate() {
            // Who its attacks may turn toward: the living who aren't
            // friends (`docs/COMBAT.md`, *Teams*).
            let others: Vec<crate::fighter::Opponent> = feet
                .iter()
                .filter(|(j, _)| *j != i && teams[*j].regards(teams[i]) != Relation::Friend)
                .map(|&(j, p)| crate::fighter::Opponent {
                    feet: p,
                    hostile: teams[i].regards(teams[j]) == Relation::Hostile,
                })
                .collect();
            let target = f
                .brain
                .as_ref()
                .and_then(|b| b.opponent())
                .and_then(|t| feet.iter().find(|(j, _)| *j == t && *j != i))
                .map(|(_, p)| *p);
            // The player's fire buttons become melee while unarmed (weapons
            // aren't modelled yet, #10). A film plays its recorded controls.
            if f.console.is_some() {
                // Using a console: the console's walk and animation only.
                moves.push(f.plan(0, 0, &others));
                continue;
            }
            let film = f.film.as_mut().map(|p| (p.next(), p.interp.take()));
            if let Some((frame, interp)) = film {
                if let Some((step, n)) = interp {
                    f.physics.feet += step;
                    if n > 1 {
                        if let Some(p) = f.film.as_mut() {
                            p.interp = Some((step, n - 1));
                        }
                    }
                }
                match frame {
                    Some((keys, turn)) => {
                        f.facing.adjust_desired(-turn / 64.0);
                        moves.push(f.plan(
                            crate::anim::input::fire_as_melee(keys, false, false),
                            0,
                            &others,
                        ));
                        continue;
                    }
                    None => f.film = None,
                }
            }
            let (held, pressed) = if i == 0 && ai_control {
                (f.npc_controls(target), 0)
            } else if i == 0 {
                (
                    crate::anim::input::fire_as_melee(controls, armed, alternate),
                    0,
                )
            } else if let Some(d) = decisions[i] {
                f.follow(&d);
                (d.held | d.pressed, d.pressed)
            } else {
                (f.npc_controls(target), 0)
            };
            moves.push(f.plan(held, pressed, &others));
        }
        self.collide(&mut moves);
        let touched = std::cell::RefCell::new(crate::chrphys::Contacts::default());
        let mut touches = Vec::new();
        // (The objects' spheres; for an AI the loose guns' too — the player
        // walks over guns, `ONrCharacter_Callback_FindPhyCollisions`.)
        let spheres = self.object_spheres();
        let n_objects = spheres.len();
        let loose: Vec<usize> = (0..self.armory.weapons.len())
            .filter(|&w| {
                self.armory.weapons[w].owner.is_none() && self.armory.weapons[w].body.is_some()
            })
            .collect();
        let with_guns: Vec<(Vec3, f32)> = spheres
            .iter()
            .copied()
            .chain(loose.iter().map(|&w| {
                (
                    self.armory.weapons[w].position,
                    self.armory.weapons[w].class.sphere_radius,
                )
            }))
            .collect();
        let mut env = crate::chrphys::Env::new(&self.quads, &self.doors.boxes);
        env.touched = Some(&touched);
        let mut ai_env = crate::chrphys::Env::new(&self.quads, &self.doors.boxes);
        ai_env.touched = Some(&touched);
        ai_env.spheres = &with_guns;
        env.spheres = &spheres;
        let mut gun_pushes: Vec<(usize, usize, Vec3)> = Vec::new();
        for (i, (f, m)) in self.fighters.iter_mut().zip(moves).enumerate() {
            if let Some(m) = m {
                f.commit(m, if f.player { &env } else { &ai_env });
                let c = std::mem::take(&mut *touched.borrow_mut());
                f.collisions.0 = c.walls;
                f.collisions.1 = c.objects;
                // (The AI's own movement is told what it met.)
                if matches!(
                    f.brain,
                    Some(
                        crate::ai::Brain::Agent(_)
                            | crate::ai::Brain::GoTo(_)
                            | crate::ai::Brain::Route(_)
                    )
                ) {
                    let feet = f.feet();
                    for t in &c.touches {
                        let with = match (t.wall, t.door, t.object) {
                            (Some(p), _, _) => crate::ai::avoid::With::Wall(p),
                            (_, Some(k), _) => crate::ai::avoid::With::Object(k),
                            (_, _, Some(k)) => crate::ai::avoid::With::Object(1 << 20 | k),
                            _ => continue,
                        };
                        f.avoid.notify(with, t.point, t.normal, t.d, feet);
                    }
                }
                touches.extend(
                    c.touches
                        .into_iter()
                        .filter(|t| t.door.is_some())
                        .map(|t| (i, t)),
                );
                gun_pushes.extend(
                    c.pushed
                        .iter()
                        .filter(|(k, _)| *k >= n_objects)
                        .map(|&(k, p)| (i, loose[k - n_objects], p)),
                );
            }
        }
        let fighters = &self.fighters;
        self.doors.touched(&touches, |who| fighters[who].feet());
        // Pushed guns (`WPiWeaponPhysics_Callback_ReceiveForce`): once they
        // have touched down, and not by an AI going for that very gun.
        for (who, w, push) in gun_pushes {
            let wanted = matches!(&self.fighters[who].brain, Some(crate::ai::Brain::Agent(a)) if a.gun_wanted() == Some(w));
            let gun = &mut self.armory.weapons[w];
            if gun.hit_ground && !wanted {
                if let Some(b) = gun.body.as_mut() {
                    b.acceleration += push;
                }
            }
        }
        self.start_throws();
        for f in &mut self.fighters {
            f.apply_reaction();
        }
        // The level's environmental particles come at its start.
        if !self.env_particles_started {
            self.env_particles_started = true;
            self.env_particles_start();
        }
        // Turrets, then guns and shots: the player's buttons, the AIs' triggers.
        self.turrets_tick();
        let mut gun_controls = self.ai_guns();
        if let Some(c) = gun_controls.first_mut() {
            *c = controls;
        }
        self.weapons_tick(&gun_controls);
        // The dead settle (`Die_3_Cosmetic`): on the ground for over 120
        // ticks running, or dying for 15 s.
        // (Falling too long, or dying for 15 s, it makes no corpse.)
        for f in &mut self.fighters {
            // (Falling too long counts from the death on, `Dead_2_Moving`:
            // a body falling out of the world never gets to lie still.)
            // (A body that slid off an edge falls without a flight: its time
            // off the ground counts the same.)
            let air_time = f.motion.air.frames.max(f.motion.air.off_ground);
            if (f.motion.dying || f.motion.corpse)
                && !f.settled
                && air_time >= crate::fighter::MAX_AIR_FRAMES + 30
            {
                f.no_corpse = true;
                f.settled = true;
                continue;
            }
            if f.motion.corpse && !f.settled {
                if !f.airborne() && f.motion.air.off_ground == 0 {
                    f.death_still += 1;
                } else {
                    f.death_still = 0;
                }
                f.death_moving += 1;
                f.settled = f.death_still > 120 || f.death_moving > 60 * 15;
                f.no_corpse = f.death_moving > 60 * 15;
            }
        }
        // Bodies of classes that delete their dead go after the delay.
        for i in 1..self.fighters.len() {
            let f = &mut self.fighters[i];
            // (Death-locked: it dies, but its body stays — `chr_death_lock`.)
            if f.gone || f.death_lock || f.death_delete == 0 || !(f.motion.dying || f.hp <= 0) {
                continue;
            }
            match f.death_timer {
                None => f.death_timer = Some(f.death_delete),
                Some(t) if t <= 1 => self.remove(i),
                Some(t) => f.death_timer = Some(t - 1),
            }
        }
        self.corpses_tick();
        // The sounds of the moves played this tick.
        for i in 0..self.fighters.len() {
            if let Some((clip, frame)) = self.fighters[i].played.take() {
                self.anim_sound(i, clip, frame, true);
            }
        }
        // Footsteps, landings, falls this tick: their impact effects.
        for i in 0..self.fighters.len() {
            if let Some((kind, at)) = self.fighters[i].step.take() {
                let f = &self.fighters[i];
                let impact = f
                    .sounds
                    .anim
                    .get(kind)
                    .and_then(|n| self.impacts.as_ref()?.impact(n));
                let material = f.last_floor.and_then(|q| self.quad_material(q));
                if let (Some(impact), Some(material)) = (impact, material) {
                    let modifier = f.sounds.modifier;
                    let data = crate::p3::effect::Data::new(self.armory.particles.tick, at);
                    self.impact_effect(impact, material, modifier, data, 1.0, Some(i), None);
                }
            }
        }
        // Noises made this tick.
        for i in 0..self.fighters.len() {
            if let Some((stimulus, volume)) = self.fighters[i].noise.take() {
                let at = self.fighters[i].feet();
                self.sound(stimulus, at, volume, Some(i), None);
            }
        }
        // What the player hears (`ONrGameState_UpdateSoundManager`): with the
        // camera following, from their ear (feet across, head high), turned
        // with their facing; else (a script's camera) from the camera.
        let following = matches!(self.camera.mode, camera::Mode::Follow { .. });
        match self.fighters.first() {
            Some(p) if following => {
                let feet = p.feet();
                let head = p
                    .bones()
                    .get(HEAD_BONE)
                    .map_or(feet.y + p.motion.height() * 2.0, |m| m.w_axis.y);
                let f = p.facing.facing;
                self.audio.listen(
                    Vec3::new(feet.x, head, feet.z),
                    Vec3::new(f.sin(), 0.0, f.cos()),
                );
            }
            _ if !following => self
                .audio
                .listen(self.camera.view.location, self.camera.view.view),
            _ => {}
        }
        self.particle_sounds();
        self.voices_tick();
        self.timer_tick();
        self.conditions_tick();
        self.audio.tick(Some(&self.quads));
        self.cheats_tick();
        self.anim_particles_tick();
        self.slowmo_timer = self.slowmo_timer.saturating_sub(1);
        for i in 0..self.fighters.len() {
            self.inventory_tick(i);
            self.super_tick(i);
            self.teleport_tick(i);
            self.magic_tick(i);
        }
        self.hud_tick();
        self.camera_tick();
        self.lens_tick();
    }

    /// Fighter `who`'s clip `clip` is on `frame`: its animation sound, and
    /// (a main clip's) impulses on that frame — at its feet.
    pub(crate) fn anim_sound(&mut self, who: usize, clip: usize, frame: u16, main: bool) {
        let f = &self.fighters[who];
        let c = f.motion.clip_of(clip);
        let kind = oni_schema::name(&oni_schema::ANIM_TYPES, c.anim_type).unwrap_or("");
        let damage = c.attacks.iter().map(|a| a.damage).max().unwrap_or(0);
        let mut play: Vec<String> = self
            .anim_sounds
            .as_ref()
            .and_then(|s| {
                s.sound(
                    f.sounds.variant.as_deref(),
                    f.motion
                        .coll
                        .entries
                        .get(clip)
                        .map_or("", |e| e.name.as_str()),
                    kind,
                    damage,
                    frame,
                )
            })
            .map(str::to_string)
            .into_iter()
            .collect();
        if main {
            play.extend(
                c.sounds
                    .iter()
                    .filter(|(_, at)| *at == frame)
                    .map(|(n, _)| n.clone()),
            );
        }
        let at = f.feet();
        for name in play {
            self.audio
                .impulse_play(&name, Some(at), None, Some(&self.quads));
        }
    }

    /// Plays an impact effect (`docs/AUDIO.md`, *Impact effects*): for each
    /// slot the impact × material × weight finds, its sound at `at` (quieter
    /// for a slow hit: `velocity`) and the noise the AI hears (its radius ×
    /// `ai_scale`, credited to `ai1`, `ai2`). (Its particles: not yet.)
    #[allow(clippy::too_many_arguments)]
    /// `ONrImpactEffect`: each slot's entry plays its sound (louder the
    /// faster the parent was going), raises its noise, then makes its
    /// particles in turn from `data`.
    pub fn impact_effect(
        &mut self,
        impact: u16,
        material: u16,
        modifier: u8,
        data: crate::p3::effect::Data,
        ai_scale: f32,
        ai1: Option<usize>,
        ai2: Option<usize>,
    ) {
        let Some(impacts) = self.impacts.clone() else {
            return;
        };
        let at = data.position;
        for k in impacts
            .lookup(impact, material, modifier)
            .into_iter()
            .flatten()
        {
            let entry = impacts.entry(k);
            if let Some(sound) = &entry.sound {
                if !sound.impulse.is_empty() {
                    let full = self.audio.impact_velocity(&sound.impulse);
                    let volume = data
                        .parent_velocity
                        .map(Vec3::length)
                        .filter(|&s| full > 0.0 && s < full)
                        .map(|s| 0.3 + 0.7 * s * s / (full * full));
                    self.audio
                        .impulse_play(&sound.impulse, Some(at), volume, Some(&self.quads));
                }
                if let Some((stimulus, radius)) = sound.ai {
                    self.sound(stimulus, at, radius * ai_scale, ai1, ai2);
                }
            }
            for p in &entry.particles {
                let Some(class) = self.armory.particles.classes.get(&p.class) else {
                    continue;
                };
                let spec = crate::p3::effect::Spec {
                    class,
                    orientation: p.orientation,
                    location: p.location,
                };
                self.create_effect(&spec, &data);
            }
        }
    }

    /// Makes an effect's particle (`docs/PARTICLES.md`, *Effects*).
    pub(crate) fn create_effect(
        &mut self,
        spec: &crate::p3::effect::Spec,
        data: &crate::p3::effect::Data,
    ) -> Option<crate::p3::Handle> {
        let now = self.now as u64;
        let weapons::Armory { particles, .. } = &mut self.armory;
        let mut host = weapons::ShotHost::new(&mut self.fighters, Some(&self.quads), now);
        host.quads = Some(&self.quads);
        particles.create_effect(spec, data, &mut host)
    }

    /// A character's health as a colour (`ONrCharacter_GetHealthShade`):
    /// along the game's ramp.
    pub(crate) fn health_shade(&self, hp: i32, max: i32) -> u32 {
        let ramp = &self.health_color;
        let (Some(first), Some(last)) = (ramp.first(), ramp.last()) else {
            return 0xFFFF_FFFF;
        };
        let v = hp as f32 / max.max(1) as f32;
        if v <= first.0 {
            return first.1;
        }
        if v >= last.0 {
            return last.1;
        }
        let k = (1..ramp.len())
            .find(|&k| v < ramp[k].0)
            .unwrap_or(ramp.len() - 1);
        let t = ((v - ramp[k - 1].0) / (ramp[k].0 - ramp[k - 1].0)).clamp(0.0, 1.0);
        crate::decal::shade_lerp(ramp[k - 1].1, ramp[k].1, t)
    }

    /// The way fighter `a`'s current move strikes (`ONrCharacter_GetAttackVector`):
    /// its facing, turned by the move's direction.
    fn attack_vector(&self, a: usize) -> Vec3 {
        use std::f32::consts::{FRAC_PI_2, PI};
        let fa = &self.fighters[a];
        let offset = match fa.motion.current().move_direction {
            2 => PI,
            3 => FRAC_PI_2,
            4 => -FRAC_PI_2,
            _ => 0.0,
        };
        let yaw = fa.facing.facing + offset;
        Vec3::new(yaw.sin(), 0.0, yaw.cos())
    }

    /// The impact effect of fighter `a`'s blow on `b` — `result` 0 hit, 1
    /// blocked, 2 killed — by the parts that met: the attack's own impact
    /// (hit or kill), else the striking part's; on the struck part's
    /// material (`docs/AUDIO.md`, *Impact effects*).
    fn blow_effect(
        &mut self,
        a: usize,
        b: usize,
        result: usize,
        attack_parts: u32,
        struck_parts: u32,
        damage: u16,
    ) {
        // Hit or killed: the particles on the striking parts are pulsed.
        if result != 1 {
            self.anim_particles_pulse(a, attack_parts);
        }
        let Some(impacts) = self.impacts.clone() else {
            return;
        };
        let (fa, fb) = (&self.fighters[a], &self.fighters[b]);
        let struck = crate::impacts::primary_part(struck_parts);
        let material = fb
            .sounds
            .part_materials
            .get(struck)
            .cloned()
            .flatten()
            .and_then(|m| impacts.material(&m))
            .or_else(|| impacts.material("Character"));
        let mut modifier = crate::impacts::ANY;
        let special = (result != 1)
            .then(|| fa.sounds.attacks.get(&fa.motion.current().attack_name))
            .flatten()
            .and_then(|(name, m)| Some((impacts.impact(name)?, *m)));
        let impact = match special {
            Some((i, m)) => {
                modifier = m;
                Some(i)
            }
            None => (attack_parts != 0)
                .then(|| {
                    fa.sounds.part_impacts[result]
                        .get(crate::impacts::primary_part(attack_parts))
                        .cloned()
                        .flatten()
                })
                .flatten()
                .and_then(|n| impacts.impact(&n))
                .or_else(|| impacts.impact("Melee")),
        };
        if modifier == crate::impacts::ANY {
            modifier = crate::impacts::damage_modifier(damage);
        }
        let at = fb
            .bones()
            .get(struck)
            .map_or(fb.feet(), |m| m.w_axis.truncate());
        let (hp, max_hp) = (fb.hp, fb.max_hp);
        let ai_scale = match modifier {
            crate::impacts::HEAVY => 2.0,
            crate::impacts::MEDIUM => 1.5,
            _ => 1.0,
        };
        if let (Some(impact), Some(material)) = (impact, material) {
            // Back toward the attacker; flashes tinted by the victim's health.
            let normal = -self.attack_vector(a);
            let mut data = crate::p3::effect::Data::new(self.armory.particles.tick, at);
            data.cause = crate::p3::effect::Cause::MeleeHit;
            data.owner = crate::p3::owner::CHARACTER_MELEE | a as u32;
            data.normal = normal;
            data.parent_velocity = Some(normal);
            self.armory.particles.global_tint = Some(self.health_shade(hp, max_hp));
            self.impact_effect(impact, material, modifier, data, ai_scale, Some(a), Some(b));
            self.armory.particles.global_tint = None;
        }
    }

    /// What fighter `me` sees of who it could throw: everyone alive except
    /// the knockdown-resistant and anyone in a wholly invulnerable clip —
    /// and anyone behind a wall (pelvis to pelvis, what bodies collide
    /// with; the shipped `AttemptThrow` doesn't look, Q25).
    fn throw_sense(&self, me: usize) -> Option<throw::Sense> {
        let f = self.fighters.get(me)?;
        let through_walls = self.quirks.throw_through_walls();
        let mine = f.sight_points()[0];
        let candidates = self
            .fighters
            .iter()
            .enumerate()
            .filter(|&(j, o)| {
                j != me
                    // (The player never throws a friend.)
                    && !(f.player && o.team().regards(f.team()) == crate::ai::perception::Relation::Friend)
                    && o.alive()
                    && !o.motion.dying
                    && !o.motion.corpse
                    && !o.motion.tuning.knockdown_resistant
                    && !o.motion.current().has_flag(flag::INVULNERABLE)
                    && (through_walls
                        || o.feet().distance(f.feet()) > throw::SEARCH_RANGE
                        || self.quads.clear(
                            mine,
                            o.sight_points()[0],
                            crate::quads::flag::CHR_COL_SKIP,
                        ))
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
            victim_needs: 0,
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
            let dest =
                fa.feet() + glam::Quat::from_rotation_y(fa.facing.facing) * info.target_offset;
            let facing = fa.facing.facing + info.target_facing;
            let coll = fa.motion.coll.clone();
            let clip = coll.entries[t.target_clip].name.clone();
            let env = crate::chrphys::Env::new(&self.quads, &self.doors.boxes);
            let victim = &mut self.fighters[t.target];
            if victim.alive() && !victim.motion.dying {
                victim.be_thrown(a, coll, t.target_clip, fight, dest, facing, &env);
                let potency = self.potency(a);
                self.fighters[t.target].thrown_potency = potency;
                eprintln!("throw: fighter {a} throws {} ({clip})", t.target);
                // (Both seen for a moment, if invisible.)
                self.reveal(a, t.target);
                let at = self.fighters[a].feet();
                self.sound(
                    crate::ai::perception::Stimulus::SoundMelee,
                    at,
                    150.0,
                    Some(a),
                    Some(t.target),
                );
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
        for f in &mut self.fighters {
            f.collisions.2 = 0;
        }
        let spheres: Vec<Vec<(usize, Vec3, f32)>> =
            self.fighters.iter().map(|f| f.body_spheres()).collect();
        let mut blows: Vec<(usize, usize, usize, crate::skel::Attack, u32, u32)> = Vec::new();
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
                    // (Counted, and the AI's movement told — not of its
                    // combat target, `AI2rTryToPathfindThrough`; the plane
                    // halfway in, through its pelvis less half the way.)
                    let f = &mut self.fighters[me];
                    f.collisions.2 += 1;
                    let ai = match &f.brain {
                        Some(crate::ai::Brain::Agent(a)) => a.opponent() != Some(other),
                        Some(crate::ai::Brain::GoTo(_) | crate::ai::Brain::Route(_)) => true,
                        _ => false,
                    };
                    if ai {
                        let point = f.sight_points()[0] - away * 0.5;
                        let feet = f.feet();
                        f.avoid.notify(
                            crate::ai::avoid::With::Character(other),
                            point,
                            away,
                            -point.dot(away),
                            feet,
                        );
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
                        .filter(|(k, atk)| {
                            atk.damage_bits & touching != 0 && !f.already_hit(*k, vic)
                        })
                        .collect();
                    let top = hits.iter().map(|(_, a)| a.precedence).max().unwrap_or(0);
                    (hits, top)
                };
                let (from_i, prec_i) = landed(i, j, touch_i);
                let (from_j, prec_j) = landed(j, i, touch_j);
                if !from_i.is_empty() && prec_i >= prec_j {
                    blows.extend(
                        from_i
                            .into_iter()
                            .map(|(k, atk)| (i, j, k, atk, atk.damage_bits & touch_i, touch_j)),
                    );
                }
                if !from_j.is_empty() && prec_j >= prec_i {
                    blows.extend(
                        from_j
                            .into_iter()
                            .map(|(k, atk)| (j, i, k, atk, atk.damage_bits & touch_j, touch_i)),
                    );
                }
            }
        }
        for (a, b, k, attack, attack_parts, struck_parts) in blows {
            // Friends' blows pass through each other (not a thrown body's).
            let friends = self.fighters[a].team().regards(self.fighters[b].team())
                == crate::ai::perception::Relation::Friend;
            if friends
                && !self.fighters[a]
                    .motion
                    .current()
                    .has_flag(flag::THROW_TARGET)
            {
                continue;
            }
            self.fighters[a].mark_hit(k, b);
            self.land_hit(a, b, attack, (attack_parts, struck_parts));
        }
    }

    /// A melee AI's chance (percent) to make a block it is in position for:
    /// its block skill, shading toward its group skill the more attackers
    /// share its fight. None for anyone else (the shared rule alone decides).
    fn block_skill(&self, b: usize) -> Option<f32> {
        let f = &self.fighters[b];
        let m = f.brain.as_ref()?.melee()?;
        let p = &m.profile;
        let n = self.fights.attackers(m.target).len().max(1) as f32;
        let skill = if n >= 4.0 {
            p.block_group
        } else {
            p.block_skill + (p.block_group - p.block_skill) * (n - 1.0) / 3.0
        };
        Some(skill * self.difficulty.block)
    }

    /// Applies one landed attack from fighter `a` to fighter `b`; `parts`:
    /// the attacking parts that met, the parts they met.
    fn land_hit(&mut self, a: usize, b: usize, attack: crate::skel::Attack, parts: (u32, u32)) {
        use crate::anim::reaction::{self, Victim};
        use crate::anim::types::{strike, ty};
        use std::f32::consts::{FRAC_PI_2, PI, TAU};

        use crate::angle::wrap_pi as wrap;
        // A blow shows them both for a moment, if invisible
        // (`ONiCharacter_BecomeTemporarilyVisible`).
        self.reveal(a, b);
        let (attacker_feet, attack_dir) = (self.fighters[a].feet(), self.attack_vector(a));
        let quirks = self.quirks;
        // (Above full health a super-hypo class hits harder.)
        let potency = self.potency(a);
        let fb = &mut self.fighters[b];
        let to_attacker = {
            let d = attacker_feet - fb.feet();
            crate::angle::yaw_of(d)
        };
        let mut damage = (attack.damage as f32 * potency).round() as i32;
        let mut shove = attack.knockback * KNOCKBACK_SCALE * potency;

        // Hit while lying dazed: the AI gets up at once.
        if let Some(m) = fb.brain.as_mut().and_then(|b| b.melee_mut()) {
            if fb.motion.state == crate::anim::types::st::FALLEN_BACK
                || fb.motion.state == crate::anim::types::st::FALLEN_FRONT
            {
                m.daze = m.daze.min(1);
            }
        }
        // The player omnipotent (the cheat): past every defence, at least
        // a killing blow.
        let omnipotent = self.cheats.omnipotent && self.fighters[a].player;
        let fb = &mut self.fighters[b];
        if omnipotent {
            damage = damage.max(fb.hp);
        }
        // A super shield up (Mutant Muro's): invulnerable and unstoppable.
        let super_shield = fb.has_super_shield();
        if (fb.motion.invulnerable() || super_shield) && !omnipotent {
            fb.take_damage(0, if super_shield { Vec3::ZERO } else { attack_dir });
            eprintln!("hit: fighter {a} → {b}: invulnerable");
            return;
        }

        let (attacker_team, attacker_feet) = (self.fighters[a].team(), self.fighters[a].feet());
        // Blocking: automatic for a defensive character facing the attacker
        // (an AI also has to pass its block-skill roll).
        let skill = self.block_skill(b);
        let roll = self.percent();
        let fb = &mut self.fighters[b];
        let blocks = !omnipotent
            && skill.is_none_or(|p| roll < p)
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
                if fb.is_unstoppable() {
                    // (Blocks without block stun.)
                    fb.please_block = true;
                } else if attack.stagger > 0 {
                    fb.pending_reaction = ty::STAGGER;
                    fb.motion.stagger_stun = attack.stagger;
                } else {
                    fb.please_block = true;
                    fb.motion.block_stun = attack.block_stun;
                }
                eprintln!("hit: fighter {a} → {b}: blocked");
                // A blocked blow's impact (its sound, the AI's melee noise).
                self.blow_effect(a, b, 1, parts.0, parts.1, attack.damage);
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
        if fb.unstoppable {
            // No reaction, no stun.
            kind = ty::NONE;
            stun = 0;
        }
        if kind != ty::NONE {
            let victim = Victim {
                airborne: fb.airborne(),
                to_state: fb.motion.current().to_state,
                resistant: fb.motion.tuning.knockdown_resistant,
            };
            let mut chosen = reaction::adapt(kind, victim, quirks);
            if chosen == ty::HIT_OVERLAY {
                // A resistant character shrugging off a light hit: a flinch
                // over what it plays (`ONrCharacter_MinorHitReaction`).
                fb.minor_hit_reaction();
            } else {
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
                    let face = if reaction::is_behind(chosen) {
                        to_attacker + PI
                    } else {
                        to_attacker
                    };
                    fb.facing.facing = face.rem_euclid(TAU);
                }
            }
        }
        fb.take_damage(damage, attack_dir * shove);
        // The victim knows who hit it.
        let now = self.now as u64;
        if let Some(p) = fb.perception.as_mut() {
            if p.team.regards(attacker_team) != crate::ai::perception::Relation::Friend
                || p.about(a).is_some()
            {
                use crate::ai::perception::{Stimulus, Strength};
                p.perceive(
                    Some(a),
                    Some(attacker_team),
                    Stimulus::HitMelee,
                    Strength::Definite,
                    attacker_feet,
                    damage,
                    now,
                );
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
        // The blow's impact: its sound, the AI's melee noise.
        let result = if self.fighters[b].hp <= 0 { 2 } else { 0 };
        self.blow_effect(a, b, result, parts.0, parts.1, attack.damage);
    }
}

/// Whether a clip picks things up as it goes, for an AI
/// (`ONcAnimFlag_Can_Pickup`, not `ONcAnimFlag_NoAIPickup`).
fn picks_up(c: &crate::skel::Clip) -> bool {
    use crate::anim::types::flag;
    c.has_flag(flag::CAN_PICKUP) && !c.has_flag(flag::NO_AI_PICKUP)
}
