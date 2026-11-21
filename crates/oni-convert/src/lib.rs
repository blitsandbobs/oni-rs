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
mod characters;
mod fonts;
mod melee;
mod bsl;
mod consoles;
mod doors;
mod lasers;
mod objects;
mod patrols;
mod placed;
mod particles;
mod materials;
mod particle_defs;
mod weapons;
mod anim_sounds;
mod impacts;
mod sounds;
mod menus;
mod pause_pages;
mod text_consoles;
mod turrets;

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
    let scripts = default_scripts_dir(input_dir);
    convert_with(input_dir, output_dir, scripts.as_deref())
}

/// Where the original level scripts (`<Level>/*.bsl`) are by default: the
/// Anniversary Edition's vanilla-script package extracted next to the game
/// data (`../bsl/…`), if present.
fn default_scripts_dir(input_dir: &Path) -> Option<PathBuf> {
    let p = input_dir
        .parent()?
        .join("bsl/Oni - Anniversary Edition/AEInstaller/packages/05100Vanilla_BSL/bsl/common");
    p.is_dir().then_some(p)
}

/// The level's name (its `ONLV` descriptor), which names its script folder.
fn level_name(dat: &Dat<'_>) -> Option<String> {
    dat.instances.iter().find(|i| i.tag == FourCc(u32::from_be_bytes(*b"ONLV")))?.name.clone()
}

/// Translates the level's scripts (`scripts_dir/<level name>/*.bsl`, any
/// case) to `scripts.lua` (`scripting/PORT.md`). Returns the number of
/// lines that couldn't be translated.
fn convert_scripts(dat: &Dat<'_>, scripts_dir: &Path, root: &Path) -> std::io::Result<usize> {
    let Some(name) = level_name(dat) else { return Ok(0) };
    let Some(folder) = std::fs::read_dir(scripts_dir)?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.file_name().and_then(|f| f.to_str()).is_some_and(|f| f.eq_ignore_ascii_case(&name)))
    else {
        return Ok(0);
    };
    let mut files: Vec<PathBuf> = std::fs::read_dir(&folder)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()).is_some_and(|x| x.eq_ignore_ascii_case("bsl")))
        .collect();
    files.sort();
    let mut texts = Vec::new();
    for f in &files {
        let bytes = std::fs::read(f)?;
        let name = f.file_name().and_then(|n| n.to_str()).unwrap_or("?").to_string();
        texts.push((name, String::from_utf8_lossy(&bytes).into_owned()));
    }
    let t = bsl::translate(&texts);
    println!("  scripts: {} files, {} functions from {}", texts.len(), t.functions, folder.display());
    for e in &t.errors {
        eprintln!("  script: {e}");
    }
    std::fs::create_dir_all(root)?;
    std::fs::write(root.join("scripts.lua"), t.lua)?;
    Ok(t.errors.len())
}

