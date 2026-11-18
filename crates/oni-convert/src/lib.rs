//! One-time converter: an Oni `GameDataFolder` to a modern asset tree.
//!
//! Output layout (settled hybrid structure):
//! ```text
//! <out>/oni.json                 manifest + provenance (source file hashes)
//! <out>/common/                  GLOBAL named assets (shared, by name) + level0
//!        ├── textures/*.png  audio/*.wav  models/*.glb
//! <out>/levels/NN/               level N's UNNAMED, level-local assets
//!        ├── textures/  models/
//! <out>/levels/NN/level.json     the level's objects (semantic data)
//! ```
//!
//! ## Cross-file resolution
//!
//! Oni shares assets across files by *name*: a reference in level 3 to a
//! texture may be a placeholder resolved, at load time, to the real instance in
//! another file. We mirror that: every **named** instance is globally
//! addressable, so all named assets go to `common/` (deduplicated by name,
//! level0 winning ties), and any named reference resolves there. **Unnamed**
//! (unique) instances are only referenced within their own file, so they stay
//! in that level's folder. This makes every reference resolve to a real file.

#![forbid(unsafe_code)]

mod level_json;
mod particles;

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use oni_dat::{Dat, FourCc, Instance};
use sha2::{Digest, Sha256};

const TAG_TXMP: FourCc = FourCc(0x5458_4D50);
const TAG_SNDD: FourCc = FourCc(0x534E_4444);

/// Per-level conversion counts.
#[derive(Default, Clone)]
pub struct LevelStats {
    /// Textures written.
    pub textures: usize,
    /// Sounds written.
    pub sounds: usize,
    /// Meshes written.
    pub meshes: usize,
    /// Character bodies written.
    pub characters: usize,
    /// Animation clips written.
    pub animations: usize,
    /// `character.json` files written.
    pub character_meta: usize,
    /// Level environments (`environment.glb`) written.
    pub environments: usize,
    /// Placed objects separated out of the environment geometry.
    pub env_objects: usize,
    /// Scripted object-animation clips (`OBAN`) written.
    pub object_animations: usize,
    /// UI/subtitle strings (`IGSt`) + internal strings (`TStr`) written.
    pub strings: usize,
    /// Cutscene recordings (`FILM`) written.
    pub films: usize,
    /// Environment particles (`ENVP`) written to `particles.json`.
    pub particles: usize,
    /// Fonts (`TSFT`) written.
    pub fonts: usize,
    /// Objects written to level.json.
    pub objects: usize,
    /// Non-fatal problems encountered.
    pub warnings: Vec<String>,
}

/// Result of a full conversion.
pub struct Report {
    /// Per-output-root stats, keyed by output subpath (`common`, `levels/03`, …).
    pub levels: BTreeMap<String, LevelStats>,
    /// Count of mesh→texture references that resolved to an existing file.
    pub refs_resolved: usize,
    /// Count of mesh→texture references that could not be resolved.
    pub refs_dangling: usize,
}

struct FileCtx {
    level: u32,
    root: String, // "common" or "levels/NN"
    path: PathBuf,
    bytes: Vec<u8>,
}

