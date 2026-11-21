//! AI perception (behaviour per `docs/PERCEPTION.md`): what a character
//! knows about others — its **contacts**, each with a strength that rises
//! with stimuli (sights, sounds, hits) and decays with time — and how alarmed
//! it is (its **alert** level). The world feeds it stimuli
//! (`World::perceive`); the vision cone test lives here.

use glam::Vec3;

/// How sure a character is about a contact.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Strength {
    Dead,
    Forgotten,
    Weak,
    Strong,
    Definite,
}

impl Strength {
    pub fn name(self) -> &'static str {
        ["dead", "forgotten", "weak", "strong", "definite"][self as usize]
    }
    fn down(self) -> Strength {
        match self {
            Strength::Definite => Strength::Strong,
            Strength::Strong => Strength::Weak,
            Strength::Weak => Strength::Forgotten,
            s => s,
        }
    }
}

/// How much a contact matters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Friendly,
    HostileNoThreat,
    HostileThreat,
    ForceTarget,
}

/// What was perceived. (Weapon hits come with weapons, #10; alarms with
/// alarm consoles, #18.)
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stimulus {
    SoundIgnore,
    SoundInteresting,
    SoundDanger,
    SoundMelee,
    SoundGunshot,
    SightPeripheral,
    SightDefinite,
    HitWeapon,
    HitMelee,
    Alarm,
}

impl Stimulus {
    /// The alert a stimulus raises the perceiver to.
    pub fn alert(self) -> Alert {
        use Stimulus::*;
        match self {
            SoundIgnore => Alert::Lull,
            SoundInteresting | SightPeripheral => Alert::Low,
            SoundDanger | SoundMelee => Alert::Medium,
            SoundGunshot | Alarm => Alert::High,
            SightDefinite | HitWeapon | HitMelee => Alert::Combat,
        }
    }

    /// How long a stimulus makes a calm character glance toward it (ticks).
    pub fn glance(self) -> u32 {
        [30, 60, 90, 120, 120, 180, 180, 600, 600, 300][self as usize]
    }

    /// A noise of a fight (danger, melee, gunshot).
    pub fn combat_sound(self) -> bool {
        matches!(self, Stimulus::SoundDanger | Stimulus::SoundMelee | Stimulus::SoundGunshot)
    }
}

/// Alert levels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Alert {
    Lull,
    Low,
    Medium,
    High,
    Combat,
}

impl Alert {
    pub fn name(self) -> &'static str {
        oni_schema::ALERT_LEVELS[self as usize]
    }
    fn down(self) -> Alert {
        match self {
            Alert::Combat => Alert::High,
            Alert::High => Alert::Medium,
            Alert::Medium => Alert::Low,
            _ => Alert::Lull,
        }
    }
}

/// Teams (`oni_schema::TEAMS`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Team {
    Konoko,
    Tctf,
    Syndicate,
    Neutral,
    SecurityGuard,
    RogueKonoko,
    Switzerland,
    SyndicateAccessory,
}

/// How a team regards another.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Relation {
    Friend,
    Neutral,
    Hostile,
}

impl Team {
    pub fn from_name(n: &str) -> Team {
        use Team::*;
        match n {
            "konoko" => Konoko,
            "tctf" => Tctf,
            "syndicate" => Syndicate,
            "securityGuard" => SecurityGuard,
            "rogueKonoko" => RogueKonoko,
            "switzerland" => Switzerland,
            "syndicateAccessory" => SyndicateAccessory,
            _ => Neutral,
        }
    }

