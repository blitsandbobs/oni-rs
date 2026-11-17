//! Oni level viewer (engine milestone **E1**).
//!
//! Loads a converted level `environment.glb` (+ its PNG textures) and renders it
//! with wgpu — baked `COLOR_0` lighting and alpha materials included. Runs
//! windowed with a free-fly camera, or headless to a screenshot PNG.
//!
//! ```text
//! oni-view <scene.glb>                         # windowed, free-fly (WASD + drag)
//! oni-view <scene.glb> --screenshot out.png \  # headless one-frame render
//!     [--size WxH] [--eye x,y,z] [--target x,y,z]
//! ```

mod app;
mod camera;
mod gltf;
mod render;

use std::path::PathBuf;

use glam::Vec3;

use crate::camera::Camera;
use crate::render::{Gpu, Renderer};

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(scene_path) = args.next().map(PathBuf::from) else {
        eprintln!("usage: oni-view <scene.glb> [--screenshot out.png] [--size WxH] [--eye x,y,z] [--target x,y,z]");
        std::process::exit(2);
    };

    let (mut screenshot, mut size, mut eye, mut target) = (None, (1280u32, 720u32), None, None);
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--screenshot" => screenshot = args.next().map(PathBuf::from),
            "--size" => {
                if let Some(s) = args.next() {
                    if let Some((w, h)) = s.split_once('x') {
                        size = (w.parse().unwrap_or(1280), h.parse().unwrap_or(720));
                    }
                }
            }
            "--eye" => eye = args.next().and_then(parse_vec3),
            "--target" => target = args.next().and_then(parse_vec3),
            other => eprintln!("warning: ignoring unknown argument {other:?}"),
        }
    }

    eprintln!("loading {}...", scene_path.display());
    let scene = gltf::load(&scene_path).expect("load scene");
    eprintln!(
        "  {} primitives, {} textures, bounds {:?}..{:?}",
        scene.prims.len(),
        scene.textures.len(),
        scene.min.to_array().map(|v| v.round()),
        scene.max.to_array().map(|v| v.round()),
    );

    match screenshot {
        Some(out) => run_screenshot(&scene, &out, size, eye, target),
        None => app::run(scene, eye, target),
    }
}

fn parse_vec3(s: String) -> Option<Vec3> {
    let p: Vec<f32> = s.split(',').filter_map(|x| x.trim().parse().ok()).collect();
    (p.len() == 3).then(|| Vec3::new(p[0], p[1], p[2]))
}

/// A sensible default camera: stand near the centre, look across the level.
pub fn default_camera(scene: &gltf::Scene, aspect: f32) -> Camera {
    let mut cam = Camera::new(aspect);
    let c = scene.center();
    let e = scene.extent();
    // Eye offset diagonally out and slightly up; look at the centre.
    let eye = c + Vec3::new(e * 0.55, e * 0.35, e * 0.55);
    cam.look_at(eye, c);
    cam
}

/// Headless: render one frame to an offscreen texture and write a PNG.
fn run_screenshot(
    scene: &gltf::Scene,
    out: &std::path::Path,
    (w, h): (u32, u32),
    eye: Option<Vec3>,
    target: Option<Vec3>,
) {
    let gpu = pollster::block_on(Gpu::new(None));
    eprintln!("  GPU: {}", gpu.adapter.get_info().name);
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let mut renderer = Renderer::new(&gpu, scene, format);

    let mut cam = default_camera(scene, w as f32 / h as f32);
    if let (Some(e), Some(t)) = (eye, target) {
        cam.look_at(e, t);
    } else if let Some(e) = eye {
        cam.look_at(e, scene.center());
    }

    // Offscreen color target.
    let color = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("screenshot"),
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
    renderer.render(&gpu, &view, w, h, cam.view_proj());

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

    // Un-pad rows into tight RGBA8.
    let mut rgba = Vec::with_capacity((4 * w * h) as usize);
    for row in 0..h {
        let start = (row * bpr) as usize;
        rgba.extend_from_slice(&data[start..start + (4 * w) as usize]);
    }
    drop(data);
    buffer.unmap();

    write_png(out, w, h, &rgba);
    eprintln!("wrote {} ({w}x{h})", out.display());
}

fn write_png(path: &std::path::Path, w: u32, h: u32, rgba: &[u8]) {
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
