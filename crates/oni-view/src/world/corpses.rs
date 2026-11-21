//! Pre-placed corpses (`ONtCorpseArray`; `levels/NN/corpses.json`): the
//! bodies a level starts with, lying as the designers left them — each its
//! class's body drawn with its parts' stored matrices (`ONrCorpse_*`).

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
}

/// The level's corpses (none without the file).
pub fn load(level_dir: &std::path::Path) -> Vec<Corpse> {
    let Ok(text) = std::fs::read_to_string(level_dir.join("corpses.json")) else { return Vec::new() };
    let doc: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
    doc["corpses"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| {
            let class = c["class"].as_str()?.to_string();
            let bones = c["matrices"]
                .as_array()?
                .iter()
                .map(|m| {
                    let f: Vec<f32> = m.as_array().into_iter().flatten().filter_map(|x| x.as_f64()).map(|x| x as f32).collect();
                    let col = |k: usize, w: f32| Vec4::from((Vec3::from_slice(&f[k * 3..k * 3 + 3]), w));
                    (f.len() == 12).then(|| Mat4::from_cols(col(0, 0.0), col(1, 0.0), col(2, 0.0), col(3, 1.0)))
                })
                .collect::<Option<Vec<Mat4>>>()?;
            Some(Corpse { class, bones, model: None })
        })
        .collect()
}

impl World {
    /// Resolves the corpses' models (`model`: a class's, loading it).
    pub fn attach_corpse_models(&mut self, mut model: impl FnMut(&str) -> Option<usize>) {
        for c in &mut self.corpses {
            c.model = model(&c.class);
            if c.model.is_none() {
                eprintln!("corpses: no model for class {}", c.class);
            }
        }
    }

    /// The corpses to draw: each its model and parts' matrices.
    pub fn corpse_draws(&self) -> impl Iterator<Item = crate::render::CharDraw<'_>> {
        self.corpses.iter().filter_map(|c| Some(crate::render::CharDraw { model: c.model?, bones: &c.bones, see_through: false }))
    }
}
