//! Environment-particle placement export.
//!
//! Oni's glowing effects (console/door screens, lens flares, fire, force
//! fields, scanners) are not textured geometry — they are the **particle
//! system**, drawn as additive sprites. This module decodes the placed
//! environment particles (`ENVP`) and the particle-class appearance they
//! reference, so the viewer can render them as additive billboards.
//!
//! ## Data path
//!
//! * `EPtEnvParticleArray` (`ENVP`, `BFW_EnvParticle.h`): `tm_pad[22]`, then
//!   `UUtUns16 numParticles` at offset 22, then `EPtEnvParticle particle[]`
//!   (inline) starting at offset 24.
//! * `EPtEnvParticle` (208 bytes): `char classname[64]`, `char tagname[48]`,
//!   `M3tMatrix4x3 matrix` (12 × f32), then scales/flags/pointers. The world
//!   position is the matrix translation row `m[9..12]`.
//! * The `classname` names a particle class stored as a `BINA` instance called
//!   `"PAR3"+classname` (the 4-char `P3cBinaryDataClass` tag is prefixed, stored
//!   byte-reversed on PC — so the instance name reads `"3RAP<classname>"`). Its
//!   payload lives in the companion `.raw` at the `BDtBinaryData.data_index`
//!   byte offset, headed by `BDtHeader { class_type, data_size }` (8 bytes),
//!   followed by a `P3tParticleDefinition` (`BFW_Particle3.h`).
//! * Every shipped particle definition is version 18 (`P3cVersion_Dodge`). In
//!   that layout `P3tAppearance` begins at offset 168; `appearance.scale`
//!   (a `P3tValue`: `u32 type`, then a `f32`) is at 168 and `appearance.instance`
//!   (`char[32]`, the `TXMP` texture name) is at 280.
//!
//! Additive / self-illuminated / animation attributes come from the referenced
//! texture's `TXMP` flags, which the converter already resolves by name.

use std::collections::BTreeMap;
use std::path::Path;

use oni_dat::{Dat, FourCc};

use crate::{cstr, le_f32, le_u16, le_u32, rel_path, sanitize};

const TAG_ENVP: FourCc = FourCc(0x454E_5650); // 'ENVP'
const TAG_BINA: FourCc = FourCc(0x4249_4E41); // 'BINA'
const CLASS_PAR3: u32 = 0x5041_5233; // 'PAR3' (P3cBinaryDataClass)

const ENVP_PARTICLE_STRIDE: usize = 208;
const ENVP_ARRAY_START: usize = 24;
const ENVP_MATRIX_OFF: usize = 112; // after classname[64] + tagname[48]

// Offsets within a version-18 `P3tParticleDefinition`.
const DEF_APPEARANCE_OFF: usize = 168;
const APPEARANCE_SCALE_OFF: usize = DEF_APPEARANCE_OFF; // scale P3tValue
const APPEARANCE_INSTANCE_OFF: usize = 280; // char instance[32]
const PAR3_DEF_VERSION: u16 = 18;

/// A particle class's drawable appearance (resolved from its `PAR3` definition).
#[derive(Clone, Debug)]
pub struct ParticleClass {
    /// `appearance.instance` — the `TXMP` texture name (sprite), or empty for a
    /// geometry/emitter-only class with no sprite texture.
    pub texture: String,
    /// `appearance.scale` constant — the sprite's world size.
    pub size: f32,
    /// True when the class draws a polygonal model instead of a sprite
    /// (`P3cParticleClassFlag_Appearance_Geometry`); such classes have no
    /// billboard texture.
    pub geometry: bool,
    /// The whole `P3tParticleDefinition` (fixed part and its variable,
    /// action and emitter arrays), decoded by `particle_defs`.
    pub def: Vec<u8>,
}

/// Reads a `P3tValue`'s leading `f32` (valid for the float constant/range/
/// bellcurve/time-based variants — all store a float at offset +4).
fn pvalue_f32(d: &[u8], off: usize) -> f32 {
    le_f32(d, off + 4)
}

