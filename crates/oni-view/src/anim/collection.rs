//! A character's animation collection and the lookup that picks a clip for
//! `(type, from-state, variant)` — the behaviour of Oni's `TRrCollection_Lookup`.
//!
//! How Oni resolves a request (see `docs/MOVEMENT.md` for anchors):
//!
//! * A collection (`TRAC`) is a list of clips. At load time each entry is keyed
//!   by `(type, from_state)` and the list is sorted by that key, ties ordered
//!   **most-specific variant first**. Among entries with the same key and
//!   variant, one is picked at random by authored weight.
//! * A clip's *shortcuts* make it reachable from extra states. They become
//!   additional "virtual" entries in a shortcut sub-collection that is spliced
//!   into the chain right after the collection that owns the clip.
//! * A collection may fall back to a parent (`recursiveLookup`). The chain is
//!   therefore `own₀ → shortcuts₀ → own₁ → shortcuts₁ → …`.
//! * A lookup scans the first link for the requested state; when it finds
//!   nothing — or nothing whose variant matches exactly — it asks the rest of
//!   the chain and keeps whichever result has the larger variant. If the whole
//!   chain has nothing for the state, the request is retried with state
//!   `Anything`.
//! * A clip is eligible only when its variant bits are a subset of the
//!   requested variant, and only from its `firstLevel` onward.

use std::cell::Cell;
use std::collections::HashMap;
use std::path::Path;

use crate::anim::types::st;
use crate::skel::{self, Clip};

/// A loaded clip and its instance name.
pub struct Entry {
    pub name: String,
    pub clip: Clip,
    /// Indices (into [`AnimCollection::entries`]) of this clip's direct
    /// follow-ups (`directAnimation`), resolved at load.
    pub direct: Vec<usize>,
}

/// One keyed slot in a link of the chain.
#[derive(Clone, Copy)]
struct Part {
    /// Before weighting: the authored weight. After: 0 = only entry of its run;
    /// the run's first entry holds the run total; later entries hold the
    /// exclusive prefix sum of the weights before them.
    weight: u32,
    /// The state this slot answers for (the clip's `fromState`, or a shortcut state).
    from_state: u16,
    /// Index into `entries`.
    clip: usize,
}

/// One link of the lookup chain.
struct Link {
    parts: Vec<Part>,
    next: Option<usize>,
}

/// A character's animation collection.
pub struct AnimCollection {
    pub entries: Vec<Entry>,
    links: Vec<Link>,
    /// The level being played: clips whose `firstLevel` is later are skipped.
    level: u16,
    /// Seeded generator for weighted picks (deterministic per seed).
    rng: Cell<u32>,
}

