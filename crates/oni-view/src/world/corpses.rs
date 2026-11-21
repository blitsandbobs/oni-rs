//! Corpses (`ONtCorpseArray`; `levels/NN/corpses.json`, `docs/CORPSES.md`):
//! the bodies a level starts with, lying as the designers left them, and
//! after them a ring for the dead of play — each its class's body drawn
//! with its parts' stored matrices (`ONrCorpse_*`).

use glam::{Mat4, Vec3, Vec4};

use super::World;

/// A pre-placed corpse.
#[derive(Clone, Debug)]
pub struct Corpse {
    pub class: String,
    /// Each body part's world matrix.
    pub bones: Vec<Mat4>,
    /// Its class's model (`None` until resolved, or a class the game
    /// doesn't have).
    pub model: Option<usize>,
    /// Its extent (`corpse_bbox`): min, max.
    pub bbox: (Vec3, Vec3),
}

/// The ring of corpses: the level's own first, then the dead of play,
/// the oldest of those recycled once the array is full
/// (`static_corpses`, `max_corpses`, `next`).
#[derive(Clone, Debug, Default)]
pub struct Corpses {
    pub list: Vec<Corpse>,
    pub fixed: usize,
    pub max: usize,
    next: usize,
}

impl Corpses {
    /// A dead body handed over (`ONrCorpse_Create`), in the next slot.
    pub fn add(&mut self, c: Corpse) {
        if self.max <= self.fixed {
            return;
        }
        if self.next < self.list.len() {
            self.list[self.next] = c;
        } else {
            self.list.push(c);
        }
        self.next += 1;
        if self.next >= self.max {
            self.next = self.fixed;
        }
    }

    /// The dead of play cleared (`corpse_reset`).
    pub fn reset(&mut self) {
        self.list.truncate(self.fixed);
        self.next = self.fixed;
    }

    /// The dead of play whose extent `meets` says is inside (a trigger
    /// volume's `trigvolume_corpse`) cleared, their slots kept.
    pub fn clear_where(&mut self, meets: impl Fn(Vec3, Vec3) -> bool) {
        for c in self.list.iter_mut().skip(self.fixed) {
            if c.model.is_some() && meets(c.bbox.0, c.bbox.1) {
                c.model = None;
            }
        }
    }
}

/// The level's corpses (none without the file).
pub fn load(level_dir: &std::path::Path) -> Corpses {
    let Ok(text) = std::fs::read_to_string(level_dir.join("corpses.json")) else {
        return Corpses::default();
    };
    let doc: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
    let v3 = |v: &serde_json::Value| {
        Vec3::new(
            v[0].as_f64().unwrap_or(0.0) as f32,
            v[1].as_f64().unwrap_or(0.0) as f32,
            v[2].as_f64().unwrap_or(0.0) as f32,
        )
    };
    let list: Vec<Corpse> = doc["corpses"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| {
            let class = c["class"].as_str()?.to_string();
            let bones = c["matrices"]
                .as_array()?
                .iter()
                .map(|m| {
                    let f: Vec<f32> = m
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|x| x.as_f64())
                        .map(|x| x as f32)
                        .collect();
                    let col =
                        |k: usize, w: f32| Vec4::from((Vec3::from_slice(&f[k * 3..k * 3 + 3]), w));
                    (f.len() == 12).then(|| {
                        Mat4::from_cols(col(0, 0.0), col(1, 0.0), col(2, 0.0), col(3, 1.0))
                    })
                })
                .collect::<Option<Vec<Mat4>>>()?;
            Some(Corpse {
                class,
                bones,
                model: None,
                bbox: (v3(&c["bbox"]["min"]), v3(&c["bbox"]["max"])),
            })
        })
        .collect();
    let fixed = list.len();
    let max = doc["max"].as_u64().map_or(fixed, |m| m as usize).max(fixed);
    Corpses {
        list,
        fixed,
        max,
        next: fixed,
    }
}

impl World {
    /// Resolves the corpses' models (`model`: a class's, loading it).
    pub fn attach_corpse_models(&mut self, mut model: impl FnMut(&str) -> Option<usize>) {
        for c in &mut self.corpses.list {
            c.model = model(&c.class);
            if c.model.is_none() {
                eprintln!("corpses: no model for class {}", c.class);
            }
        }
    }

    /// The corpses to draw: each its model and parts' matrices.
    pub fn corpse_draws(&self) -> impl Iterator<Item = crate::render::CharDraw<'_>> {
        self.corpses.list.iter().filter_map(|c| {
            Some(crate::render::CharDraw {
                model: c.model?,
                bones: &c.bones,
                see_through: false,
                alpha: 1.0,
                shade: None,
            })
        })
    }
}

/// The array's size outside `--faithful` (Q21; the levels' own is 20).
pub const FIXED_MAX: usize = 50;

/// The corner of the view cone past the field of view a body still counts
/// as on screen (`fovFudge`).
const FOV_FUDGE: f32 = 0.25 * std::f32::consts::FRAC_PI_2;

impl World {
    /// The dead of play become corpses (`ONrCharacter_Die_4_Gone_Hook`): a
    /// settled body (not the player's, not death-locked, not waiting on its
    /// class's delete delay, nor shielded, invisible or teleporting) once it
    /// isn't drawn — outside the view by more than the fudge, or past the far
    /// plane (`ONiCharacter_NeedsRendering`). Its pose goes to the ring and
    /// the character is gone; a body that fell or died too long makes none.
    pub(super) fn corpses_tick(&mut self) {
        let cam = &self.camera;
        let (eye, look) = (cam.view.location, cam.view.view.normalize_or_zero());
        let (fov, far) = (cam.fov, cam.far);
        for i in 1..self.fighters.len() {
            let f = &self.fighters[i];
            if f.gone
                || !f.settled
                || f.death_lock
                || f.death_delete > 0
                || f.effects.shield_active
                || f.effects.invis_active
                || f.effects.teleporting
            {
                continue;
            }
            let at = f.feet() + Vec3::Y * 10.0;
            let to = (at - eye).normalize_or_zero();
            let drawn =
                to.angle_between(look) <= fov + FOV_FUDGE && at.distance_squared(eye) <= far * far;
            if drawn {
                continue;
            }
            if !f.no_corpse {
                let bones = f.bones();
                let spheres = f.body_spheres();
                let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
                for (_, c, r) in spheres {
                    lo = lo.min(c - Vec3::splat(r));
                    hi = hi.max(c + Vec3::splat(r));
                }
                // (Its model is known: no class to look up.)
                let class = String::new();
                let model = Some(f.model);
                self.corpses.add(Corpse {
                    class,
                    bones,
                    model,
                    bbox: (lo, hi),
                });
            }
            self.remove(i);
        }
    }
}
