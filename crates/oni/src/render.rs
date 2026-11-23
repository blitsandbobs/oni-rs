//! wgpu renderer: loads a [`Scene`] onto the GPU and draws it with a textured,
//! baked-lit shader. Works both windowed and headless (offscreen).

use std::path::Path;

use bytemuck::{Pod, Zeroable};
use glam::Mat4;
use wgpu::util::DeviceExt;

use crate::gltf::{Prim, Scene, Vertex};

pub use crate::draw::*;

pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// A GPU device + queue (no surface — usable headless).
pub struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

impl Gpu {
    pub async fn new(compatible_surface: Option<&wgpu::Surface<'_>>) -> Gpu {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface,
                force_fallback_adapter: false,
            })
            .await
            .expect("no suitable GPU adapter");
        let limits = wgpu::Limits {
            max_push_constant_size: 128, // a 4x4 model matrix and a vec4
            ..wgpu::Limits::default()
        };
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("oni"),
                required_features: wgpu::Features::PUSH_CONSTANTS,
                required_limits: limits,
                memory_hints: wgpu::MemoryHints::Performance,
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                trace: wgpu::Trace::Off,
            })
            .await
            .expect("failed to create device");
        Gpu {
            instance,
            adapter,
            device,
            queue,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    view_proj: [[f32; 4]; 4],
    /// The fog's colour (rgb), and its start and end in screen depth (−1 at
    /// the near plane, 1 at the far).
    fog_color: [f32; 4],
    fog: [f32; 4],
}

/// One corner of a billboard quad. The world position is built in the vertex
/// shader as `center + (cam_right * corner.x + cam_up * corner.y) * size`, so
/// the quad always faces the camera.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ParticleVertex {
    center: [f32; 3],
    size: f32,
    corner: [f32; 2],
    uv: [f32; 2],
}

/// One corner of a decal: where, its texture coordinate, its colour
/// (lighting × opacity).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct DecalVertex {
    pos: [f32; 3],
    uv: [f32; 2],
    color: [f32; 4],
}

/// Camera billboard basis, pushed per particle draw (right/up world vectors).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct BillboardPush {
    right: [f32; 3],
    _pad0: f32,
    up: [f32; 3],
    _pad1: f32,
}

/// One corner of a screen-space overlay triangle (the HUD, text, the
/// letterbox): already in clip space, its texture coordinate, its colour.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct OverlayVertex {
    pos: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
}

/// A batch of billboards sharing one texture.
struct ParticleGroup {
    vbuf: wgpu::Buffer,
    ibuf: wgpu::Buffer,
    count: u32,
    texture: usize,
}

/// Which blend bucket a primitive draws in (controls pass order + pipeline).
#[derive(Clone, Copy, PartialEq)]
enum Bucket {
    Opaque,
    Alpha,
    Additive,
    /// Lens flares: added over everything, without the depth test.
    Flare,
}

struct GpuPrim {
    vbuf: wgpu::Buffer,
    ibuf: wgpu::Buffer,
    count: u32,
    texture: usize,
    bucket: Bucket,
    /// The door (object id) and leaf this is: drawn where the leaf has moved.
    door: Option<(u32, u8)>,
    /// The `env_show` reference it is drawn by.
    env_ref: Option<u32>,
    glass: Option<u32>,
    /// Bone index for a character part (world matrix comes from the pose), or
    /// `None` for pre-baked world-space environment geometry (identity model).
    bone: Option<usize>,
    /// Culled from behind (a one-sided level quad).
    cull: bool,
}

/// One uploaded character model (its bone-local parts). Several world instances
/// (fighters) can draw the same model with different per-instance poses, so the
/// mesh is uploaded once here and referenced by index.
struct CharModel {
    prims: Vec<GpuPrim>,
}

/// The push constants: a model matrix and a vec4 — for most draws an
/// override shade (r, g, b, on: flat lighting, `env_shade`), for the faded
/// and magic draws (alpha, time, kind).
const PUSH_SIZE: u32 = 80;

/// A draw's push constants: its model matrix, and the vec4.
fn push_block(model: Mat4, extra: [f32; 4]) -> [f32; 20] {
    let mut b = [0.0f32; 20];
    b[..16].copy_from_slice(&model.to_cols_array());
    b[16..].copy_from_slice(&extra);
    b
}

/// An override shade as the push constants' vec4 (none: off).
fn shade_extra(shade: Option<[f32; 3]>) -> [f32; 4] {
    shade.map_or([0.0; 4], |[r, g, b]| [r, g, b, 1.0])
}

pub struct Renderer {
    /// The world as this frame shows it (`frame.rs` fills it).
    pub scene: crate::draw::Scene,
    opaque_pipeline: wgpu::RenderPipeline,
    blend_pipeline: wgpu::RenderPipeline,
    jello_pipeline: wgpu::RenderPipeline,
    /// The objects in the camera's way (31 of 255).
    object_jello_pipeline: wgpu::RenderPipeline,
    /// A body fading (invisibility), and magic effects over bodies.
    faded_pipeline: wgpu::RenderPipeline,
    magic_pipeline: wgpu::RenderPipeline,
    /// The one-sided level quads' pipelines (backs culled).
    opaque_cull_pipeline: wgpu::RenderPipeline,
    blend_cull_pipeline: wgpu::RenderPipeline,
    additive_cull_pipeline: wgpu::RenderPipeline,
    overlay_pipeline: wgpu::RenderPipeline,
    overlay_additive_pipeline: wgpu::RenderPipeline,
    /// The HUD's textures (unfiltered, as the original draws it), by file.
    overlay_textures: std::collections::HashMap<std::path::PathBuf, usize>,
    /// The movie playing's picture (`movie.rs`): its texture (rewritten each
    /// frame), its place in `tex_bgs`, its size. An overlay draws it by
    /// naming [`MOVIE_TEXTURE`].
    movie: Option<(wgpu::Texture, usize, (u32, u32))>,
    overlay_white: wgpu::BindGroup,
    nearest: wgpu::Sampler,
    /// This frame's overlay, in screen pixels of a screen `overlay_size`
    /// big (scaled to the target).
    pub overlay: Vec<OverlayDraw>,
    pub overlay_size: [f32; 2],
    /// The gamma option (0–1; 0.5 leaves the frame as drawn): `gamma.rs`.
    pub gamma: f32,
    gamma_pass: crate::gamma::GammaPass,
    /// The level's prims' triangles by quad (`attach_quads`), for drawing
    /// some quads see-through.
    tri_quads: Vec<Option<TriQuads>>,
    /// Index buffers for the jello last built: for it, per prim, the solid
    /// rest and the see-through part.
    jello_cache: (Vec<u32>, std::collections::HashMap<usize, JelloSplit>),
    additive_pipeline: wgpu::RenderPipeline,
    flare_pipeline: wgpu::RenderPipeline,
    particle_pipeline: wgpu::RenderPipeline,
    decal_pipeline: wgpu::RenderPipeline,
    decal_additive_pipeline: wgpu::RenderPipeline,
    uniform_buf: wgpu::Buffer,
    uniform_bg: wgpu::BindGroup,
    tex_bgl: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    tex_bgs: Vec<wgpu::BindGroup>,
    default_tex: wgpu::BindGroup,
    prims: Vec<GpuPrim>,
    char_models: Vec<CharModel>,
    /// Rigid models loaded by file (guns), drawn as one-bone characters.
    models_by_path: std::collections::HashMap<std::path::PathBuf, Option<usize>>,
    textures_by_path: std::collections::HashMap<std::path::PathBuf, usize>,
    /// The scene's and models' textures by index, their files (to find the
    /// animated ones).
    tex_paths: std::collections::HashMap<usize, std::path::PathBuf>,
    /// The animated textures (by file stem) and, for each loaded one, its
    /// frames' texture indices (`texanim`).
    tex_anims: std::collections::HashMap<String, crate::texanim::TexAnim>,
    animated: std::collections::HashMap<usize, (crate::texanim::TexAnim, Vec<usize>)>,
    particles: Vec<ParticleGroup>,
    depth: Option<(wgpu::TextureView, u32, u32)>,
    /// The characters' depth alone, for the lens flares when characters hide
    /// them (`flares_behind_characters`).
    char_depth: Option<wgpu::TextureView>,
    /// An option, off as the original draws it (#58,,
    /// *Lens flares*): lens flares tested against the characters' depth, so
    /// a body covers the glow behind it. The level still hides them only by
    /// the visibility ray.
    pub flares_behind_characters: bool,
    char_depth_pipeline: wgpu::RenderPipeline,
    flare_depth_pipeline: wgpu::RenderPipeline,
    /// The level's sky: its box's faces, each a quad round
    /// the eye with its texture (in `tex_bgs`), drawn first — no depth, no
    /// fog — and its pipeline.
    sky: Vec<(GpuPrim, usize)>,
    sky_pipeline: wgpu::RenderPipeline,
}

impl Renderer {
    pub fn new(
        gpu: &Gpu,
        scene: &Scene,
        particles: &[crate::particles::Particle],
        format: wgpu::TextureFormat,
    ) -> Renderer {
        let device = &gpu.device;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("level"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let particle_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("particle"),
            source: wgpu::ShaderSource::Wgsl(PARTICLE_SHADER.into()),
        });

        let uniform_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("uniform"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let tex_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("texture"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("level"),
            bind_group_layouts: &[&uniform_bgl, &tex_bgl],
            push_constant_ranges: &[wgpu::PushConstantRange {
                stages: wgpu::ShaderStages::VERTEX,
                range: 0..PUSH_SIZE,
            }],
        });