impl AnimCollection {
    /// Loads collection `name` from `<anim_dir>/collections/<name>.json`,
    /// following its `recursive` parents, and every clip it references from
    /// `<anim_dir>/<clip>.glb`.
    pub fn load(anim_dir: &Path, name: &str, level: u16) -> std::io::Result<Self> {
        // Read the chain of collection documents, root first.
        let mut docs: Vec<Vec<(String, u32)>> = Vec::new();
        let mut cur = Some(name.to_string());
        let mut seen = std::collections::HashSet::new();
        while let Some(n) = cur.take() {
            if !seen.insert(n.clone()) {
                break; // cycle guard
            }
            let path = anim_dir.join("collections").join(format!("{n}.json"));
            let text = std::fs::read_to_string(&path)?;
            let doc: serde_json::Value = serde_json::from_str(&text)
                .map_err(|e| std::io::Error::other(format!("{}: {e}", path.display())))?;
            let entries = doc["entries"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|e| {
                            Some((e["anim"].as_str()?.to_string(), e["weight"].as_u64()? as u32))
                        })
                        .collect()
                })
                .unwrap_or_default();
            docs.push(entries);
            cur = doc["recursive"].as_str().map(String::from);
        }

        // Load each referenced clip once.
        let mut entries: Vec<Entry> = Vec::new();
        let mut by_name: HashMap<String, usize> = HashMap::new();
        let mut doc_parts: Vec<Vec<(usize, u32)>> = Vec::new();
        for doc in &docs {
            let mut parts = Vec::new();
            for (anim, weight) in doc {
                let idx = match by_name.get(anim) {
                    Some(&i) => i,
                    None => {
                        let Ok(clip) = skel::load_clip(&anim_dir.join(format!("{anim}.glb")))
                        else {
                            continue; // a clip the converter could not export
                        };
                        entries.push(Entry {
                            name: anim.clone(),
                            clip,
                            direct: Vec::new(),
                        });
                        by_name.insert(anim.clone(), entries.len() - 1);
                        entries.len() - 1
                    }
                };
                parts.push((idx, *weight));
            }
            doc_parts.push(parts);
        }
        for e in &mut entries {
            e.direct = e
                .clip
                .direct
                .iter()
                .filter_map(|n| by_name.get(n).copied())
                .collect();
        }

        // Build the chain: each collection, then (if any) its shortcut entries.
        let mut links: Vec<Link> = Vec::new();
        for parts in &doc_parts {
            let own: Vec<Part> = parts
                .iter()
                .map(|&(clip, weight)| Part {
                    weight,
                    from_state: entries[clip].clip.from_state,
                    clip,
                })
                .collect();
            let shortcuts: Vec<Part> = parts
                .iter()
                .flat_map(|&(clip, _)| {
                    entries[clip].clip.shortcuts.iter().map(move |s| Part {
                        weight: 100,
                        from_state: s.state,
                        clip,
                    })
                })
                .collect();
            links.push(Link { parts: own, next: None });
            if !shortcuts.is_empty() {
                links.push(Link { parts: shortcuts, next: None });
            }
        }
        let n = links.len();
        for (i, link) in links.iter_mut().enumerate() {
            link.next = (i + 1 < n).then_some(i + 1);
        }

        let mut coll = AnimCollection {
            entries,
            links,
            level,
            rng: Cell::new(0x1234_5678),
        };
        for l in 0..coll.links.len() {
            coll.sort_and_weight(l);
        }
        Ok(coll)
    }

    /// Reseeds the weighted-pick generator (scenarios use this for repeatability).
    pub fn seed(&self, seed: u32) {
        self.rng.set(seed.max(1));
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn key(&self, p: &Part) -> u32 {
        (u32::from(self.entries[p.clip].clip.anim_type) << 16) | u32::from(p.from_state)
    }

    fn varient(&self, p: &Part) -> u16 {
        self.entries[p.clip].clip.varient
    }

    /// Sorts a link by key (variant descending within a key) and turns the
    /// authored weights into the run-total / prefix-sum form the scan uses.
    /// The sort is stable (on-disk order breaks ties); Oni used `qsort`, whose
    /// tie order is platform-defined, so only the *set* of candidates is exact.
    fn sort_and_weight(&mut self, l: usize) {
        let mut parts = std::mem::take(&mut self.links[l].parts);
        parts.sort_by(|a, b| {
            self.key(a)
                .cmp(&self.key(b))
                .then_with(|| self.varient(b).cmp(&self.varient(a)))
        });
        let mut start = 0;
        while start < parts.len() {
            let (k, v) = (self.key(&parts[start]), self.varient(&parts[start]));
            let mut stop = start;
            while stop + 1 < parts.len()
                && self.key(&parts[stop + 1]) == k
                && self.varient(&parts[stop + 1]) == v
            {
                stop += 1;
            }
            if start == stop {
                parts[start].weight = 0;
            } else {
                let mut total = 0;
                for p in &mut parts[start..=stop] {
                    let w = p.weight;
                    p.weight = total;
                    total += w;
                }
                parts[start].weight = total;
            }
            start = stop + 1;
        }
        self.links[l].parts = parts;
    }

    /// A value in `[0, n)` from the seeded generator (xorshift32).
    fn random_below(&self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        let mut x = self.rng.get();
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng.set(x);
        x % n
    }

    /// Finds the clip for `(anim_type, from_state, varient)`, returning its
    /// index into [`Self::entries`].
    pub fn lookup(&self, anim_type: u16, from_state: u16, varient: u16) -> Option<usize> {
        if self.links.is_empty() {
            return None;
        }
        self.lookup_from(0, anim_type, from_state, varient)
    }

    /// Lookup starting at chain link `l`: the requested state, then `Anything`.
    fn lookup_from(&self, l: usize, anim_type: u16, from_state: u16, varient: u16) -> Option<usize> {
        self.scan(l, anim_type, from_state, varient)
            .or_else(|| self.scan(l, anim_type, st::ANYTHING, varient))
    }

    /// One link's scan plus the "ask the rest of the chain" step.
    fn scan(&self, l: usize, anim_type: u16, from_state: u16, varient: u16) -> Option<usize> {
        let link = &self.links[l];
        if anim_type == crate::anim::types::ty::NONE {
            // No particular type requested: the first eligible clip that starts
            // from this state, of any type.
            let any = link.parts.iter().find(|p| {
                let c = &self.entries[p.clip].clip;
                p.from_state == from_state
                    && c.varient & !varient == 0
                    && c.first_level <= self.level
            });
            return any
                .map(|p| p.clip)
                .or_else(|| link.next.and_then(|n| self.lookup_from(n, anim_type, from_state, varient)));
        }
        let key = (u32::from(anim_type) << 16) | u32::from(from_state);
        let start = link.parts.partition_point(|p| self.key(p) < key);

        let mut result: Option<usize> = None;
        let mut roll = 0;
        let mut found: Option<(u16, u16)> = None; // (variant, type) of the picked run
        for p in &link.parts[start..] {
            if self.key(p) != key {
                break;
            }
            let clip = &self.entries[p.clip].clip;
            if clip.varient & !varient != 0 || clip.first_level > self.level {
                continue;
            }
            if let Some((fv, ft)) = found {
                if p.weight > roll || clip.varient != fv || clip.anim_type != ft {
                    break;
                }
            }
            result = Some(p.clip);
            if p.weight == 0 {
                break;
            }
            if found.is_none() {
                roll = self.random_below(p.weight);
                found = Some((clip.varient, clip.anim_type));
            }
        }

        let exact = result.is_some_and(|i| self.entries[i].clip.varient == varient);
        if !exact {
            if let Some(next) = link.next {
                if let Some(r) = self.lookup_from(next, anim_type, from_state, varient) {
                    let better = match result {
                        None => true,
                        Some(cur) => self.entries[r].clip.varient > self.entries[cur].clip.varient,
                    };
                    if better {
                        result = Some(r);
                    }
                }
            }
        }
        result
    }

    /// Every clip filed under exactly `(anim_type, from_state)` — shortcut
    /// entries included — through the whole chain, in chain order, with no
    /// variant or level filtering (the caller judges each one).
    pub fn all(&self, anim_type: u16, from_state: u16) -> Vec<usize> {
        let key = (u32::from(anim_type) << 16) | u32::from(from_state);
        self.links
            .iter()
            .flat_map(|link| {
                let start = link.parts.partition_point(|p| self.key(p) < key);
                link.parts[start..]
                    .iter()
                    .take_while(move |p| self.key(p) == key)
                    .map(|p| p.clip)
            })
            .collect()
    }

    /// Whether `to` is one of `from`'s direct follow-ups (`TRrAnimation_IsDirect`).
    pub fn is_direct(&self, from: usize, to: usize) -> bool {
        self.entries[from].direct.contains(&to)
    }

    /// Frames to blend from `from` into `to` when `from` ends naturally
    /// (`TRrAnimation_GetEndInterpolation`).
    pub fn end_interpolation(&self, from: usize, to: usize) -> u16 {
        blend_between(&self.entries[from].clip, &self.entries[to].clip, self.is_direct(from, to))
    }

    /// Hard / soft pause frames between two clips (0 when `to` is direct).
    pub fn hard_pause(&self, from: usize, to: usize) -> u16 {
        if self.is_direct(from, to) {
            0
        } else {
            self.entries[from].clip.hard_pause
        }
    }

    pub fn soft_pause(&self, from: usize, to: usize) -> u16 {
        if self.is_direct(from, to) {
            0
        } else {
            self.entries[from].clip.soft_pause
        }
    }

}

