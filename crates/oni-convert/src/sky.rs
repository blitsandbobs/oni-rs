//! The level's sky (`ONtSkyClass`, `ONSK`; `docs/SKY.md`): the five textures
//! of its skybox and its planets. The level names its sky (`ONLV`
//! `sky_class`); the shared skies live in level 0. Written to
//! `levels/NN/sky.json`; a level without a sky (or whose sky has no data
//! anywhere: level 4's `sky13`) gets none.

use std::path::Path;

use oni_dat::{Dat, FourCc};
use oni_schema::files;
#[cfg(test)]
use serde_json::json;

use crate::{le_f32, le_u32};

const TAG_ONLV: FourCc = FourCc(u32::from_be_bytes(*b"ONLV"));
const TAG_ONSK: FourCc = FourCc(u32::from_be_bytes(*b"ONSK"));

/// `ONtSkyClass` (`Oni_Sky.h`): `skybox[6]` @0 (top, left, right, front,
/// back; the sixth unused), `planet_tex[8]` @24, `lens_flare` @56,
/// `star_textures[5]` @60, `num_planets` @80, `planet_with_lensflare` @84,
/// `planet_xsize[8]` @88, `planet_ysize[8]` @120, `planet_azimuth[8]` @152,
/// `planet_bearing[8]` @184, `lensflare_scale` @216, `lensflare_alpha` @220,
/// `star_count` @224, `star_seed` @228, `star_texture_count` @232.
fn decode(dat: &Dat<'_>, d: &[u8]) -> Option<files::SkyFile> {
    let name = |r: u32| -> Option<String> {
        (r & 1 != 0)
            .then(|| dat.instances.get((r >> 8) as usize))
            .flatten()
            .and_then(|i| i.name.clone())
    };
    if d.len() < 236 {
        return None;
    }
    Some(files::SkyFile {
        name: String::new(),
        textures: (0..5).map(|k| name(le_u32(d, k * 4))).collect(),
        planets: (0..(le_u32(d, 80) as usize).min(8))
            .filter_map(|k| {
                Some(files::Planet {
                    texture: name(le_u32(d, 24 + k * 4))?,
                    size: [le_f32(d, 88 + k * 4), le_f32(d, 120 + k * 4)],
                    azimuth: le_f32(d, 152 + k * 4),
                    bearing: le_f32(d, 184 + k * 4),
                })
            })
            .collect(),
        star_count: le_u32(d, 224),
    })
}

/// The level's sky: its `ONLV`'s `sky_class`, found (with data) in the level
/// or in level 0.
pub fn sky(
    dat: &Dat<'_>,
    level0: &Dat<'_>,
    registry: &oni_template::Registry,
) -> Option<files::SkyFile> {
    let onlv = dat.instances.iter().find(|i| i.tag == TAG_ONLV)?;
    let layout = registry.get(TAG_ONLV)?;
    let rec = oni_template::decode(layout, dat.instance_data(onlv)?, dat.header.endian).ok()?;
    let fields = crate::level_json::record_to_json(&rec, dat);
    let wanted = fields["sky_class"]["$ref"].as_str()?.to_string();
    for d in [dat, level0] {
        let found = d
            .instances
            .iter()
            .filter(|i| i.tag == TAG_ONSK && i.name.as_deref() == Some(wanted.as_str()))
            .find_map(|i| d.instance_data(i).map(|data| (i, data)));
        if let Some((_, data)) = found {
            let mut v = decode(d, data)?;
            v.name = wanted;
            return Some(v);
        }
    }
    None
}

pub fn write(
    dat: &Dat<'_>,
    level0: &Dat<'_>,
    registry: &oni_template::Registry,
    root: &Path,
) -> std::io::Result<()> {
    let Some(s) = sky(dat, level0, registry) else {
        return Ok(());
    };
    std::fs::create_dir_all(root)?;
    std::fs::write(root.join("sky.json"), serde_json::to_string_pretty(&s)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_take_their_sky_from_level_0() {
        let dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../original/gamedata");
        let (Ok(l0), Ok(l2)) = (
            std::fs::read(dir.join("level0_Final.dat")),
            std::fs::read(dir.join("level2_Final.dat")),
        ) else {
            return;
        };
        let (l0, l2) = (Dat::parse(&l0).unwrap(), Dat::parse(&l2).unwrap());
        let registry = oni_template::Registry::load();
        let s = serde_json::to_value(sky(&l2, &l0, &registry).expect("level 2 has a sky")).unwrap();
        assert_eq!(s["name"], "sunset");
        assert_eq!(
            s["textures"],
            json!([
                "sunset_top",
                "sunset_left",
                "sunset_right",
                "sunset_front",
                "sunset_back"
            ])
        );
        assert_eq!(s["planets"][0]["texture"], "sunset_sun");
        assert_eq!(s["planets"][0]["size"], json!([1.0, 1.0]));
    }
}