        let vbl = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![
                0 => Float32x3, // pos
                1 => Float32x3, // normal
                2 => Float32x2, // uv
                3 => Float32x3, // color
            ],
        };

        let make_pipeline_cull = |label: &str,
                                  blend: Option<wgpu::BlendState>,
                                  depth_write: bool,
                                  fragment: &str,
                                  depth_compare: wgpu::CompareFunction,
                                  cull: Option<wgpu::Face>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: std::slice::from_ref(&vbl),
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fragment),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    // (Two-sided, but for the level's one-sided quads:
                    // their backs culled, as the original culls them.)
                    cull_mode: cull,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: depth_write,
                    depth_compare,
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                multiview: None,
                cache: None,
            })
        };
        // Additive: src*alpha + dst (glowing, no depth write).
        let additive = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent::OVER,
        };
        let make_pipeline_full =
            |label: &str,
             blend: Option<wgpu::BlendState>,
             depth_write: bool,
             fragment: &str,
             depth_compare: wgpu::CompareFunction| {
                make_pipeline_cull(label, blend, depth_write, fragment, depth_compare, None)
            };
        let make_pipeline_with =
            |label: &str, blend: Option<wgpu::BlendState>, depth_write: bool, fragment: &str| {
                make_pipeline_full(
                    label,
                    blend,
                    depth_write,
                    fragment,
                    wgpu::CompareFunction::Less,
                )
            };
        // The one-sided level quads' twins: backs culled.
        let culled =
            |label: &str, blend: Option<wgpu::BlendState>, depth_write: bool, fragment: &str| {
                make_pipeline_cull(
                    label,
                    blend,
                    depth_write,
                    fragment,
                    wgpu::CompareFunction::Less,
                    Some(wgpu::Face::Back),
                )
            };
        let make_pipeline = |label: &str, blend: Option<wgpu::BlendState>, depth_write: bool| {
            make_pipeline_with(label, blend, depth_write, "fs_main")
        };
        let opaque_pipeline = make_pipeline("opaque", None, true);
        // See-through walls (the camera's jello): faint, see-through.
        let jello_pipeline = make_pipeline_with(
            "jello",
            Some(wgpu::BlendState::ALPHA_BLENDING),
            false,
            "fs_jello",
        );
        let object_jello_pipeline = make_pipeline_with(
            "object jello",
            Some(wgpu::BlendState::ALPHA_BLENDING),
            false,
            "fs_jello_object",
        );
        let blend_pipeline = make_pipeline("alpha", Some(wgpu::BlendState::ALPHA_BLENDING), false);
        // (Additive textures draw with the fog forced off, as the original's
        // GL engine does — `gl_utility.c`, "force fog off!")
        let additive_pipeline = make_pipeline_with("additive", Some(additive), false, "fs_nofog");
        let opaque_cull_pipeline = culled("opaque culled", None, true, "fs_main");
        let blend_cull_pipeline = culled(
            "alpha culled",
            Some(wgpu::BlendState::ALPHA_BLENDING),
            false,
            "fs_main",
        );
        let additive_cull_pipeline = culled("additive culled", Some(additive), false, "fs_nofog");
        let flare_pipeline = make_pipeline_full(
            "flare",
            Some(additive),
            false,
            "fs_nofog",
            wgpu::CompareFunction::Always,
        );
        // (The option: the characters' depth laid down with the colour left
        // as it is, then the flares tested against it.)
        let keep_colour = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let char_depth_pipeline = make_pipeline_full(
            "character depth",
            Some(keep_colour),
            true,
            "fs_main",
            wgpu::CompareFunction::Less,
        );
        // The sky: under everything, unfogged (`ONiGameState_Display_NonReflectable`:
        // fog, depth writes and the depth test off).
        let sky_pipeline = make_pipeline_full(
            "sky",
            None,
            false,
            "fs_nofog",
            wgpu::CompareFunction::Always,
        );
        let flare_depth_pipeline = make_pipeline_full(
            "flare behind characters",
            Some(additive),
            false,
            "fs_nofog",
            wgpu::CompareFunction::LessEqual,
        );
        // A faded body (blended at its alpha, no depth written) and magic
        // effects (added on, no depth written): their own vertex stages.
        let effect_pipeline =
            |label: &str, blend: wgpu::BlendState, vertex: &str, fragment: &str| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(label),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some(vertex),
                        buffers: std::slice::from_ref(&vbl),
                        compilation_options: Default::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: Some(fragment),
                        targets: &[Some(wgpu::ColorTargetState {
                            format,
                            blend: Some(blend),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                        compilation_options: Default::default(),
                    }),
                    primitive: wgpu::PrimitiveState {
                        topology: wgpu::PrimitiveTopology::TriangleList,
                        ..Default::default()
                    },
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: DEPTH_FORMAT,
                        depth_write_enabled: false,
                        depth_compare: wgpu::CompareFunction::Less,
                        stencil: Default::default(),
                        bias: Default::default(),
                    }),
                    multisample: Default::default(),
                    multiview: None,
                    cache: None,
                })
            };
        let faded_pipeline = effect_pipeline(
            "faded",
            wgpu::BlendState::ALPHA_BLENDING,
            "vs_faded",
            "fs_faded",
        );
        let magic_pipeline = effect_pipeline("magic", additive, "vs_magic", "fs_magic");

        // Billboard particle pipeline: camera-facing quads, additive glow,
        // depth-tested against the scene but with no depth write.
        let particle_vbl = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<ParticleVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![
                0 => Float32x3, // center
                1 => Float32,   // size
                2 => Float32x2, // corner
                3 => Float32x2, // uv
            ],
        };
        let particle_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("particle"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &particle_shader,
                entry_point: Some("vs_main"),
                buffers: std::slice::from_ref(&particle_vbl),
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &particle_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(additive),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });

        // Decals: lit, see-through, depth-tested without writing depth
        // (they sit a little off their surface).
        let decal_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("decal"),
            source: wgpu::ShaderSource::Wgsl(DECAL_SHADER.into()),
        });
        let decal_vbl = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<DecalVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![
                0 => Float32x3, // pos
                1 => Float32x2, // uv
                2 => Float32x4, // color
            ],
        };
        let make_decal_pipeline = |label: &str, blend: wgpu::BlendState| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &decal_shader,
                    entry_point: Some("vs_main"),
                    buffers: std::slice::from_ref(&decal_vbl),
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &decal_shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(blend),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: false,
                    depth_compare: wgpu::CompareFunction::Less,
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                multiview: None,
                cache: None,
            })
        };
        // The overlay: screen-space, see-through, no depth.
        let overlay_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("overlay"),
            source: wgpu::ShaderSource::Wgsl(OVERLAY_SHADER.into()),
        });
        let overlay_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("overlay"),
            bind_group_layouts: &[&tex_bgl],
            push_constant_ranges: &[],
        });
        let overlay_vbl = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<OverlayVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4],
        };
        let make_overlay = |blend: wgpu::BlendState| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("overlay"),
                layout: Some(&overlay_layout),
                vertex: wgpu::VertexState {
                    module: &overlay_shader,
                    entry_point: Some("vs_main"),
                    buffers: std::slice::from_ref(&overlay_vbl),
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &overlay_shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(blend),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: Default::default(),
                multiview: None,
                cache: None,
            })
        };
        let overlay_pipeline = make_overlay(wgpu::BlendState::ALPHA_BLENDING);
        let overlay_additive_pipeline = make_overlay(additive);
        let decal_pipeline = make_decal_pipeline("decal", wgpu::BlendState::ALPHA_BLENDING);
        let decal_additive_pipeline = make_decal_pipeline("decal additive", additive);

        let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniform_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("uniform"),
            layout: &uniform_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buf.as_entire_binding(),
            }],
        });

        // (Anisotropic, #97: floors and walls at a slant stay sharp at
        // today's resolutions.)
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            anisotropy_clamp: 16,
            ..Default::default()
        });

        // Textures (missing/unreadable → magenta placeholder). Scene textures
        // keep their indices; character textures are appended and mapped.
        let mut tex_bgs = Vec::with_capacity(scene.textures.len());
        let mut tex_paths = std::collections::HashMap::new();
        for path in &scene.textures {
            let (w, h, rgba) = load_png(path).unwrap_or_else(|_| (1, 1, vec![255, 0, 255, 255]));
            tex_paths.insert(tex_bgs.len(), path.clone());
            tex_bgs.push(make_texture_bg(gpu, &tex_bgl, &sampler, w, h, &rgba));
        }
        let default_tex = make_texture_bg(gpu, &tex_bgl, &sampler, 1, 1, &[200, 200, 200, 255]);
        let nearest = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let overlay_white = make_texture_bg(gpu, &tex_bgl, &nearest, 1, 1, &[255, 255, 255, 255]);

        let prims: Vec<GpuPrim> = scene
            .prims
            .iter()
            .filter(|p| p.visible)
            .map(|p| upload_prim(device, p))
            .collect();

        // Billboard particles, batched by texture. Each particle contributes a
        // quad (4 verts / 6 indices); the quad is oriented in the vertex shader.
        let mut particle_tex: std::collections::BTreeMap<std::path::PathBuf, usize> =
            std::collections::BTreeMap::new();
        let mut batches: std::collections::BTreeMap<usize, (Vec<ParticleVertex>, Vec<u32>)> =
            std::collections::BTreeMap::new();
        for p in particles {
            let Some(path) = &p.texture else { continue };
            let tex = *particle_tex.entry(path.clone()).or_insert_with(|| {
                let (w, h, rgba) =
                    load_png(path).unwrap_or_else(|_| (1, 1, vec![255, 0, 255, 255]));
                tex_bgs.push(make_texture_bg(gpu, &tex_bgl, &sampler, w, h, &rgba));
                tex_bgs.len() - 1
            });
            let (verts, indices) = batches.entry(tex).or_default();
            let base = verts.len() as u32;
            // Corners CCW: (-,-) (+,-) (+,+) (-,+); the texture's top row on
            // the sprite's −Y side, as the original draws sprites.
            for (cx, cy, u, v) in [
                (-1.0, -1.0, 0.0, 0.0),
                (1.0, -1.0, 1.0, 0.0),
                (1.0, 1.0, 1.0, 1.0),
                (-1.0, 1.0, 0.0, 1.0),
            ] {
                verts.push(ParticleVertex {
                    center: p.position.to_array(),
                    size: p.size,
                    corner: [cx, cy],
                    uv: [u, v],
                });
            }
            indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        let particle_groups: Vec<ParticleGroup> = batches
            .into_iter()
            .map(|(texture, (verts, indices))| {
                let vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("particle_vbuf"),
                    contents: bytemuck::cast_slice(&verts),
                    usage: wgpu::BufferUsages::VERTEX,
                });
                let ibuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("particle_ibuf"),
                    contents: bytemuck::cast_slice(&indices),
                    usage: wgpu::BufferUsages::INDEX,
                });
                ParticleGroup {
                    vbuf,
                    ibuf,
                    count: indices.len() as u32,
                    texture,
                }
            })
            .collect();

        Renderer {
            opaque_pipeline,
            blend_pipeline,
            jello_pipeline,
            object_jello_pipeline,
            faded_pipeline,
            magic_pipeline,
            opaque_cull_pipeline,
            blend_cull_pipeline,
            additive_cull_pipeline,
            overlay_pipeline,
            overlay_additive_pipeline,
            overlay_textures: Default::default(),
            movie: None,
            overlay_white,
            nearest,
            scene: crate::draw::Scene::default(),
            overlay: Vec::new(),
            overlay_size: [640.0, 480.0],
            gamma: 0.5,
            gamma_pass: crate::gamma::GammaPass::new(device, format),
            tri_quads: Vec::new(),
            jello_cache: Default::default(),
            additive_pipeline,
            flare_pipeline,
            char_depth: None,
            flares_behind_characters: false,
            char_depth_pipeline,
            flare_depth_pipeline,
            sky: Vec::new(),
            sky_pipeline,
            particle_pipeline,
            decal_pipeline,
            decal_additive_pipeline,
            uniform_buf,
            uniform_bg,
            tex_bgl,
            sampler,
            tex_bgs,
            default_tex,
            prims,
            char_models: Vec::new(),
            models_by_path: Default::default(),
            textures_by_path: Default::default(),
            tex_paths,
            tex_anims: Default::default(),
            animated: Default::default(),
            particles: particle_groups,
            depth: None,
        }
    }

    /// A rigid model (a gun's `.glb`) as a character model whose every part
    /// rides the first bone — loaded once per file; None if it won't load.
    /// A model with every part's texture replaced by `texture` (a console's
    /// screen in its state), loaded once per pair.
    pub fn model_textured(
        &mut self,
        gpu: &Gpu,
        path: &std::path::Path,
        texture: &std::path::Path,
    ) -> Option<usize> {
        let key = std::path::PathBuf::from(format!("{}#{}", path.display(), texture.display()));
        if let Some(m) = self.models_by_path.get(&key) {
            return *m;
        }
        let texture = if texture.is_file() {
            texture.to_path_buf()
        } else {
            texture.with_file_name("NONE.png")
        };
        let m = crate::skel::load_character(path).ok().map(|mut ch| {
            for p in &mut ch.parts {
                p.bone = 0;
                p.texture = Some(texture.clone());
            }
            self.add_character(gpu, &ch)
        });
        self.models_by_path.insert(key, m);
        m
    }

    pub fn model_from(&mut self, gpu: &Gpu, path: &std::path::Path) -> Option<usize> {
        if let Some(m) = self.models_by_path.get(path) {
            return *m;
        }
        let m = crate::skel::load_character(path).ok().map(|mut ch| {
            for p in &mut ch.parts {
                p.bone = 0;
            }
            self.add_character(gpu, &ch)
        });
        self.models_by_path.insert(path.to_path_buf(), m);
        m
    }

    /// Uploads a character model's bone-local parts and returns its model index
    /// (for [`CharDraw::model`]). Several world instances can share one uploaded
    /// model, each drawn with its own pose. Character textures are appended to
    /// the shared texture table and de-duplicated within this model.
    /// The level's sky (`levels/NN/sky.json`,;
    /// `MSrGeomContext_Method_Skybox_Create`): a box 5 each way round the
    /// eye, its top, left, right, front and back faces each one texture
    /// across it (clamped at the edges, so the faces meet cleanly). None
    /// without the file.
    pub fn attach_sky(&mut self, gpu: &Gpu, level_dir: &std::path::Path) {
        self.sky.clear();
        let Some(doc) =
            crate::json::read_some::<oni_schema::files::SkyFile>(level_dir.join("sky.json"))
        else {
            return;
        };
        const D: f32 = 5.0;
        let corner = [
            [-D, D, -D],
            [D, D, -D],
            [D, D, D],
            [-D, D, D],
            [-D, -D, -D],
            [D, -D, -D],
            [D, -D, D],
            [-D, -D, D],
        ];
        // (Top, left, right, front, back: the corners each face takes, in
        // the order of its texture's (0,0), (1,0), (1,1), (0,1).)
        const FACES: [[usize; 4]; 5] = [
            [3, 2, 1, 0],
            [3, 0, 4, 7],
            [1, 2, 6, 5],
            [0, 1, 5, 4],
            [2, 3, 7, 6],
        ];
        const UV: [[f32; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let common = level_dir.join("../../common/textures");
        for (face, name) in FACES.iter().zip(&doc.textures) {
            let Some(name) = name else { continue };
            let path = [
                level_dir.join("textures").join(format!("{name}.png")),
                common.join(format!("{name}.png")),
            ]
            .into_iter()
            .find(|p| p.is_file());
            let Some((w, h, rgba)) = path.and_then(|p| load_png(&p).ok()) else {
                continue;
            };
            self.tex_bgs
                .push(make_texture_bg(gpu, &self.tex_bgl, &sampler, w, h, &rgba));
            let texture = self.tex_bgs.len() - 1;
            let vertices = face
                .iter()
                .zip(UV)
                .map(|(&c, uv)| Vertex {
                    pos: corner[c],
                    normal: [0.0, 1.0, 0.0],
                    uv,
                    color: [0.8; 3],
                })
                .collect();
            let prim = Prim {
                vertices,
                indices: vec![0, 1, 2, 0, 2, 3],
                texture: None,
                blend: false,
                additive: false,
                door: None,
                leaf: 0,
                visible: true,
                collide: false,
                env_ref: None,
                glass: None,
                one_sided: false,
            };
            self.sky.push((upload_prim(&gpu.device, &prim), texture));
        }
    }

    /// The movie's picture this frame (`w` × `h` RGBA, display values): its
    /// texture made the first time (or at a new size), then rewritten.
    pub fn movie_frame(&mut self, gpu: &Gpu, w: u32, h: u32, rgba: &[u8]) {
        if self.movie.as_ref().is_none_or(|m| m.2 != (w, h)) {
            let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("movie"),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
            // (Smoothed as it is scaled to the window.)
            let bg = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("movie"),
                layout: &self.tex_bgl,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            });
            self.tex_bgs.push(bg);
            self.movie = Some((tex, self.tex_bgs.len() - 1, (w, h)));
        }
        let Some((tex, _, _)) = &self.movie else {
            return;
        };
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * w),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
    }

    pub fn add_character(&mut self, gpu: &Gpu, ch: &crate::skel::Character) -> usize {
        let device = &gpu.device;
        let mut char_tex: std::collections::BTreeMap<std::path::PathBuf, usize> =
            std::collections::BTreeMap::new();
        let mut prims = Vec::with_capacity(ch.parts.len());
        for part in &ch.parts {
            let texture = part.texture.as_ref().map(|path| {
                *char_tex.entry(path.clone()).or_insert_with(|| {
                    let (w, h, rgba) =
                        load_png(path).unwrap_or_else(|_| (1, 1, vec![255, 0, 255, 255]));
                    self.tex_bgs.push(make_texture_bg(
                        gpu,
                        &self.tex_bgl,
                        &self.sampler,
                        w,
                        h,
                        &rgba,
                    ));
                    let i = self.tex_bgs.len() - 1;
                    self.tex_paths.insert(i, path.clone());
                    i
                })
            });
            let bucket = if part.additive {
                Bucket::Additive
            } else if part.blend {
                Bucket::Alpha
            } else {
                Bucket::Opaque
            };
            let vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("char_vbuf"),
                contents: bytemuck::cast_slice(&part.vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
            let ibuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("char_ibuf"),
                contents: bytemuck::cast_slice(&part.indices),
                usage: wgpu::BufferUsages::INDEX,
            });
            prims.push(GpuPrim {
                vbuf,
                ibuf,
                count: part.indices.len() as u32,
                texture: texture.unwrap_or(usize::MAX),
                bucket,
                door: None,
                env_ref: None,
                glass: None,
                bone: Some(part.bone),
                cull: false,
            });
        }
        // (Its animated textures' frames, once the table is known.)
        for i in char_tex.into_values() {
            self.animate_texture(gpu, i);
        }
        self.char_models.push(CharModel { prims });
        self.char_models.len() - 1
    }

    fn ensure_depth(&mut self, device: &wgpu::Device, w: u32, h: u32) {
        if matches!(self.depth, Some((_, dw, dh)) if dw == w && dh == h) {
            return;
        }
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("depth"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        self.depth = Some((view, w, h));
        let chars = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("character depth"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        self.char_depth = Some(chars.create_view(&wgpu::TextureViewDescriptor::default()));
    }

    /// Renders the scene into `target` (`w`×`h`) from `view_proj`. `characters`
    /// lists the character instances to draw this frame, each naming an uploaded
    /// model and its posed bone world-matrices (empty for an environment-only
    /// view).
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        gpu: &Gpu,
        target: &wgpu::TextureView,
        w: u32,
        h: u32,
        view_proj: Mat4,
        characters: &[CharDraw],
        cam_right: glam::Vec3,
        cam_up: glam::Vec3,
    ) {
        self.ensure_depth(&gpu.device, w, h);
        // (Gamma set: the frame drawn into a texture, then through the ramp.)
        let power = crate::gamma::power(self.gamma);
        let ramp = (power - 1.0).abs() > 1e-3;
        let screen = target;
        let frame = ramp.then(|| self.gamma_pass.frame(&gpu.device, w, h));
        let target = frame.as_ref().unwrap_or(screen);
        let beams = self.beam_prim(&gpu.device, cam_right, cam_up);
        let sprites = self.sprite_prims(gpu, cam_right, cam_up);
        let decals = self.decal_batches(gpu);
        // The magic effects, their textures loaded.
        let magic: Vec<(MagicDraw, usize)> = std::mem::take(&mut self.scene.magic)
            .into_iter()
            .map(|m| {
                let t = self.texture_index(gpu, &m.texture);
                (m, t)
            })
            .collect();
        self.jello_buffers(gpu);
        gpu.queue.write_buffer(
            &self.uniform_buf,
            0,
            bytemuck::bytes_of(&Uniforms {
                view_proj: view_proj.to_cols_array_2d(),
                fog_color: [
                    self.scene.fog.color[0],
                    self.scene.fog.color[1],
                    self.scene.fog.color[2],
                    1.0,
                ],
                fog: [self.scene.fog.start, self.scene.fog.end, 0.0, 0.0],
            }),
        );

        let depth_view = &self.depth.as_ref().unwrap().0;
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("level"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.04,
                            g: 0.05,
                            b: 0.07,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_bind_group(0, &self.uniform_bg, &[]);

            // Opaque (depth write), then alpha, then additive glow on top. Within
            // each bucket: the baked world-space scene prims, then every character
            // instance's parts posed by that instance's bones.
            // (A level part shaded by its script reference.)
            // (A door's quads, flat in its shade.)
            let env_shade = |p: &GpuPrim| {
                shade_extra(
                    p.env_ref
                        .and_then(|r| self.scene.env_shades.get(&r))
                        .or_else(|| p.door.and_then(|(d, _)| self.scene.door_shades.get(&d)))
                        .copied(),
                )
            };
            let draw_shaded =
                |pass: &mut wgpu::RenderPass, p: &GpuPrim, model: Mat4, extra: [f32; 4]| {
                    pass.set_push_constants(
                        wgpu::ShaderStages::VERTEX,
                        0,
                        bytemuck::cast_slice(&push_block(model, extra)),
                    );
                    let bg = self
                        .tex_bgs
                        .get(self.shown(p.texture))
                        .unwrap_or(&self.default_tex);
                    pass.set_bind_group(1, bg, &[]);
                    pass.set_vertex_buffer(0, p.vbuf.slice(..));
                    pass.set_index_buffer(p.ibuf.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..p.count, 0, 0..1);
                };
            let draw_prim = |pass: &mut wgpu::RenderPass, p: &GpuPrim, model: Mat4| {
                pass.set_push_constants(
                    wgpu::ShaderStages::VERTEX,
                    0,
                    bytemuck::cast_slice(&push_block(model, env_shade(p))),
                );
                let bg = self
                    .tex_bgs
                    .get(self.shown(p.texture))
                    .unwrap_or(&self.default_tex);
                pass.set_bind_group(1, bg, &[]);
                pass.set_vertex_buffer(0, p.vbuf.slice(..));
                pass.set_index_buffer(p.ibuf.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..p.count, 0, 0..1);
            };
            // The sky first, round the eye (`ONrSky_Draw`).
            if !self.sky.is_empty() {
                pass.set_pipeline(&self.sky_pipeline);
                let model = Mat4::from_translation(self.scene.eye);
                for (p, t) in &self.sky {
                    pass.set_push_constants(
                        wgpu::ShaderStages::VERTEX,
                        0,
                        bytemuck::cast_slice(&push_block(model, [0.0; 4])),
                    );
                    pass.set_bind_group(1, self.tex_bgs.get(*t).unwrap_or(&self.default_tex), &[]);
                    pass.set_vertex_buffer(0, p.vbuf.slice(..));
                    pass.set_index_buffer(p.ibuf.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..p.count, 0, 0..1);
                }
            }
            for (bucket, pipeline, cull_pipeline) in [
                (
                    Bucket::Opaque,
                    &self.opaque_pipeline,
                    &self.opaque_cull_pipeline,
                ),
                (
                    Bucket::Alpha,
                    &self.blend_pipeline,
                    &self.blend_cull_pipeline,
                ),
                (
                    Bucket::Additive,
                    &self.additive_pipeline,
                    &self.additive_cull_pipeline,
                ),
                (Bucket::Flare, &self.flare_pipeline, &self.flare_pipeline),
            ] {
                // (The option: the flares in a pass of their own, below.)
                if bucket == Bucket::Flare && self.flares_behind_characters {
                    continue;
                }
                let shown = |p: &&GpuPrim| {
                    p.env_ref
                        .is_none_or(|r| !self.scene.hidden_refs.contains(&r))
                        && p.glass
                            .is_none_or(|g| !self.scene.broken_glass.contains(&g))
                };
                // (Its solid and see-through parts; what glows on it glows on.)
                let door_seen_through = |p: &GpuPrim| {
                    matches!(p.bucket, Bucket::Opaque | Bucket::Alpha)
                        && p.door.is_some_and(|d| self.scene.jello_doors.contains(&d))
                };
                // (Each bucket starts on its pipeline: what follows the level's
                // prims — beams, sprites, characters, objects — draws with it,
                // unculled.)
                pass.set_pipeline(pipeline);
                let mut culling = false;
                for (k, p) in self
                    .prims
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| p.bucket == bucket && shown(p) && !door_seen_through(p))
                {
                    if culling != p.cull {
                        pass.set_pipeline(if p.cull { cull_pipeline } else { pipeline });
                        culling = p.cull;
                    }
                    let model = p
                        .door
                        .and_then(|d| self.scene.door_motion.get(&d))
                        .copied()
                        .unwrap_or(Mat4::IDENTITY);
                    match self.jello_cache.1.get(&k) {
                        // (Partly see-through: the solid rest only, here.)
                        Some((solid, n, _, _)) => {
                            pass.set_push_constants(
                                wgpu::ShaderStages::VERTEX,
                                0,
                                bytemuck::cast_slice(&push_block(model, env_shade(p))),
                            );
                            pass.set_bind_group(
                                1,
                                self.tex_bgs
                                    .get(self.shown(p.texture))
                                    .unwrap_or(&self.default_tex),
                                &[],
                            );
                            pass.set_vertex_buffer(0, p.vbuf.slice(..));
                            pass.set_index_buffer(solid.slice(..), wgpu::IndexFormat::Uint32);
                            pass.draw_indexed(0..*n, 0, 0..1);
                        }
                        None => draw_prim(&mut pass, p, model),
                    }
                }
                if culling {
                    pass.set_pipeline(pipeline);
                }
                // The see-through walls, with what is seen through.
                if bucket == Bucket::Alpha && !self.jello_cache.1.is_empty() {
                    for (&k, (_, _, seen, n)) in &self.jello_cache.1 {
                        let p = &self.prims[k];
                        if *n == 0 || !shown(&p) {
                            continue;
                        }
                        // (Both sides: the original counts a jello quad two-sided,
                        // `AKcGQ_Flag_2Sided`, so from behind it is still faintly
                        // there.)
                        pass.set_pipeline(&self.jello_pipeline);
                        // (A door's leaf where it stands: a closed door's
                        // quads go see-through as any wall's.)
                        let model = p
                            .door
                            .and_then(|d| self.scene.door_motion.get(&d))
                            .copied()
                            .unwrap_or(Mat4::IDENTITY);
                        pass.set_push_constants(
                            wgpu::ShaderStages::VERTEX,
                            0,
                            bytemuck::cast_slice(&push_block(model, env_shade(p))),
                        );
                        pass.set_bind_group(
                            1,
                            self.tex_bgs
                                .get(self.shown(p.texture))
                                .unwrap_or(&self.default_tex),
                            &[],
                        );
                        pass.set_vertex_buffer(0, p.vbuf.slice(..));
                        pass.set_index_buffer(seen.slice(..), wgpu::IndexFormat::Uint32);
                        pass.draw_indexed(0..*n, 0, 0..1);
                    }
                    pass.set_pipeline(pipeline);
                }
                // The objects in the camera's way, faint (`OBrDraw`'s jello
                // alpha, 31 of 255): all their parts, with what is seen
                // through.
                if bucket == Bucket::Alpha && !self.scene.jello_doors.is_empty() {
                    pass.set_pipeline(&self.object_jello_pipeline);
                    for p in self
                        .prims
                        .iter()
                        .filter(|p| door_seen_through(p) && shown(p))
                    {
                        let model = p
                            .door
                            .and_then(|d| self.scene.door_motion.get(&d))
                            .copied()
                            .unwrap_or(Mat4::IDENTITY);
                        draw_prim(&mut pass, p, model);
                    }
                    pass.set_pipeline(pipeline);
                }
                if bucket == Bucket::Alpha && characters.iter().any(|d| d.see_through) {
                    pass.set_pipeline(&self.object_jello_pipeline);
                    for d in characters.iter().filter(|d| d.see_through) {
                        let Some(cm) = self.char_models.get(d.model) else {
                            continue;
                        };
                        for p in &cm.prims {
                            let model = p
                                .bone
                                .and_then(|i| d.bones.get(i).copied())
                                .unwrap_or(Mat4::IDENTITY);
                            draw_shaded(&mut pass, p, model, shade_extra(d.shade));
                        }
                    }
                    pass.set_pipeline(pipeline);
                }
                if let Some(b) = beams.as_ref().filter(|b| b.bucket == bucket) {
                    draw_prim(&mut pass, b, Mat4::IDENTITY);
                }
                for p in sprites.iter().filter(|p| p.bucket == bucket) {
                    draw_prim(&mut pass, p, Mat4::IDENTITY);
                }
                for d in characters
                    .iter()
                    .filter(|d| !d.see_through && d.alpha >= 1.0)
                {
                    let Some(cm) = self.char_models.get(d.model) else {
                        continue;
                    };
                    for p in cm.prims.iter().filter(|p| p.bucket == bucket) {
                        let model = match p.bone {
                            Some(i) => d.bones.get(i).copied().unwrap_or(Mat4::IDENTITY),
                            None => Mat4::IDENTITY,
                        };
                        draw_shaded(&mut pass, p, model, shade_extra(d.shade));
                    }
                }
                // (A part drawn with the push constants' extra vec4.)
                let draw_with = |pass: &mut wgpu::RenderPass,
                                 p: &GpuPrim,
                                 model: Mat4,
                                 extra: [f32; 4],
                                 texture: usize| {
                    pass.set_push_constants(
                        wgpu::ShaderStages::VERTEX,
                        0,
                        bytemuck::cast_slice(&push_block(model, extra)),
                    );
                    pass.set_bind_group(
                        1,
                        self.tex_bgs.get(texture).unwrap_or(&self.default_tex),
                        &[],
                    );
                    pass.set_vertex_buffer(0, p.vbuf.slice(..));
                    pass.set_index_buffer(p.ibuf.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..p.count, 0, 0..1);
                };
                // Bodies fading (invisibility): every part blended at the
                // body's alpha, depth unwritten (`ONrCharacter_Draw`).
                if bucket == Bucket::Alpha
                    && characters.iter().any(|d| d.alpha < 1.0 && d.alpha > 0.0)
                {
                    pass.set_pipeline(&self.faded_pipeline);
                    for d in characters
                        .iter()
                        .filter(|d| !d.see_through && d.alpha < 1.0 && d.alpha > 0.0)
                    {
                        let Some(cm) = self.char_models.get(d.model) else {
                            continue;
                        };
                        for p in &cm.prims {
                            let model = p
                                .bone
                                .and_then(|i| d.bones.get(i).copied())
                                .unwrap_or(Mat4::IDENTITY);
                            draw_with(
                                &mut pass,
                                p,
                                model,
                                [d.alpha, 0.0, 0.0, 0.0],
                                self.shown(p.texture),
                            );
                        }
                    }
                    pass.set_pipeline(pipeline);
                }
                // The magic effects over them, added on (`TRrBody_DrawMagic`).
                if bucket == Bucket::Additive && !magic.is_empty() {
                    pass.set_pipeline(&self.magic_pipeline);
                    for (m, texture) in &magic {
                        let Some(cm) = self.char_models.get(m.model) else {
                            continue;
                        };
                        let kind = match m.kind {
                            MagicKind::Shield => 0.0,
                            MagicKind::Daodan => 1.0,
                            MagicKind::Invis => 2.0,
                        };
                        for p in &cm.prims {
                            let Some(bone) = p.bone else { continue };
                            let alpha = m.alpha.get(bone).copied().unwrap_or(0.0);
                            if alpha <= 0.0 {
                                continue;
                            }
                            let model = m.bones.get(bone).copied().unwrap_or(Mat4::IDENTITY);
                            draw_with(
                                &mut pass,
                                p,
                                model,
                                [alpha, m.time as f32, kind, 0.0],
                                *texture,
                            );
                        }
                    }
                    pass.set_pipeline(pipeline);
                }
                // The decals: after the solid scene, before what is seen
                // through or glows (in order: the level's, then the rest,
                // oldest first).
                if bucket == Bucket::Opaque {
                    for (vbuf, ibuf, count, texture, additive) in &decals {
                        pass.set_pipeline(if *additive {
                            &self.decal_additive_pipeline
                        } else {
                            &self.decal_pipeline
                        });
                        let bg = self.tex_bgs.get(*texture).unwrap_or(&self.default_tex);
                        pass.set_bind_group(1, bg, &[]);
                        pass.set_vertex_buffer(0, vbuf.slice(..));
                        pass.set_index_buffer(ibuf.slice(..), wgpu::IndexFormat::Uint32);
                        pass.draw_indexed(0..*count, 0, 0..1);
                    }
                }
            }

            // Billboard particles last: additive glow on top of everything,
            // oriented to the camera via the push-constant right/up basis.
            if !self.particles.is_empty() {
                pass.set_pipeline(&self.particle_pipeline);
                let push = BillboardPush {
                    right: cam_right.to_array(),
                    _pad0: 0.0,
                    up: cam_up.to_array(),
                    _pad1: 0.0,
                };
                pass.set_push_constants(wgpu::ShaderStages::VERTEX, 0, bytemuck::bytes_of(&push));
                for g in &self.particles {
                    let bg = self.tex_bgs.get(g.texture).unwrap_or(&self.default_tex);
                    pass.set_bind_group(1, bg, &[]);
                    pass.set_vertex_buffer(0, g.vbuf.slice(..));
                    pass.set_index_buffer(g.ibuf.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..g.count, 0, 0..1);
                }
            }
        }
        if self.flares_behind_characters {
            self.flare_pass(&mut encoder, target, characters, &sprites);
        }
        self.overlay_pass(gpu, &mut encoder, target);
        if ramp {
            self.gamma_pass.apply(&mut encoder, screen, power);
        }
        gpu.queue.submit([encoder.finish()]);
    }
}