/// Converts every `*.dat` in `input_dir` into the modern tree at `output_dir`.
pub fn convert(input_dir: &Path, output_dir: &Path) -> std::io::Result<Report> {
    std::fs::create_dir_all(output_dir)?;
    let mut dats: Vec<PathBuf> = std::fs::read_dir(input_dir)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("dat"))
        .collect();
    dats.sort();

    // Load file contexts (level0 first so it wins name ties).
    let mut files: Vec<FileCtx> = Vec::new();
    let mut source_files = Vec::new();
    for p in &dats {
        let Some(level) = level_number(p) else {
            continue;
        };
        let bytes = std::fs::read(p)?;
        source_files.push(source_entry(p, &bytes));
        if let Ok(rb) = std::fs::read(p.with_extension("raw")) {
            source_files.push(source_entry(&p.with_extension("raw"), &rb));
        }
        let root = if level == 0 {
            "common".to_string()
        } else {
            format!("levels/{level:02}")
        };
        files.push(FileCtx {
            level,
            root,
            path: p.clone(),
            bytes,
        });
    }
    files.sort_by_key(|f| f.level);

    // --- Pass 1: global named-asset index: (tag, name) -> canonical file index.
    // Also collect texture render flags (additive / self-illuminated) by name,
    // so a placeholder reference in one level resolves to the real flags. ---
    let mut global_named: BTreeMap<(FourCc, String), usize> = BTreeMap::new();
    let mut texture_flags: BTreeMap<String, (bool, bool)> = BTreeMap::new();
    // Particle classes (PAR3 binary data) are globally named; level0 wins ties.
    let mut particle_classes: BTreeMap<String, particles::ParticleClass> = BTreeMap::new();
    for (fi, f) in files.iter().enumerate() {
        let dat = Dat::parse(&f.bytes).map_err(|e| std::io::Error::other(format!("{e}")))?;
        let raw = std::fs::read(f.path.with_extension("raw")).ok();
        particles::collect_particle_classes(&dat, raw.as_deref(), &mut particle_classes);
        for inst in &dat.instances {
            if let Some(name) = &inst.name {
                if !name.is_empty() {
                    if let Some(d) = dat.instance_data(inst) {
                        global_named.entry((inst.tag, name.clone())).or_insert(fi);
                        // TXMP flags live at offset 128 (M3cTextureFlags).
                        if inst.tag == TAG_TXMP_ && d.len() >= 132 {
                            let fl = u32::from_le_bytes([d[128], d[129], d[130], d[131]]);
                            texture_flags.entry(name.clone()).or_insert((
                                fl & oni_texture::FLAG_BLEND_ADDITIVE != 0,
                                fl & oni_texture::FLAG_SELF_ILLUMINENT != 0,
                            ));
                        }
                    }
                }
            }
        }
    }

    let mut report = Report {
        levels: BTreeMap::new(),
        refs_resolved: 0,
        refs_dangling: 0,
    };
    let registry = oni_template::Registry::load();
    // Character classes (ONCC) are named/global; accumulate the canonical ones
    // across files into a single catalog written at the end.
    let mut character_classes: Vec<serde_json::Value> = Vec::new();

    // --- Pass 2: per file, export canonical-named + unnamed assets, resolving refs ---
    for (fi, f) in files.iter().enumerate() {
        let dat = Dat::parse(&f.bytes).map_err(|e| std::io::Error::other(format!("{e}")))?;
        let raw = std::fs::read(f.path.with_extension("raw")).ok();
        // Retail PC / Mac CDs keep texture pixels in a companion `.sep`.
        let sep = std::fs::read(f.path.with_extension("sep")).ok();
        let root = output_dir.join(&f.root);

        let mut stats = LevelStats::default();
        if raw.is_none() {
            stats
                .warnings
                .push("no .raw: textures and audio skipped".into());
        }

        convert_textures(
            &dat,
            raw.as_deref(),
            sep.as_deref(),
            &f.root,
            &root,
            fi,
            &global_named,
            &mut stats,
        )?;
        convert_sounds(
            &dat,
            raw.as_deref(),
            &f.root,
            &root,
            fi,
            &global_named,
            &mut stats,
        )?;
        convert_meshes(
            &dat,
            &f.root,
            &root,
            fi,
            &global_named,
            &mut stats,
            &mut report,
        )?;
        convert_characters(&dat, raw.as_deref(), &root, fi, &global_named, &mut stats)?;
        convert_animations(&dat, raw.as_deref(), &root, fi, &global_named, &mut stats)?;
        let env_objects = convert_environment(&dat, &f.root, &root, &texture_flags, &mut stats)?;
        stats.object_animations = convert_object_animations(&dat, &root)?;
        stats.particles =
            particles::convert_particles(&dat, &f.root, &root, &particle_classes, &texture_flags)?;
        stats.strings = convert_strings(&dat, &root)? + convert_ui_text(&dat, &root)?;
        stats.films = convert_films(&dat, &root)?;
        stats.fonts = convert_fonts(&dat, &root)?;
        collect_character_classes(&dat, &f.root, fi, &global_named, &mut character_classes);

        // level.json for playable levels.
        if f.level != 0 {
            match level_json::write_level_json(&dat, &root, &registry, &env_objects) {
                Ok(n) => stats.objects = n,
                Err(e) => stats.warnings.push(format!("level.json: {e}")),
            }
        }

        report.levels.insert(f.root.clone(), stats);
    }

    // Write the character-class catalog (ONCC): body, variant, textures, aiming
    // screens, and the exact animation set per class.
    if !character_classes.is_empty() {
        let dir = output_dir.join("common/characters");
        std::fs::create_dir_all(&dir)?;
        let doc = serde_json::json!({
            "count": character_classes.len(),
            "classes": character_classes,
        });
        std::fs::write(
            dir.join("classes.json"),
            serde_json::to_string_pretty(&doc)?,
        )?;
    }

    write_manifest(output_dir, &report, &source_files)?;
    Ok(report)
}

/// Appends this file's canonical `ONCC` character classes to `out` as JSON.
fn collect_character_classes(
    dat: &Dat<'_>,
    this_root: &str,
    fi: usize,
    global_named: &BTreeMap<(FourCc, String), usize>,
    out: &mut Vec<serde_json::Value>,
) {
    use oni_geom::charclass::{decode_character_class, TAG_ONCC};
    for inst in dat.instances.iter().filter(|i| i.tag == TAG_ONCC) {
        // Only the canonical file for this named class emits it.
        if export_root(inst, dat, this_root, fi, global_named) != Some("common") {
            continue;
        }
        let Some(cc) = decode_character_class(dat, inst) else {
            continue;
        };
        let model = cc
            .body
            .as_ref()
            .map(|b| format!("{}/model.glb", sanitize(b)));
        let stats = cc.stats.map(|s| {
            use oni_geom::charclass::DAMAGE_TYPES;
            let resist: serde_json::Map<String, serde_json::Value> = DAMAGE_TYPES
                .iter()
                .zip(s.damage_resistance.iter())
                .filter(|(_, &r)| r != 0.0)
                .map(|(name, &r)| (name.to_string(), serde_json::json!(r)))
                .collect();
            serde_json::json!({
                "maxHitPoints": s.max_hit_points,
                "scale": [s.scale_low, s.scale_high],
                "damageResistance": resist,
                "bossShield": s.boss_shield,
                "shotTime": s.shot_time,
                "fightModeDuration": s.fight_mode_duration,
                "idleDelay": s.idle_delay,
                "idleRestartDelay": s.idle_restart_delay,
                "deathDeleteDelay": s.death_delete_delay,
                "feetBits": s.feet_bits,
                "leftHanded": s.left_handed,
                "superHypo": s.super_hypo,
                "superShield": s.super_shield,
                "knockdownResistant": s.knockdown_resistant,
            })
        });
        out.push(serde_json::json!({
            "name": cc.name,
            "variant": cc.variant,
            "body": cc.body,
            "model": model,
            "textures": cc.textures,
            "aimingScreens": cc.aiming_screens,
            "animationCollection": cc.animation_collection,
            "animationCount": cc.animations.len(),
            "animations": cc.animations,
            "stats": stats,
        }));
    }
}

