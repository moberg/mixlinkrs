//! Textured-quad pipeline for MixLink bitmaps (FaderCap.png).

use crate::scene::{DrawCmd, Rect, TextureId};

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ImageVertex {
    pub pos: [f32; 2],
    pub uv: [f32; 2],
}

unsafe impl bytemuck::Pod for ImageVertex {}
unsafe impl bytemuck::Zeroable for ImageVertex {}

pub struct ImageAtlas {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
    pub bind_group: wgpu::BindGroup,
    pub bind_layout: wgpu::BindGroupLayout,
    pub pipeline: wgpu::RenderPipeline,
    pub vbo: wgpu::Buffer,
    pub vbo_cap: u64,
    pub src_size: (u32, u32),
}

impl ImageAtlas {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        png_bytes: &[u8],
    ) -> Self {
        let decoder = png::Decoder::new(std::io::Cursor::new(png_bytes));
        let mut reader = decoder.read_info().expect("decode FaderCap.png");
        let mut buf = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buf).expect("png frame");
        let (w, h) = (info.width, info.height);
        let rgba = match info.color_type {
            png::ColorType::Rgba => buf,
            png::ColorType::Rgb => {
                let mut out = Vec::with_capacity((w * h * 4) as usize);
                for px in buf.chunks_exact(3) {
                    out.extend_from_slice(&[px[0], px[1], px[2], 255]);
                }
                out
            }
            other => panic!("unsupported FaderCap color type {other:?}"),
        };

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("fader-cap"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &rgba,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(w * 4),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("fader-cap-sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("image-bgl"),
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
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("image-bg"),
            layout: &bind_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("image"),
            source: wgpu::ShaderSource::Wgsl(include_str!("image.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("image-pl"),
            bind_group_layouts: &[&bind_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("image-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<ImageVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let vbo_cap = 256u64 * std::mem::size_of::<ImageVertex>() as u64;
        let vbo = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("image-vbo"),
            size: vbo_cap,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            texture,
            view,
            sampler,
            bind_group,
            bind_layout,
            pipeline,
            vbo,
            vbo_cap,
            src_size: (w, h),
        }
    }

    pub fn tessellate(scene: &[DrawCmd], viewport: (f32, f32)) -> Vec<ImageVertex> {
        let (vw, vh) = viewport;
        let map = |x: f32, y: f32| -> [f32; 2] { [2.0 * x / vw - 1.0, 1.0 - 2.0 * y / vh] };
        let mut out = Vec::new();
        for cmd in scene {
            if let DrawCmd::Image { rect, uv, texture: TextureId::FaderCap } = cmd {
                push_image(&mut out, rect, uv, map);
            }
        }
        out
    }
}

fn push_image(
    out: &mut Vec<ImageVertex>,
    rect: &Rect,
    uv: &Rect,
    map: impl Fn(f32, f32) -> [f32; 2],
) {
    let p0 = map(rect.x, rect.y);
    let p1 = map(rect.x + rect.w, rect.y);
    let p2 = map(rect.x + rect.w, rect.y + rect.h);
    let p3 = map(rect.x, rect.y + rect.h);
    let u0 = [uv.x, uv.y];
    let u1 = [uv.x + uv.w, uv.y];
    let u2 = [uv.x + uv.w, uv.y + uv.h];
    let u3 = [uv.x, uv.y + uv.h];
    out.extend_from_slice(&[
        ImageVertex { pos: p0, uv: u0 },
        ImageVertex { pos: p1, uv: u1 },
        ImageVertex { pos: p2, uv: u2 },
        ImageVertex { pos: p0, uv: u0 },
        ImageVertex { pos: p2, uv: u2 },
        ImageVertex { pos: p3, uv: u3 },
    ]);
}