/// A prim split for the jello: its solid rest and its see-through part
/// (index buffers and counts).
type JelloSplit = (wgpu::Buffer, u32, wgpu::Buffer, u32);

/// A level prim's triangles (its index list) and the quad each came from.
struct TriQuads {
    indices: Vec<u32>,
    quads: Vec<u32>,
}

/// How wide a drawn laser beam is.
const BEAM_WIDTH: f32 = 0.25;

impl Renderer {
    /// Learns which level quad each of the scene's level triangles came
    /// from (matching their corners exactly: the converter copies them),
    /// so quads can be drawn see-through.
    pub fn attach_quads(&mut self, scene: &Scene, quads: &crate::quads::Quads) {
        // (A triangle by its corners in any order: a quad may be wound
        // either way round to face along its plane.)
        let key = |a: [f32; 3], b: [f32; 3], c: [f32; 3]| -> [u32; 9] {
            let mut t = [a, b, c].map(|v| v.map(f32::to_bits));
            t.sort_unstable();
            std::array::from_fn(|i| t[i / 3][i % 3])
        };
        let mut by_tri = std::collections::HashMap::new();
        for (qi, q) in quads.quads.iter().enumerate() {
            let p = |k: usize| quads.corner(q, k).to_array();
            by_tri.insert(key(p(0), p(1), p(2)), qi as u32);
            if q.corners() == 4 {
                by_tri.insert(key(p(0), p(2), p(3)), qi as u32);
            }
        }
        self.tri_quads = scene
            .prims
            .iter()
            .filter(|p| p.visible)
            .map(|p| {
                let at = |i: u32| p.vertices[i as usize].pos;
                let tris: Vec<u32> = p
                    .indices
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .map(|t| {
                        by_tri
                            .get(&key(at(t[0]), at(t[1]), at(t[2])))
                            .copied()
                            .unwrap_or(u32::MAX)
                    })
                    .collect();
                tris.iter().any(|&q| q != u32::MAX).then(|| TriQuads {
                    indices: p.indices.clone(),
                    quads: tris,
                })
            })
            .collect();
    }

