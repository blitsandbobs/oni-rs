//! wgpu renderer: loads a [`Scene`] onto the GPU and draws it with a textured,
//! baked-lit shader. Works both windowed and headless (offscreen).

use std::path::Path;

use bytemuck::{Pod, Zeroable};
use glam::Mat4;
use wgpu::util::DeviceExt;

use crate::gltf::{Prim, Scene, Vertex};

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
            max_push_constant_size: 64, // one 4x4 model matrix
            ..wgpu::Limits::default()
        };
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("oni-view"),
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

/// Camera billboard basis, pushed per particle draw (right/up world vectors).
/// One corner of a decal: where, its texture coordinate, its colour
/// (lighting × opacity).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct DecalVertex {
    pos: [f32; 3],
    uv: [f32; 2],
    color: [f32; 4],
}

/// A decal to draw this frame (`docs/PARTICLES.md`, *Decals*): its
/// triangles over the level, lit corners (RGB, 0–1), its opacity, texture,
/// and whether the texture glows (additive).
pub struct DecalDraw {
    pub positions: std::rc::Rc<crate::decal::Mesh>,
    pub alpha: f32,
    pub texture: std::path::PathBuf,
    pub additive: bool,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct BillboardPush {
    right: [f32; 3],
    _pad0: f32,
    up: [f32; 3],
    _pad1: f32,
}

/// A particle sprite: where, its half-size (x, y), and — for a sprite
/// stretched along its flight — the direction its x runs; else it faces the
/// camera.
pub struct Sprite {
    pub at: glam::Vec3,
    pub size: [f32; 2],
    pub axis: Option<glam::Vec3>,
    /// Lying in this plane (its two axes) instead of facing the camera — a
    /// "flat" sprite, perpendicular to its direction.
    pub plane: Option<[glam::Vec3; 2]>,
    /// Its colour (tint × alpha: additive).
    pub color: [f32; 3],
    pub texture: std::path::PathBuf,
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
}

struct GpuPrim {
    vbuf: wgpu::Buffer,
    ibuf: wgpu::Buffer,
    count: u32,
    texture: usize,
    bucket: Bucket,
    /// The door (object id) whose closed leaf this is: hidden while it is open.
    door: Option<u32>,
    /// The `env_show` reference it is drawn by.
    env_ref: Option<u32>,
    glass: Option<u32>,
    /// Bone index for a character part (world matrix comes from the pose), or
    /// `None` for pre-baked world-space environment geometry (identity model).
    bone: Option<usize>,
}

/// One uploaded character model (its bone-local parts). Several world instances
/// (fighters) can draw the same model with different per-instance poses, so the
/// mesh is uploaded once here and referenced by index.
struct CharModel {
    prims: Vec<GpuPrim>,
}

/// One character instance to draw this frame: which uploaded [`CharModel`] and
/// the posed bone world-matrices for it (from the instance's own pose).
pub struct CharDraw<'a> {
    pub model: usize,
    pub bones: &'a [Mat4],
}

pub struct Renderer {
    opaque_pipeline: wgpu::RenderPipeline,
    blend_pipeline: wgpu::RenderPipeline,
    additive_pipeline: wgpu::RenderPipeline,
    particle_pipeline: wgpu::RenderPipeline,
    decal_pipeline: wgpu::RenderPipeline,
    decal_additive_pipeline: wgpu::RenderPipeline,
    /// This frame's decals, in drawing order.
    pub decals: Vec<DecalDraw>,
    uniform_buf: wgpu::Buffer,
    uniform_bg: wgpu::BindGroup,
    tex_bgl: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    tex_bgs: Vec<wgpu::BindGroup>,
    default_tex: wgpu::BindGroup,
    prims: Vec<GpuPrim>,
    /// Doors (object ids) open now: their closed leaves aren't drawn.
    pub open_doors: std::collections::HashSet<u32>,
    /// Level parts scripts have hidden (`env_show` references).
    pub hidden_refs: std::collections::HashSet<u32>,
    /// Broken glass panes (by quad) — not drawn.
    pub broken_glass: std::collections::HashSet<u32>,
    /// Laser beams to draw this frame (from, to, colour): thin glowing
    /// strips facing the camera.
    pub beams: Vec<(glam::Vec3, glam::Vec3, [f32; 3])>,
    char_models: Vec<CharModel>,
    /// Rigid models loaded by file (guns), drawn as one-bone characters.
    models_by_path: std::collections::HashMap<std::path::PathBuf, Option<usize>>,
    /// Particles to draw this frame (glowing sprites), and textures loaded
    /// for them by file.
    pub sprites: Vec<Sprite>,
    textures_by_path: std::collections::HashMap<std::path::PathBuf, usize>,
    particles: Vec<ParticleGroup>,
    depth: Option<(wgpu::TextureView, u32, u32)>,
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
                visibility: wgpu::ShaderStages::VERTEX,
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
                range: 0..64,
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