/// The blend length when clip `a` runs out into clip `b` (`direct`: `b` is
/// one of `a`'s direct follow-ups): a shortcut's own length, or a short blend
/// when the states or variants don't line up, and `a`'s own end blend unless
/// `b` follows on directly.
pub fn blend_between(a: &Clip, b: &Clip, direct: bool) -> u16 {
    let mut n = match b.shortcut_from(a.to_state) {
        Some(s) => s.length,
        None if a.to_state != b.from_state => 8,
        None => 0,
    };
    if a.varient != b.varient {
        n = n.max(8);
    }
    if !direct {
        n = n.max(a.end_interpolation);
    }
    n
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::anim::types::{st, ty, var};
    use std::path::PathBuf;
    use std::rc::Rc;

    /// Konoko's real collection chain, or None when assets aren't generated.
    pub(crate) fn konoko() -> Option<Rc<AnimCollection>> {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../generated/oni/common/animations");
        AnimCollection::load(&dir, "konoko_animations", 1).ok().map(Rc::new)
    }

    #[test]
    fn standing_lookups_resolve() {
        let Some(c) = konoko() else {
            eprintln!("SKIP: generated animations not found");
            return;
        };
        let stand = c.lookup(ty::STAND, st::STANDING, 0).expect("stand from standing");
        assert_eq!(c.entries[stand].clip.anim_type, ty::STAND);
        let run = c.lookup(ty::RUN_START, st::STANDING, 0).expect("run start from standing");
        assert_eq!(c.entries[run].clip.from_state, st::STANDING);
        // A combat clip needs the Fight bit: plain varient can't see it.
        assert!(c.lookup(ty::PUNCH, st::STANDING, var::FIGHT).is_some());
    }

    #[test]
    fn jump_attack_reachable_through_shortcut() {
        let Some(c) = konoko() else {
            eprintln!("SKIP: generated animations not found");
            return;
        };
        // The Fly loop ends in state 38 (Falling); KONOKOjump_punch has fromState
        // None but a shortcut from 38 — that is the only way to reach it.
        let p = c
            .lookup(ty::PUNCH, st::FALLING, var::FIGHT)
            .expect("punch from the airborne state");
        let name = &c.entries[p].name;
        assert!(name.contains("jump"), "airborne punch is a jump attack, got {name}");
        let k = c.lookup(ty::KICK, st::FALLING, var::FIGHT).expect("kick in the air");
        assert!(c.entries[k].name.contains("jump"));
        let f = c.lookup(ty::FLIP, st::FALLING, var::FIGHT).expect("flip in the air");
        assert_eq!(c.entries[f].clip.anim_type, ty::FLIP);
    }

    #[test]
    fn weighted_pick_is_deterministic_per_seed() {
        let Some(c) = konoko() else {
            eprintln!("SKIP: generated animations not found");
            return;
        };
        let pick = |seed| {
            c.seed(seed);
            (0..8)
                .map(|_| c.lookup(ty::IDLE, st::STANDING, 0))
                .collect::<Vec<_>>()
        };
        assert_eq!(pick(7), pick(7), "same seed, same sequence");
    }
}
