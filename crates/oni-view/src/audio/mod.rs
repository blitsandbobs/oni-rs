//! Sound (`docs/AUDIO.md`; the design: `docs/SOUND_ENGINE.md`).
//!
//! Layers, as in the original: the game's (`game`: placed ambients' earshot
//! and muffling, the impulse gate) over the sound system's (`system`: the 16
//! channels and how sounds get them, impulses, playing ambients' stages)
//! over the mixer (`mixer`: renders, with the PC volume law) and the output
//! device (`output`). Everything is decided on the 60 Hz tick.

mod defs;
mod game;
pub mod mixer;
mod objects;
pub mod output;
mod script;
mod system;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use glam::Vec3;

use mixer::{Clip, Mixer, CHANNELS};
pub use system::Stage;

/// How far the overall volume moves an update.
const OVERALL_ADJUST: f32 = 0.05;

pub struct Audio {
    defs: defs::Defs,
    clips: HashMap<String, Option<Arc<Clip>>>,
    dirs: Vec<PathBuf>,
    pub mixer: Arc<Mutex<Mixer>>,
    /// An output device drives the mixer (else each tick advances it).
    pub realtime: bool,
    chans: [system::Chan; CHANNELS],
    playing: Vec<system::Playing>,
    handles: Vec<Option<game::Handle>>,
    objects: Vec<objects::SoundObject>,
    /// Music and script ambients: their playing ambients and names.
    script_slots: Vec<Option<(Option<u32>, String)>>,
    dialog: script::Dialog,
    next_id: u32,
    listener: (Vec3, Vec3),
    tick: u64,
    rng: u64,
    overall: f32,
    desired: f32,
    /// Level intros, splashes, menus: no impulses, no placed ambients, no
    /// details.
    pub script_only: bool,
}

/// What a script sees of a playing ambient.
pub struct Heard {
    pub name: String,
    pub stage: Stage,
    /// The clip playing now on its first channel (none between parts).
    pub clip: Option<String>,
    /// Its first channel's output gains.
    pub gain: [f32; 2],
}

/// What a script sees of a channel.
pub struct ChannelState {
    pub clip: Option<String>,
    pub locked: bool,
    /// The composed linear volume, and the output gains.
    pub volume: f32,
    pub gain: [f32; 2],
}

impl Audio {
    /// Nothing to play.
    pub fn empty() -> Audio {
        Audio {
            defs: Default::default(),
            clips: HashMap::new(),
            dirs: Vec::new(),
            mixer: Arc::new(Mutex::new(Mixer::new(44100.0))),
            realtime: false,
            chans: Default::default(),
            playing: Vec::new(),
            handles: Vec::new(),
            objects: Vec::new(),
            script_slots: Vec::new(),
            dialog: Default::default(),
            next_id: 1,
            listener: (Vec3::ZERO, Vec3::Z),
            tick: 0,
            rng: 0x9E37_79B9_7F4A_7C15,
            overall: 1.0,
            desired: 1.0,
            script_only: false,
        }
    }

    /// The definitions in `common/sounds.json`; sounds from `common/audio`
    /// then the level's `audio`.
    pub fn load(common: &Path, level_dir: &Path) -> Audio {
        let mut a = Audio::empty();
        a.dirs = vec![common.join("audio"), level_dir.join("audio")];
        a.defs = defs::Defs::load(common);
        a.objects = objects::load(level_dir);
        a
    }