/// Is this instance exported by file `fi` (either its canonical name lives here,
/// or it is an unnamed real instance)? Returns the output root the asset goes to.
fn export_root<'a>(
    inst: &Instance,
    dat: &Dat<'_>,
    this_root: &'a str,
    this_fi: usize,
    global_named: &BTreeMap<(FourCc, String), usize>,
) -> Option<&'a str> {
    match &inst.name {
        Some(name) if !name.is_empty() => {
            // Named -> global pool in common, but only the canonical file emits it.
            if global_named.get(&(inst.tag, name.clone())) == Some(&this_fi) {
                Some("common")
            } else {
                None // a placeholder/duplicate here; the canonical file emits it
            }
        }
        _ => {
            // Unnamed real instance -> this file's root.
            dat.instance_data(inst).map(|_| this_root)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn convert_textures(
    dat: &Dat<'_>,
    raw: Option<&[u8]>,
    sep: Option<&[u8]>,
    this_root: &str,
    root: &Path,
    fi: usize,
    global_named: &BTreeMap<(FourCc, String), usize>,
    stats: &mut LevelStats,
) -> std::io::Result<()> {
    let Some(raw) = raw else { return Ok(()) };
    for inst in dat.instances.iter().filter(|i| i.tag == TAG_TXMP) {
        let Ok(tex) = oni_texture::TextureMap::from_instance(dat, inst) else {
            continue;
        };
        // Pixels live in .raw (no-sep PC) or .sep (retail PC / Mac); skip only
        // if neither holds them.
        if tex.raw_offset == 0 && tex.separate_offset == 0 {
            continue;
        }
        let dest_root = match export_root(inst, dat, this_root, fi, global_named) {
            Some("common") => root_to_common(root),
            Some(_) => root.to_path_buf(),
            None => continue,
        };
        let dir = dest_root.join("textures");
        std::fs::create_dir_all(&dir)?;
        match tex.decode_top_level_with(raw, sep) {
            Ok(rgba) => {
                let file = dir.join(format!("{}.png", asset_name(dat, inst)));
                if write_png(&file, tex.width as u32, tex.height as u32, &rgba).is_ok() {
                    stats.textures += 1;
                }
            }
            Err(e) => stats
                .warnings
                .push(format!("decode TXMP {}: {e}", inst.index)),
        }
    }
    Ok(())
}

fn convert_sounds(
    dat: &Dat<'_>,
    raw: Option<&[u8]>,
    this_root: &str,
    root: &Path,
    fi: usize,
    global_named: &BTreeMap<(FourCc, String), usize>,
    stats: &mut LevelStats,
) -> std::io::Result<()> {
    let Some(raw) = raw else { return Ok(()) };
    for inst in dat.instances.iter().filter(|i| i.tag == TAG_SNDD) {
        let Ok(snd) = oni_sound::Sound::from_instance(dat, inst) else {
            continue;
        };
        let dest_root = match export_root(inst, dat, this_root, fi, global_named) {
            Some("common") => root_to_common(root),
            Some(_) => root.to_path_buf(),
            None => continue,
        };
        let dir = dest_root.join("audio");
        std::fs::create_dir_all(&dir)?;
        // Emit decoded 16-bit PCM WAV (plays everywhere, no ADPCM support needed).
        if let Ok(wav) = snd.to_pcm_wav(raw) {
            std::fs::write(dir.join(format!("{}.wav", asset_name(dat, inst))), &wav)?;
            stats.sounds += 1;
        }
    }
    Ok(())
}

fn convert_meshes(
    dat: &Dat<'_>,
    this_root: &str,
    root: &Path,
    fi: usize,
    global_named: &BTreeMap<(FourCc, String), usize>,
    stats: &mut LevelStats,
    report: &mut Report,
) -> std::io::Result<()> {
    for (idx, mesh) in oni_geom::meshes(dat) {
        let inst = &dat.instances[idx];
        let dest_root = match export_root(inst, dat, this_root, fi, global_named) {
            Some("common") => root_to_common(root),
            Some(_) => root.to_path_buf(),
            None => continue,
        };
        let mesh_root = if dest_root == root_to_common(root) {
            "common"
        } else {
            this_root
        };
        let dir = dest_root.join("models");
        std::fs::create_dir_all(&dir)?;

        let tex_uri = resolve_texture_ref(dat, idx, this_root, fi, global_named, mesh_root);
        if let Some((_, resolved)) = &tex_uri {
            if *resolved {
                report.refs_resolved += 1;
            } else {
                report.refs_dangling += 1;
            }
        }
        let glb = oni_geom::glb::to_glb(&mesh, tex_uri.as_ref().map(|(u, _)| u.as_str()));
        std::fs::write(dir.join(format!("{}.glb", asset_name(dat, inst))), &glb)?;
        stats.meshes += 1;
    }
    Ok(())
}

const TAG_TRBS: FourCc = FourCc(0x5452_4253);
const TAG_TRAM: FourCc = FourCc(0x5452_414D);
const TAG_AKEV: FourCc = FourCc(0x414B_4556);
const TAG_TXMP_: FourCc = FourCc(0x5458_4D50);

/// A placed object's baked-geometry summary, for `level.json`.
pub(crate) struct EnvObjectInfo {
    pub node: String,
    pub type_name: String,
    pub tag: u32,
    pub pivot: [f32; 3],
    pub triangles: usize,
}

/// One spec for a node before meshes are borrowed.
struct NodeSpec {
    name: String,
    translation: [f32; 3],
    parent: Option<usize>,
    mesh: Option<usize>,
    uri: Option<String>,
    blend: bool,
    additive: bool,
    self_illum: bool,
}

/// Looks up a texture's Oni render flags `(additive, self_illuminated)` by name
/// (resolves placeholder references via the global flag map).
fn env_texture_flags(
    dat: &Dat<'_>,
    tex_idx: usize,
    flags: &BTreeMap<String, (bool, bool)>,
) -> (bool, bool) {
    dat.instances
        .get(tex_idx)
        .and_then(|i| i.name.as_ref())
        .and_then(|n| flags.get(n))
        .copied()
        .unwrap_or((false, false))
}

/// Emits the level world geometry as `<root>/environment.glb`: the AKEV static
/// world (one material-bound node per texture) plus every placed object as its
/// own pivot node (doors, consoles, furniture — separate, transformable). The
/// object list is returned for cross-linking in `level.json`.
fn convert_environment(
    dat: &Dat<'_>,
    this_root: &str,
    root: &Path,
    tex_flags: &BTreeMap<String, (bool, bool)>,
    stats: &mut LevelStats,
) -> std::io::Result<Vec<EnvObjectInfo>> {
    use oni_geom::glb::{scene_to_glb, SceneNode};
    use oni_geom::level::extract_level;
    use oni_geom::Mesh;

    let mut object_infos: Vec<EnvObjectInfo> = Vec::new();

    for inst in dat.instances.iter().filter(|i| i.tag == TAG_AKEV) {
        let Some(level) = extract_level(dat, inst) else {
            continue;
        };
        if level.environment.is_empty() && level.objects.is_empty() {
            continue;
        }

        let mut meshes: Vec<Mesh> = Vec::new();
        let mut specs: Vec<NodeSpec> = Vec::new();
        let push_mesh = |meshes: &mut Vec<Mesh>, g: &oni_geom::level::TextureGroup| -> usize {
            meshes.push(Mesh {
                positions: g.positions.clone(),
                normals: g.normals.clone(),
                texcoords: g.texcoords.clone(),
                colors: g.colors.clone(),
                indices: g.indices.clone(),
                base_texture: None,
            });
            meshes.len() - 1
        };

        // Static world: one root node per texture.
        for (i, g) in level.environment.iter().enumerate() {
            let m = push_mesh(&mut meshes, g);
            let (additive, self_illum) = g
                .tex_instance
                .map(|ti| env_texture_flags(dat, ti, tex_flags))
                .unwrap_or((false, false));
            specs.push(NodeSpec {
                name: format!(
                    "env_{}",
                    sanitize(g.texture.as_deref().unwrap_or(&format!("mat{i}")))
                ),
                translation: [0.0; 3],
                parent: None,
                mesh: Some(m),
                uri: g
                    .tex_instance
                    .and_then(|ti| env_texture_uri(dat, ti, this_root)),
                blend: g.blend,
                additive,
                self_illum,
            });
        }

        // Placed objects: a pivot parent + one child per texture.
        for obj in &level.objects {
            let parent = specs.len();
            let node_name = obj.node_name();
            let tris: usize = obj.groups.iter().map(|g| g.indices.len() / 3).sum();
            specs.push(NodeSpec {
                name: node_name.clone(),
                translation: obj.pivot,
                parent: None,
                mesh: None,
                uri: None,
                blend: false,
                additive: false,
                self_illum: false,
            });
            for g in &obj.groups {
                let m = push_mesh(&mut meshes, g);
                let (additive, self_illum) = g
                    .tex_instance
                    .map(|ti| env_texture_flags(dat, ti, tex_flags))
                    .unwrap_or((false, false));
                specs.push(NodeSpec {
                    name: format!(
                        "{}_{}",
                        node_name,
                        sanitize(g.texture.as_deref().unwrap_or("mat"))
                    ),
                    translation: [0.0; 3],
                    parent: Some(parent),
                    mesh: Some(m),
                    uri: g
                        .tex_instance
                        .and_then(|ti| env_texture_uri(dat, ti, this_root)),
                    blend: g.blend,
                    additive,
                    self_illum,
                });
            }
            object_infos.push(EnvObjectInfo {
                node: node_name,
                type_name: obj.type_name.to_string(),
                tag: obj.tag,
                pivot: obj.pivot,
                triangles: tris,
            });
        }

        let nodes: Vec<SceneNode> = specs
            .iter()
            .map(|s| SceneNode {
                name: s.name.clone(),
                translation: s.translation,
                rotation: [0.0, 0.0, 0.0, 1.0],
                parent: s.parent,
                mesh: s.mesh.map(|mi| &meshes[mi]),
                texture_uri: s.uri.clone(),
                blend: s.blend,
                additive: s.additive,
                self_illum: s.self_illum,
            })
            .collect();
        std::fs::create_dir_all(root)?;
        std::fs::write(root.join("environment.glb"), scene_to_glb(&nodes))?;
        stats.environments += 1;
        stats.env_objects += level.objects.len();
    }
    Ok(object_infos)
}

const TAG_IGST: FourCc = FourCc(0x4947_5374); // 'I','G','S','t'
const TAG_TSTR: FourCc = FourCc(0x5453_7472); // 'T','S','t','r'
const TAG_FILM: FourCc = FourCc(0x4649_4C4D);
const TAG_TSFT: FourCc = FourCc(0x5453_4654);

pub(crate) fn le_u16(d: &[u8], o: usize) -> u16 {
    d.get(o..o + 2)
        .map_or(0, |b| u16::from_le_bytes([b[0], b[1]]))
}
fn le_i16(d: &[u8], o: usize) -> i16 {
    le_u16(d, o) as i16
}
pub(crate) fn le_u32(d: &[u8], o: usize) -> u32 {
    d.get(o..o + 4)
        .map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}
fn le_u64(d: &[u8], o: usize) -> u64 {
    (le_u32(d, o) as u64) | ((le_u32(d, o + 4) as u64) << 32)
}
pub(crate) fn le_f32(d: &[u8], o: usize) -> f32 {
    f32::from_bits(le_u32(d, o))
}
pub(crate) fn cstr(d: &[u8], o: usize, n: usize) -> String {
    let s = d.get(o..o + n).unwrap_or(&[]);
    let end = s.iter().position(|&b| b == 0).unwrap_or(s.len());
    String::from_utf8_lossy(&s[..end]).into_owned()
}

/// Writes `<root>/strings.json` — the internal `TStr` string table (animation
/// names, identifiers). Returns the count.
fn convert_strings(dat: &Dat<'_>, root: &Path) -> std::io::Result<usize> {
    let mut items = Vec::new();
    for inst in dat.instances.iter().filter(|i| i.tag == TAG_TSTR) {
        let Some(d) = dat.instance_data(inst) else {
            continue;
        };
        items.push(serde_json::json!({ "name": inst.name, "string": cstr(d, 0, 128) }));
    }
    if items.is_empty() {
        return Ok(0);
    }
    std::fs::create_dir_all(root)?;
    std::fs::write(
        root.join("strings.json"),
        serde_json::to_string_pretty(&serde_json::json!({"count": items.len(), "strings": items}))?,
    )?;
    Ok(items.len())
}

/// Writes `<root>/text.json` — the user-facing `IGSt` UI/tutorial/subtitle
/// strings (font size + text). Returns the count.
fn convert_ui_text(dat: &Dat<'_>, root: &Path) -> std::io::Result<usize> {
    let mut items = Vec::new();
    for inst in dat.instances.iter().filter(|i| i.tag == TAG_IGST) {
        let Some(d) = dat.instance_data(inst) else {
            continue;
        };
        // ONtIGUI_String: font_info(16) then char string[384].
        items.push(serde_json::json!({
            "name": inst.name,
            "fontSize": le_u16(d, 12),
            "text": cstr(d, 16, 384),
        }));
    }
    if items.is_empty() {
        return Ok(0);
    }
    std::fs::create_dir_all(root)?;
    std::fs::write(
        root.join("text.json"),
        serde_json::to_string_pretty(&serde_json::json!({"count": items.len(), "strings": items}))?,
    )?;
    Ok(items.len())
}

/// Writes `<root>/films.json` — `FILM` cutscene recordings (initial pose +
/// per-key input/camera state). Returns the count.
fn convert_films(dat: &Dat<'_>, root: &Path) -> std::io::Result<usize> {
    let mut films = Vec::new();
    for inst in dat.instances.iter().filter(|i| i.tag == TAG_FILM) {
        let Some(d) = dat.instance_data(inst) else {
            continue;
        };
        // ONtFilm: initialLocation@0, facings/aiming @12..28, length@28, numKeys@52, keys@56.
        let num_keys = le_u32(d, 52) as usize;
        if 56 + num_keys.saturating_mul(24) > d.len() {
            continue; // malformed
        }
        let keys: Vec<serde_json::Value> = (0..num_keys)
            .map(|k| {
                let o = 56 + k * 24; // ONtFilmKey: turnUD,turnLR,current(u64),time
                serde_json::json!({
                    "turnUD": le_f32(d, o), "turnLR": le_f32(d, o + 4),
                    "buttons": le_u64(d, o + 8), "time": le_u32(d, o + 16),
                })
            })
            .collect();
        films.push(serde_json::json!({
            "name": inst.name,
            "initialLocation": [le_f32(d, 0), le_f32(d, 4), le_f32(d, 8)],
            "initialFacing": le_f32(d, 12),
            "aiming": [le_f32(d, 20), le_f32(d, 24)],
            "length": le_u32(d, 28),
            "keyCount": keys.len(),
            "keys": keys,
        }));
    }
    if films.is_empty() {
        return Ok(0);
    }
    std::fs::create_dir_all(root)?;
    std::fs::write(
        root.join("films.json"),
        serde_json::to_string(
            &serde_json::json!({"ticksPerSecond": 60, "count": films.len(), "films": films}),
        )?,
    )?;
    Ok(films.len())
}

/// Writes `<root>/fonts.json` — `TSFT` font metrics (size + heights). The glyph
/// bitmaps are not atlased (a modern engine supplies its own font rendering).
fn convert_fonts(dat: &Dat<'_>, root: &Path) -> std::io::Result<usize> {
    let mut fonts = Vec::new();
    for inst in dat.instances.iter().filter(|i| i.tag == TAG_TSFT) {
        let Some(d) = dat.instance_data(inst) else {
            continue;
        };
        // TStFont: pad[6], font_size@6, font_style@8, ascending@10, descending@12,
        // leading_height@14, leading_width@16.
        fonts.push(serde_json::json!({
            "name": inst.name,
            "fontSize": le_u16(d, 6),
            "ascendingHeight": le_i16(d, 10),
            "descendingHeight": le_i16(d, 12),
            "leadingHeight": le_i16(d, 14),
            "leadingWidth": le_i16(d, 16),
        }));
    }
    if fonts.is_empty() {
        return Ok(0);
    }
    std::fs::create_dir_all(root)?;
    std::fs::write(
        root.join("fonts.json"),
        serde_json::to_string_pretty(&serde_json::json!({"count": fonts.len(), "fonts": fonts}))?,
    )?;
    Ok(fonts.len())
}

/// Emits `<root>/object_animations.json` — every `OBAN` scripted object
/// animation (sliding doors, lifts, driving vehicles) as keyframed transforms.
/// Returns the number of clips written.
fn convert_object_animations(dat: &Dat<'_>, root: &Path) -> std::io::Result<usize> {
    use oni_geom::objanim::{decode_object_animation, TAG_OBAN};

    let mut clips = Vec::new();
    for inst in dat.instances.iter().filter(|i| i.tag == TAG_OBAN) {
        let Some(a) = decode_object_animation(dat, inst) else {
            continue;
        };
        let keys: Vec<serde_json::Value> = a
            .keys
            .iter()
            .map(|k| {
                serde_json::json!({
                    "frame": k.frame,
                    "translation": [k.translation[0], k.translation[1], k.translation[2]],
                    "rotation": [k.rotation[0], k.rotation[1], k.rotation[2], k.rotation[3]],
                })
            })
            .collect();
        clips.push(serde_json::json!({
            "name": a.name,
            "flags": a.flags,
            "ticksPerFrame": a.ticks_per_frame,
            "numFrames": a.num_frames,
            "doorOpenFrames": a.door_open_frames,
            "keyframes": keys,
        }));
    }
    if clips.is_empty() {
        return Ok(0);
    }
    let doc = serde_json::json!({
        "ticksPerSecond": 60,
        "count": clips.len(),
        "animations": clips,
    });
    std::fs::create_dir_all(root)?;
    std::fs::write(
        root.join("object_animations.json"),
        serde_json::to_string(&doc)?,
    )?;
    Ok(clips.len())
}

/// Relative URI from `<this_root>/environment.glb` to a texture's PNG, routed
/// like standalone textures (named → `common/`, unnamed → this level).
fn env_texture_uri(dat: &Dat<'_>, tex_idx: usize, this_root: &str) -> Option<String> {
    let inst = dat.instances.get(tex_idx)?;
    if inst.tag != TAG_TXMP_ {
        return None;
    }
    let fname = asset_name(dat, inst);
    let tex_root = if inst.name.as_deref().is_some_and(|n| !n.is_empty()) {
        "common".to_string()
    } else {
        this_root.to_string()
    };
    Some(rel_path(
        this_root,
        &format!("{tex_root}/textures/{fname}.png"),
    ))
}

/// Writes `character.json` per character folder: a readable rig manifest (bone
/// names, part count, LOD) plus the likely animation-set code derived from the
/// name. (Oni has no static per-character animation list — animations resolve
/// at runtime by name convention; see `animations/index.json`.)
fn write_character_json(
    folder: &Path,
    body: &oni_geom::character::Body,
    anim_sets: &std::collections::BTreeSet<String>,
) -> std::io::Result<()> {
    let name = body.name.clone().unwrap_or_default();
    // `konoko_body_high` -> base `konoko` -> set code `KONOKO`.
    let base = name.split("_body").next().unwrap_or(&name);
    let code = base.to_ascii_uppercase();
    let animation_set = anim_sets.contains(&code).then(|| code.clone());
    let bones: Vec<&str> = body.parts.iter().map(|p| p.bone.as_str()).collect();

    let json = serde_json::json!({
        "name": name,
        "lod": "high",
        "parts": body.parts.len(),
        "bones": bones,
        "animationSet": animation_set,
    });
    let mut f = std::fs::File::create(folder.join("character.json"))?;
    use std::io::Write;
    f.write_all(serde_json::to_string_pretty(&json)?.as_bytes())?;
    f.write_all(b"\n")?;
    Ok(())
}

/// The leading set code of an animation name (uppercase run, e.g. `NINPIS`),
/// which identifies the character/weapon animation set.
fn anim_set_prefix(name: &str) -> &str {
    let end = name
        .find(|c: char| !c.is_ascii_uppercase() && !c.is_ascii_digit())
        .unwrap_or(name.len());
    &name[..end]
}

/// Emits shared animation clips as `common/animations/<name>.glb` — a named-bone
/// reference skeleton plus the animation, retargetable by bone name.
/// Builds an animation's glTF `extras` JSON from a decoded clip: root motion
/// (per-frame local `[x,z]`) and combat data (flags + attack hit windows).
fn animation_extras(clip: &oni_geom::character::Clip) -> String {
    let mut obj = serde_json::Map::new();
    if !clip.root_motion.is_empty() {
        obj.insert("rootMotion".into(), serde_json::json!(clip.root_motion));
    }
    if !clip.heights.is_empty() {
        obj.insert("rootHeight".into(), serde_json::json!(clip.heights));
    }
    if clip.flags != 0 {
        obj.insert("flags".into(), serde_json::json!(clip.flags));
    }
    // State-machine fields (TRtAnimType / TRtAnimState): type, from/to body
    // state, and variant bits. Drives the locomotion animation state machine.
    obj.insert("animType".into(), serde_json::json!(clip.anim_type));
    obj.insert("fromState".into(), serde_json::json!(clip.from_state));
    obj.insert("toState".into(), serde_json::json!(clip.to_state));
    if clip.varient != 0 {
        obj.insert("varient".into(), serde_json::json!(clip.varient));
    }
    if clip.final_rotation != 0.0 {
        obj.insert("finalRotation".into(), serde_json::json!(clip.final_rotation));
    }
    if clip.move_direction != 0 {
        obj.insert("moveDirection".into(), serde_json::json!(clip.move_direction));
    }
    if clip.end_interpolation != 0 {
        obj.insert(
            "endInterp".into(),
            serde_json::json!(clip.end_interpolation),
        );
    }
    if clip.max_interpolation != 0 {
        obj.insert(
            "maxInterp".into(),
            serde_json::json!(clip.max_interpolation),
        );
    }
    if !clip.attacks.is_empty() {
        let attacks: Vec<serde_json::Value> = clip
            .attacks
            .iter()
            .map(|a| {
                serde_json::json!({
                    "damage": a.damage,
                    "knockback": a.knockback,
                    "firstDamageFrame": a.first_damage_frame,
                    "lastDamageFrame": a.last_damage_frame,
                    "hitStun": a.hit_stun,
                    "blockStun": a.block_stun,
                    "stagger": a.stagger,
                    "damageBits": a.damage_bits,
                    "attackFlags": a.flags,
                })
            })
            .collect();
        obj.insert("attacks".into(), serde_json::json!(attacks));
    }
    if obj.is_empty() {
        String::new()
    } else {
        serde_json::to_string(&serde_json::Value::Object(obj)).unwrap_or_default()
    }
}

fn convert_animations(
    dat: &Dat<'_>,
    raw: Option<&[u8]>,
    root: &Path,
    fi: usize,
    global_named: &BTreeMap<(FourCc, String), usize>,
    stats: &mut LevelStats,
) -> std::io::Result<()> {
    use oni_geom::character;
    use oni_geom::glb::{animation_to_glb, AnimChannel, Joint};

    let Some(raw) = raw else { return Ok(()) };

    // Reference skeleton (bone names + translations + hierarchy) from a canonical
    // biped. Animations retarget to any character sharing these bone names.
    let ref_body = dat
        .instances
        .iter()
        .filter(|i| i.tag == TAG_TRBS)
        .find(|i| i.name.as_deref() == Some("konoko_body_high"))
        .or_else(|| dat.instances.iter().find(|i| i.tag == TAG_TRBS))
        .and_then(|i| character::extract_body(dat, i, character::lod::HIGH));
    let Some(ref_body) = ref_body else {
        return Ok(());
    };
    let joints: Vec<Joint> = ref_body
        .parts
        .iter()
        .map(|p| Joint {
            name: p.bone.clone(),
            translation: p.translation,
            parent: p.parent,
        })
        .collect();

    let dir = root_to_common(root).join("animations");
    std::fs::create_dir_all(&dir)?;

    let mut emitted: Vec<String> = Vec::new();
    for inst in dat.instances.iter().filter(|i| i.tag == TAG_TRAM) {
        if export_root(inst, dat, "common", fi, global_named) != Some("common") {
            continue;
        }
        let Some(clip) = character::decode_animation(dat, inst, raw) else {
            continue;
        };
        // Only emit clips that match the reference biped rig.
        if clip.channels.len() != joints.len() {
            continue;
        }
        let channels: Vec<AnimChannel> = clip
            .channels
            .iter()
            .map(|c| AnimChannel {
                node: c.part,
                times: &c.times,
                rotations: &c.rotations,
            })
            .collect();
        let name = asset_name(dat, inst);
        let extras = animation_extras(&clip);
        let glb = animation_to_glb(&joints, &name, &channels, &extras);
        std::fs::write(dir.join(format!("{name}.glb")), &glb)?;
        emitted.push(name);
        stats.animations += 1;
    }

    // Index clips by their set prefix (e.g. `NINPIS`) so the per-character/weapon
    // animation sets are discoverable (Oni resolves them by this naming code).
    let mut sets: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for n in &emitted {
        sets.entry(anim_set_prefix(n).to_string())
            .or_default()
            .push(n.clone());
    }
    let index = serde_json::json!({ "count": emitted.len(), "sets": sets });
    let mut f = std::fs::File::create(dir.join("index.json"))?;
    use std::io::Write;
    f.write_all(serde_json::to_string_pretty(&index)?.as_bytes())?;
    Ok(())
}

/// The default neutral pose: frame 0 of the unarmed standing idle.
const DEFAULT_POSE_ANIM: &str = "NINJAidle1";

const TAG_TRMA: FourCc = FourCc(0x5452_4D41);
const TAG_ONCC_: FourCc = FourCc(0x4F4E_4343);

/// Decodes a `TRMA` (body texture array): one TXMP name per body part, in part
/// order. `pad[22]`, `numMaps` u16 @22, `maps[]` (TXMP refs) @24.
fn decode_trma_maps(dat: &Dat<'_>, trma_name: &str) -> Vec<Option<String>> {
    let Some(inst) = dat
        .instances
        .iter()
        .find(|i| i.tag == TAG_TRMA && i.name.as_deref() == Some(trma_name))
    else {
        return Vec::new();
    };
    let Some(d) = dat.instance_data(inst) else {
        return Vec::new();
    };
    let n = le_u16(d, 22) as usize;
    (0..n)
        .map(|k| {
            let r = le_u32(d, 24 + k * 4);
            (r & 1 != 0)
                .then(|| {
                    dat.instances
                        .get((r >> 8) as usize)
                        .and_then(|t| t.name.clone())
                })
                .flatten()
        })
        .collect()
}

fn convert_characters(
    dat: &Dat<'_>,
    raw: Option<&[u8]>,
    root: &Path,
    fi: usize,
    global_named: &BTreeMap<(FourCc, String), usize>,
    stats: &mut LevelStats,
) -> std::io::Result<()> {
    use oni_geom::character;
    use oni_geom::glb::{scene_to_glb, SceneNode};

    // body name -> its TRMA (body-texture array) name, via the character class.
    let body_to_trma: BTreeMap<String, String> = dat
        .instances
        .iter()
        .filter(|i| i.tag == TAG_ONCC_)
        .filter_map(|i| oni_geom::charclass::decode_character_class(dat, i))
        .filter_map(|cc| Some((cc.body?, cc.textures?)))
        .collect();

    // Resolve the default neutral pose once (its keyframes live in .raw).
    let pose = raw.and_then(|raw| {
        character::find_animation(dat, DEFAULT_POSE_ANIM)
            .and_then(|a| character::frame0_pose(dat, a, raw))
    });
    // Animation-set codes present (for linking characters to their set).
    let anim_sets: std::collections::BTreeSet<String> = dat
        .instances
        .iter()
        .filter(|i| i.tag == TAG_TRAM)
        .filter_map(|i| i.name.as_deref())
        .map(|n| anim_set_prefix(n).to_string())
        .collect();

    for inst in dat.instances.iter().filter(|i| i.tag == TAG_TRBS) {
        // Characters are named/global; only the canonical file emits them.
        if export_root(inst, dat, "common", fi, global_named) != Some("common") {
            continue;
        }
        let Some(mut body) = character::extract_body(dat, inst, character::lod::HIGH) else {
            continue;
        };
        if let Some(pose) = &pose {
            // Pose into a neutral standing stance (root facing dropped).
            character::apply_pose(&mut body, pose, true);
        }
        let name = asset_name(dat, inst);
        let dir = root_to_common(root).join("characters").join(&name);
        std::fs::create_dir_all(&dir)?;

        // Per-part body textures (TRMA), one TXMP per part in part order.
        let part_textures = body_to_trma
            .get(&name)
            .map(|trma| decode_trma_maps(dat, trma))
            .unwrap_or_default();

        // model.glb is at common/characters/<name>/, textures at common/textures/.
        let nodes: Vec<SceneNode> = body
            .parts
            .iter()
            .enumerate()
            .map(|(i, p)| SceneNode {
                name: p.bone.clone(),
                translation: p.translation,
                rotation: p.rotation,
                parent: p.parent,
                mesh: Some(&p.mesh),
                blend: false,
                additive: false,
                self_illum: false,
                // Prefer the per-part body texture; fall back to the mesh base map.
                texture_uri: part_textures
                    .get(i)
                    .cloned()
                    .flatten()
                    .or_else(|| p.mesh.base_texture.clone())
                    .map(|t| format!("../../textures/{}.png", sanitize(&t))),
            })
            .collect();
        std::fs::write(dir.join("model.glb"), scene_to_glb(&nodes))?;
        write_character_json(&dir, &body, &anim_sets)?;
        stats.characters += 1;
        stats.character_meta += 1;
    }
    Ok(())
}

/// Resolves an M3GM's base texture to a relative URI from the mesh's folder, and
/// whether that target file is expected to exist. Returns None if no base map.
fn resolve_texture_ref(
    dat: &Dat<'_>,
    m3gm_index: usize,
    this_root: &str,
    _this_fi: usize,
    global_named: &BTreeMap<(FourCc, String), usize>,
    mesh_root: &str,
) -> Option<(String, bool)> {
    let inst = &dat.instances[m3gm_index];
    let d = dat.instance_data(inst)?;
    let r = u32::from_le_bytes([*d.get(28)?, d[29], d[30], d[31]]);
    if r & 1 == 0 {
        return None;
    }
    let tex = dat.instances.get((r >> 8) as usize)?;
    if tex.tag != TAG_TXMP {
        return None;
    }
    let name = asset_name(dat, tex);
    // Named textures live in common; unnamed live in this file's root.
    let (tex_root, exists) = match &tex.name {
        Some(n) if !n.is_empty() => (
            "common".to_string(),
            global_named.contains_key(&(TAG_TXMP, n.clone())),
        ),
        _ => (this_root.to_string(), dat.instance_data(tex).is_some()),
    };
    let from_dir = format!("{mesh_root}/models");
    let to_path = format!("{tex_root}/textures/{name}.png");
    Some((rel_path(&from_dir, &to_path), exists))
}

fn root_to_common(root: &Path) -> PathBuf {
    // root is <out>/<this_root>; swap the last component(s) for "common".
    let mut out = root.to_path_buf();
    // walk up to the output dir: this_root is either "common" or "levels/NN".
    if out.file_name().and_then(|n| n.to_str()) == Some("common") {
        return out;
    }
    out.pop(); // NN
    out.pop(); // levels
    out.join("common")
}

/// Stable asset name: the instance's name (sanitized) if present, else `<tag>_<index>`.
fn asset_name(_dat: &Dat<'_>, inst: &Instance) -> String {
    match &inst.name {
        Some(n) if !n.is_empty() => sanitize(n),
        _ => format!("{}_{}", sanitize(&inst.tag.to_string()), inst.index),
    }
}

/// Computes a relative path from directory `from_dir` to file `to_path`
/// (both output-root-relative, `/`-separated).
pub(crate) fn rel_path(from_dir: &str, to_path: &str) -> String {
    let from: Vec<&str> = from_dir.split('/').filter(|s| !s.is_empty()).collect();
    let to: Vec<&str> = to_path.split('/').filter(|s| !s.is_empty()).collect();
    let mut common = 0;
    while common < from.len() && common + 1 < to.len() && from[common] == to[common] {
        common += 1;
    }
    let ups = from.len() - common;
    let mut parts: Vec<String> = std::iter::repeat_n("..".to_string(), ups).collect();
    parts.extend(to[common..].iter().map(|s| s.to_string()));
    parts.join("/")
}

pub(crate) fn sanitize(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.is_empty() {
        "_".into()
    } else {
        s
    }
}

fn level_number(dat_path: &Path) -> Option<u32> {
    let stem = dat_path.file_stem()?.to_str()?;
    let digits: String = stem
        .trim_start_matches("level")
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

fn source_entry(path: &Path, bytes: &[u8]) -> serde_json::Value {
    let mut h = Sha256::new();
    h.update(bytes);
    serde_json::json!({
        "file": path.file_name().and_then(|n| n.to_str()).unwrap_or(""),
        "bytes": bytes.len(),
        "sha256": hex(&h.finalize()),
    })
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn write_manifest(
    out: &Path,
    report: &Report,
    source_files: &[serde_json::Value],
) -> std::io::Result<()> {
    let levels: serde_json::Map<String, serde_json::Value> = report
        .levels
        .iter()
        .map(|(k, s)| {
            (
                k.clone(),
                serde_json::json!({
                    "textures": s.textures, "sounds": s.sounds, "meshes": s.meshes,
                    "characters": s.characters, "animations": s.animations,
                    "environments": s.environments, "envObjects": s.env_objects,
                    "objectAnimations": s.object_animations, "strings": s.strings,
                    "films": s.films, "fonts": s.fonts, "objects": s.objects,
                    "particles": s.particles,
                    "warnings": s.warnings,
                }),
            )
        })
        .collect();
    let manifest = serde_json::json!({
        "format": "oni-modern-assets",
        "formatVersion": 1,
        "converter": env!("CARGO_PKG_VERSION"),
        "formats": { "models": "glTF 2.0 (.glb)", "textures": "PNG", "audio": "WAV", "data": "JSON" },
        "references": { "resolved": report.refs_resolved, "dangling": report.refs_dangling },
        "source": { "files": source_files },
        "levels": levels,
    });
    let mut f = std::fs::File::create(out.join("oni.json"))?;
    f.write_all(serde_json::to_string_pretty(&manifest)?.as_bytes())?;
    f.write_all(b"\n")?;
    Ok(())
}

fn write_png(path: &Path, w: u32, h: u32, rgba: &[u8]) -> std::io::Result<()> {
    let file = std::fs::File::create(path)?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(std::io::Error::other)?;
    writer
        .write_image_data(rgba)
        .map_err(std::io::Error::other)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::rel_path;

    #[test]
    fn relative_paths() {
        assert_eq!(
            rel_path("common/models", "common/textures/foo.png"),
            "../textures/foo.png"
        );
        assert_eq!(
            rel_path("levels/03/models", "common/textures/foo.png"),
            "../../../common/textures/foo.png"
        );
        assert_eq!(
            rel_path("levels/03/models", "levels/03/textures/bar.png"),
            "../textures/bar.png"
        );
    }
}