/// [`convert`], with the level scripts taken from `scripts_dir` (none: no
/// scripts).
pub fn convert_with(input_dir: &Path, output_dir: &Path, scripts_dir: Option<&Path>) -> std::io::Result<Report> {
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
    // The textures that break like glass (their material is glass).
    let mut glass_textures = std::collections::BTreeSet::new();
    // Particle classes (PAR3 binary data) are globally named; level0 wins ties.
    let mut particle_classes: BTreeMap<String, particles::ParticleClass> = BTreeMap::new();
    for (fi, f) in files.iter().enumerate() {
        let dat = Dat::parse(&f.bytes).map_err(|e| std::io::Error::other(format!("{e}")))?;
        let raw = std::fs::read(f.path.with_extension("raw")).ok();
        particles::collect_particle_classes(&dat, raw.as_deref(), &mut particle_classes);
        if let (0, Some(raw)) = (f.level, raw.as_deref()) {
            let glass = materials::breakable(&dat);
            glass_textures = materials::texture_materials(&dat, raw)
                .into_iter()
                .filter(|(_, m)| glass.contains(m))
                .map(|(t, _)| t)
                .collect();
        }
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

    set_case_shadowed(global_named.keys().map(|(_, n)| n.as_str()));

    let mut report = Report {
        levels: BTreeMap::new(),
        refs_resolved: 0,
        refs_dangling: 0,
    };
    let registry = oni_template::Registry::load();
    // Character classes (ONCC) are named/global; accumulate the canonical ones
    // across files into a single catalog written at the end.
    let mut character_classes: Vec<serde_json::Value> = Vec::new();
    let mut melee_profiles: Vec<melee::Profile> = Vec::new();
    let mut game_settings: Option<serde_json::Value> = None;
    let mut part_specs = serde_json::Map::new();
    let mut console_classes = std::collections::HashMap::new();
    let mut laser_classes = std::collections::BTreeMap::new();
    // The text console dialog's text font (level 0's), the pages' base.
    let mut console_font = text_consoles::Font { size: 7, style: 0, shade: Some(0xFFFF_FFFF) };
    // The pause screen's main area font (level 0's), its pages' base.
    let mut pause_font = text_consoles::Font { size: 10, style: 1, shade: Some(0xFFFF_7F00) };
    // The animated textures (by file stem), from every file.
    let mut texture_anims: BTreeMap<String, serde_json::Value> = BTreeMap::new();

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
        texture_animations(&dat, &f.root, fi, &global_named, &mut texture_anims);
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
        let env_objects = convert_environment(&dat, &f.root, &root, &texture_flags, &glass_textures, &mut stats)?;
        if let Some(raw) = raw.as_deref() {
            convert_messages(&dat, raw, &root)?;
            convert_navigation(&dat, raw, &root)?;
            convert_quads(&dat, &root)?;
            if f.level != 0 {
                characters::write(&dat, raw, &root)?;
                patrols::write(&dat, raw, &root)?;
                consoles::write(&dat, raw, &root, &console_classes)?;
                doors::write(&dat, raw, &root)?;
                placed::write(&dat, &root)?;
                lasers::write(&dat, raw, &root, &laser_classes)?;
                weapons::write_items(&dat, raw, &root)?;
                turrets::write(&dat, raw, &root)?;
                sounds::write_objects(&dat, raw, &root)?;
                particles::write_env_particles(&dat, raw, &root)?;
                if let Some(dir) = scripts_dir {
                    let bad = convert_scripts(&dat, dir, &root)?;
                    if bad > 0 {
                        stats.warnings.push(format!("{bad} script lines not translated"));
                    }
                }
            } else {
                console_classes = consoles::classes(&dat, &registry);
                laser_classes = lasers::classes(&dat, &registry);
                weapons::write_classes(&dat, &root)?;
                turrets::write_classes(&dat, &root)?;
                sounds::write(&dat, raw, &root)?;
                impacts::write(&dat, raw, &root)?;
                anim_sounds::write(&dat, raw, &root)?;
                std::fs::write(
                    root.join("laser_classes.json"),
                    serde_json::to_string_pretty(&serde_json::json!({ "classes": lasers::class_sounds(&dat) }))?,
                )?;
                let door_classes = doors::class_sounds(&dat);
                if !door_classes.is_empty() {
                    std::fs::write(
                        root.join("door_classes.json"),
                        serde_json::to_string_pretty(&serde_json::json!({ "classes": door_classes }))?,
                    )?;
                }
                let profiles = patrols::combat_profiles(&dat, raw);
                if !profiles.is_empty() {
                    let dir = output_dir.join("common");
                    std::fs::create_dir_all(&dir)?;
                    std::fs::write(
                        dir.join("combat_profiles.json"),
                        serde_json::to_string_pretty(&serde_json::json!({ "profiles": profiles }))?,
                    )?;
                }
            }
        }
        stats.object_animations = convert_object_animations(&dat, &root)?;
        stats.particles =
            particles::convert_particles(&dat, &f.root, &root, &particle_classes, &texture_flags)?;
        stats.strings = convert_strings(&dat, &root)? + convert_ui_text(&dat, &root)?;
        stats.films = convert_films(&dat, &root)?;
        stats.fonts = fonts::convert(&dat, &root)?;
        collect_character_classes(&dat, &f.root, fi, &global_named, &registry, &mut character_classes);
        for (k, v) in collect_part_specs(&dat, &registry) {
            part_specs.entry(k).or_insert(v);
        }
        if game_settings.is_none() {
            game_settings = collect_game_settings(&dat, &registry);
        }
        for p in melee::collect(&dat, raw.as_deref()) {
            if !melee_profiles.iter().any(|q| q.id == p.id) {
                melee_profiles.push(p);
            }
        }
        write_animation_collections(&dat, &f.root, fi, &global_named, output_dir)?;

        // The text console dialog (level 0), then each level's consoles.
        if f.level == 0 {
            if let Some((dialog, font)) = text_consoles::dialog(&dat, &registry) {
                text_consoles::write_dialog(&dialog, &root_to_common(&root))?;
                console_font = font;
            }
            if let Some((doc, font)) = pause_pages::common(&dat, &registry) {
                pause_pages::write(&doc, &root_to_common(&root).join("pause_screen.json"))?;
                pause_font = font;
            }
            if let Some(doc) = menus::common(&dat, &registry) {
                menus::write(&doc, &root_to_common(&root).join("menus.json"))?;
            }
        } else {
            if let Err(e) = text_consoles::write(text_consoles::consoles(&dat, &registry, console_font), &root) {
                stats.warnings.push(format!("text_consoles.json: {e}"));
            }
            if let Some(doc) = pause_pages::level(&dat, &registry, pause_font) {
                pause_pages::write(&doc, &root.join("pause_pages.json"))?;
            }
        }

        // level.json for playable levels.
        if f.level != 0 {
            match level_json::write_level_json(&dat, &root, &registry, &env_objects) {
                Ok(n) => stats.objects = n,
                Err(e) => stats.warnings.push(format!("level.json: {e}")),
            }
            if let Err(e) = characters::write_setups(&dat, &registry, &root) {
                stats.warnings.push(format!("setups.json: {e}"));
            }
        }

        report.levels.insert(f.root.clone(), stats);
    }

    // Write the character-class catalog (ONCC): body, variant, textures, aiming
    // screens, and the exact animation set per class.
    if !character_classes.is_empty() {
        let dir = output_dir.join("common/characters");
        std::fs::create_dir_all(&dir)?;
        // Each class's melee profile, by name (the data links them by id).
        for c in &mut character_classes {
            let id = c.as_object_mut().and_then(|o| o.remove("meleeProfileId"));
            let name = id
                .and_then(|v| v.as_u64())
                .and_then(|id| melee_profiles.iter().find(|p| u64::from(p.id) == id))
                .map(|p| p.name.clone());
            c["meleeProfile"] = serde_json::json!(name);
        }
        if !melee_profiles.is_empty() {
            let doc = serde_json::json!({
                "profiles": melee_profiles.iter().map(|p| &p.json).collect::<Vec<_>>(),
            });
            std::fs::write(dir.join("melee_profiles.json"), serde_json::to_string_pretty(&doc)?)?;
        }
        let doc = serde_json::json!({
            "count": character_classes.len(),
            "classes": character_classes,
        });
        std::fs::write(
            dir.join("classes.json"),
            serde_json::to_string_pretty(&doc)?,
        )?;
    }

    if !part_specs.is_empty() {
        let dir = output_dir.join("common");
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join("part_specs.json"), serde_json::to_string_pretty(&serde_json::json!({ "partSpecs": part_specs }))?)?;
    }
    if let Some(gs) = game_settings {
        let dir = output_dir.join("common");
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join("game_settings.json"), serde_json::to_string_pretty(&gs)?)?;
    }

    // The textures drawn additively / self-lit (by file name).
    {
        let pick = |k: usize| -> Vec<String> {
            texture_flags.iter().filter(|(_, f)| if k == 0 { f.0 } else { f.1 }).map(|(n, _)| sanitize(n)).collect()
        };
        let dir = output_dir.join("common");
        std::fs::create_dir_all(&dir)?;
        std::fs::write(
            dir.join("texture_flags.json"),
            serde_json::to_string_pretty(&serde_json::json!({ "additive": pick(0), "selfIlluminated": pick(1) }))?,
        )?;
    }

    // The animated textures: their frames and how they run.
    std::fs::write(
        output_dir.join("common/texture_animations.json"),
        serde_json::to_string_pretty(&texture_anims)?,
    )?;

    // Every particle class, decoded in full (`particle_defs`).
    let defs: BTreeMap<String, &[u8]> = particle_classes.iter().map(|(n, c)| (n.clone(), c.def.as_slice())).collect();
    particle_defs::write(&defs, &output_dir.join("common"))?;

    write_manifest(output_dir, &report, &source_files)?;
    Ok(report)
}

/// The game settings (`ONGS`) the engine uses: per-difficulty multipliers
/// (easy, normal, hard) on the AI's attack notice, block and dodge chances
/// and its aim inaccuracy.
/// The part specifications (`PSpc`, `WM_PartSpecification.h`): nine-slice
/// frames on a texture — by name, the texture, and each slice's texel
/// corners (`tl` / `br`, nine `[x, y]` each, column by column: left column
/// top, middle, bottom, then the middle column, then the right).
fn collect_part_specs(dat: &Dat<'_>, registry: &oni_template::Registry) -> serde_json::Map<String, serde_json::Value> {
    const TAG_PSPC: FourCc = FourCc(0x5053_7063);
    let mut out = serde_json::Map::new();
    let Some(layout) = registry.get(TAG_PSPC) else { return out };
    for inst in dat.instances.iter().filter(|i| i.tag == TAG_PSPC) {
        let (Some(name), Some(data)) = (inst.name.as_deref(), dat.instance_data(inst)) else { continue };
        let Ok(rec) = oni_template::decode(layout, data, dat.header.endian) else { continue };
        let fields = level_json::record_to_json(&rec, dat);
        let points = |k: &str| -> Vec<[i64; 2]> {
            fields[k].as_array().into_iter().flatten().map(|p| [p["x"].as_i64().unwrap_or(0), p["y"].as_i64().unwrap_or(0)]).collect()
        };
        let Some(texture) = fields["texture"]["$ref"].as_str() else { continue };
        out.insert(
            name.to_string(),
            serde_json::json!({ "texture": sanitize(texture), "tl": points("part_matrix_tl"), "br": points("part_matrix_br") }),
        );
    }
    out
}