    /// How this team regards `other` (the original's table — not symmetric:
    /// the Syndicate is hostile to nearly everyone, who are mostly only
    /// neutral to it).
    pub fn regards(self, other: Team) -> Relation {
        use Relation::{Friend, Hostile};
        use Team::*;
        if self == other || self == Switzerland || other == Switzerland {
            return Friend;
        }
        match (self, other) {
            (Konoko, Tctf) | (Tctf, Konoko) => Friend,
            (Konoko, Syndicate) => Hostile,
            (Tctf, Syndicate | RogueKonoko) => Hostile,
            (Syndicate, SyndicateAccessory) | (SyndicateAccessory, Syndicate) => Friend,
            (Syndicate, _) => Hostile,
            (SecurityGuard, Syndicate | RogueKonoko) => Hostile,
            (RogueKonoko, Syndicate | Tctf) => Hostile,
            _ => Relation::Neutral,
        }
    }
}

/// A class's perception tuning (`classes.json` `perception`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tuning {
    pub central_distance: f32,
    pub peripheral_distance: f32,
    /// Cone angles (radians).
    pub vertical: f32,
    pub central_full: f32,
    pub central_max: f32,
    pub peripheral_full: f32,
    pub peripheral_max: f32,
    pub hearing: f32,
    /// Ticks for a contact to drop a step from definite / strong / weak.
    pub contact_decay: [u32; 3],
    /// Ticks for the alert to drop from high / medium / low (0 = never).
    pub alert_decay: [u32; 3],
    pub never_startle: bool,
}

impl Default for Tuning {
    fn default() -> Self {
        let d = |deg: f32| deg.to_radians();
        Tuning {
            central_distance: 475.0,
            peripheral_distance: 150.0,
            vertical: d(40.0),
            central_full: d(35.0),
            central_max: d(55.0),
            peripheral_full: d(80.0),
            peripheral_max: d(115.0),
            hearing: 1.0,
            contact_decay: [180, 900, 3600],
            alert_decay: [3600, 18000, 36000],
            never_startle: false,
        }
    }
}

impl Tuning {
    pub fn from_json(p: &serde_json::Value) -> Tuning {
        let f = |v: &serde_json::Value, d: f32| v.as_f64().map_or(d, |x| x as f32);
        let u = |v: &serde_json::Value, d: u32| v.as_u64().map_or(d, |x| x as u32);
        let base = Tuning::default();
        let v = &p["vision"];
        Tuning {
            central_distance: f(&v["centralDistance"], base.central_distance),
            peripheral_distance: f(&v["peripheralDistance"], base.peripheral_distance),
            vertical: f(&v["verticalAngle"], base.vertical),
            central_full: f(&v["centralFullAngle"], base.central_full),
            central_max: f(&v["centralMaxAngle"], base.central_max),
            peripheral_full: f(&v["peripheralFullAngle"], base.peripheral_full),
            peripheral_max: f(&v["peripheralMaxAngle"], base.peripheral_max),
            hearing: f(&p["hearingAcuity"], 1.0),
            contact_decay: [
                u(&p["contactDecay"]["definite"], 180),
                u(&p["contactDecay"]["strong"], 900),
                u(&p["contactDecay"]["weak"], 3600),
            ],
            alert_decay: [
                u(&p["alertDecay"]["high"], 3600),
                u(&p["alertDecay"]["medium"], 18000),
                u(&p["alertDecay"]["low"], 36000),
            ],
            never_startle: p["flags"].as_array().is_some_and(|a| a.iter().any(|f| f == "neverStartle")),
        }
    }

    fn contact_decay(&self, s: Strength) -> u32 {
        match s {
            Strength::Definite => self.contact_decay[0],
            Strength::Strong => self.contact_decay[1],
            Strength::Weak => self.contact_decay[2],
            _ => 0,
        }
    }

    fn alert_decay(&self, a: Alert) -> u32 {
        match a {
            Alert::High => self.alert_decay[0],
            Alert::Medium => self.alert_decay[1],
            Alert::Low => self.alert_decay[2],
            _ => 0,
        }
    }