    /// How many see-through (jello) triangles the last frame drew.
    pub fn jello_triangles(&self) -> u32 {
        self.jello_cache.1.values().map(|(_, _, _, n)| n / 3).sum()
    }

    /// How many of those were doors' (a closed door's own quads).
    pub fn jello_door_triangles(&self) -> u32 {
        self.jello_cache
            .1
            .iter()
            .filter(|(&k, _)| self.prims.get(k).is_some_and(|p| p.door.is_some()))
            .map(|(_, (_, _, _, n))| n / 3)
            .sum()
    }

    /// Rebuilds the see-through split of the prims the jello touches, when
    /// it changed.
    fn jello_buffers(&mut self, gpu: &Gpu) {
        let mut want = self.scene.jello.clone();
        want.sort_unstable();
        if want == self.jello_cache.0 {
            return;
        }
        let set: std::collections::HashSet<u32> = want.iter().copied().collect();
        let mut split = std::collections::HashMap::new();
        for (k, tq) in self.tri_quads.iter().enumerate() {
            let Some(tq) = tq else { continue };
            if !tq.quads.iter().any(|q| set.contains(q)) {
                continue;
            }
            let (mut solid, mut seen) = (Vec::new(), Vec::new());
            for (t, q) in tq.indices.as_chunks::<3>().0.iter().zip(&tq.quads) {
                if set.contains(q) {
                    &mut seen
                } else {
                    &mut solid
                }
                .extend_from_slice(t);
            }
            let buf = |v: &[u32]| {
                gpu.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("jello ibuf"),
                        contents: bytemuck::cast_slice(if v.is_empty() {
                            &[0u32, 0, 0][..]
                        } else {
                            v
                        }),
                        usage: wgpu::BufferUsages::INDEX,
                    })
            };
            split.insert(
                k,
                (
                    buf(&solid),
                    solid.len() as u32,
                    buf(&seen),
                    seen.len() as u32,
                ),
            );
        }
        self.jello_cache = (want, split);
    }

    /// The lens flares when characters hide them (`flares_behind_characters`):
    /// the solid bodies' depth laid down on its own (the colour kept), then
    /// the flares added where no body is in front of them.
    fn flare_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        characters: &[CharDraw],
        sprites: &[GpuPrim],
    ) {
        let Some(depth) = self.char_depth.as_ref() else {
            return;
        };
        let flares = self
            .prims
            .iter()
            .chain(sprites)
            .filter(|p| p.bucket == Bucket::Flare);
        if flares.clone().next().is_none() {
            return;
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("flares behind characters"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_bind_group(0, &self.uniform_bg, &[]);
        let draw = |pass: &mut wgpu::RenderPass, p: &GpuPrim, model: Mat4| {
            pass.set_push_constants(
                wgpu::ShaderStages::VERTEX,
                0,
                bytemuck::cast_slice(&push_block(model, [0.0; 4])),
            );
            pass.set_bind_group(
                1,
                self.tex_bgs
                    .get(self.shown(p.texture))
                    .unwrap_or(&self.default_tex),
                &[],
            );
            pass.set_vertex_buffer(0, p.vbuf.slice(..));
            pass.set_index_buffer(p.ibuf.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..p.count, 0, 0..1);
        };
        pass.set_pipeline(&self.char_depth_pipeline);
        for d in characters
            .iter()
            .filter(|d| !d.see_through && d.alpha >= 1.0)
        {
            let Some(cm) = self.char_models.get(d.model) else {
                continue;
            };
            for p in cm.prims.iter().filter(|p| p.bucket == Bucket::Opaque) {
                let model = p
                    .bone
                    .and_then(|i| d.bones.get(i).copied())
                    .unwrap_or(Mat4::IDENTITY);
                draw(&mut pass, p, model);
            }
        }
        pass.set_pipeline(&self.flare_depth_pipeline);
        for p in flares {
            draw(&mut pass, p, Mat4::IDENTITY);
        }
    }

    /// Draws this frame's overlay over the target, in order.
    fn overlay_pass(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
    ) {
        let draws = std::mem::take(&mut self.overlay);
        if draws.is_empty() {
            return;
        }
        let [w, h] = self.overlay_size;
        let mut verts: Vec<OverlayVertex> = Vec::new();
        let mut batches: Vec<(Option<usize>, bool, std::ops::Range<u32>)> = Vec::new();
        for d in &draws {
            let tex = d.texture.as_ref().map(|p| {
                if let (true, Some((_, i, _))) = (
                    p.as_path() == std::path::Path::new(MOVIE_TEXTURE),
                    &self.movie,
                ) {
                    return *i;
                }
                if let Some(&i) = self.overlay_textures.get(p) {
                    return i;
                }
                let (tw, th, rgba) = load_png(p).unwrap_or_else(|_| (1, 1, vec![255, 0, 255, 255]));
                self.tex_bgs.push(make_texture_bg(
                    gpu,
                    &self.tex_bgl,
                    &self.nearest,
                    tw,
                    th,
                    &rgba,
                ));
                let i = self.tex_bgs.len() - 1;
                self.overlay_textures.insert(p.clone(), i);
                i
            });
            let start = verts.len() as u32;
            for v in &d.tris {
                verts.push(OverlayVertex {
                    pos: [v[0] / w * 2.0 - 1.0, 1.0 - v[1] / h * 2.0],
                    uv: [v[2], v[3]],
                    color: d.color,
                });
            }
            let end = verts.len() as u32;
            match batches.last_mut() {
                Some((t, a, r)) if *t == tex && *a == d.additive && r.end == start => r.end = end,
                _ => batches.push((tex, d.additive, start..end)),
            }
        }
        if verts.is_empty() {
            return;
        }
        let vbuf = gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("overlay vbuf"),
                contents: bytemuck::cast_slice(&verts),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("overlay"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_vertex_buffer(0, vbuf.slice(..));
        for (tex, additive, range) in batches {
            pass.set_pipeline(if additive {
                &self.overlay_additive_pipeline
            } else {
                &self.overlay_pipeline
            });
            let bg = tex
                .and_then(|t| self.tex_bgs.get(t))
                .unwrap_or(&self.overlay_white);
            pass.set_bind_group(0, bg, &[]);
            pass.draw(range, 0..1);
        }
    }

    /// Takes the animated textures (`common/texture_animations.json`) and
    /// loads the frames of those among the scene's and models' textures;
    /// models added later get theirs as they load.
    pub fn attach_texture_anims(
        &mut self,
        gpu: &Gpu,
        anims: &std::collections::HashMap<String, crate::texanim::TexAnim>,
    ) {
        self.tex_anims = anims.clone();
        let loaded: Vec<usize> = self.tex_paths.keys().copied().collect();
        for i in loaded {
            self.animate_texture(gpu, i);
        }
    }

    /// The animated textures loaded, by file stem, with the frame each
    /// shows now (sorted by name).
    pub fn animated_textures(&self) -> Vec<(String, usize)> {
        let mut out: Vec<(String, usize)> = self
            .animated
            .iter()
            .filter_map(|(i, (anim, _))| {
                let stem = self
                    .tex_paths
                    .get(i)?
                    .file_stem()?
                    .to_string_lossy()
                    .into_owned();
                Some((stem, anim.frame(self.scene.anim_time, 0)))
            })
            .collect();
        out.sort();
        out.dedup_by(|a, b| a.0 == b.0);
        out
    }

    /// Loads texture `i`'s frames if it animates.
    fn animate_texture(&mut self, gpu: &Gpu, i: usize) {
        if self.animated.contains_key(&i) {
            return;
        }
        let Some(stem) = self
            .tex_paths
            .get(&i)
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
        else {
            return;
        };
        let Some(anim) = self.tex_anims.get(&stem).cloned() else {
            return;
        };
        let mut frames = vec![i];
        for f in anim.frames.iter().skip(1) {
            frames.push(self.texture_index(gpu, f));
        }
        self.animated.insert(i, (anim, frames));
    }

    /// The texture showing for texture `i` now: an animated one's frame at
    /// the game time (`M3iResolveTexture`; the level's draws carry no random
    /// index of their own: 0).
    fn shown(&self, i: usize) -> usize {
        match self.animated.get(&i) {
            Some((anim, frames)) => {
                frames[anim.frame(self.scene.anim_time, 0).min(frames.len() - 1)]
            }
            None => i,
        }
    }

    /// An overlay texture made in memory (`name`: how draws refer to it;
    /// given again, replaced), drawn smoothed or pixel for pixel.
    pub fn overlay_texture(
        &mut self,
        gpu: &Gpu,
        name: &std::path::Path,
        w: u32,
        h: u32,
        rgba: &[u8],
        smooth: bool,
    ) {
        let sampler = if smooth { &self.sampler } else { &self.nearest };
        self.tex_bgs
            .push(make_texture_bg(gpu, &self.tex_bgl, sampler, w, h, rgba));
        let i = self.tex_bgs.len() - 1;
        self.overlay_textures.insert(name.to_path_buf(), i);
    }

    /// Whether an overlay texture `name` is there (made or loaded).
    pub fn has_overlay_texture(&self, name: &std::path::Path) -> bool {
        self.overlay_textures.contains_key(name)
    }

    /// A texture from a PNG file, loaded once.
    fn texture_index(&mut self, gpu: &Gpu, path: &std::path::Path) -> usize {
        if let Some(&i) = self.textures_by_path.get(path) {
            return i;
        }
        let (w, h, rgba) = load_png(path).unwrap_or_else(|_| (1, 1, vec![255, 255, 255, 255]));
        self.tex_bgs.push(make_texture_bg(
            gpu,
            &self.tex_bgl,
            &self.sampler,
            w,
            h,
            &rgba,
        ));
        let i = self.tex_bgs.len() - 1;
        self.textures_by_path.insert(path.to_path_buf(), i);
        i
    }

    /// This frame's particle sprites as additive textured quads, one mesh
    /// per texture.
    fn sprite_prims(&mut self, gpu: &Gpu, right: glam::Vec3, up: glam::Vec3) -> Vec<GpuPrim> {
        let sprites = std::mem::take(&mut self.scene.sprites);
        let mut by_texture: std::collections::BTreeMap<(usize, bool), (Vec<Vertex>, Vec<u32>)> =
            Default::default();
        for s in &sprites {
            let t = self.texture_index(gpu, &s.texture);
            if let SpriteMode::Quad(corners) = s.mode {
                let (verts, idx) = by_texture.entry((t, s.no_depth)).or_default();
                let base = verts.len() as u32;
                for (p, uv, color) in corners {
                    verts.push(Vertex {
                        pos: p.into(),
                        normal: [0.0, 1.0, 0.0],
                        uv,
                        color,
                    });
                }
                idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
                continue;
            }
            let (mut x, mut y) = match s.mode {
                SpriteMode::Facing => (right, up),
                // Its direction's on-screen heading becomes its X.
                SpriteMode::Rotated(d) => {
                    let (ox, oy) = (right.dot(d), up.dot(d));
                    let l = (ox * ox + oy * oy).sqrt();
                    let (ox, oy) = if l * l < 1e-8 {
                        (1.0, 0.0)
                    } else {
                        (ox / l, oy / l)
                    };
                    (ox * right + oy * up, oy * right - ox * up)
                }
                // Along its direction; across it, square to the line of
                // sight to it.
                SpriteMode::Billboard(d) => {
                    let x = d.try_normalize().unwrap_or(glam::Vec3::Y);
                    (
                        x,
                        x.cross(s.at - self.scene.eye).try_normalize().unwrap_or(up),
                    )
                }
                SpriteMode::Fixed([u, v]) => (u, v),
                SpriteMode::Quad(_) => unreachable!("drawn above"),
            };
            if s.rotation != 0.0 {
                let (sin, cos) = s.rotation.to_radians().sin_cos();
                (x, y) = (x * cos - y * sin, x * sin + y * cos);
            }
            let (x, y) = (x * s.size[0], y * s.size[1]);
            let (verts, idx) = by_texture.entry((t, s.no_depth)).or_default();
            let base = verts.len() as u32;
            // (The texture's top row — v 0 — on the sprite's −Y side, as the
            // original's sprite drawing has it.)
            for (p, uv) in [
                (s.at - x - y, [0.0, 0.0]),
                (s.at + x - y, [1.0, 0.0]),
                (s.at + x + y, [1.0, 1.0]),
                (s.at - x + y, [0.0, 1.0]),
            ] {
                let uv = if s.contrail { [uv[1], uv[0]] } else { uv };
                verts.push(Vertex {
                    pos: p.into(),
                    normal: [0.0, 1.0, 0.0],
                    uv,
                    color: s.color,
                });
            }
            idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        by_texture
            .into_iter()
            .map(|((t, flare), (vertices, indices))| {
                let prim = Prim {
                    vertices,
                    indices,
                    texture: None,
                    blend: false,
                    additive: true,
                    door: None,
                    leaf: 0,
                    visible: true,
                    collide: false,
                    env_ref: None,
                    glass: None,
                    one_sided: false,
                };
                let mut g = upload_prim(&gpu.device, &prim);
                g.texture = t;
                if flare {
                    g.bucket = Bucket::Flare;
                }
                g
            })
            .collect()
    }

    /// This frame's decals as meshes: consecutive ones sharing a texture
    /// and blend in one (so the drawing order holds).
    fn decal_batches(&mut self, gpu: &Gpu) -> Vec<(wgpu::Buffer, wgpu::Buffer, u32, usize, bool)> {
        let decals = std::mem::take(&mut self.scene.decals);
        let mut batches: Vec<(usize, bool, Vec<DecalVertex>, Vec<u32>)> = Vec::new();
        for d in &decals {
            let t = self.texture_index(gpu, &d.texture);
            if !batches
                .last()
                .is_some_and(|b| b.0 == t && b.1 == d.additive)
            {
                batches.push((t, d.additive, Vec::new(), Vec::new()));
            }
            let b = batches.last_mut().unwrap();
            let base = b.2.len() as u32;
            let m = &d.positions;
            for k in 0..m.positions.len() {
                // Lit as the level is (its baked shades, a little brighter).
                let s = m.shades[k];
                let ch = |shift: u32| (((s >> shift) & 0xFF) as f32 / 255.0).max(0.12) * 1.25;
                b.2.push(DecalVertex {
                    pos: m.positions[k].into(),
                    uv: m.uvs[k],
                    color: [ch(16), ch(8), ch(0), d.alpha],
                });
            }
            b.3.extend(m.indices.iter().map(|&i| base + u32::from(i)));
        }
        batches
            .into_iter()
            .filter(|b| !b.3.is_empty())
            .map(|(t, additive, v, i)| {
                let vbuf = gpu
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("decal vbuf"),
                        contents: bytemuck::cast_slice(&v),
                        usage: wgpu::BufferUsages::VERTEX,
                    });
                let ibuf = gpu
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("decal ibuf"),
                        contents: bytemuck::cast_slice(&i),
                        usage: wgpu::BufferUsages::INDEX,
                    });
                (vbuf, ibuf, i.len() as u32, t, additive)
            })
            .collect()
    }

    /// This frame's laser beams as one additive strip mesh.
    fn beam_prim(
        &self,
        device: &wgpu::Device,
        right: glam::Vec3,
        up: glam::Vec3,
    ) -> Option<GpuPrim> {
        if self.scene.beams.is_empty() {
            return None;
        }
        let forward = right.cross(up).normalize_or_zero();
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        for &(a, b, color) in &self.scene.beams {
            let side = (b - a).cross(forward).try_normalize().unwrap_or(up) * BEAM_WIDTH;
            let base = vertices.len() as u32;
            for p in [a - side, a + side, b + side, b - side] {
                vertices.push(Vertex {
                    pos: p.into(),
                    normal: [0.0, 1.0, 0.0],
                    uv: [0.0, 0.0],
                    color,
                });
            }
            indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        let prim = Prim {
            vertices,
            indices,
            texture: None,
            blend: false,
            additive: true,
            door: None,
            leaf: 0,
            visible: true,
            collide: false,
            env_ref: None,
            glass: None,
            one_sided: false,
        };
        Some(upload_prim(device, &prim))
    }
}

