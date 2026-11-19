//! Oni level/character viewer (engine milestones **E1**–**E2**).
//!
//! Loads a converted level `environment.glb` (textures + baked lighting) and,
//! optionally, a character `model.glb` posed by a `TRAM` animation clip. Runs
//! windowed with a free-fly camera, or headless to a screenshot PNG.
//!
//! Placed environment particles (`particles.json`, from the converter) render as
//! additive glowing billboards; they auto-load next to the scene `.glb`, or via
//! `--particles`.
//!
//! ```text
//! oni-view <scene.glb> [--screenshot out.png] [--size WxH] [--eye x,y,z] [--target x,y,z]
//!     [--particles particles.json] [--character model.glb] [--anim clip.glb] [--char-pos x,y,z] [--time seconds]
//! ```

mod anim_sounds;
mod impacts;
mod audio;
mod ai;
mod anim;
mod app;
mod camera;
mod classes;
mod consoles;
mod control;
mod decal;
mod doors;
mod lasers;
mod p3;
mod weapons;
mod fighter;
mod gltf;
mod mission;
mod nav;
mod particles;
mod physics;
mod quads;
mod quirks;
mod render;
mod script;
mod skel;
mod world;

use std::path::PathBuf;

use glam::{Mat4, Vec3};

use crate::camera::Camera;
use crate::render::{CharDraw, Gpu, Renderer};
use crate::skel::{Character, Clip};

/// Which character classes play: the player's, and the opponent's (the
/// training dummy / sparring partner; the player's class when unset).
pub struct Cast {
    pub player: String,
    pub opponent: Option<String>,
    /// "easy", "normal" or "hard": how sharp the AI is.
    pub difficulty: String,
    /// The level's folder (its navigation data).
    pub level_dir: PathBuf,
}

/// A character placed in the world.
pub struct CharView {
    pub character: Character,
    pub clip: Option<Clip>,
    pub placement: Mat4,
}