    /// Sound's own random numbers in [0, 1) (the original draws on the
    /// gameplay's; here sound leaves gameplay alone — `docs/QUIRKS.md`).
    fn random(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 40) as f32 / (1u64 << 24) as f32
    }

    /// A random number in [0, 1) from sound's own numbers (the original's
    /// voices roll on the gameplay's).
    pub fn chance(&mut self) -> f32 {
        self.random()
    }

    fn range(&mut self, (lo, hi): (f32, f32)) -> f32 {
        lo + self.random() * (hi - lo)
    }

    fn clip(&mut self, sound: &str) -> Option<Arc<Clip>> {
        if let Some(c) = self.clips.get(sound) {
            return c.clone();
        }
        let c = self.dirs.iter().find_map(|d| Clip::load(&d.join(format!("{sound}.wav")), sound)).map(Arc::new);
        self.clips.insert(sound.to_string(), c.clone());
        c
    }

    /// How fast a hit must be for impulse `name` to sound at full volume
    /// (0: always).
    pub fn impact_velocity(&self, name: &str) -> f32 {
        self.defs.impulses.get(name).map_or(0.0, |i| i.impact_velocity)
    }

    /// The listener: the player's ear and facing.
    pub fn listen(&mut self, ear: Vec3, facing: Vec3) {
        self.listener = (ear, facing);
    }

    /// Where the listener's ear is.
    pub fn ear(&self) -> Vec3 {
        self.listener.0
    }

    /// Sets the overall volume to head for (it moves 0.05 an update).
    pub fn set_volume(&mut self, v: f32) {
        self.desired = v.clamp(0.0, 1.0);
    }

    /// Every channel playing paused (`SSrPlayingChannels_Pause`: the main
    /// menu over a game), or every paused one going on again.
    pub fn pause_channels(&mut self, on: bool) {
        self.mixer.lock().unwrap().pause_all(on);
    }

    /// The overall volume now (`SS2rVolume_Get`).
    pub fn volume(&self) -> f32 {
        self.overall
    }

    /// Every sound stopped at once (`SSrStopAll`): the playing ones, the
    /// scripts' slots, the dialogue.
    pub fn stop_all(&mut self) {
        let ids: Vec<u32> = self.playing.iter().map(|p| p.id).collect();
        for id in ids {
            self.halt(id);
        }
        self.script_slots.clear();
        self.dialog = Default::default();
    }

    /// A script slot's sound of that name halted at once (`OSrMusic_Halt`).
    pub fn script_halt(&mut self, name: &str) {
        let ids: Vec<u32> = self.script_slots.iter().flatten().filter(|(_, n)| n.eq_ignore_ascii_case(name)).filter_map(|(id, _)| *id).collect();
        for id in ids {
            self.halt(id);
        }
        self.script_slots.retain(|s| s.as_ref().is_none_or(|(_, n)| !n.eq_ignore_ascii_case(name)));
    }

    /// A tick: without an output device the mixer first plays a tick's
    /// worth; the overall volume moves; the handles follow their sounds in
    /// and out of earshot; the playing ambients step; every channel takes its
    /// level and pan.
    pub fn tick(&mut self, sight: Option<&crate::quads::Quads>) {
        self.tick += 1;
        if !self.realtime {
            let mut m = self.mixer.lock().unwrap();
            let frames = (m.rate / 60.0).round() as usize;
            m.mix(None, frames);
        }
        self.overall += (self.desired - self.overall).clamp(-OVERALL_ADJUST, OVERALL_ADJUST);
        self.update_handles(sight);
        self.update_objects(sight);
        self.script_tick();
        let mut k = 0;
        while k < self.playing.len() {
            self.update_one(k);
            if self.playing[k].stage == Stage::Done {
                self.playing.remove(k);
            } else {
                k += 1;
            }
        }
        let levels: Vec<f32> = (0..CHANNELS).map(|c| self.level(c)).collect();
        let mut m = self.mixer.lock().unwrap();
        for (c, level) in levels.into_iter().enumerate() {
            let pan = self.chans[c].pan;
            if let Some(v) = m.voice_mut(c) {
                v.level = level;
                v.pan = pan;
            }
        }
    }

    /// The playing ambients (for scripts and tests).
    pub fn heard(&self) -> Vec<Heard> {
        let m = self.mixer.lock().unwrap();
        self.playing
            .iter()
            .map(|p| Heard {
                name: p.name.clone(),
                stage: p.stage,
                clip: p.ch1.and_then(|c| m.voice(c)).map(|v| v.now.clip.name.clone()),
                gain: p.ch1.map_or([0.0; 2], |c| m.gains(c)),
            })
            .collect()
    }

    /// The 16 channels (for scripts and tests).
    pub fn channels(&self) -> Vec<ChannelState> {
        let m = self.mixer.lock().unwrap();
        (0..CHANNELS)
            .map(|c| ChannelState {
                clip: m.voice(c).map(|v| v.now.clip.name.clone()),
                locked: self.chans[c].locked,
                volume: m.voice(c).map_or(0.0, |v| v.volume()),
                gain: m.gains(c),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::defs::{AmbientDef, Group, ImpulseDef, Permutation, PAN};
    use super::*;

    /// Sounds `name` (mono, `frames` long at a 60 Hz output: a frame a
    /// tick), each its own group.
    fn audio(sounds: &[(&str, usize, usize)]) -> Audio {
        let mut a = Audio::empty();
        a.mixer.lock().unwrap().rate = 60.0;
        for &(name, frames, channels) in sounds {
            let clip = Clip { name: name.into(), rate: 60.0, channels, samples: vec![0.1; frames * channels] };
            a.clips.insert(name.into(), Some(Arc::new(clip)));
            let p = Permutation { weight: 10, volume: (1.0, 1.0), pitch: (1.0, 1.0), sound: name.into() };
            a.defs.groups.insert(
                name.into(),
                Group { volume: 1.0, pitch: 1.0, channels: channels as u32, prevent_repeats: false, last: 0, permutations: vec![p] },
            );
        }
        a.listen(Vec3::ZERO, Vec3::Z);
        a
    }

    fn ambient(flags: u32, detail: Option<&str>, base: Option<&str>) -> AmbientDef {
        AmbientDef {
            priority: 1,
            flags,
            sphere_radius: 0.0,
            detail_time: (0.5, 0.5),
            max_distance: 10.0,
            min_distance: 50.0,
            detail: detail.map(Into::into),
            base: base.map(Into::into),
            base2: None,
            in_sound: base.map(|_| "i".into()),
            out_sound: base.map(|_| "o".into()),
            threshold: 3,
            min_occlusion: 0.0,
        }
    }

    fn hum(flags: u32) -> Audio {
        let mut a = audio(&[("i", 6, 1), ("b", 12, 1), ("o", 6, 1), ("d", 3, 1)]);
        a.defs.ambients.insert("hum".into(), ambient(flags, None, Some("b")));
        a
    }

    fn stage(a: &Audio) -> Option<(Stage, String)> {
        a.heard().first().map(|h| (h.stage, h.clip.clone().unwrap_or_default()))
    }

    #[test]
    fn in_loop_out_and_done() {
        let mut a = hum(PAN);
        let h = a.ambient_start("hum", Vec3::new(0.0, 0.0, 30.0), None).unwrap();
        assert_eq!(stage(&a), Some((Stage::In, "i".into())));
        for _ in 0..8 {
            a.tick(None);
        }
        assert_eq!(stage(&a), Some((Stage::Body, "b".into())));
        for _ in 0..40 {
            a.tick(None);
        }
        assert_eq!(stage(&a), Some((Stage::Body, "b".into())), "it loops, each pass rolled afresh");
        a.ambient_stop(h);
        let mut seen: Vec<Option<(Stage, String)>> = vec![stage(&a)];
        for _ in 0..30 {
            a.tick(None);
            if seen.last() != Some(&stage(&a)) {
                seen.push(stage(&a));
            }
        }
        assert_eq!(seen, vec![Some((Stage::Stopping, "b".into())), Some((Stage::Out, "o".into())), None]);
        assert!(a.channels().iter().all(|c| !c.locked && c.clip.is_none()), "its channel is free again");
    }

    #[test]
    fn heard_by_distance_halted_out_of_earshot_and_restarted() {
        let mut a = hum(0);
        // 30 units = 10 ft: full; 90 = 30 ft: halfway (−16.7 dB on a PC).
        a.ambient_start("hum", Vec3::new(0.0, 0.0, 30.0), None).unwrap();
        a.tick(None);
        assert_eq!(a.heard()[0].gain, [1.0, 1.0]);
        a.listen(Vec3::new(0.0, 0.0, -60.0), Vec3::Z);
        a.tick(None);
        assert!((a.heard()[0].gain[0] - 0.1468).abs() < 1e-3, "{:?}", a.heard()[0].gain);
        a.listen(Vec3::new(0.0, 0.0, -200.0), Vec3::Z);
        a.tick(None);
        assert!(a.heard().is_empty(), "halted beyond 50 ft, no out sound");
        a.listen(Vec3::ZERO, Vec3::Z);
        a.tick(None);
        assert_eq!(stage(&a), Some((Stage::In, "i".into())), "back in earshot: from its in sound");
    }

    #[test]
    fn pans_toward_its_side() {
        let a = hum(PAN);
        // Facing +Z, right is −X: full right, −16.7 dB left.
        let (v, pan) = a.spatialize(Some(Vec3::new(-20.0, 0.0, 0.0)), 10.0, 50.0, true);
        assert_eq!((v, pan.0), (1.0, mixer::Side::Right));
        assert!((pan.1 - 0.5).abs() < 1e-5);
        let (_, pan) = a.spatialize(Some(Vec3::new(0.0, 0.0, 20.0)), 10.0, 50.0, true);
        assert_eq!(pan.0, mixer::Side::Centre, "straight ahead");
    }

    #[test]
    fn ten_mono_channels_then_the_quietest_is_taken() {
        let mut a = audio(&[("s", 600, 1)]);
        a.defs.impulses.insert(
            "s".into(),
            ImpulseDef { group: "s".into(), priority: 1, max_distance: 10.0, min_distance: 50.0, alt: None, impact_velocity: 0.0, min_occlusion: 0.0 },
        );
        // Ten at falling volumes fill the mono pool (channels 6..16).
        for k in 0..10 {
            let c = a.impulse_play("s", None, Some(1.0 - k as f32 * 0.05), None);
            assert_eq!(c, Some(6 + k));
        }
        // The eleventh takes the quietest of them.
        assert_eq!(a.impulse_play("s", None, Some(1.0), None), Some(15));
        // A low one only ever takes a free channel: none.
        a.defs.impulses.get_mut("s").unwrap().priority = 0;
        assert_eq!(a.impulse_play("s", None, Some(1.0), None), None);
    }

    #[test]
    fn past_its_threshold_the_alt_plays() {
        let mut a = audio(&[("s", 600, 1), ("t", 600, 1)]);
        let imp = |g: &str, alt: Option<(usize, String)>| ImpulseDef {
            group: g.into(),
            priority: 1,
            max_distance: 10.0,
            min_distance: 50.0,
            alt,
            impact_velocity: 0.0,
            min_occlusion: 0.0,
        };
        a.defs.impulses.insert("s".into(), imp("s", Some((2, "t".into()))));
        a.defs.impulses.insert("t".into(), imp("t", None));
        for _ in 0..3 {
            a.impulse_play("s", None, None, None);
        }
        let clips: Vec<String> = a.channels().into_iter().filter_map(|c| c.clip).collect();
        assert_eq!(clips, ["s", "s", "t"]);
    }

    #[test]
    fn a_detail_only_ambient_plays_details_from_the_start() {
        let mut a = audio(&[("d", 3, 1)]);
        a.defs.ambients.insert("city".into(), ambient(0, Some("d"), None));
        a.ambient_start("city", Vec3::new(0.0, 0.0, 10.0), None).unwrap();
        a.tick(None);
        assert_eq!(a.heard()[0].stage, Stage::DetailOnly);
        let detail = |a: &Audio| a.channels().iter().filter(|c| c.clip.as_deref() == Some("d")).count();
        assert_eq!(detail(&a), 1, "the first detail at once");
        for _ in 0..40 {
            a.tick(None);
        }
        assert_eq!(a.heard()[0].stage, Stage::DetailOnly, "and it goes on");
    }

    #[test]
    fn a_looping_body_rolls_each_pass_afresh() {
        let mut a = audio(&[("i", 2, 1), ("x", 4, 1), ("y", 4, 1), ("o", 2, 1)]);
        let p = |s: &str| Permutation { weight: 10, volume: (1.0, 1.0), pitch: (1.0, 1.0), sound: s.into() };
        a.defs.groups.insert(
            "b".into(),
            Group { volume: 1.0, pitch: 1.0, channels: 1, prevent_repeats: false, last: 0, permutations: vec![p("x"), p("y")] },
        );
        a.defs.ambients.insert("hum".into(), ambient(0, None, Some("b")));
        a.ambient_start("hum", Vec3::new(0.0, 0.0, 10.0), None).unwrap();
        let mut clips = std::collections::HashSet::new();
        for _ in 0..80 {
            a.tick(None);
            clips.extend(a.heard()[0].clip.clone());
        }
        assert!(clips.contains("x") && clips.contains("y"), "both permutations heard: {clips:?}");
    }

    /// `hum` (no in or out sound here: its body only) as sound objects.
    fn with_objects(test: &str, json: &str) -> Audio {
        let mut a = audio(&[("b", 12, 1)]);
        let mut def = ambient(0, None, Some("b"));
        (def.in_sound, def.out_sound) = (None, None);
        a.defs.ambients.insert("hum".into(), def);
        let dir = std::env::temp_dir().join(format!("oni-sound-objects-{}-{test}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("sounds.json"), json).unwrap();
        a.objects = objects::load(&dir);
        a
    }

    #[test]
    fn a_sphere_fades_up_while_inside_and_stops_outside() {
        // Heard within 18 ft (54 units), full within 6.
        let mut a = with_objects("sphere", r#"{"objects":[{"ambient":"hum","position":[0,0,0],"volume":0.8,"sphere":{"max":6,"min":18}}]}"#);
        a.listen(Vec3::new(0.0, 0.0, 100.0), Vec3::Z);
        a.tick(None);
        assert!(a.heard().is_empty(), "outside: silent");
        a.listen(Vec3::new(0.0, 0.0, 10.0), Vec3::Z);
        a.tick(None);
        let v = |a: &Audio| a.channels().iter().map(|c| c.volume).fold(0.0, f32::max);
        assert_eq!(a.heard().len(), 1, "inside: it starts");
        // Its muffled volume climbs 0.05 an update; × 0.8.
        for _ in 0..40 {
            a.tick(None);
        }
        assert!((v(&a) - 0.8).abs() < 1e-4, "{}", v(&a));
        a.listen(Vec3::new(0.0, 0.0, 100.0), Vec3::Z);
        a.tick(None);
        assert_eq!(a.heard()[0].stage, Stage::Stopping, "left: it plays out");
    }

    #[test]
    fn boxes_ease_in_and_out_and_hand_over() {
        let mut a = with_objects(
            "boxes",
            r#"{"objects":[
                {"ambient":"hum","position":[0,0,0],"volume":1,"box":{"min":[-10,-10,-10],"max":[10,10,10]}},
                {"ambient":"hum","position":[20,0,0],"volume":1,"box":{"min":[-10,-10,-10],"max":[10,10,10]}}]}"#,
        );
        let obj = |a: &Audio, k: usize| {
            let o = &a.sound_objects()[k];
            (o.3, o.4)
        };
        a.listen(Vec3::ZERO, Vec3::Z);
        a.tick(None);
        assert_eq!(obj(&a, 0).0, objects::State::Starting);
        for _ in 0..60 {
            a.tick(None);
        }
        assert!((obj(&a, 0).1 - 0.75).abs() < 0.03, "an ease-in, three quarters up after a second: {:?}", obj(&a, 0));
        // Into the next box (same ambient) while the first fades out: it
        // takes the sound over rather than starting another.
        a.listen(Vec3::new(25.0, 0.0, 0.0), Vec3::Z);
        a.tick(None);
        a.tick(None);
        assert_eq!(a.heard().len(), 1, "one sound");
        assert_eq!(obj(&a, 1).0, objects::State::Starting);
        assert_eq!(obj(&a, 0).0, objects::State::Off);
        // Out of both: a 2-second ease-out, then halted.
        a.listen(Vec3::new(100.0, 0.0, 0.0), Vec3::Z);
        for _ in 0..125 {
            a.tick(None);
        }
        assert!(a.heard().is_empty(), "faded out and halted");
    }

    #[test]
    fn the_overall_volume_moves_by_steps() {
        let mut a = hum(0);
        a.ambient_start("hum", Vec3::new(0.0, 0.0, 10.0), None).unwrap();
        a.set_volume(0.5);
        a.tick(None);
        assert!((a.channels().iter().map(|c| c.volume).fold(0.0, f32::max) - 0.95).abs() < 1e-5);
        for _ in 0..20 {
            a.tick(None);
        }
        assert!((a.channels().iter().map(|c| c.volume).fold(0.0, f32::max) - 0.5).abs() < 1e-5);
    }
}