fn upload_prim(device: &wgpu::Device, p: &Prim) -> GpuPrim {
    let vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("vbuf"),
        contents: bytemuck::cast_slice(&p.vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let ibuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("ibuf"),
        contents: bytemuck::cast_slice(&p.indices),
        usage: wgpu::BufferUsages::INDEX,
    });
    let bucket = if p.additive {
        Bucket::Additive
    } else if p.blend {
        Bucket::Alpha
    } else {
        Bucket::Opaque
    };
    GpuPrim {
        vbuf,
        ibuf,
        count: p.indices.len() as u32,
        texture: p.texture.unwrap_or(usize::MAX),
        bucket,
        door: p.door.map(|d| (d, p.leaf)),
        env_ref: p.env_ref,
        glass: p.glass,
        bone: None,
        cull: p.one_sided,
    }
}

fn make_texture_bg(
    gpu: &Gpu,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    w: u32,
    h: u32,
    rgba: &[u8],
) -> wgpu::BindGroup {
    // Full mip chain (CPU box-filter) — kills distant-texture shimmer.
    let mips = generate_mips(w, h, rgba.to_vec());
    let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("tex"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: mips.len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (level, (lw, lh, data)) in mips.iter().enumerate() {
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &tex,
                mip_level: level as u32,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * lw),
                rows_per_image: Some(*lh),
            },
            wgpu::Extent3d {
                width: *lw,
                height: *lh,
                depth_or_array_layers: 1,
            },
        );
    }
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("tex"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

/// Builds a full mip chain by 2×2 box filtering, averaging RGB in **linear**
/// space (sRGB-correct) and alpha directly, down to 1×1.
fn generate_mips(w: u32, h: u32, base: Vec<u8>) -> Vec<(u32, u32, Vec<u8>)> {
    let lut: [f32; 256] = std::array::from_fn(|i| {
        let x = i as f32 / 255.0;
        if x <= 0.04045 {
            x / 12.92
        } else {
            ((x + 0.055) / 1.055).powf(2.4)
        }
    });
    let to_srgb = |x: f32| -> u8 {
        let v = if x <= 0.0031308 {
            x * 12.92
        } else {
            1.055 * x.powf(1.0 / 2.4) - 0.055
        };
        (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
    };

    let mut chain = vec![(w, h, base)];
    loop {
        let (pw, ph, prev) = {
            let last = chain.last().unwrap();
            if last.0 == 1 && last.1 == 1 {
                break;
            }
            (last.0, last.1, last.2.clone())
        };
        let (nw, nh) = ((pw / 2).max(1), (ph / 2).max(1));
        let mut out = vec![0u8; (nw * nh * 4) as usize];
        for y in 0..nh {
            for x in 0..nw {
                let mut acc = [0f32; 4];
                for dy in 0..2 {
                    for dx in 0..2 {
                        let sx = (x * 2 + dx).min(pw - 1);
                        let sy = (y * 2 + dy).min(ph - 1);
                        let o = ((sy * pw + sx) * 4) as usize;
                        acc[0] += lut[prev[o] as usize];
                        acc[1] += lut[prev[o + 1] as usize];
                        acc[2] += lut[prev[o + 2] as usize];
                        acc[3] += prev[o + 3] as f32 / 255.0;
                    }
                }
                let o = ((y * nw + x) * 4) as usize;
                out[o] = to_srgb(acc[0] / 4.0);
                out[o + 1] = to_srgb(acc[1] / 4.0);
                out[o + 2] = to_srgb(acc[2] / 4.0);
                out[o + 3] = (acc[3] / 4.0 * 255.0 + 0.5) as u8;
            }
        }
        chain.push((nw, nh, out));
    }
    chain
}

/// Loads a PNG as RGBA8.
pub(crate) fn load_png(path: &Path) -> std::io::Result<(u32, u32, Vec<u8>)> {
    let file = std::fs::File::open(path)?;
    let decoder = png::Decoder::new(file);
    let mut reader = decoder.read_info().map_err(std::io::Error::other)?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(std::io::Error::other)?;
    buf.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|c| [c[0], c[1], c[2], 255])
            .collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        _ => return Err(std::io::Error::other("unsupported PNG color type")),
    };
    Ok((info.width, info.height, rgba))
}