impl CharView {
    /// Posed bone world matrices at `time` (placement baked in).
    pub fn bones(&self, time: f32) -> Vec<Mat4> {
        self.character
            .pose(self.clip.as_ref(), time)
            .into_iter()
            .map(|m| self.placement * m)
            .collect()
    }
    /// Posed bone world matrices using a specific clip (for the state machine,
    /// which chooses the clip per tick rather than using `self.clip`), with an
    /// optional stitch blend `(saved pose, weight)`.
    pub fn bones_with(
        &self,
        clip: &Clip,
        time: f32,
        stitch: Option<(&[glam::Quat], f32)>,
    ) -> Vec<Mat4> {
        self.character
            .pose_blended(Some(clip), time, stitch)
            .into_iter()
            .map(|m| self.placement * m)
            .collect()
    }
    /// World-space bounding box of the posed character.
    pub fn aabb(&self, time: f32) -> (Vec3, Vec3) {
        let bones = self.bones(time);
        let (mut mn, mut mx) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
        for part in &self.character.parts {
            let m = bones.get(part.bone).copied().unwrap_or(Mat4::IDENTITY);
            for v in &part.vertices {
                let p = m.transform_point3(Vec3::from_array(v.pos));
                mn = mn.min(p);
                mx = mx.max(p);
            }
        }
        (mn, mx)
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(scene_path) = args.next().map(PathBuf::from) else {
        eprintln!("usage: oni-view <scene.glb> [--screenshot out.png] [--size WxH] [--eye x,y,z] [--target x,y,z] [--particles particles.json] [--character model.glb] [--anim clip.glb] [--char-pos x,y,z] [--time s]");
        std::process::exit(2);
    };

    let mut screenshot = None;
    let mut size = (1280u32, 720u32);
    let mut particles_path = None;
    let (mut eye, mut target, mut char_path, mut anim_path, mut char_pos, mut time) =
        (None, None, None, None, None, None);
    let mut play = false;
    let mut class_name = String::from("konoko_generic");
    let mut opponent_class: Option<String> = None;
    let mut difficulty = String::from("normal");
    let mut level_number: Option<u16> = None;
    let mut faithful = false;
    let mut spar = false;
    let mut mission = false;
    let mut sound_law = audio::mixer::Law::Pc;
    let mut script_path: Option<PathBuf> = None;
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--play" => play = true,
            "--class" => class_name = args.next().unwrap_or(class_name),
            "--opponent" => opponent_class = args.next(),
            "--difficulty" => difficulty = args.next().unwrap_or(difficulty),
            "--faithful" => faithful = true,
            "--spar" => spar = true,
            "--mission" => mission = true,
            "--sound-law" => {
                sound_law = match args.next().as_deref() {
                    Some("mac") => audio::mixer::Law::Mac,
                    _ => audio::mixer::Law::Pc,
                }
            }
            "--level" => level_number = args.next().and_then(|s| s.parse().ok()),
            "--script" => script_path = args.next().map(PathBuf::from),
            "--screenshot" => screenshot = args.next().map(PathBuf::from),
            "--particles" => particles_path = args.next().map(PathBuf::from),
            "--size" => {
                if let Some((w, h)) = args.next().and_then(|s| {
                    s.split_once('x')
                        .map(|(a, b)| (a.parse().unwrap_or(1280), b.parse().unwrap_or(720)))
                }) {
                    size = (w, h);
                }
            }
            "--eye" => eye = args.next().and_then(parse_vec3),
            "--target" => target = args.next().and_then(parse_vec3),
            "--character" => char_path = args.next().map(PathBuf::from),
            "--anim" => anim_path = args.next().map(PathBuf::from),
            "--char-pos" => char_pos = args.next().and_then(parse_vec3),
            "--time" => time = args.next().and_then(|s| s.parse().ok()),
            other => eprintln!("warning: ignoring unknown argument {other:?}"),
        }
    }

    eprintln!("loading {}...", scene_path.display());
    let scene = gltf::load(&scene_path).expect("load scene");
    eprintln!(
        "  {} primitives, {} textures",
        scene.prims.len(),
        scene.textures.len()
    );

    // Environment particles: an explicit path, else `particles.json` next to the
    // scene (the converter writes it into each level folder beside `environment.glb`).
    // (Playing, or running a scenario, the level's environmental particles
    // run in the particle system instead — `env_particles.json`.)
    let live = (play || script_path.is_some()) && scene_path.with_file_name("env_particles.json").is_file();
    let particles_path = particles_path.or_else(|| {
        let p = scene_path.with_file_name("particles.json");
        (p.is_file() && !live).then_some(p)
    });
    let particles = particles_path
        .as_ref()
        .map(|p| {
            let ps = particles::load(p);
            let additive = ps.iter().filter(|q| q.additive).count();
            eprintln!(
                "  {} particles ({additive} additive) from {}",
                ps.len(),
                p.display()
            );
            ps
        })
        .unwrap_or_default();

    // Optional character.
    let charview = char_path.map(|p| {
        eprintln!("loading character {}...", p.display());
        let character = skel::load_character(&p).expect("load character");
        let clip = anim_path.as_ref().map(|ap| {
            eprintln!("  animation {}", ap.display());
            skel::load_clip(ap).expect("load clip")
        });
        let pos = char_pos.unwrap_or(scene.center());
        eprintln!(
            "  {} bones, {} parts at {:?}",
            character.bones.len(),
            character.parts.len(),
            pos.to_array().map(|v| v.round())
        );
        CharView {
            character,
            clip,
            placement: Mat4::from_translation(pos),
        }
    });

    // Play / script mode: the character classes (bodies, skins, animation
    // collections, tuning), loaded on demand.
    let classes = if play || script_path.is_some() {
        let oni_root = oni_root_from(&scene_path);
        let level = level_number.unwrap_or_else(|| level_from(&scene_path));
        match classes::Classes::open(&oni_root, level) {
            Ok(c) => Some(c),
            Err(e) => {
                eprintln!("play: cannot read the character classes: {e}");
                None
            }
        }
    } else {
        None
    };
    let cast = Cast {
        player: class_name,
        opponent: opponent_class,
        difficulty,
        level_dir: scene_path.parent().map(PathBuf::from).unwrap_or_default(),
    };

    // Scripted scenario (headless Lua driver): drives physics + the locomotion
    // state machine from a `.lua` file (see `scenarios/`). Replaces the old
    // `--sim`/`--walk`/`--facing` test flags.
    if let Some(sp) = script_path {
        match charview {
            Some(cv) => script::run_script(&sp, scene, cv, particles, classes, cast, faithful),
            None => eprintln!("--script requires --character"),
        }
        return;
    }

    match screenshot {
        Some(out) => run_screenshot(
            &scene,
            charview.as_ref(),
            &particles,
            &out,
            size,
            eye,
            target,
            time,
        ),
        None => app::run(
            scene,
            charview,
            particles,
            eye,
            target,
            app::PlaySetup {
                play,
                classes,
                cast,
                faithful,
                spar,
                mission,
                sound_law,
            },
        ),
    }
}