    /// The vision cone (behaviour per `docs/PERCEPTION.md`, *Vision*): how a
    /// target at `to` (from the eye, world) is seen by an eye looking along
    /// `look` (horizontal unit). `known`: already a definite contact.
    /// `faithful_vertical`: the shipped game's vertical limit (Q9).
    pub fn sees(&self, to: Vec3, look: Vec3, known: bool, faithful_vertical: bool) -> Option<Stimulus> {
        const VERTICAL_FROM: f32 = 20.0;
        let dist = to.length();
        if dist > self.central_distance || dist < 1e-3 {
            return None;
        }
        let dir = to / dist;
        if dist > VERTICAL_FROM {
            // The class stores the vertical limit as a cosine; the shipped
            // game compares the elevation's *sine* against it, which allows
            // the complement of the authored angle.
            let limit = if faithful_vertical { self.vertical.cos() } else { self.vertical.sin() };
            if dir.y.abs() > limit {
                return None;
            }
        }
        let along = dir.dot(look);
        let (pmax, pfull, cmax, cfull) = (
            self.peripheral_max.cos(),
            self.peripheral_full.cos(),
            self.central_max.cos(),
            self.central_full.cos(),
        );
        if known {
            return (along >= pmax).then_some(Stimulus::SightDefinite);
        }
        if along < pmax {
            return None;
        }
        let ramp = |lo: f32, hi: f32| if hi > lo { ((along - lo) / (hi - lo)).clamp(0.0, 1.0) } else { 1.0 };
        let peripheral = self.peripheral_distance * ramp(pmax, pfull);
        let central = if along < cmax { 0.0 } else { self.central_distance * ramp(cmax, cfull) };
        if dist < central {
            Some(Stimulus::SightDefinite)
        } else if dist < peripheral {
            Some(Stimulus::SightPeripheral)
        } else {
            None
        }
    }
}

/// One thing a character knows about.
#[derive(Clone, Debug)]
pub struct Contact {
    /// This contact's serial (stable while it's remembered).
    pub id: u32,
    /// About whom (None: a noise with nobody behind it).
    pub who: Option<usize>,
    pub location: Vec3,
    pub time: u64,
    pub stimulus: Stimulus,
    pub strength: Strength,
    pub priority: Priority,
    pub highest: Strength,
    degrade_at: u64,
    pub damage: i32,
    pub hurt_me: bool,
    pub hurt_me_melee: bool,
    /// A neutral that turned hostile (until `hostile_until`).
    pub been_hostile: bool,
    hostile_until: u64,
}

/// What perception tells the character's behaviour this tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Notice {
    /// Fight this character (a combat-level contact).
    Combat(usize),
    /// Startled by something there (turn toward it, jumpily).
    Startle(Vec3),
    /// Glance toward a point for a while (ticks).
    Glance(Vec3, u32),
    /// A threat worth pursuing (contact id): the casual alert handler's
    /// "investigate" (`docs/GOALS.md`, *Pursuit*).
    Pursue(u32),
    /// Something changed about a contact (new information, or it faded:
    /// `degrade`) — what goals that track a contact listen to.
    Knowledge { id: u32, degrade: bool },
}

/// A character's perception: contacts, alert, and the stimuli's effects.
#[derive(Clone, Debug)]
pub struct Perception {
    pub tuning: Tuning,
    pub team: Team,
    pub contacts: Vec<Contact>,
    pub alert: Alert,
    pub minimum_alert: Alert,
    alert_timer: u32,
    /// When the alert first rose to medium or above (startle window).
    alert_raised: Option<u64>,
    pub startled_at: Option<u64>,
    /// Glancing until this tick (looks more often meanwhile).
    pub glance_until: u64,
    /// Doesn't treat neutrals as threats; never fights.
    pub non_combatant: bool,
    /// Knows everything it has ever perceived forever.
    pub omniscient: bool,
    /// A boss (its placement's flag; the player always), and — while a
    /// boss battle is on (`ai2_boss_battle`) — which characters are bosses:
    /// bosses go after bosses, the rest after the rest.
    pub boss: bool,
    pub boss_battle: Option<std::rc::Rc<Vec<bool>>>,
    /// Sees nothing (`ai2_makeblind`) / hears nothing (`ai2_makedeaf`).
    pub blind: bool,
    pub deaf: bool,
    /// What behaviour should act on, since last asked.
    pub notices: Vec<Notice>,
    /// Pays the player no attention (`ai2_makeignoreplayer`).
    pub ignore_player: bool,
    /// Locked into its patrol: stimuli change nothing (no alert, startle,
    /// pursuit or combat).
    pub locked: bool,
    next_id: u32,
    /// Lines its AI asks to say (vocalization kinds), for the world.
    pub vocals: Vec<usize>,
    /// It has said its alert (or startle) line: once.
    pub said_alert: bool,
    alert_vocal_timer: u32,
}