/// The world shaders' shared head: the uniforms and the fog (linear in
/// screen depth: −1 near, 1 far; `gl_fog_*`).
macro_rules! world_wgsl {
    () => {
        r#"
struct Uniforms { view_proj: mat4x4<f32>, fog_color: vec4<f32>, fog: vec4<f32> };
@group(0) @binding(0) var<uniform> u: Uniforms;
// The fog (linear in screen depth: −1 near, 1 far; `gl_fog_*`).
fn fogged(rgb: vec3<f32>, depth: f32) -> vec3<f32> {
    let z = depth * 2.0 - 1.0;
    let span = u.fog.y - u.fog.x;
    var f = 1.0;
    if (span > 0.0) {
        f = clamp((u.fog.y - z) / span, 0.0, 1.0);
    } else if (z >= u.fog.y) {
        f = 0.0;
    }
    return mix(u.fog_color.rgb, rgb, f);
}

"#
    };
}

const SHADER: &str = concat!(
    world_wgsl!(),
    r#"
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

// The model matrix; for the faded and magic draws, (alpha, time, kind, _).
struct Push { model: mat4x4<f32>, extra: vec4<f32> };
var<push_constant> pc: Push;

struct VSOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec3<f32>,
    @location(2) alpha: f32,
};

fn placed(pos: vec3<f32>, uv: vec2<f32>, color: vec3<f32>) -> VSOut {
    var out: VSOut;
    out.clip = u.view_proj * pc.model * vec4<f32>(pos, 1.0);
    out.uv = uv;
    out.color = color;
    out.alpha = 1.0;
    return out;
}