/// Concatenates all of the scene's triangles into one vertex/index buffer for a
/// Rapier trimesh collider (the level's collision geometry).
fn level_collision_mesh(scene: &gltf::Scene) -> (Vec<[f32; 3]>, Vec<u32>) {
    let mut verts: Vec<[f32; 3]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    // (Doors' closed leaves collide on their own, while closed.)
    // (Stair steps don't collide — the ramp under them does; invisible
    // surfaces do.)
    // (Glass panes collide on their own, until broken.)
    for p in scene.prims.iter().filter(|p| p.door.is_none() && p.env_ref.is_none() && p.glass.is_none() && p.collide) {
        let base = verts.len() as u32;
        verts.extend(p.vertices.iter().map(|v| v.pos));
        indices.extend(p.indices.iter().map(|i| base + i));
    }
    (verts, indices)
}

/// Each collision triangle's texture (its file's name), in
/// `level_collision_mesh`'s order: the floor's material for footsteps.
fn floor_textures(scene: &gltf::Scene) -> Vec<String> {
    let mut out = Vec::new();
    for p in scene.prims.iter().filter(|p| p.door.is_none() && p.env_ref.is_none() && p.glass.is_none() && p.collide) {
        let name = p
            .texture
            .and_then(|t| scene.textures.get(t))
            .and_then(|t| t.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        out.extend(std::iter::repeat_n(name, p.indices.len() / 3));
    }
    out
}

/// The level parts scripts show and hide (`env_show`), by reference: what
/// collides, and what blocks sight.
type Mesh = (Vec<[f32; 3]>, Vec<u32>);
type Parts = std::collections::BTreeMap<u32, Mesh>;
fn env_parts(scene: &gltf::Scene) -> (Parts, Parts) {
    let (mut solid, mut opaque) = (Parts::new(), Parts::new());
    for p in &scene.prims {
        let Some(r) = p.env_ref.filter(|_| p.door.is_none() && p.glass.is_none()) else { continue };
        for (set, keep) in [(&mut solid, p.collide), (&mut opaque, !p.blend && p.visible)] {
            if keep {
                let (verts, indices) = set.entry(r).or_default();
                let base = verts.len() as u32;
                verts.extend(p.vertices.iter().map(|v| v.pos));
                indices.extend(p.indices.iter().map(|i| base + i));
            }
        }
    }
    (solid, opaque)
}

/// Each door's closed-leaf triangles, by its object id.
fn door_meshes(scene: &gltf::Scene) -> std::collections::HashMap<u32, (Vec<[f32; 3]>, Vec<u32>)> {
    let mut out: std::collections::HashMap<u32, (Vec<[f32; 3]>, Vec<u32>)> = Default::default();
    for p in &scene.prims {
        let Some(d) = p.door else { continue };
        let (verts, indices) = out.entry(d).or_default();
        let base = verts.len() as u32;
        verts.extend(p.vertices.iter().map(|v| v.pos));
        indices.extend(p.indices.iter().map(|i| base + i));
    }
    out
}

/// The level's breakable glass panes: each one's quad, script reference
/// and triangles.
fn glass_parts(scene: &gltf::Scene) -> Vec<(u32, Option<u32>, Mesh)> {
    let mut out: Vec<(u32, Option<u32>, Mesh)> = Vec::new();
    for p in &scene.prims {
        let Some(q) = p.glass else { continue };
        let k = match out.iter().position(|(g, _, _)| *g == q) {
            Some(k) => k,
            None => {
                out.push((q, p.env_ref, Default::default()));
                out.len() - 1
            }
        };
        let (verts, indices) = &mut out[k].2;
        let base = verts.len() as u32;
        verts.extend(p.vertices.iter().map(|v| v.pos));
        indices.extend(p.indices.iter().map(|i| base + i));
    }
    out
}

/// The scene's opaque triangles (what blocks sight: not glass).
fn opaque_mesh(scene: &gltf::Scene) -> (Vec<[f32; 3]>, Vec<u32>) {
    let mut verts: Vec<[f32; 3]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    // (Doors' closed leaves block sight on their own, while closed.)
    for p in scene.prims.iter().filter(|p| !p.blend && p.visible && p.door.is_none() && p.env_ref.is_none() && p.glass.is_none()) {
        let base = verts.len() as u32;
        verts.extend(p.vertices.iter().map(|v| v.pos));
        indices.extend(p.indices.iter().map(|i| base + i));
    }
    (verts, indices)
}

/// The converted-asset root (`…/oni`) for a scene at `…/oni/levels/NN/x.glb`.
fn oni_root_from(scene: &std::path::Path) -> PathBuf {
    scene
        .parent()
        .and_then(|lvl| lvl.parent())
        .and_then(|levels| levels.parent())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// The level number from a scene path's `levels/NN` folder (0 if absent).
fn level_from(scene: &std::path::Path) -> u16 {
    scene
        .parent()
        .and_then(|d| d.file_name())
        .and_then(|n| n.to_str())
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}


fn parse_vec3(s: String) -> Option<Vec3> {
    let p: Vec<f32> = s.split(',').filter_map(|x| x.trim().parse().ok()).collect();
    (p.len() == 3).then(|| Vec3::new(p[0], p[1], p[2]))
}

/// Default camera: stand near the centre, look across the level.
pub fn default_camera(scene: &gltf::Scene, aspect: f32) -> Camera {
    let mut cam = Camera::new(aspect);
    let c = scene.center();
    let e = scene.extent();
    cam.look_at(c + Vec3::new(e * 0.55, e * 0.35, e * 0.55), c);
    cam
}

/// Frames the camera on a character (eye pulled back to fit its height).
pub fn frame_character(cv: &CharView, aspect: f32, time: f32) -> Camera {
    let mut cam = Camera::new(aspect);
    let (mn, mx) = cv.aabb(time);
    let center = (mn + mx) * 0.5;
    let height = (mx.y - mn.y).max(1.0);
    let dist = height * 1.8;
    cam.look_at(
        center + Vec3::new(dist * 0.6, height * 0.2, dist * 0.8),
        center,
    );
    cam
}

/// Headless: render one frame to an offscreen texture and write a PNG.
#[allow(clippy::too_many_arguments)]
fn run_screenshot(
    scene: &gltf::Scene,
    charview: Option<&CharView>,
    particles: &[particles::Particle],
    out: &std::path::Path,
    (w, h): (u32, u32),
    eye: Option<Vec3>,
    target: Option<Vec3>,
    time: Option<f32>,
) {
    let gpu = pollster::block_on(Gpu::new(None));
    eprintln!("  GPU: {}", gpu.adapter.get_info().name);
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let mut renderer = Renderer::new(&gpu, scene, particles, format);
    let model = charview.map(|c| renderer.add_character(&gpu, &c.character));

    let aspect = w as f32 / h as f32;
    let t = time.unwrap_or(0.0);
    let mut cam = match charview {
        Some(cv) => frame_character(cv, aspect, t),
        None => default_camera(scene, aspect),
    };
    match (eye, target) {
        (Some(e), Some(tg)) => cam.look_at(e, tg),
        (Some(e), None) => cam.look_at(e, scene.center()),
        _ => {}
    }
    let bones = charview.map(|c| c.bones(t)).unwrap_or_default();
    let draws: Vec<CharDraw> = match model {
        Some(m) => vec![CharDraw {
            model: m,
            bones: &bones,
        }],
        None => Vec::new(),
    };
    let (cam_right, cam_up) = cam.billboard_basis();

    let rgba = render_to_rgba(
        &gpu,
        &mut renderer,
        format,
        (w, h),
        cam.view_proj(),
        &draws,
        (cam_right, cam_up),
    );
    write_png(out, w, h, &rgba);
    eprintln!("wrote {} ({w}x{h})", out.display());
}

/// Renders one frame to an offscreen texture and reads it back as RGBA8 bytes.
/// Shared by `run_screenshot` and the scripting host.
#[allow(clippy::too_many_arguments)]
pub fn render_to_rgba(
    gpu: &Gpu,
    renderer: &mut Renderer,
    format: wgpu::TextureFormat,
    (w, h): (u32, u32),
    view_proj: Mat4,
    characters: &[CharDraw],
    (cam_right, cam_up): (Vec3, Vec3),
) -> Vec<u8> {
    let color = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("offscreen"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = color.create_view(&wgpu::TextureViewDescriptor::default());
    renderer.render(gpu, &view, w, h, view_proj, characters, cam_right, cam_up);

    // Copy to a readback buffer (bytes_per_row aligned to 256).
    let bpr = (4 * w).div_ceil(256) * 256;
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: (bpr * h) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &color,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bpr),
                rows_per_image: Some(h),
            },
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
    gpu.queue.submit([enc.finish()]);

    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    gpu.device.poll(wgpu::PollType::wait_indefinitely()).ok();
    let data = slice.get_mapped_range();
    let mut rgba = Vec::with_capacity((4 * w * h) as usize);
    for row in 0..h {
        let start = (row * bpr) as usize;
        rgba.extend_from_slice(&data[start..start + (4 * w) as usize]);
    }
    drop(data);
    buffer.unmap();
    rgba
}

pub fn write_png(path: &std::path::Path, w: u32, h: u32, rgba: &[u8]) {
    let file = std::fs::File::create(path).expect("create png");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(rgba)
        .unwrap();
}