        let make_pipeline = |label: &str, blend: Option<wgpu::BlendState>, depth_write: bool| {
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
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    cull_mode: None, // doubleSided
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: depth_write,
                    depth_compare: wgpu::CompareFunction::Less,
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
        let opaque_pipeline = make_pipeline("opaque", None, true);
        let blend_pipeline = make_pipeline("alpha", Some(wgpu::BlendState::ALPHA_BLENDING), false);
        let additive_pipeline = make_pipeline("additive", Some(additive), false);

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
                    targets: &[Some(wgpu::ColorTargetState { format, blend: Some(blend), write_mask: wgpu::ColorWrites::ALL })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: None, ..Default::default() },
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

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        // Textures (missing/unreadable → magenta placeholder). Scene textures
        // keep their indices; character textures are appended and mapped.
        let mut tex_bgs = Vec::with_capacity(scene.textures.len());
        for path in &scene.textures {
            let (w, h, rgba) = load_png(path).unwrap_or_else(|_| (1, 1, vec![255, 0, 255, 255]));
            tex_bgs.push(make_texture_bg(gpu, &tex_bgl, &sampler, w, h, &rgba));
        }
        let default_tex = make_texture_bg(gpu, &tex_bgl, &sampler, 1, 1, &[200, 200, 200, 255]);

        let prims: Vec<GpuPrim> = scene.prims.iter().filter(|p| p.visible).map(|p| upload_prim(device, p)).collect();

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
            additive_pipeline,
            particle_pipeline,
            decal_pipeline,
            decal_additive_pipeline,
            decals: Vec::new(),
            uniform_buf,
            uniform_bg,
            tex_bgl,
            sampler,
            tex_bgs,
            default_tex,
            prims,
            open_doors: Default::default(),
            hidden_refs: Default::default(),
            broken_glass: Default::default(),
            beams: Vec::new(),
            char_models: Vec::new(),
            models_by_path: Default::default(),
            sprites: Vec::new(),
            textures_by_path: Default::default(),
            particles: particle_groups,
            depth: None,
        }
    }

    /// A rigid model (a gun's `.glb`) as a character model whose every part
    /// rides the first bone — loaded once per file; None if it won't load.
    /// A model with every part's texture replaced by `texture` (a console's
    /// screen in its state), loaded once per pair.
    pub fn model_textured(&mut self, gpu: &Gpu, path: &std::path::Path, texture: &std::path::Path) -> Option<usize> {
        let key = std::path::PathBuf::from(format!("{}#{}", path.display(), texture.display()));
        if let Some(m) = self.models_by_path.get(&key) {
            return *m;
        }
        let texture = if texture.is_file() { texture.to_path_buf() } else { texture.with_file_name("NONE.png") };
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
                    self.tex_bgs
                        .push(make_texture_bg(gpu, &self.tex_bgl, &self.sampler, w, h, &rgba));
                    self.tex_bgs.len() - 1
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
            });
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
        let beams = self.beam_prim(&gpu.device, cam_right, cam_up);
        let sprites = self.sprite_prims(gpu, cam_right, cam_up);
        let decals = self.decal_batches(gpu);
        gpu.queue.write_buffer(
            &self.uniform_buf,
            0,
            bytemuck::bytes_of(&Uniforms {
                view_proj: view_proj.to_cols_array_2d(),
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
            let draw_prim = |pass: &mut wgpu::RenderPass, p: &GpuPrim, model: Mat4| {
                pass.set_push_constants(
                    wgpu::ShaderStages::VERTEX,
                    0,
                    bytemuck::cast_slice(&model.to_cols_array()),
                );
                let bg = self.tex_bgs.get(p.texture).unwrap_or(&self.default_tex);
                pass.set_bind_group(1, bg, &[]);
                pass.set_vertex_buffer(0, p.vbuf.slice(..));
                pass.set_index_buffer(p.ibuf.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..p.count, 0, 0..1);
            };
            for (bucket, pipeline) in [
                (Bucket::Opaque, &self.opaque_pipeline),
                (Bucket::Alpha, &self.blend_pipeline),
                (Bucket::Additive, &self.additive_pipeline),
            ] {
                pass.set_pipeline(pipeline);
                let shown = |p: &&GpuPrim| {
                    p.door.is_none_or(|d| !self.open_doors.contains(&d)) && p.env_ref.is_none_or(|r| !self.hidden_refs.contains(&r))
                        && p.glass.is_none_or(|g| !self.broken_glass.contains(&g))
                };
                for p in self.prims.iter().filter(|p| p.bucket == bucket).filter(shown) {
                    draw_prim(&mut pass, p, Mat4::IDENTITY);
                }
                if let Some(b) = beams.as_ref().filter(|b| b.bucket == bucket) {
                    draw_prim(&mut pass, b, Mat4::IDENTITY);
                }
                for p in sprites.iter().filter(|p| p.bucket == bucket) {
                    draw_prim(&mut pass, p, Mat4::IDENTITY);
                }
                for d in characters {
                    let Some(cm) = self.char_models.get(d.model) else {
                        continue;
                    };
                    for p in cm.prims.iter().filter(|p| p.bucket == bucket) {
                        let model = match p.bone {
                            Some(i) => d.bones.get(i).copied().unwrap_or(Mat4::IDENTITY),
                            None => Mat4::IDENTITY,
                        };
                        draw_prim(&mut pass, p, model);
                    }
                }
                // The decals: after the solid scene, before what is seen
                // through or glows (in order: the level's, then the rest,
                // oldest first).
                if bucket == Bucket::Opaque {
                    for (vbuf, ibuf, count, texture, additive) in &decals {
                        pass.set_pipeline(if *additive { &self.decal_additive_pipeline } else { &self.decal_pipeline });
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
        gpu.queue.submit([encoder.finish()]);
    }
}

/// How wide a drawn laser beam is.
const BEAM_WIDTH: f32 = 0.25;

impl Renderer {
    /// A texture from a PNG file, loaded once.
    fn texture_index(&mut self, gpu: &Gpu, path: &std::path::Path) -> usize {
        if let Some(&i) = self.textures_by_path.get(path) {
            return i;
        }
        let (w, h, rgba) = load_png(path).unwrap_or_else(|_| (1, 1, vec![255, 255, 255, 255]));
        self.tex_bgs.push(make_texture_bg(gpu, &self.tex_bgl, &self.sampler, w, h, &rgba));
        let i = self.tex_bgs.len() - 1;
        self.textures_by_path.insert(path.to_path_buf(), i);
        i
    }

    /// This frame's particle sprites as additive textured quads, one mesh
    /// per texture.
    fn sprite_prims(&mut self, gpu: &Gpu, right: glam::Vec3, up: glam::Vec3) -> Vec<GpuPrim> {
        let forward = right.cross(up).normalize_or_zero();
        let sprites = std::mem::take(&mut self.sprites);
        let mut by_texture: std::collections::BTreeMap<usize, (Vec<Vertex>, Vec<u32>)> = Default::default();
        for s in &sprites {
            let t = self.texture_index(gpu, &s.texture);
            let (x, y) = match (s.plane, s.axis.and_then(|a| a.try_normalize())) {
                (Some([u, v]), _) => (u.normalize_or(right), v.normalize_or(up)),
                // Stretched along its flight, turned to face the camera.
                (None, Some(a)) => (a, a.cross(forward).try_normalize().unwrap_or(up)),
                (None, None) => (right, up),
            };
            let (x, y) = (x * s.size[0], y * s.size[1]);
            let (verts, idx) = by_texture.entry(t).or_default();
            let base = verts.len() as u32;
            // (The texture's top row — v 0 — on the sprite's −Y side, as the
            // original's sprite drawing has it.)
            for (p, uv) in [(s.at - x - y, [0.0, 0.0]), (s.at + x - y, [1.0, 0.0]), (s.at + x + y, [1.0, 1.0]), (s.at - x + y, [0.0, 1.0])] {
                verts.push(Vertex { pos: p.into(), normal: [0.0, 1.0, 0.0], uv, color: s.color });
            }
            idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        by_texture
            .into_iter()
            .map(|(t, (vertices, indices))| {
                let prim = Prim {
                    vertices,
                    indices,
                    texture: None,
                    blend: false,
                    additive: true,
                    door: None,
                    visible: true,
                    collide: false,
                    env_ref: None,
                    glass: None,
                };
                let mut g = upload_prim(&gpu.device, &prim);
                g.texture = t;
                g
            })
            .collect()
    }

    /// This frame's decals as meshes: consecutive ones sharing a texture
    /// and blend in one (so the drawing order holds).
    fn decal_batches(&mut self, gpu: &Gpu) -> Vec<(wgpu::Buffer, wgpu::Buffer, u32, usize, bool)> {
        let decals = std::mem::take(&mut self.decals);
        let mut batches: Vec<(usize, bool, Vec<DecalVertex>, Vec<u32>)> = Vec::new();
        for d in &decals {
            let t = self.texture_index(gpu, &d.texture);
            if !batches.last().is_some_and(|b| b.0 == t && b.1 == d.additive) {
                batches.push((t, d.additive, Vec::new(), Vec::new()));
            }
            let b = batches.last_mut().unwrap();
            let base = b.2.len() as u32;
            let m = &d.positions;
            for k in 0..m.positions.len() {
                // Lit as the level is (its baked shades, a little brighter).
                let s = m.shades[k];
                let ch = |shift: u32| (((s >> shift) & 0xFF) as f32 / 255.0).max(0.12) * 1.25;
                b.2.push(DecalVertex { pos: m.positions[k].into(), uv: m.uvs[k], color: [ch(16), ch(8), ch(0), d.alpha] });
            }
            b.3.extend(m.indices.iter().map(|&i| base + u32::from(i)));
        }
        batches
            .into_iter()
            .filter(|b| !b.3.is_empty())
            .map(|(t, additive, v, i)| {
                let vbuf = gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("decal vbuf"),
                    contents: bytemuck::cast_slice(&v),
                    usage: wgpu::BufferUsages::VERTEX,
                });
                let ibuf = gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("decal ibuf"),
                    contents: bytemuck::cast_slice(&i),
                    usage: wgpu::BufferUsages::INDEX,
                });
                (vbuf, ibuf, i.len() as u32, t, additive)
            })
            .collect()
    }

    /// This frame's laser beams as one additive strip mesh.
    fn beam_prim(&self, device: &wgpu::Device, right: glam::Vec3, up: glam::Vec3) -> Option<GpuPrim> {
        if self.beams.is_empty() {
            return None;
        }
        let forward = right.cross(up).normalize_or_zero();
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        for &(a, b, color) in &self.beams {
            let side = (b - a).cross(forward).try_normalize().unwrap_or(up) * BEAM_WIDTH;
            let base = vertices.len() as u32;
            for p in [a - side, a + side, b + side, b - side] {
                vertices.push(Vertex { pos: p.into(), normal: [0.0, 1.0, 0.0], uv: [0.0, 0.0], color });
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
            visible: true,
            collide: false,
            env_ref: None,
            glass: None,
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
        door: p.door,
        env_ref: p.env_ref,
        glass: p.glass,
        bone: None,
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
fn load_png(path: &Path) -> std::io::Result<(u32, u32, Vec<u8>)> {
    let file = std::fs::File::open(path)?;
    let decoder = png::Decoder::new(file);
    let mut reader = decoder.read_info().map_err(std::io::Error::other)?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(std::io::Error::other)?;
    buf.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf
            .chunks_exact(3)
            .flat_map(|c| [c[0], c[1], c[2], 255])
            .collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        _ => return Err(std::io::Error::other("unsupported PNG color type")),
    };
    Ok((info.width, info.height, rgba))
}

const SHADER: &str = r#"
struct Uniforms { view_proj: mat4x4<f32> };
@group(0) @binding(0) var<uniform> u: Uniforms;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

struct Push { model: mat4x4<f32> };
var<push_constant> pc: Push;

struct VSOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec3<f32>,
};

@vertex
fn vs_main(
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec3<f32>,
) -> VSOut {
    var out: VSOut;
    out.clip = u.view_proj * pc.model * vec4<f32>(pos, 1.0);
    out.uv = uv;
    out.color = color;
    return out;
}

@fragment
fn fs_main(in: VSOut) -> @location(0) vec4<f32> {
    let texel = textureSample(tex, samp, in.uv);
    // Baked vertex lighting modulates the texture; small ambient floor.
    let shade = max(in.color, vec3<f32>(0.12));
    return vec4<f32>(texel.rgb * shade * 1.25, texel.a);
}
"#;

/// Decals: the texture times the corners' light, its alpha times the
/// decal's opacity.
const DECAL_SHADER: &str = r#"
struct Uniforms { view_proj: mat4x4<f32> };
@group(0) @binding(0) var<uniform> u: Uniforms;
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
    return vec4<f32>(texel.rgb * in.color.rgb, texel.a * in.color.a);
}
"#;

/// Camera-facing billboard shader for additive particle sprites. The quad is
/// built from each vertex's `center`/`size`/`corner` using the camera's
/// world-space right/up basis (pushed per draw).
const PARTICLE_SHADER: &str = r#"
struct Uniforms { view_proj: mat4x4<f32> };
@group(0) @binding(0) var<uniform> u: Uniforms;
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
    return textureSample(tex, samp, in.uv);
}
"#;
