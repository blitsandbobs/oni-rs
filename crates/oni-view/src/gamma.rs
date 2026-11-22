//! The gamma option (`M3rSetGamma`): the original sets the
//! display's gamma ramp, each displayed value v becoming
//! `v^(0.4 + 1.2 (1 − gamma))` — at the default 0.5, unchanged. The port
//! does the same to the finished frame: drawn into a texture, then copied to
//! the screen through the ramp (on the encoded values, as a ramp is).

/// The ramp's power for a gamma setting (0–1).
pub fn power(gamma: f32) -> f32 {
    0.4 + 1.2 * (1.0 - gamma.clamp(0.0, 1.0))
}

const SHADER: &str = r#"
@group(0) @binding(0) var frame: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
struct P { power: f32, srgb: f32, _a: f32, _b: f32 };
var<push_constant> p: P;
struct VO { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs(@builtin(vertex_index) i: u32) -> VO {
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    var o: VO;
    o.pos = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    o.uv = vec2<f32>(x, y);
    return o;
}
fn enc(c: vec3<f32>) -> vec3<f32> {
    return select(1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055, c * 12.92, c <= vec3<f32>(0.0031308));
}
fn dec(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
}
@fragment fn fs(v: VO) -> @location(0) vec4<f32> {
    let c = clamp(textureSample(frame, samp, v.uv).rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    if (p.srgb > 0.5) {
        return vec4<f32>(dec(pow(enc(c), vec3<f32>(p.power))), 1.0);
    }
    return vec4<f32>(pow(c, vec3<f32>(p.power)), 1.0);
}
"#;

/// The pass: its pipeline, and the frame texture (by size).
pub struct GammaPass {
    pipeline: wgpu::RenderPipeline,
    bgl: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    format: wgpu::TextureFormat,
    frame: Option<(wgpu::TextureView, wgpu::BindGroup, u32, u32)>,
}

impl GammaPass {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> GammaPass {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gamma"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gamma"),
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
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gamma"),
            bind_group_layouts: &[&bgl],
            push_constant_ranges: &[wgpu::PushConstantRange {
                stages: wgpu::ShaderStages::FRAGMENT,
                range: 0..16,
            }],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("gamma"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("gamma"),
            ..Default::default()
        });
        GammaPass {
            pipeline,
            bgl,
            sampler,
            format,
            frame: None,
        }
    }

    /// The texture to draw a `w` × `h` frame into (made to size).
    pub fn frame(&mut self, device: &wgpu::Device, w: u32, h: u32) -> wgpu::TextureView {
        if let Some((view, _, fw, fh)) = &self.frame {
            if *fw == w && *fh == h {
                return view.clone();
            }
        }
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("gamma frame"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("gamma"),
            layout: &self.bgl,
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
        self.frame = Some((view.clone(), bg, w, h));
        view
    }

    /// The frame copied to `target` through the ramp.
    pub fn apply(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        power: f32,
    ) {
        let Some((_, bg, _, _)) = &self.frame else {
            return;
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("gamma"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, bg, &[]);
        let srgb = if self.format.is_srgb() { 1.0f32 } else { 0.0 };
        pass.set_push_constants(
            wgpu::ShaderStages::FRAGMENT,
            0,
            bytemuck::cast_slice(&[power, srgb, 0.0, 0.0]),
        );
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_default_changes_nothing() {
        assert!((super::power(0.5) - 1.0).abs() < 1e-6);
        assert!((super::power(1.0) - 0.4).abs() < 1e-6, "brightest");
        assert!((super::power(0.0) - 1.6).abs() < 1e-6, "darkest");
    }
}