@vertex
fn vs_main(
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec3<f32>,
) -> VSOut {
    // (Shaded flat: the draw's colour instead of its own.)
    if (pc.extra.w > 0.5) {
        return placed(pos, uv, pc.extra.rgb);
    }
    return placed(pos, uv, color);
}

// A body fading out (invisibility): blended at the draw's alpha.
@vertex
fn vs_faded(
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec3<f32>,
) -> VSOut {
    var out = placed(pos, uv, color);
    out.alpha = pc.extra.x;
    return out;
}

@fragment
fn fs_faded(in: VSOut) -> @location(0) vec4<f32> {
    let c = lit(in);
    return vec4<f32>(fogged(c.rgb, in.clip.z), c.a * in.alpha);
}

// The magic effects' shimmer (`MSrShade_Vertices_GouraudActive`): each
// vertex's colour swings through a sine table, at its own speed (by its
// index in its part, the table's 64 speeds) and phase, at game time t.
fn vary(t: f32) -> f32 {
    let k = u32(floor(t * 256.0 + 0.5)) % 256u;
    return floor(255.0 * 0.5 * (1.0 + sin(f32(k) * 6.2831853 / 256.0)) + 0.5);
}
fn swing(most: f32, t: f32) -> f32 {
    return floor(most * vary(t) / 256.0);
}