/// A neutral that hurt us stays hostile this long.
const HOSTILITY_TICKS: u64 = 900;
/// The startle only follows the alert's first rise within this long.
const STARTLE_TOLERANCE: u64 = 60;

impl Perception {
    /// A tick of its lines: its alert line once the delay runs out; the
    /// lines asked for, taken.
    pub fn vocal_tick(&mut self) -> Vec<usize> {
        if self.alert_vocal_timer > 0 {
            self.alert_vocal_timer -= 1;
            if self.alert_vocal_timer == 0 {
                self.vocals.push(crate::world::vocal::ALERT);
                self.said_alert = true;
            }
        }
        std::mem::take(&mut self.vocals)
    }

    pub fn new(tuning: Tuning, team: Team) -> Perception {
        Perception {
            tuning,
            team,
            contacts: Vec::new(),
            alert: Alert::Lull,
            minimum_alert: Alert::Lull,
            alert_timer: 0,
            alert_raised: None,
            startled_at: None,
            glance_until: 0,
            non_combatant: false,
            omniscient: false,
            boss: false,
            boss_battle: None,
            blind: false,
            deaf: false,
            notices: Vec::new(),
            vocals: Vec::new(),
            said_alert: false,
            alert_vocal_timer: 0,
            locked: false,
            ignore_player: false,
            next_id: 1,
        }
    }

    /// The contact with serial `id`, if still remembered.
    pub fn contact(&self, id: u32) -> Option<&Contact> {
        self.contacts.iter().find(|c| c.id == id)
    }

    /// Keeps a contact from fading this tick (it is being investigated).
    pub fn hold(&mut self, id: u32) {
        if let Some(c) = self.contacts.iter_mut().find(|c| c.id == id) {
            c.degrade_at = c.degrade_at.saturating_add(1);
        }
    }

    /// The contact about `who`, if any.
    pub fn about(&self, who: usize) -> Option<&Contact> {
        self.contacts.iter().find(|c| c.who == Some(who))
    }

    /// Whether `other` (of `their_team`) is someone to fight: hostile by the
    /// team table, or a neutral that has turned hostile or threatens us.
    pub fn potentially_hostile(&self, other: usize, their_team: Team) -> bool {
        match self.team.regards(their_team) {
            Relation::Friend => false,
            Relation::Hostile => true,
            Relation::Neutral => {
                !self.non_combatant
                    && self.about(other).is_some_and(|c| c.been_hostile || c.priority >= Priority::HostileThreat)
            }
        }
    }

    /// Whether a stimulus caused by `who` (of team `their_team`; `player`)
    /// is beneath notice: ourselves, the dead, and anyone neither side is
    /// hostile to — never the player.
    pub fn ignores(&self, me: usize, who: usize, their_team: Team, player: bool, combat_sound: bool) -> bool {
        if who == me {
            return true;
        }
        if player {
            return self.ignore_player;
        }
        if combat_sound {
            return true; // the shipped rule: fights between others are heard only through their victims
        }
        !self.potentially_hostile(who, their_team) && their_team.regards(self.team) != Relation::Hostile
    }