/// Collects every particle class in this file into `out` (keyed by class name,
/// first writer wins so level0's global classes are canonical). `raw` is the
/// companion `.raw` holding the `PAR3` payloads.
pub fn collect_particle_classes(
    dat: &Dat<'_>,
    raw: Option<&[u8]>,
    out: &mut BTreeMap<String, ParticleClass>,
) {
    let Some(raw) = raw else { return };
    for inst in dat.instances.iter().filter(|i| i.tag == TAG_BINA) {
        // Binary-data instances are named "<class-tag><identifier>"; the 4-char
        // class tag is stored byte-reversed on PC, so a PAR3 class reads "3RAP…".
        let Some(name) = inst.name.as_deref() else {
            continue;
        };
        let Some(classname) = name.strip_prefix("3RAP") else {
            continue;
        };
        if classname.is_empty() || out.contains_key(classname) {
            continue;
        }
        // BDtBinaryData: data_size @0, data_index (byte offset into .raw) @4.
        let Some(bd) = dat.instance_data(inst) else {
            continue;
        };
        let data_index = le_u32(bd, 4) as usize;
        // BDtHeader { class_type, data_size }, then the definition.
        let Some(hdr) = raw.get(data_index..data_index + 8) else {
            continue;
        };
        if le_u32(hdr, 0) != CLASS_PAR3 {
            continue;
        }
        let def_size = le_u32(hdr, 4) as usize;
        let def_start = data_index + 8;
        let Some(def) = raw.get(def_start..def_start + def_size) else {
            continue;
        };
        if le_u16(def, 2) != PAR3_DEF_VERSION || def.len() < APPEARANCE_INSTANCE_OFF + 32 {
            continue;
        }
        let flags = le_u32(def, 4);
        let geometry = flags & (1 << 8) != 0; // P3cParticleClassFlag_Appearance_Geometry
        let size = pvalue_f32(def, APPEARANCE_SCALE_OFF);
        let texture = cstr(def, APPEARANCE_INSTANCE_OFF, 32);
        out.insert(
            classname.to_string(),
            ParticleClass {
                texture,
                size: if size.is_finite() && size > 0.0 {
                    size
                } else {
                    1.0
                },
                geometry,
                def: def.to_vec(),
            },
        );
    }
}

/// One placed environment particle resolved for `particles.json`.
struct Placed {
    class: String,
    position: [f32; 3],
}

/// Decodes all `ENVP` arrays in `dat` into placed particles.
fn decode_env_particles(dat: &Dat<'_>) -> Vec<Placed> {
    let mut out = Vec::new();
    for inst in dat.instances.iter().filter(|i| i.tag == TAG_ENVP) {
        let Some(d) = dat.instance_data(inst) else {
            continue;
        };
        let num = le_u16(d, 22) as usize;
        for k in 0..num {
            let o = ENVP_ARRAY_START + k * ENVP_PARTICLE_STRIDE;
            if o + ENVP_PARTICLE_STRIDE > d.len() {
                break;
            }
            let class = cstr(d, o, 64);
            if class.is_empty() {
                continue;
            }
            let m = o + ENVP_MATRIX_OFF;
            let position = [le_f32(d, m + 36), le_f32(d, m + 40), le_f32(d, m + 44)];
            out.push(Placed { class, position });
        }
    }
    out
}

/// The level's environmental particles (`docs/PARTICLES.md`, *Environmental
/// particles*): the `ENVP` arrays' (a 4×3 matrix @112, decal x / y scale
/// @160 / @164, u16 flags @168 — 0x2: not created at the start) and the
/// particle objects' (`CJBOParticle`: class[64], tag[48], u16 flags, decal
/// scales; placed by the object's position and XYZ rotation). Each: class,
/// tag (what scripts' `particle` command names), matrix (12 floats, columns
/// X Y Z then position), flags, decal scale.
pub fn env_particles(dat: &Dat<'_>, raw: &[u8]) -> Vec<serde_json::Value> {
    const NOT_INITIALLY_CREATED: u16 = 0x2;
    let entry = |class: String, tag: String, m: [f32; 12], flags: u16, decal: [f32; 2], from: &str| {
        let mut e = serde_json::json!({ "class": class, "matrix": m, "decalScale": decal, "from": from });
        if !tag.is_empty() {
            e["tag"] = serde_json::json!(tag);
        }
        if flags & NOT_INITIALLY_CREATED != 0 {
            e["notInitiallyCreated"] = serde_json::json!(true);
        }
        e
    };
    let mut out = Vec::new();
    for inst in dat.instances.iter().filter(|i| i.tag == TAG_ENVP) {
        let Some(d) = dat.instance_data(inst) else { continue };
        let num = le_u16(d, 22) as usize;
        for k in 0..num {
            let o = ENVP_ARRAY_START + k * ENVP_PARTICLE_STRIDE;
            if o + ENVP_PARTICLE_STRIDE > d.len() {
                break;
            }
            let class = cstr(d, o, 64);
            if class.is_empty() {
                continue;
            }
            let m: [f32; 12] = std::array::from_fn(|i| le_f32(d, o + ENVP_MATRIX_OFF + i * 4));
            let decal = [le_f32(d, o + 160), le_f32(d, o + 164)];
            out.push(entry(class, cstr(d, o + 64, 48), m, le_u16(d, o + 168), decal, "env"));
        }
    }
    for o in crate::objects::read(dat, raw, "Particle").1 {
        let d = o.data;
        if d.len() < 114 {
            continue;
        }
        let axes = crate::patrols::euler_axes(o.rotation);
        let p = o.position;
        let m = [
            axes[0][0], axes[0][1], axes[0][2], axes[1][0], axes[1][1], axes[1][2], axes[2][0], axes[2][1], axes[2][2], p[0], p[1], p[2],
        ];
        let decal = if d.len() >= 122 { [le_f32(d, 114), le_f32(d, 118)] } else { [1.0, 1.0] };
        out.push(entry(cstr(d, 0, 64), cstr(d, 64, 48), m, le_u16(d, 112), decal, "object"));
    }
    out
}