@vertex
fn vs_magic(
    @builtin(vertex_index) i: u32,
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec3<f32>,
) -> VSOut {
    var out: VSOut;
    out.clip = u.view_proj * pc.model * vec4<f32>(pos, 1.0);
    out.uv = uv;
    out.alpha = pc.extra.x;
    let time = pc.extra.y;
    let speed = 0.01 + 0.005 * sin(f32(i % 64u));
    let phase = 0.15 * f32(i);
    var rgb: vec3<f32>;
    if (pc.extra.z > 1.5) {
        // Invisibility: red and green together, blue a little apart.
        let r = swing(24.0, 1.0 * speed * time + phase);
        var b = swing(64.0, 0.3 * speed * time + phase);
        if (b > 48.0) {
            b = r + (b - 48.0);
        } else {
            b = r;
        }
        rgb = vec3<f32>(r, r, b);
    } else if (pc.extra.z > 0.5) {
        // A super shield's, red.
        rgb = vec3<f32>(swing(255.0, 1.1 * speed * time + phase), swing(32.0, 1.0 * speed * time + phase), swing(80.0, 0.9 * speed * time + phase));
    } else {
        // A shield's, blue.
        rgb = vec3<f32>(swing(15.0, 0.9 * speed * time + phase), swing(127.0, 1.0 * speed * time + phase), swing(255.0, 1.1 * speed * time + phase));
    }
    out.color = rgb / 255.0;
    return out;
}

// The sRGB transfer, each way (the frame is sRGB; the original worked in
// display values).
fn to_display(c: vec3<f32>) -> vec3<f32> {
    return select(1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055, c * 12.92, c <= vec3<f32>(0.0031308));
}
fn to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
}

// What the original adds in display values — the texture times the
// shimmer times the alpha — brought into the frame's light (so one layer
// adds exactly what the original's did). Unfogged: an additive texture's
// fog is forced off.
@fragment
fn fs_magic(in: VSOut) -> @location(0) vec4<f32> {
    let texel = textureSample(tex, samp, in.uv);
    let added = to_display(texel.rgb) * in.color * texel.a * in.alpha;
    return vec4<f32>(to_linear(added), 1.0);
}

fn lit(in: VSOut) -> vec4<f32> {
    let texel = textureSample(tex, samp, in.uv);
    // Baked vertex lighting modulates the texture; small ambient floor.
    let shade = max(in.color, vec3<f32>(0.12));
    return vec4<f32>(texel.rgb * shade * 1.25, texel.a);
}

@fragment
fn fs_main(in: VSOut) -> @location(0) vec4<f32> {
    let c = lit(in);
    return vec4<f32>(fogged(c.rgb, in.clip.z), c.a);
}

// Unfogged (lens flares: drawn with the fog off).
@fragment
fn fs_nofog(in: VSOut) -> @location(0) vec4<f32> {
    return lit(in);
}

// A see-through wall: its opacity cut to the camera's jello amount
// (`CAgJelloAmt`, 20 of 255).
@fragment
fn fs_jello(in: VSOut) -> @location(0) vec4<f32> {
    let c = lit(in);
    return vec4<f32>(fogged(c.rgb, in.clip.z), c.a * (20.0 / 255.0));
}

// An object in the camera's way: drawn at 31 of 255 (`OBrDraw`).
@fragment
fn fs_jello_object(in: VSOut) -> @location(0) vec4<f32> {
    let c = lit(in);
    return vec4<f32>(fogged(c.rgb, in.clip.z), c.a * (31.0 / 255.0));
}
"#
);

/// The overlay: the texture times the colour.
const OVERLAY_SHADER: &str = r#"
@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;

struct VSOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_main(@location(0) pos: vec2<f32>, @location(1) uv: vec2<f32>, @location(2) color: vec4<f32>) -> VSOut {
    var out: VSOut;
    out.pos = vec4<f32>(pos, 0.0, 1.0);
    out.uv = uv;
    out.color = color;
    return out;
}

@fragment
fn fs_main(in: VSOut) -> @location(0) vec4<f32> {
    return textureSample(tex, samp, in.uv) * in.color;
}
"#;

/// Decals: the texture times the corners' light, its alpha times the
/// decal's opacity.
const DECAL_SHADER: &str = concat!(
    world_wgsl!(),
    r#"
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

struct VSOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_main(@location(0) pos: vec3<f32>, @location(1) uv: vec2<f32>, @location(2) color: vec4<f32>) -> VSOut {
    var out: VSOut;
    out.clip = u.view_proj * vec4<f32>(pos, 1.0);
    out.uv = uv;
    out.color = color;
    return out;
}

@fragment
fn fs_main(in: VSOut) -> @location(0) vec4<f32> {
    let texel = textureSample(tex, samp, in.uv);
    return vec4<f32>(fogged(texel.rgb * in.color.rgb, in.clip.z), texel.a * in.color.a);
}
"#
);

/// Camera-facing billboard shader for additive particle sprites. The quad is
/// built from each vertex's `center`/`size`/`corner` using the camera's
/// world-space right/up basis (pushed per draw).
const PARTICLE_SHADER: &str = concat!(
    world_wgsl!(),
    r#"
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

struct Push { right: vec3<f32>, up: vec3<f32> };
var<push_constant> pc: Push;

struct VSOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(
    @location(0) center: vec3<f32>,
    @location(1) size: f32,
    @location(2) corner: vec2<f32>,
    @location(3) uv: vec2<f32>,
) -> VSOut {
    let world = center + (pc.right * corner.x + pc.up * corner.y) * size;
    var out: VSOut;
    out.clip = u.view_proj * vec4<f32>(world, 1.0);
    out.uv = uv;
    return out;
}

@fragment
fn fs_main(in: VSOut) -> @location(0) vec4<f32> {
    // Additive sprite: the glow texture is added (SrcAlpha, One). The dark
    // surround of these textures contributes ~0, leaving a clean glow.
    let c = textureSample(tex, samp, in.uv);
    return vec4<f32>(fogged(c.rgb, in.clip.z), c.a);
}
"#
);