    /// Takes in a stimulus about `who` (or nobody) at `location`.
    #[allow(clippy::too_many_arguments)]
    pub fn perceive(
        &mut self,
        who: Option<usize>,
        their_team: Option<Team>,
        stimulus: Stimulus,
        strength: Strength,
        location: Vec3,
        damage: i32,
        now: u64,
    ) {
        let threat = |team: Option<Team>, me: Team| match team {
            None => Priority::HostileNoThreat,
            Some(t) if t.regards(me) == Relation::Hostile => Priority::HostileThreat,
            Some(t) if me.regards(t) == Relation::Hostile => Priority::HostileNoThreat,
            Some(_) => Priority::Friendly,
        };
        let idx = match self.contacts.iter().position(|c| c.who == who && who.is_some()) {
            Some(i) => i,
            None => {
                self.next_id += 1;
                self.contacts.push(Contact {
                    id: self.next_id,
                    who,
                    location,
                    time: now,
                    stimulus,
                    strength: Strength::Forgotten,
                    priority: threat(their_team, self.team),
                    highest: Strength::Forgotten,
                    degrade_at: u64::MAX,
                    damage: 0,
                    hurt_me: false,
                    hurt_me_melee: false,
                    been_hostile: false,
                    hostile_until: u64::MAX,
                });
                self.contacts.len() - 1
            }
        };
        let tuning = self.tuning;
        let c = &mut self.contacts[idx];
        c.location = location;
        c.time = now;
        c.stimulus = stimulus;
        let upgraded = strength > c.strength;
        let first_sighting = strength > c.highest;
        if first_sighting {
            c.highest = strength;
        }
        if strength >= c.strength {
            c.strength = strength;
            c.degrade_at = now + u64::from(tuning.contact_decay(strength));
        }
        if matches!(stimulus, Stimulus::HitWeapon | Stimulus::HitMelee) {
            c.hurt_me = true;
            c.hurt_me_melee |= stimulus == Stimulus::HitMelee;
            c.damage += damage;
            if c.priority == Priority::Friendly {
                c.been_hostile = true;
                c.priority = Priority::HostileThreat;
                c.hostile_until = now + HOSTILITY_TICKS;
            } else {
                c.been_hostile = true;
                c.priority = c.priority.max(Priority::HostileThreat);
            }
        }
        let contact = c.clone();
        self.rank();
        if !self.non_combatant {
            // (Non-combatants don't get alerted: their goals judge danger.)
            self.notify(&contact, upgraded, first_sighting, now);
        }
        self.notices.push(Notice::Knowledge { id: contact.id, degrade: false });
    }

    /// Orders the contacts most important first (behaviour per
    /// `docs/PERCEPTION.md`, *Memory*).
    fn rank(&mut self) {
        let mut contacts = std::mem::take(&mut self.contacts);
        contacts.sort_by(|a, b| self.compare(a, b));
        self.contacts = contacts;
    }