fn collect_game_settings(dat: &Dat<'_>, registry: &oni_template::Registry) -> Option<serde_json::Value> {
    const TAG_ONGS: FourCc = FourCc(0x4F4E_4753);
    let inst = dat.instances.iter().find(|i| i.tag == TAG_ONGS)?;
    let rec = oni_template::decode(registry.get(TAG_ONGS)?, dat.instance_data(inst)?, dat.header.endian).ok()?;
    let floats = |name: &str| -> Vec<f32> {
        match rec.get(name).map(|e| &e.value) {
            Some(oni_template::Value::Array(items)) => items
                .iter()
                .filter_map(|r| match r.entries.first().map(|e| &e.value) {
                    Some(oni_template::Value::Float(f)) => Some(*f),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        }
    };
    // The game's event and condition sounds (impulse / ambient names; empty:
    // silent): `event_soundname[18][32]` @2000, `condition_soundname[5][32]`
    // @2576.
    let d = dat.instance_data(inst)?;
    const EVENTS: [&str; 18] = [
        "doorSuccess", "doorFail", "doorLock", "doorUnlock", "useHypo", "healthFull", "inventoryFail", "receiveAmmo",
        "receiveCell", "receiveHypo", "receiveLsi", "compass", "objectiveNew", "objectivePrompt", "objectiveComplete",
        "autosave", "timerTick", "timerSuccess",
    ];
    const CONDITIONS: [&str; 5] = ["healthLow", "healthOver", "shield", "invis", "timerCritical"];
    let names = |at: usize, keys: &[&str]| -> serde_json::Map<String, serde_json::Value> {
        keys.iter().enumerate().map(|(k, n)| (n.to_string(), serde_json::json!(cstr(d, at + k * 32, 32)))).collect()
    };
    let (events, conditions) = if d.len() >= 2576 + 5 * 32 { (names(2000, &EVENTS), names(2576, &CONDITIONS)) } else { Default::default() };
    // The colour ramp of a character's health (melee flashes, the HUD):
    // health fractions and their colours (ARGB hex).
    let fl = |name: &str| match rec.get(name).map(|e| &e.value) {
        Some(oni_template::Value::Float(f)) => *f,
        _ => 0.0,
    };
    let points = rec.int("healthcolor_numpoints").unwrap_or(0).clamp(0, 16) as usize;
    let colors: Vec<String> = match rec.get("healthcolor_colors").map(|e| &e.value) {
        Some(oni_template::Value::Array(items)) => {
            items.iter().filter_map(|r| r.entries.first()?.value.as_int()).map(|c| format!("#{:08x}", c as u32)).collect()
        }
        _ => Vec::new(),
    };
    let health_color: Vec<serde_json::Value> = floats("healthcolor_values")
        .into_iter()
        .zip(colors)
        .take(points)
        .map(|(v, c)| serde_json::json!({ "health": v, "color": c }))
        .collect();
    // The powerups' looks (`powerup_geom[7][128]` @152, `powerup_glow` @1048,
    // `powerup_glowsize` @1944), by type: the model, and the physics
    // sphere a dropped one falls with (`WPrPowerup_AddPhysics`: down to the
    // geometry's lowest point, at least 1; 2 without geometry).
    const TAG_M3GM: FourCc = FourCc(u32::from_be_bytes(*b"M3GM"));
    const POWERUPS: [&str; 6] = ["ammoBallistic", "ammoEnergy", "hypo", "shieldBelt", "invisibility", "lsi"];
    let powerups: serde_json::Map<String, serde_json::Value> = POWERUPS
        .iter()
        .enumerate()
        .map(|(k, name)| {
            let geom = cstr(d, 152 + k * 128, 128);
            let inst = (!geom.is_empty())
                .then(|| dat.instances.iter().find(|i| i.tag == TAG_M3GM && i.name.as_deref().is_some_and(|n| n == geom || n.trim_start_matches("M3GM") == geom)))
                .flatten();
            let radius = inst
                .and_then(|i| oni_geom::extract_mesh(dat, i))
                .map(|m| m.positions.iter().map(|p| p[1]).fold(f32::MAX, f32::min).abs().max(1.0))
                .unwrap_or(2.0);
            let mut v = serde_json::json!({ "radius": radius, "glowSize": le_f32(d, 1944 + 4 * k) });
            if let Some(i) = inst {
                v["model"] = serde_json::json!(format!("models/{}.glb", asset_name(dat, i)));
            }
            let glow = cstr(d, 1048 + k * 128, 128);
            if !glow.is_empty() {
                v["glow"] = serde_json::json!(sanitize(&glow));
            }
            (name.to_string(), v)
        })
        .collect();
    // The autoprompts (`num_autoprompts` @2808, `ONtAutoPrompt[16]` @2812:
    // event name[32], first and last level, message name[32]).
    let autoprompts: Vec<serde_json::Value> = dat
        .instance_data(inst)
        .map(|d| {
            (0..(le_u32(d, 2808) as usize).min(16))
                .map(|k| {
                    let o = 2812 + k * 68;
                    serde_json::json!({
                        "event": cstr(d, o, 32),
                        "levels": [le_u16(d, o + 32), le_u16(d, o + 34)],
                        "message": cstr(d, o + 36, 32),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Some(serde_json::json!({
        "powerups": powerups,
        "autoprompts": autoprompts,
        "healthColor": health_color,
        "eventSounds": events,
        "conditionSounds": conditions,
        // A hypo heals this share of full health (`hypo_amount`), past full
        // this share (`hypo_boost_amount`); above full, a super-hypo class
        // hits harder, from the base to the most at double health.
        "hypo": {
            "amount": fl("hypo_amount"),
            "boostAmount": fl("hypo_boost_amount"),
            "overhypoBaseDamage": fl("overhypo_base_damage"),
            "overhypoMaxDamage": fl("overhypo_max_damage"),
        },
        "boostedHealth": match rec.get("boosted_health").map(|e| &e.value) {
            Some(oni_template::Value::Float(f)) => *f,
            _ => 1.5,
        },
        "difficulty": {
            "levels": ["easy", "normal", "hard"],
            "noticeMultiplier": floats("notice_multipliers"),
            "blockMultiplier": floats("blocking_multipliers"),
            "dodgeMultiplier": floats("dodge_multipliers"),
            "inaccuracyMultiplier": floats("inaccuracy_multipliers"),
            "playerHpMultiplier": floats("player_hp_multipliers"),
            "enemyHpMultiplier": floats("enemy_hp_multipliers"),
        }
    }))
}

/// A class's AI perception tuning (`docs/PERCEPTION.md`): vision (the
/// stored cosines turned back into angles, radians), hearing, memory and
/// alert decay (ticks), behaviour flags, turning speed.
fn perception_json(rec: &oni_template::Record) -> serde_json::Value {
    let float = |name: &str| match rec.get(name).map(|e| &e.value) {
        Some(oni_template::Value::Float(f)) => *f,
        _ => 0.0,
    };
    let int = |name: &str| rec.int(name).unwrap_or(0);
    let angle = |name: &str| float(name).clamp(-1.0, 1.0).acos();
    serde_json::json!({
        "vision": {
            "centralDistance": float("vision.central_distance"),
            "peripheralDistance": float("vision.periph_distance"),
            "verticalAngle": angle("vision.vertical_range"),
            "centralFullAngle": angle("vision.central_range"),
            "centralMaxAngle": angle("vision.central_max"),
            "peripheralFullAngle": angle("vision.periph_range"),
            "peripheralMaxAngle": angle("vision.periph_max"),
        },
        "hearingAcuity": float("hearing"),
        "contactDecay": {
            "definite": int("memory.definite_to_strong"),
            "strong": int("memory.strong_to_weak"),
            "weak": int("memory.weak_to_forgotten"),
        },
        "alertDecay": {
            "high": int("memory.high_to_med"),
            "medium": int("memory.med_to_low"),
            "low": int("memory.low_to_lull"),
        },
        "flags": oni_schema::bit_names(&oni_schema::AI_FLAGS, int("ai2_behavior.flags") as u32),
        "turningNimbleness": float("ai2_behavior.turning_nimbleness"),
        "combat": {
            "makeSureTicks": int("ai2_behavior.combat_parameters.dead_makesure_delay"),
            "investigateBodyTicks": int("ai2_behavior.combat_parameters.investigate_body_delay"),
            "lostContactTicks": int("ai2_behavior.combat_parameters.lost_contact_delay"),
            "deadTauntPercent": int("ai2_behavior.combat_parameters.dead_taunt_percentage"),
            "gunPickupPercent": int("ai2_behavior.combat_parameters.gun_pickup_percentage"),
            "gunRunningPickupPercent": int("ai2_behavior.combat_parameters.gun_runningpickup_percentage"),
            "defaultCombatProfileId": int("ai2_behavior.default_combat_id"),
        },
        "dodge": {
            "reactTicks": int("ai2_behavior.dodge_react_frames"),
            "timeScale": float("ai2_behavior.dodge_time_scale"),
            "weightScale": float("ai2_behavior.dodge_weight_scale"),
        },
        "targeting": {
            "startleMissRatio": float("ai2_behavior.combat_parameters.targeting_params.startle_miss_angle"),
            "startleMissDistance": float("ai2_behavior.combat_parameters.targeting_params.startle_miss_distance"),
            "predictAmount": float("ai2_behavior.combat_parameters.targeting_params.predict_amount"),
            "predictPositionDelay": int("ai2_behavior.combat_parameters.targeting_params.predict_positiondelayframes"),
            "predictDelay": int("ai2_behavior.combat_parameters.targeting_params.predict_delayframes"),
            "predictVelocity": int("ai2_behavior.combat_parameters.targeting_params.predict_velocityframes"),
            "predictTrend": int("ai2_behavior.combat_parameters.targeting_params.predict_trendframes"),
        },
        "shootingSkill": shooting_skill(rec),
    })
}

/// A class's shooting skill per weapon slot (`AI2tShootingSkill` ×13; a
/// gun picks its slot by `shootSkillIndex`).
fn shooting_skill(rec: &oni_template::Record) -> Vec<serde_json::Value> {
    let Some(oni_template::Value::Array(slots)) = rec.get("shooting_skill").map(|e| &e.value) else {
        return Vec::new();
    };
    slots
        .iter()
        .map(|s| {
            let float = |name: &str| match s.get(name).map(|e| &e.value) {
                Some(oni_template::Value::Float(f)) => *f,
                _ => 0.0,
            };
            serde_json::json!({
                "recoilCompensation": float("recoil_compensation"),
                "bestAimingSine": float("best_aiming_angle"),
                "shotErrorPercent": float("shot_error"),
                "shotDecay": float("shot_decay"),
                "gunInaccuracy": float("gun_inaccuracy"),
                "delayMin": s.int("delay_min").unwrap_or(0),
                "delayMax": s.int("delay_max").unwrap_or(0),
            })
        })
        .collect()
}

/// Appends this file's canonical `ONCC` character classes to `out` as JSON.
/// A class's aiming screens (`TRSC` named `name` → its `TRAS` list): each
/// screen's clip (a grid of yaw × pitch poses) and its steps — per side, the
/// angle between frames and how many frames.
fn aiming_screens(dat: &Dat<'_>, name: &str) -> Vec<serde_json::Value> {
    const TAG_TRSC: FourCc = FourCc(u32::from_be_bytes(*b"TRSC"));
    const TAG_TRAS: FourCc = FourCc(u32::from_be_bytes(*b"TRAS"));
    let named = |i: &&oni_dat::Instance| i.name.as_deref().is_some_and(|n| n == name || n.strip_prefix("TRSC") == Some(name));
    let Some(d) = dat.instances.iter().filter(|i| i.tag == TAG_TRSC).find(named).and_then(|i| dat.instance_data(i)) else {
        return Vec::new();
    };
    let at = |r: u32| (r & 1 != 0).then(|| dat.instances.get((r >> 8) as usize)).flatten();
    let n = le_u16(d, 22) as usize;
    (0..n)
        .filter_map(|k| {
            let s = at(le_u32(d, 24 + k * 4)).filter(|i| i.tag == TAG_TRAS)?;
            let t = dat.instance_data(s)?;
            let clip = at(le_u32(t, 0)).and_then(|i| i.name.clone())?;
            let i16_at = |o: usize| le_u16(t, o) as i16;
            Some(serde_json::json!({
                "clip": clip,
                "yaw": { "negativeDelta": le_f32(t, 4), "positiveDelta": le_f32(t, 8), "negativeFrames": i16_at(12), "positiveFrames": i16_at(14) },
                "pitch": { "negativeDelta": le_f32(t, 16), "positiveDelta": le_f32(t, 20), "negativeFrames": i16_at(24), "positiveFrames": i16_at(26) },
            }))
        })
        .collect()
}

fn collect_character_classes(
    dat: &Dat<'_>,
    this_root: &str,
    fi: usize,
    global_named: &BTreeMap<(FourCc, String), usize>,
    registry: &oni_template::Registry,
    out: &mut Vec<serde_json::Value>,
) {
    use oni_geom::charclass::{decode_character_class, TAG_ONCC};
    let layout = registry.get(TAG_ONCC);
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
        let air = cc.air.map(|a| {
            serde_json::json!({
                "fallGravity": a.fall_gravity,
                "jumpGravity": a.jump_gravity,
                "jumpAmount": a.jump_amount,
                "maxVelocity": a.max_velocity,
                "jumpAccelAmount": a.jump_accel_amount,
                "framesFallGravity": a.frames_fall_gravity,
                "numJumpAccelFrames": a.num_jump_accel_frames,
                "damageStart": a.damage_start,
                "damageEnd": a.damage_end,
            })
        });
        // The class's own skin: one texture per body part (part order), as a
        // path relative to this file. Several classes share one body model
        // with different skins (easy / medium / hard colours).
        let part_textures: Vec<Option<String>> = cc
            .textures
            .as_deref()
            .map(|t| decode_trma_maps(dat, t))
            .unwrap_or_default()
            .into_iter()
            .map(|t| t.map(|t| format!("../textures/{}.png", sanitize(&t))))
            .collect();
        out.push(serde_json::json!({
            "name": cc.name,
            "variant": cc.variant,
            "body": cc.body,
            "model": model,
            "textures": cc.textures,
            "partTextures": part_textures,
            "aimingScreens": cc.aiming_screens,
            "aiming": cc.aiming_screens.as_deref().map(|n| aiming_screens(dat, n)).unwrap_or_default(),
            "animationCollection": cc.animation_collection,
            "animationCount": cc.animations.len(),
            "animations": cc.animations,
            "stats": stats,
            "air": air,
        }));
        if let (Some(d), Some(entry)) = (dat.instance_data(inst), out.last_mut()) {
            if let (Some(e), Some(f)) = (entry.as_object_mut(), impacts::class_fields(dat, d).as_object()) {
                e.extend(f.clone());
            }
        }
        // The AI's melee profile (resolved to a name once all are read) and
        // how long it lies dazed after a knockdown.
        let rec = layout.and_then(|l| {
            oni_template::decode(l, dat.instance_data(inst)?, dat.header.endian).ok()
        });
        if let (Some(rec), Some(entry)) = (rec, out.last_mut()) {
            let field = |n: &str| rec.int(&format!("ai2_behavior.{n}"));
            if let Some(id) = field("default_melee_id").filter(|&id| id != 0xFFFF) {
                entry["meleeProfileId"] = serde_json::json!(id);
            }
            if let (Some(min), Some(max)) = (field("dazed_minframes"), field("dazed_maxframes")) {
                entry["dazed"] = serde_json::json!({ "min": min, "max": max });
            }
            entry["perception"] = perception_json(&rec);
            // A hypo heals a point every this many ticks (`inventoryConstants`).
            entry["hypoTime"] = serde_json::json!(rec.int("inventoryConstants.hypoTime").unwrap_or(0));
            // Its shadow (`ONtShadowConstants`): texture, heights (fade
            // begins, none above), sizes (half-widths: on the ground, at the
            // fade, at the top), opacities (0–255: on the ground, at the
            // fade).
            let float = |n: &str| match rec.get(&format!("shadowConstants.{n}")).map(|e| &e.value) {
                Some(oni_template::Value::Float(v)) => *v,
                _ => 0.0,
            };
            let texture = match rec.get("shadowConstants.shadow_texture").map(|e| &e.value) {
                Some(oni_template::Value::Ref(_, r)) if r & 1 != 0 => dat.instances.get((r >> 8) as usize).map(|i| asset_name(dat, i)),
                _ => None,
            };
            entry["shadow"] = serde_json::json!({
                "texture": texture,
                "heightMax": float("height_max"),
                "heightFade": float("height_fade"),
                "sizeMax": float("size_max"),
                "sizeFade": float("size_fade"),
                "sizeMin": float("size_min"),
                "alphaMax": rec.int("shadowConstants.alpha_max").unwrap_or(0),
                "alphaFade": rec.int("shadowConstants.alpha_fade").unwrap_or(0),
            });
        }
    }
}

/// Writes each canonical `TRAC` animation collection to
/// `common/animations/collections/<name>.json`: its entries in on-disk order
/// (animation name + authored weight) and the parent it recursively falls back
/// to. The runtime rebuilds Oni's sorted/weighted/shortcut lookup from this.
fn write_animation_collections(
    dat: &Dat<'_>,
    this_root: &str,
    fi: usize,
    global_named: &BTreeMap<(FourCc, String), usize>,
    output_dir: &Path,
) -> std::io::Result<()> {
    use oni_geom::charclass::decode_animation_collection;
    let tag_trac = FourCc(0x5452_4143);
    let dir = output_dir.join("common/animations/collections");
    for inst in dat.instances.iter().filter(|i| i.tag == tag_trac) {
        if export_root(inst, dat, this_root, fi, global_named) != Some("common") {
            continue;
        }
        let Some(c) = decode_animation_collection(dat, inst) else {
            continue;
        };
        std::fs::create_dir_all(&dir)?;
        let entries: Vec<serde_json::Value> = c
            .entries
            .iter()
            .map(|e| serde_json::json!({ "anim": e.animation, "weight": e.weight }))
            .collect();
        let doc = serde_json::json!({
            "name": c.name,
            "recursive": c.recursive,
            "entries": entries,
        });
        std::fs::write(
            dir.join(format!("{}.json", sanitize(&c.name))),
            serde_json::to_string_pretty(&doc)?,
        )?;
    }
    Ok(())
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
    // Big textures (`TXMB`: the splash screens), assembled from their tiles.
    for inst in dat.instances.iter().filter(|i| i.tag == TAG_TXMB) {
        let Some(name) = inst.name.as_deref().filter(|n| !n.is_empty()) else { continue };
        let Some(d) = dat.instance_data(inst) else { continue };
        if let Some((w, h, rgba)) = big_texture(dat, raw, sep, d) {
            // (Each level has its own: `win_splash_screen` differs by level.)
            let dir = root.join("textures");
            std::fs::create_dir_all(&dir)?;
            if write_png(&dir.join(format!("{}.png", sanitize(name))), w, h, &rgba).is_ok() {
                stats.textures += 1;
            }
        }
    }
    Ok(())
}

const TAG_TXMB: FourCc = FourCc(0x5458_4D42);

/// The named textures exported from this file that animate: each by its
/// file stem, with its frames (output-relative paths, itself first), its
/// time per frame (ticks) and its animation flags. A texture's `animation`
/// (@140) links a `TXAN` (`M3tTextureMapAnimation`: timePerFrame u16 @12,
/// numFrames u32 @20, the frames' links @24 — the first empty, standing for
/// the texture itself).
fn texture_animations(
    dat: &Dat<'_>,
    this_root: &str,
    fi: usize,
    global_named: &BTreeMap<(FourCc, String), usize>,
    out: &mut BTreeMap<String, serde_json::Value>,
) {
    const FLAGS: [(u32, &str); 4] = [(1 << 6, "pingPong"), (1 << 7, "randomFrame"), (1 << 8, "randomStart"), (1 << 14, "dontAnimate")];
    let frame_path = |inst: &Instance| match &inst.name {
        Some(n) if !n.is_empty() => format!("common/textures/{}.png", asset_name(dat, inst)),
        _ => format!("{this_root}/textures/{}.png", asset_name(dat, inst)),
    };
    for inst in dat.instances.iter().filter(|i| i.tag == TAG_TXMP) {
        if export_root(inst, dat, this_root, fi, global_named) != Some("common") {
            continue;
        }
        let Some(d) = dat.instance_data(inst).filter(|d| d.len() >= 144) else { continue };
        let link = le_u32(d, 140);
        let Some(anim) = (link & 1 != 0).then(|| dat.instances.get((link >> 8) as usize)).flatten() else { continue };
        let Some(a) = dat.instance_data(anim).filter(|a| a.len() >= 24) else { continue };
        let count = le_u32(a, 20) as usize;
        if count == 0 || a.len() < 24 + count * 4 {
            continue;
        }
        let mut frames = vec![frame_path(inst)];
        for k in 1..count {
            let r = le_u32(a, 24 + k * 4);
            match (r & 1 != 0).then(|| dat.instances.get((r >> 8) as usize)).flatten() {
                Some(t) => frames.push(frame_path(t)),
                None => frames.push(frame_path(inst)),
            }
        }
        let flags = le_u32(d, 128);
        let names: Vec<&str> = FLAGS.iter().filter(|(b, _)| flags & b != 0).map(|(_, n)| *n).collect();
        let mut entry = serde_json::json!({ "timePerFrame": le_u16(a, 12), "frames": frames });
        if !names.is_empty() {
            entry["flags"] = serde_json::json!(names);
        }
        out.insert(asset_name(dat, inst), entry);
    }
}

/// A big texture's whole image (`M3tTextureMap_Big`: pad[8], width@8,
/// height@10, texel type@12, tiles across@16 and down@18 — zero as stored,
/// worked out at load as the 256-pixel tiles the size needs — tile
/// count@20, tiles@24), its tiles laid row by row 256 apart, each's
/// top-left part (`M3rDraw_BigBitmap`).
fn big_texture(dat: &Dat<'_>, raw: &[u8], sep: Option<&[u8]>, d: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    const TILE: usize = 256;
    let (w, h) = (le_u16(d, 8) as usize, le_u16(d, 10) as usize);
    let (nx, ny) = (w.div_ceil(TILE), h.div_ceil(TILE));
    if w == 0 || h == 0 || nx * ny > le_u32(d, 20) as usize {
        return None;
    }
    let mut out = vec![0u8; w * h * 4];
    for y in 0..ny {
        for x in 0..nx {
            let r = le_u32(d, 24 + (x + y * nx) * 4);
            let Some(t) = (r & 1 != 0).then(|| dat.instances.get((r >> 8) as usize)).flatten() else { continue };
            let Ok(tex) = oni_texture::TextureMap::from_instance(dat, t) else { continue };
            let Ok(rgba) = tex.decode_top_level_with(raw, sep) else { continue };
            let (tw, th) = (tex.width as usize, tex.height as usize);
            let (ox, oy) = (x * TILE, y * TILE);
            for row in 0..th.min(h.saturating_sub(oy)) {
                let n = tw.min(w.saturating_sub(ox));
                let src = row * tw * 4;
                let dst = ((oy + row) * w + ox) * 4;
                out[dst..dst + n * 4].copy_from_slice(&rgba[src..src + n * 4]);
            }
        }
    }
    Some((w as u32, h as u32, out))
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
    one_sided: bool,
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
/// Writes `quads.bin` (`docs/ASSET_FORMAT.md`): every quad of the level
/// with its corners, flags, plane, corner shades and texture — what decals
/// are built on and particles hit.
fn convert_quads(dat: &Dat<'_>, root: &Path) -> std::io::Result<()> {
    let Some(akev) = dat.instances.iter().find(|i| i.tag == oni_geom::level::TAG_AKEV) else {
        return Ok(());
    };
    let Some(q) = oni_geom::gq::extract_quads(dat, akev) else {
        return Ok(());
    };
    let mut b: Vec<u8> = b"ONIQ".to_vec();
    let u = |b: &mut Vec<u8>, v: u32| b.extend_from_slice(&v.to_le_bytes());
    for n in [q.points.len(), q.planes.len(), q.textures.len(), q.quads.len()] {
        u(&mut b, n as u32);
    }
    for v in q.points.iter().flatten().chain(q.planes.iter().flatten()) {
        b.extend_from_slice(&v.to_le_bytes());
    }
    for t in &q.textures {
        let name = t.and_then(|i| dat.instances.get(i)).map(|i| asset_name(dat, i)).unwrap_or_default();
        b.push(name.len().min(255) as u8);
        b.extend_from_slice(&name.as_bytes()[..name.len().min(255)]);
    }
    for g in &q.quads {
        for v in g.points.iter().chain([g.flags, g.plane].iter()).chain(g.shades.iter()).chain([g.texture as u32, g.object_tag].iter()) {
            u(&mut b, *v);
        }
        for v in g.bbox.iter().flatten() {
            b.extend_from_slice(&v.to_le_bytes());
        }
        u(&mut b, g.env_ref.unwrap_or(u32::MAX));
    }
    std::fs::create_dir_all(root)?;
    std::fs::write(root.join("quads.bin"), b)
}

/// Writes the level's navigation data (`docs/PATHFINDING.md`,
/// `docs/ASSET_FORMAT.md`): `navigation.json` (rooms, their hulls, links and
/// portals) and `navigation.bin` (every room's grid, one byte per square).
fn convert_navigation(dat: &Dat<'_>, raw: &[u8], root: &Path) -> std::io::Result<()> {
    use oni_geom::nav;
    use oni_schema as sc;
    let Some(akev) = dat.instances.iter().find(|i| i.tag == oni_geom::level::TAG_AKEV) else {
        return Ok(());
    };
    let Some(rooms) = nav::extract_navigation(dat, akev, raw) else {
        return Ok(());
    };
    let mut bin: Vec<u8> = Vec::new();
    let rooms_json: Vec<serde_json::Value> = rooms
        .iter()
        .map(|r| {
            let mut o = serde_json::json!({
                "flags": sc::bit_names(&sc::ROOM_FLAGS, r.flags),
                "min": r.min,
                "max": r.max,
                "planes": r.planes,
                "links": r.links.iter().map(|l| serde_json::json!({
                    "to": l.to,
                    "portal": sc::bit_names(&sc::PORTAL_FLAGS, l.quad_flags),
                    "quad": l.quad,
                })).collect::<Vec<_>>(),
            });
            if let Some((plane, height)) = r.stair {
                o["stair"] = serde_json::json!({ "plane": plane, "height": height });
            }
            if let Some(g) = &r.grid {
                o["grid"] = serde_json::json!({
                    "origin": g.origin,
                    "width": g.width,
                    "height": g.height,
                    "offset": bin.len(),
                });
                bin.extend_from_slice(&g.cells);
            }
            o
        })
        .collect();
    let doc = serde_json::json!({
        "squareSize": nav::SQUARE,
        "squareTypes": sc::SQUARE_TYPES,
        "grids": "navigation.bin",
        "rooms": rooms_json,
    });
    std::fs::create_dir_all(root)?;
    std::fs::write(root.join("navigation.json"), serde_json::to_string(&doc)?)?;
    std::fs::write(root.join("navigation.bin"), bin)?;
    Ok(())
}

fn convert_environment(
    dat: &Dat<'_>,
    this_root: &str,
    root: &Path,
    tex_flags: &BTreeMap<String, (bool, bool)>,
    glass_textures: &std::collections::BTreeSet<String>,
    stats: &mut LevelStats,
) -> std::io::Result<Vec<EnvObjectInfo>> {
    use oni_geom::glb::{scene_to_glb, SceneNode};
    use oni_geom::level::extract_level_with;
    use oni_geom::Mesh;

    let mut object_infos: Vec<EnvObjectInfo> = Vec::new();

    for inst in dat.instances.iter().filter(|i| i.tag == TAG_AKEV) {
        let breakable = |ti: usize| dat.instances.get(ti).and_then(|i| i.name.as_ref()).is_some_and(|n| glass_textures.contains(n));
        let Some(level) = extract_level_with(dat, inst, &breakable) else {
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
                bound_sphere: None,
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
                    "env_{}{}{}",
                    sanitize(g.texture.as_deref().unwrap_or(&format!("mat{i}"))),
                    g.env_ref.map(|r| format!("__ref{r}")).unwrap_or_default(),
                    surface_suffix(g),
                ) + &g.glass.map(|q| format!("__glass{q}")).unwrap_or_default(),
                translation: [0.0; 3],
                parent: None,
                mesh: Some(m),
                uri: g
                    .tex_instance
                    .and_then(|ti| env_texture_uri(dat, ti, this_root)),
                blend: g.blend,
                additive,
                self_illum,
                one_sided: !g.two_sided,
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
                one_sided: false,
            });
            for g in &obj.groups {
                let m = push_mesh(&mut meshes, g);
                let (additive, self_illum) = g
                    .tex_instance
                    .map(|ti| env_texture_flags(dat, ti, tex_flags))
                    .unwrap_or((false, false));
                specs.push(NodeSpec {
                    name: format!(
                        "{}_{}{}{}",
                        node_name,
                        sanitize(g.texture.as_deref().unwrap_or("mat")),
                        g.env_ref.map(|r| format!("__ref{r}")).unwrap_or_default(),
                        surface_suffix(g)
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
                    one_sided: !g.two_sided,
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
                one_sided: s.one_sided,
            })
            .collect();
        std::fs::create_dir_all(root)?;
        std::fs::write(root.join("environment.glb"), scene_to_glb(&nodes))?;
        stats.environments += 1;
        stats.env_objects += level.objects.len();
    }
    Ok(object_infos)
}

/// Marks a level node's surface kind in its name: `__invisible` (collides,
/// never drawn) or `__nocollide` (drawn; characters pass through — e.g.
/// stair steps over the ramp they walk on). See `docs/ASSET_FORMAT.md`.
fn surface_suffix(g: &oni_geom::level::TextureGroup) -> &'static str {
    match (g.visible, g.collide) {
        (false, _) => "__invisible",
        (true, false) => "__nocollide",
        _ => "",
    }
}

const TAG_IGST: FourCc = FourCc(0x4947_5374); // 'I','G','S','t'
const TAG_TSTR: FourCc = FourCc(0x5453_7472); // 'T','S','t','r'
const TAG_FILM: FourCc = FourCc(0x4649_4C4D);

pub(crate) fn le_u16(d: &[u8], o: usize) -> u16 {
    d.get(o..o + 2)
        .map_or(0, |b| u16::from_le_bytes([b[0], b[1]]))
}
pub(crate) fn le_i16(d: &[u8], o: usize) -> i16 {
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

/// Writes `<root>/messages.json` — the `SUBT` tables `messages` (what
/// scripts' `message` shows, by name) and `subtitles` (dialogue lines):
/// each entry a name and its text in the `.raw` block. Returns the count.
fn convert_messages(dat: &Dat<'_>, raw: &[u8], root: &Path) -> std::io::Result<usize> {
    const TAG_SUBT: FourCc = FourCc(0x5355_4254);
    let mut out = serde_json::Map::new();
    let mut n = 0;
    for inst in dat.instances.iter().filter(|i| i.tag == TAG_SUBT) {
        let Some(d) = dat.instance_data(inst) else { continue };
        let name = inst.name.as_deref().unwrap_or("").trim_start_matches("SUBT").to_string();
        let (base, count) = (le_u32(d, 16) as usize, le_u32(d, 20) as usize);
        // (Messages are found by exact name; subtitles by the first entry, in
        // the table's order, whose key the line's name starts with — so they
        // keep their order: `[key, text]` pairs.)
        let ordered = name == "subtitles";
        let mut table = serde_json::Map::new();
        let mut pairs = Vec::new();
        for k in 0..count {
            let at = base + le_u32(d, 24 + 4 * k) as usize;
            let key = cstr(raw, at, 256);
            let text = cstr(raw, at + key.len() + 1, 1024);
            if !key.is_empty() {
                if ordered {
                    pairs.push(serde_json::json!([key, text]));
                } else {
                    table.insert(key, serde_json::json!(text));
                }
            }
        }
        n += table.len() + pairs.len();
        out.insert(name, if ordered { serde_json::Value::Array(pairs) } else { serde_json::Value::Object(table) });
    }
    if n == 0 {
        return Ok(0);
    }
    std::fs::create_dir_all(root)?;
    std::fs::write(root.join("messages.json"), serde_json::to_string_pretty(&serde_json::Value::Object(out))?)?;
    Ok(n)
}

/// Writes `<root>/films.json` — `FILM` cutscene recordings (initial pose +
/// per-key input/camera state). Returns the count.
fn convert_films(dat: &Dat<'_>, root: &Path) -> std::io::Result<usize> {
    let mut films = Vec::new();
    for inst in dat.instances.iter().filter(|i| i.tag == TAG_FILM) {
        let Some(d) = dat.instance_data(inst) else {
            continue;
        };
        // ONtFilm: initialLocation@0, facing@12, desired facing@16, aiming
        // LR/UD@20/24, length@28, special animations@32, numKeys@52, keys@56.
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
            "initialDesiredFacing": le_f32(d, 16),
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


/// Emits `<root>/object_animations.json` — every `OBAN` scripted object
/// animation (sliding doors, lifts, driving vehicles) as keyframed transforms.
/// Returns the number of clips written.
/// An object animation as `object_animations.json` writes it (and the door
/// classes theirs): its name, flags by name, timing, the `scale` matrix (4×3,
/// column-major) and its keys.
pub(crate) fn object_animation_json(a: &oni_geom::objanim::ObjectAnimation) -> serde_json::Value {
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
    serde_json::json!({
        "name": a.name,
        "flags": oni_schema::bit_names(&oni_schema::OBJECT_ANIMATION_FLAGS, a.flags),
        "ticksPerFrame": a.ticks_per_frame,
        "numFrames": a.num_frames,
        "doorOpenFrames": a.door_open_frames,
        "scale": a.scale,
        "keyframes": keys,
    })
}

fn convert_object_animations(dat: &Dat<'_>, root: &Path) -> std::io::Result<usize> {
    use oni_geom::objanim::{decode_object_animation, TAG_OBAN};

    let mut clips = Vec::new();
    for inst in dat.instances.iter().filter(|i| i.tag == TAG_OBAN) {
        let Some(a) = decode_object_animation(dat, inst) else {
            continue;
        };
        clips.push(object_animation_json(&a));
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

/// Builds an animation's glTF `extras` JSON from a decoded clip, in the
/// schema of `docs/ASSET_FORMAT.md`: enumerations as names (`oni-schema`),
/// ranges as objects, defaults omitted.
fn animation_extras(clip: &oni_geom::character::Clip) -> String {
    use oni_schema as sc;
    use serde_json::json;
    let type_name = |t: u16| sc::name(&sc::ANIM_TYPES, t).unwrap_or("none");
    let state_name = |t: u16| sc::name(&sc::ANIM_STATES, t).unwrap_or("none");

    let mut obj = serde_json::Map::new();
    let mut put = |k: &str, v: serde_json::Value| {
        obj.insert(k.into(), v);
    };
    put("animType", json!(type_name(clip.anim_type)));
    put("fromState", json!(state_name(clip.from_state)));
    put("toState", json!(state_name(clip.to_state)));
    if clip.varient != 0 {
        put("variant", json!(sc::bit_names(&sc::VARIANTS, u32::from(clip.varient))));
    }
    if clip.varient_end != 0 {
        put("variantEnd", json!(sc::bit_names(&sc::VARIANTS, u32::from(clip.varient_end))));
    }
    if clip.flags != 0 {
        put("flags", json!(sc::bit_names(&sc::ANIM_FLAGS, clip.flags)));
    }
    if !clip.root_motion.is_empty() {
        put("rootMotion", json!(clip.root_motion));
    }
    if !clip.heights.is_empty() {
        put("rootHeight", json!(clip.heights));
    }
    if clip.final_rotation != 0.0 {
        put("finalRotation", json!(clip.final_rotation));
    }
    if clip.move_direction != 0 {
        put("moveDirection", json!(sc::name(&sc::DIRECTIONS, clip.move_direction).unwrap_or("none")));
    }
    if clip.aiming_type != 0 {
        put("aimingType", json!(type_name(clip.aiming_type)));
    }
    if clip.used_parts != 0 {
        put("usedParts", json!(clip.used_parts));
    }
    if clip.replace_parts != 0 {
        put("replaceParts", json!(clip.replace_parts));
    }
    if !clip.shortcuts.is_empty() {
        let list: Vec<_> = clip
            .shortcuts
            .iter()
            .map(|s| {
                let mut o = json!({ "fromState": state_name(s.state), "blendLength": s.length });
                if s.flags & 1 != 0 {
                    o["interruptsAtomic"] = json!(true);
                }
                o
            })
            .collect();
        put("shortcuts", json!(list));
    }
    if !clip.direct.is_empty() {
        put("direct", json!(clip.direct));
    }
    if clip.atomic_start <= clip.atomic_end {
        // An end past the last tick (the importer writes 0xFFFF) means "to
        // the end of the clip": omitted.
        let mut range = json!({ "start": clip.atomic_start });
        if u32::from(clip.atomic_end) + 1 < clip.num_frames {
            range["end"] = json!(clip.atomic_end);
        }
        put("atomic", range);
    }
    if clip.invulnerable_length != 0 {
        put(
            "invulnerable",
            json!({ "start": clip.invulnerable_start, "length": clip.invulnerable_length }),
        );
    }
    if clip.first_level != 0 {
        put("firstLevel", json!(clip.first_level));
    }
    if clip.end_interpolation != 0 {
        put("endBlend", json!(clip.end_interpolation));
    }
    if clip.max_interpolation != u16::MAX {
        put("maxBlendIn", json!(clip.max_interpolation)); // absent = no cap
    }
    if clip.hard_pause != 0 {
        put("hardPause", json!(clip.hard_pause));
    }
    if clip.soft_pause != 0 {
        put("softPause", json!(clip.soft_pause));
    }
    // The importer's "none" sentinels are -1 / 0xFFFF: omit those.
    if clip.action_frame >= 0 {
        put("actionFrame", json!(clip.action_frame));
    }
    if let Some(v) = sc::name(&sc::VOCALIZATIONS, clip.vocalization_type) {
        put("vocalization", json!(v));
    }
    if !clip.attacks.is_empty() {
        let attacks: Vec<_> = clip
            .attacks
            .iter()
            .map(|a| {
                let mut o = json!({
                    "damageWindow": { "start": a.first_damage_frame, "end": a.last_damage_frame },
                    "damage": a.damage,
                    "knockback": a.knockback,
                    "strikingParts": sc::bit_names(&sc::BODY_PARTS, a.damage_bits),
                    "precedence": a.precedence,
                });
                if a.damage_animation != 0 {
                    o["reaction"] = json!(type_name(a.damage_animation));
                }
                if a.flags != 0 {
                    o["flags"] = json!(sc::bit_names(&sc::ATTACK_FLAGS, a.flags));
                }
                for (k, v) in [("hitStun", a.hit_stun), ("blockStun", a.block_stun), ("stagger", a.stagger)] {
                    if v != 0 {
                        o[k] = json!(v);
                    }
                }
                o
            })
            .collect();
        put("attacks", json!(attacks));
    }
    if let Some(t) = &clip.throw {
        put(
            "throw",
            json!({
                "targetOffset": t.target_offset,
                "targetFacing": t.target_facing,
                "range": t.range,
                "targetAnimType": type_name(t.target_anim_type),
            }),
        );
    }
    if let Some(r) = &clip.reach {
        let bound = |b: &oni_geom::character::StrikeBound| {
            json!({
                "tick": b.tick, "attack": b.attack, "strike": b.strike,
                "offset": b.offset, "height": b.height, "distance": b.distance,
                "minHeight": b.min_height, "maxHeight": b.max_height, "angle": b.angle,
            })
        };
        let strikes: Vec<_> = r
            .strikes
            .iter()
            .map(|s| {
                json!({
                    "tick": s.tick, "angle": s.angle, "distance": s.distance,
                    "minHeight": s.min_height, "maxHeight": s.max_height,
                })
            })
            .collect();
        put(
            "reach",
            json!({
                "maxDistance": r.max_distance,
                "minHeight": r.min_height,
                "maxHeight": r.max_height,
                "spokes": r.spokes.to_vec(),
                "first": bound(&r.first),
                "farthest": bound(&r.farthest),
                "direction": sc::name(&sc::DIRECTIONS, r.direction).unwrap_or("none"),
                "strikes": strikes,
            }),
        );
    }
    if !clip.body.is_empty() {
        let track: Vec<_> = clip
            .body
            .iter()
            .map(|b| [b.offset[0], b.offset[1], b.floor, b.height])
            .collect();
        put("bodyTrack", json!(track));
    }
    if !clip.self_damage.is_empty() {
        let list: Vec<_> = clip
            .self_damage
            .iter()
            .map(|&(tick, damage)| json!({ "tick": tick, "damage": damage }))
            .collect();
        put("selfDamage", json!(list));
    }
    if !clip.footsteps.is_empty() {
        put("footsteps", json!(clip.footsteps));
        put("footstepKinds", json!(clip.footstep_kinds));
    }
    if !clip.attack_name.is_empty() {
        put("attackName", json!(clip.attack_name));
    }
    if clip.sound_frame != 0 {
        put("soundFrame", json!(clip.sound_frame));
    }
    if !clip.sounds.is_empty() {
        put("sounds", json!(clip.sounds.iter().map(|(n, f)| json!({ "impulse": n, "frame": f })).collect::<Vec<_>>()));
    }
    serde_json::to_string(&serde_json::Value::Object(obj)).unwrap_or_default()
}

/// Emits shared animation clips as `common/animations/<name>.glb` — a named-bone
/// reference skeleton plus the animation, retargetable by bone name.
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
                one_sided: false,
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
pub(crate) fn asset_name(_dat: &Dat<'_>, inst: &Instance) -> String {
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

/// Names whose file names would collide, ignoring case, with another asset
/// name's (`left` and `Left` are different textures, but one file on a
/// case-insensitive disk), with the suffix each takes (`~1`, `~2`, …; the
/// one written in lowercase, else the first, keeps its plain name).
static CASE_SHADOWED: std::sync::RwLock<BTreeMap<String, usize>> = std::sync::RwLock::new(BTreeMap::new());

/// Works out [`CASE_SHADOWED`] from every asset name in the game.
fn set_case_shadowed<'a>(names: impl Iterator<Item = &'a str>) {
    let mut groups: BTreeMap<String, std::collections::BTreeSet<&str>> = BTreeMap::new();
    for n in names {
        groups.entry(sanitize_plain(n).to_ascii_lowercase()).or_default().insert(n);
    }
    let mut shadowed = BTreeMap::new();
    for (folded, group) in groups.into_iter().filter(|(_, g)| g.len() > 1) {
        let plain = group.iter().copied().find(|n| sanitize_plain(n) == folded).or_else(|| group.iter().copied().next());
        let mut k = 0;
        for n in group {
            if Some(n) != plain {
                k += 1;
                shadowed.insert(n.to_string(), k);
            }
        }
    }
    *CASE_SHADOWED.write().unwrap() = shadowed;
}

/// An asset name as a file name: what a file system takes (other
/// characters as `_`), and a `~N` suffix when it would collide with
/// another name, ignoring case.
pub(crate) fn sanitize(name: &str) -> String {
    let s = sanitize_plain(name);
    match CASE_SHADOWED.read().unwrap().get(name) {
        Some(k) => format!("{s}~{k}"),
        None => s,
    }
}

fn sanitize_plain(name: &str) -> String {
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

pub(crate) fn write_png(path: &Path, w: u32, h: u32, rgba: &[u8]) -> std::io::Result<()> {
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