/// Writes `<root>/env_particles.json` (see `env_particles`), if any.
pub fn write_env_particles(dat: &Dat<'_>, raw: &[u8], root: &Path) -> std::io::Result<usize> {
    let p = env_particles(dat, raw);
    if !p.is_empty() {
        std::fs::create_dir_all(root)?;
        std::fs::write(root.join("env_particles.json"), serde_json::to_string_pretty(&serde_json::json!({ "particles": p }))?)?;
    }
    Ok(p.len())
}

/// Writes `<root>/particles.json` — every placed environment particle with its
/// world position, size, sprite texture reference, and additive/self-illum
/// flags (resolved through the particle class and texture). Returns the count.
///
/// `this_root` is the level's output root (`levels/NN`); `classes` is the global
/// particle-class map; `tex_flags` maps a texture name to
/// `(additive, self_illuminated)`.
pub fn convert_particles(
    dat: &Dat<'_>,
    this_root: &str,
    root: &Path,
    classes: &BTreeMap<String, ParticleClass>,
    tex_flags: &BTreeMap<String, (bool, bool)>,
) -> std::io::Result<usize> {
    let placed = decode_env_particles(dat);
    if placed.is_empty() {
        return Ok(0);
    }

    let mut items = Vec::with_capacity(placed.len());
    for p in &placed {
        let class = classes.get(&p.class);
        // Sprite texture (skip geometry-only classes, which have no billboard).
        let texture =
            class.and_then(|c| (!c.geometry && !c.texture.is_empty()).then(|| c.texture.clone()));
        let size = class.map(|c| c.size).unwrap_or(1.0);
        let (additive, self_illum) = texture
            .as_deref()
            .and_then(|t| tex_flags.get(t))
            .copied()
            .unwrap_or((false, false));
        let texture_path = texture
            .as_deref()
            .map(|t| rel_path(this_root, &format!("common/textures/{}.png", sanitize(t))));
        items.push(serde_json::json!({
            "class": p.class,
            "texture": texture.as_deref().map(sanitize),
            "texturePath": texture_path,
            "position": { "x": p.position[0], "y": p.position[1], "z": p.position[2] },
            "size": size,
            "additive": additive,
            "selfIlluminated": self_illum,
        }));
    }

    std::fs::create_dir_all(root)?;
    std::fs::write(
        root.join("particles.json"),
        serde_json::to_string(&serde_json::json!({
            "count": items.len(),
            "particles": items,
        }))?,
    )?;
    Ok(items.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn gamedata_dir() -> Option<PathBuf> {
        if let Ok(p) = std::env::var("ONI_GAMEDATA_DIR") {
            let p = PathBuf::from(p);
            return p.is_dir().then_some(p);
        }
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .join("original/gamedata");
        p.is_dir().then_some(p)
    }

    /// Decodes level1's placed particles and resolves each through its global
    /// (level0) particle class: level1 has 60 environment particles, all of
    /// which are additive `lensflare*` sprites.
    #[test]
    fn level1_env_particles_resolve_to_additive_lensflares() {
        let Some(dir) = gamedata_dir() else {
            eprintln!("SKIP: gamedata dir not found (set ONI_GAMEDATA_DIR)");
            return;
        };

        // Global particle classes live in level0.
        let l0 = std::fs::read(dir.join("level0_Final.dat")).expect("level0 dat");
        let l0raw = std::fs::read(dir.join("level0_Final.raw")).expect("level0 raw");
        let d0 = Dat::parse(&l0).expect("parse level0");
        let mut classes = BTreeMap::new();
        collect_particle_classes(&d0, Some(&l0raw), &mut classes);
        assert!(classes.contains_key("locklight"), "locklight class present");
        assert_eq!(classes["locklight"].texture, "lensflare11");
        assert!(classes["locklight"].size > 0.0);

        let l1 = std::fs::read(dir.join("level1_Final.dat")).expect("level1 dat");
        let d1 = Dat::parse(&l1).expect("parse level1");
        let placed = decode_env_particles(&d1);
        assert_eq!(placed.len(), 60, "level1 has 60 env particles");

        // Every placed particle resolves to a known class with a sprite texture.
        for p in &placed {
            let c = classes
                .get(&p.class)
                .unwrap_or_else(|| panic!("class {:?} missing", p.class));
            assert!(!c.texture.is_empty(), "class {:?} has a texture", p.class);
            assert!(c.texture.starts_with("lensflare"));
        }
    }
}