    /// `AI2rKnowledge_CompareFunc` as this AI ranks them: in a boss battle,
    /// one of its own kind (boss or not) first — dropping it only once
    /// forgotten — then [`compare`]'s order.
    pub fn compare(&self, a: &Contact, b: &Contact) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        let preference = match (&self.boss_battle, a.who, b.who) {
            (Some(bosses), Some(x), Some(y)) => {
                let (bx, by) = (bosses.get(x).copied().unwrap_or(false), bosses.get(y).copied().unwrap_or(false));
                if bx == self.boss && by != self.boss {
                    Ordering::Less
                } else if bx != self.boss && by == self.boss {
                    Ordering::Greater
                } else {
                    Ordering::Equal
                }
            }
            _ => Ordering::Equal,
        };
        if preference == Ordering::Equal {
            return compare(a, b);
        }
        let ignorable = |c: &Contact| c.strength <= Strength::Forgotten;
        let solid = |c: &Contact| c.strength >= Strength::Strong;
        if ignorable(a) && solid(b) {
            Ordering::Greater
        } else if ignorable(b) && solid(a) {
            Ordering::Less
        } else {
            preference
        }
    }

    /// Runs the alert handler again on the most important contact (a patrol
    /// lock released); returns what it noticed.
    pub fn reconsider(&mut self, now: u64) -> Vec<Notice> {
        let before = self.notices.len();
        if let Some(top) = self.contacts.first().cloned() {
            self.notify(&top, false, false, now);
        }
        self.notices.split_off(before)
    }

    /// Where a contact is now known to be (combat tracks its target).
    pub fn set_location(&mut self, id: u32, at: Vec3) {
        if let Some(c) = self.contacts.iter_mut().find(|c| c.id == id) {
            c.location = at;
        }
    }

    /// The casual alert handler (`Oni_AI2_Alert.c`; `docs/PERCEPTION.md`,
    /// *Alert*): new information about `c` may glance at a non-threat, raise
    /// the alert (startling, or starting combat at combat level) and ask to
    /// pursue a threat.
    fn notify(&mut self, c: &Contact, upgraded: bool, first_sighting: bool, now: u64) {
        if self.locked {
            return;
        }
        let top = self.contacts.first();
        let is_top = top.is_some_and(|t| t.id == c.id);
        if !is_top && top.is_some_and(|t| t.strength == Strength::Definite) {
            return; // its main concern is in plain sight
        }
        let threat = c.stimulus.combat_sound() || c.priority >= Priority::HostileNoThreat;
        if !threat {
            if upgraded {
                self.glance_until = now + u64::from(c.stimulus.glance());
                self.notices.push(Notice::Glance(c.location, c.stimulus.glance()));
            }
            return;
        }
        if !is_top || c.strength <= Strength::Forgotten {
            return;
        }
        let level = c.stimulus.alert();
        if level >= self.alert {
            if level != self.alert {
                self.alert = level;
                if level >= Alert::Medium && self.alert_raised.is_none() {
                    self.alert_raised = Some(now);
                }
                // A jumpy start, the first time it sees a threat.
                let window = self.alert_raised.is_none_or(|t| now <= t + STARTLE_TOLERANCE);
                let startled = level >= Alert::High && !self.tuning.never_startle && first_sighting && window;
                if startled {
                    self.startled_at = Some(now);
                    self.notices.push(Notice::Startle(c.location));
                }
                // Its alert line, once: startled or straight into a fight,
                // its startle line now; else its alert line in a moment, if
                // it doesn't come to a fight first.
                if !self.said_alert {
                    if startled || level == Alert::Combat {
                        self.vocals.push(crate::world::vocal::STARTLE);
                        self.alert_vocal_timer = 0;
                        self.said_alert = true;
                    } else if self.alert_vocal_timer == 0 {
                        self.alert_vocal_timer = 60 + (now.wrapping_mul(2_654_435_761) >> 16) as u32 % 31;
                    }
                }
            }
            self.alert_timer = 0;
            if level == Alert::Combat && !self.non_combatant {
                if let Some(who) = c.who {
                    self.notices.push(Notice::Combat(who));
                    return;
                }
            }
        }
        if self.alert > Alert::Lull {
            self.notices.push(Notice::Pursue(c.id));
        }
    }

    /// Told to fight `who`: a definite contact it won't drop for another.
    pub fn force_target(&mut self, who: usize, team: Team, at: Vec3, now: u64) {
        let notices = self.notices.len();
        self.perceive(Some(who), Some(team), Stimulus::SightDefinite, Strength::Definite, at, 0, now);
        self.notices.truncate(notices); // ordered, not noticed
        if let Some(c) = self.contacts.iter_mut().find(|c| c.who == Some(who)) {
            c.priority = Priority::ForceTarget;
        }
        self.rank();
    }

    /// Forgets what it knew (`AI2rKnowledgeState_Forget`, `ai2_forget`): of
    /// `who`, or of everyone — each contact degraded to forgotten (goals
    /// tracking it hear so), then dropped.
    pub fn forget(&mut self, who: Option<usize>) {
        let matches = |c: &Contact| who.is_none() || c.who == who;
        for c in self.contacts.iter_mut().filter(|c| matches(c)) {
            c.strength = Strength::Forgotten;
            self.notices.push(Notice::Knowledge { id: c.id, degrade: true });
        }
        self.contacts.retain(|c| !matches(c));
        self.rank();
    }

    /// Someone died: what we knew of them is dead.
    pub fn died(&mut self, who: usize) {
        for c in self.contacts.iter_mut().filter(|c| c.who == Some(who)) {
            c.strength = Strength::Dead;
            self.notices.push(Notice::Knowledge { id: c.id, degrade: true });
        }
        self.rank();
    }

    /// Every 60 ticks: contacts fade a step once their time is up; hostility
    /// wears off. `visible(who)`: can that character be seen at all (not
    /// invisible).
    pub fn decay(&mut self, now: u64, visible: &dyn Fn(usize) -> bool) {
        let tuning = self.tuning;
        for c in &mut self.contacts {
            if c.strength <= Strength::Forgotten {
                continue;
            }
            if c.been_hostile && now >= c.hostile_until {
                c.been_hostile = false;
                if c.priority == Priority::HostileThreat {
                    c.priority = Priority::HostileNoThreat;
                }
                continue;
            }
            let unseen = c.strength == Strength::Definite && c.who.is_some_and(|w| !visible(w));
            if unseen || (now >= c.degrade_at && !self.omniscient) {
                c.strength = c.strength.down();
                c.degrade_at = now + u64::from(tuning.contact_decay(c.strength));
                self.notices.push(Notice::Knowledge { id: c.id, degrade: true });
            }
        }
        self.rank();
    }

    /// Each tick while not fighting: the alert eases a level once its time
    /// is up (never below the minimum; a time of 0 means it doesn't).
    pub fn calm(&mut self) {
        if self.alert <= self.minimum_alert || self.alert == Alert::Combat {
            return;
        }
        if self.alert_timer == 0 {
            let t = self.tuning.alert_decay(self.alert);
            if t == 0 {
                return;
            }
            self.alert_timer = t;
        }
        self.alert_timer -= 1;
        if self.alert_timer == 0 {
            self.alert = self.alert.down().max(self.minimum_alert);
            if self.alert < Alert::Medium {
                self.alert_raised = None;
            }
        }
    }

    /// How often (ticks) it looks around now: every 4 just after a startle,
    /// 15 while glancing, else 59.
    pub fn look_every(&self, now: u64) -> u64 {
        if self.startled_at.is_some_and(|t| now < t + 180) && self.alert < Alert::Combat {
            4
        } else if now < self.glance_until {
            15
        } else {
            59
        }
    }
}

