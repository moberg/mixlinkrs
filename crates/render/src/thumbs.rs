//! Per-id GPU textures for plugin editor screenshots.

use std::collections::HashMap;

use crate::image::{decode_rgba_png, ImageVertex};
use crate::scene::{DrawCmd, Rect};

struct Slot {
    bind_group: wgpu::BindGroup,
    len: usize,
}

pub struct ThumbCache {
    slots: HashMap<u128, Slot>,
    vbo: wgpu::Buffer,
    vbo_cap: u64,
}

impl ThumbCache {
    pub fn new(device: &wgpu::Device) -> Self {
        let vbo_cap = 256u64 * std::mem::size_of::<ImageVertex>() as u64;
        let vbo = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("thumb-vbo"),
            size: vbo_cap,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self { slots: HashMap::new(), vbo, vbo_cap }
    }

    pub fn sync(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        pngs: &[(u128, &[u8])],
    ) {
        let keep: std::collections::HashSet<u128> = pngs.iter().map(|(id, _)| *id).collect();
        self.slots.retain(|id, _| keep.contains(id));
        for (id, png) in pngs {
            if self.slots.get(id).is_some_and(|s| s.len == png.len()) {
                continue;
            }
            let Some((w, h, rgba)) = decode_rgba_png(png) else {
                continue;
            };
            if w == 0 || h == 0 {
                continue;
            }
            let padded = pad_rows(&rgba, w, h);
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("plugin-thumb"),
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
                &padded,
                wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bpr(w)),
                    rows_per_image: Some(h),
                },
                wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            );
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("plugin-thumb-bg"),
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
            });
            self.slots.insert(*id, Slot { bind_group, len: png.len() });
        }
    }

    pub fn tessellate(&self, scene: &[DrawCmd], viewport: (f32, f32)) -> (Vec<ImageVertex>, Vec<u128>) {
        let (vw, vh) = viewport;
        let map = |x: f32, y: f32| -> [f32; 2] { [2.0 * x / vw - 1.0, 1.0 - 2.0 * y / vh] };
        let mut verts = Vec::new();
        let mut order = Vec::new();
        for cmd in scene {
            if let DrawCmd::Thumb { rect, id } = cmd {
                if !self.slots.contains_key(id) {
                    continue;
                }
                let uv = Rect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 };
                push_quad(&mut verts, rect, &uv, map);
                order.push(*id);
            }
        }
        (verts, order)
    }

    pub fn draw<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        order: &[u128],
        vert_offset: u32,
        pipeline: &'a wgpu::RenderPipeline,
    ) {
        if order.is_empty() {
            return;
        }
        pass.set_pipeline(pipeline);
        pass.set_vertex_buffer(0, self.vbo.slice(..));
        for (i, id) in order.iter().enumerate() {
            let Some(slot) = self.slots.get(id) else {
                continue;
            };
            pass.set_bind_group(0, &slot.bind_group, &[]);
            let start = vert_offset + (i * 6) as u32;
            pass.draw(start..start + 6, 0..1);
        }
    }

    pub fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, verts: &[ImageVertex]) {
        crate::upload_vbo(device, queue, &mut self.vbo, &mut self.vbo_cap, bytemuck::cast_slice(verts));
    }
}

fn padded_bpr(width: u32) -> u32 {
    let raw = width * 4;
    raw.div_ceil(256) * 256
}

fn pad_rows(rgba: &[u8], w: u32, h: u32) -> Vec<u8> {
    let src_bpr = (w * 4) as usize;
    let dst_bpr = padded_bpr(w) as usize;
    if src_bpr == dst_bpr {
        return rgba.to_vec();
    }
    let mut out = vec![0u8; dst_bpr * h as usize];
    for y in 0..h as usize {
        let s = y * src_bpr;
        let d = y * dst_bpr;
        out[d..d + src_bpr].copy_from_slice(&rgba[s..s + src_bpr]);
    }
    out
}

fn push_quad(
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