/// How two contacts rank: `Less` when `a` comes first (more important).
pub fn compare(a: &Contact, b: &Contact) -> std::cmp::Ordering {
    let solid = |c: &Contact| c.strength >= Strength::Strong;
    solid(b)
        .cmp(&solid(a))
        .then(b.priority.cmp(&a.priority))
        .then(b.strength.cmp(&a.strength))
        .then(b.hurt_me.cmp(&a.hurt_me))
        .then(b.hurt_me_melee.cmp(&a.hurt_me_melee))
        .then(b.highest.cmp(&a.highest))
}

/// A unit horizontal direction for a yaw.
pub fn forward(yaw: f32) -> Vec3 {
    Vec3::new(yaw.sin(), 0.0, yaw.cos())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cone_sees_ahead_far_and_aside_near() {
        let t = Tuning::default();
        let look = forward(0.0);
        assert_eq!(t.sees(Vec3::new(0.0, 0.0, 300.0), look, false, false), Some(Stimulus::SightDefinite));
        assert_eq!(t.sees(Vec3::new(0.0, 0.0, 500.0), look, false, false), None, "beyond sight");
        // 70° off: inside the peripheral cone only, near enough.
        let a = 70f32.to_radians();
        assert_eq!(t.sees(Vec3::new(a.sin(), 0.0, a.cos()) * 100.0, look, false, false), Some(Stimulus::SightPeripheral));
        assert_eq!(t.sees(Vec3::new(a.sin(), 0.0, a.cos()) * 200.0, look, false, false), None);
        // Behind: never.
        assert_eq!(t.sees(Vec3::new(0.0, 0.0, -50.0), look, false, false), None);
        // A known target stays seen anywhere in the widest cone.
        assert_eq!(t.sees(Vec3::new(a.sin(), 0.0, a.cos()) * 400.0, look, true, false), Some(Stimulus::SightDefinite));
        // 45° up, 100 away: over the authored 40° — but within the shipped 50°.
        let up = Vec3::new(0.0, 1.0, 1.0).normalize() * 100.0;
        assert_eq!(t.sees(up, look, false, false), None);
        assert!(t.sees(up, look, false, true).is_some());
    }

    #[test]
    fn contacts_rise_decay_and_alarm() {
        let mut p = Perception::new(Tuning::default(), Team::Syndicate);
        p.perceive(Some(0), Some(Team::Konoko), Stimulus::SightPeripheral, Strength::Weak, Vec3::ZERO, 0, 0);
        assert_eq!(p.alert, Alert::Low);
        p.perceive(Some(0), Some(Team::Konoko), Stimulus::SightDefinite, Strength::Definite, Vec3::ZERO, 0, 10);
        assert_eq!(p.alert, Alert::Combat);
        assert!(p.notices.contains(&Notice::Combat(0)));
        // Unseen, a definite contact fades a step after its time.
        p.decay(10 + 180, &|_| true);
        assert_eq!(p.about(0).unwrap().strength, Strength::Strong);
    }

    #[test]
    fn in_a_boss_battle_bosses_go_after_bosses() {
        let mut p = Perception::new(Tuning::default(), Team::Syndicate);
        // 0 the player (a boss), 1 another boss, 2 not.
        p.boss_battle = Some(std::rc::Rc::new(vec![true, true, false]));
        p.perceive(Some(0), Some(Team::Konoko), Stimulus::SightDefinite, Strength::Definite, Vec3::ZERO, 0, 0);
        p.perceive(Some(2), Some(Team::Konoko), Stimulus::SightDefinite, Strength::Definite, Vec3::ZERO, 0, 0);
        assert_eq!(p.contacts[0].who, Some(2), "not a boss: the non-boss first");
        p.boss = true;
        p.rank();
        assert_eq!(p.contacts[0].who, Some(0), "a boss: the boss first");
        // Faded to weak, still preferred — only forgotten lets it go.
        p.contacts.iter_mut().find(|c| c.who == Some(0)).unwrap().strength = Strength::Weak;
        p.rank();
        assert_eq!(p.contacts[0].who, Some(0));
        p.contacts.iter_mut().find(|c| c.who == Some(0)).unwrap().strength = Strength::Forgotten;
        p.rank();
        assert_eq!(p.contacts[0].who, Some(2));
        // No boss battle: as usual.
        p.boss_battle = None;
        p.contacts.iter_mut().find(|c| c.who == Some(0)).unwrap().strength = Strength::Weak;
        p.rank();
        assert_eq!(p.contacts[0].who, Some(2), "strong before weak");
    }

    #[test]
    fn teams_regard_each_other_as_in_the_original() {
        assert_eq!(Team::Syndicate.regards(Team::Neutral), Relation::Hostile);
        assert_eq!(Team::Neutral.regards(Team::Syndicate), Relation::Neutral);
        assert_eq!(Team::Konoko.regards(Team::Tctf), Relation::Friend);
        assert_eq!(Team::Konoko.regards(Team::Switzerland), Relation::Friend);
    }
}
