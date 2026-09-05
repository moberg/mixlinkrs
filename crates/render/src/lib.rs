//! wgpu renderer + a small batched scene graph.
//!
//! Public types:
//! - `Scene`: a list of `DrawCmd`s the app builds each frame.
//! - `DrawCmd::Rect { rect, color }`: solid rectangle.
//! - `DrawCmd::Line { a, b, color }`: thin line (expanded to a triangle quad).
//! - `DrawCmd::WaveformBins`: min/max vertical bars (used by the arrangement
//!   view for audio clip rendering).
//! - `Renderer::render_scene`: uploads a vertex buffer for the scene and
//!   submits a single draw call. Batching is by pipeline, not by material.

use std::sync::Arc;

use winit::window::Window;

pub mod image;
pub mod scene;
pub mod text;
pub use scene::{
    aspect_fill_uv, srgb_to_gpu, srgb_to_linear, Align, Color, DrawCmd, Rect, TextCmd, TextureId,
    Vertex,
};
pub use text::{measure_text, TextSystem};

pub struct Renderer {
    pub window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    vbo: wgpu::Buffer,
    vbo_cap: u64,
    num_verts: u32,
    max_surface_dim: u32,
    /// User-controlled UI zoom multiplier (1.0 = 100%). Layered *on top of*
    /// the OS DPR so the whole UI scales consistently on both 1× and Retina
    /// displays.
    ui_zoom: f32,
    pub clear_color: wgpu::Color,
    pub text: TextSystem,
    images: image::ImageAtlas,
    num_image_verts: u32,
}

impl Renderer {
    pub async fn new(window: Arc<Window>) -> Self {
        let size = window.inner_size();
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::METAL | wgpu::Backends::GL | wgpu::Backends::VULKAN,
            ..Default::default()
        });
        let surface = instance
            .create_surface(window.clone())
            .expect("create surface");
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .expect("adapter");
        // Use the adapter's own limits rather than `downlevel_defaults` so we
        // don't clamp 2D textures to 2048×2048 on HiDPI (Retina) displays.
        let limits = adapter.limits();
        let max_surface_dim = limits.max_texture_dimension_2d;
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("mixlinkrs"),
                    required_features: wgpu::Features::empty(),
                    required_limits: limits,
                    memory_hints: wgpu::MemoryHints::default(),
                },
                None,
            )
            .await
            .expect("device");

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.clamp(1, max_surface_dim),
            height: size.height.clamp(1, max_surface_dim),
            present_mode: caps.present_modes[0],
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        // An initially-small vertex buffer; we grow on demand in render_scene.
        let vbo_cap = 4096u64 * std::mem::size_of::<Vertex>() as u64;
        let vbo = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("vbo"),
            size: vbo_cap,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("simple"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pipeline-layout"),
            bind_group_layouts: &[],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
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

        let text = TextSystem::new(&device, &queue, config.format);
        let images = image::ImageAtlas::new(
            &device,
            &queue,
            config.format,
            &[
                include_bytes!("../../../assets/FaderCap.png") as &[u8],
                include_bytes!("../../../assets/HardwareFaderCap.png") as &[u8],
            ],
        );

        Self {
            window,
            surface,
            device,
            queue,
            config,
            pipeline,
            vbo,
            vbo_cap,
            num_verts: 0,
            max_surface_dim,
            ui_zoom: 1.0,
            // MixLink window is sRGB (0.10, 0.10, 0.11). wgpu clear is linear.
            clear_color: wgpu::Color {
                r: scene::srgb_to_linear(0.10) as f64,
                g: scene::srgb_to_linear(0.10) as f64,
                b: scene::srgb_to_linear(0.11) as f64,
                a: 1.0,
            },
            text,
            images,
            num_image_verts: 0,
        }
    }

    /// Rebuild the vertex buffer from a scene and submit a frame.
    ///
    /// `DrawCmd::Layer` splits the scene so later geometry and text paint on
    /// top of earlier text (menus / Settings / Channels).
    pub fn render_scene(&mut self, scene: &[DrawCmd]) -> Result<(), wgpu::SurfaceError> {
        let (lw, lh) = self.logical_size();
        let layers = split_layers(scene);
        let mut all_verts = Vec::new();
        let mut all_images = Vec::new();
        let mut packed: Vec<LayerBatch> = Vec::with_capacity(layers.len());
        for layer in &layers {
            let v0 = all_verts.len() as u32;
            all_verts.extend(scene::tessellate(layer, (lw, lh)));
            let i0 = all_images.len() as u32;
            all_images.extend(self.images.tessellate(layer, (lw, lh)));
            let texts: Vec<TextCmd> = layer
                .iter()
                .filter_map(|c| match c {
                    DrawCmd::Text(t) => Some(t.clone()),
                    _ => None,
                })
                .collect();
            packed.push(LayerBatch {
                vert_start: v0,
                vert_count: all_verts.len() as u32 - v0,
                image_start: i0,
                image_count: all_images.len() as u32 - i0,
                texts,
            });
        }

        upload_vbo(
            &self.device,
            &self.queue,
            &mut self.vbo,
            &mut self.vbo_cap,
            bytemuck::cast_slice(&all_verts),
        );
        upload_vbo(
            &self.device,
            &self.queue,
            &mut self.images.vbo,
            &mut self.images.vbo_cap,
            bytemuck::cast_slice(&all_images),
        );
        self.num_verts = all_verts.len() as u32;
        self.num_image_verts = all_images.len() as u32;

        let frame = self.surface.get_current_texture()?;
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("frame"),
        });
        let sf = self.effective_scale();
        let phys = (self.config.width, self.config.height);
        // Prepare every layer before encoding any pass. Glyphon's prepare
        // uploads vertices with `queue.write_buffer`; one renderer would let
        // the overlay prepare overwrite sidebar vertices before the GPU ran
        // the base text draw. Each layer has its own renderer, and preparing
        // them all first also keeps the atlas bind group stable.
        for (i, layer) in packed.iter().enumerate() {
            if let Err(e) = self.text.prepare(&self.device, &self.queue, phys, sf, &layer.texts, i)
            {
                log::error!("text prepare failed: {e:?}");
            }
        }
        for (i, layer) in packed.iter().enumerate() {
            let load = if i == 0 {
                wgpu::LoadOp::Clear(self.clear_color)
            } else {
                wgpu::LoadOp::Load
            };
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("layer"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                if layer.vert_count > 0 {
                    pass.set_pipeline(&self.pipeline);
                    pass.set_vertex_buffer(0, self.vbo.slice(..));
                    pass.draw(layer.vert_start..layer.vert_start + layer.vert_count, 0..1);
                }
                if layer.image_count > 0 {
                    pass.set_pipeline(&self.images.pipeline);
                    pass.set_bind_group(0, &self.images.bind_group, &[]);
                    pass.set_vertex_buffer(0, self.images.vbo.slice(..));
                    pass.draw(layer.image_start..layer.image_start + layer.image_count, 0..1);
                }
                if let Err(e) = self.text.render_pass(&mut pass, i) {
                    log::error!("text render failed: {e:?}");
                }
            }
        }
        self.queue.submit(std::iter::once(encoder.finish()));
        frame.present();
        self.text.end_frame();
        Ok(())
    }

    /// Physical surface size (pixels). Used for wgpu configuration.
    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    /// Logical size (points) used to lay out UI. Divides physical size by
    /// the effective scale (OS DPR × user zoom), so increasing `ui_zoom`
    /// makes layouts fit fewer points — everything looks bigger on screen.
    pub fn logical_size(&self) -> (f32, f32) {
        let s = self.effective_scale();
        (
            (self.config.width as f32) / s,
            (self.config.height as f32) / s,
        )
    }

    /// OS DPR × user zoom. Used both to render text at the correct physical
    /// size and to convert physical cursor coords into logical points.
    pub fn effective_scale(&self) -> f32 {
        (self.window.scale_factor() as f32).max(1.0) * self.ui_zoom.max(0.1)
    }

    /// Current user zoom factor (1.0 == 100%).
    pub fn ui_zoom(&self) -> f32 {
        self.ui_zoom
    }

    /// Set the user zoom factor. Clamped to a sensible range so extreme
    /// values don't make the UI unusable.
    pub fn set_ui_zoom(&mut self, zoom: f32) {
        self.ui_zoom = zoom.clamp(0.5, 3.0);
    }

    pub fn resize(&mut self, w: u32, h: u32) {
        self.config.width = w.clamp(1, self.max_surface_dim);
        self.config.height = h.clamp(1, self.max_surface_dim);
        self.surface.configure(&self.device, &self.config);
    }

}

struct LayerBatch {
    vert_start: u32,
    vert_count: u32,
    image_start: u32,
    image_count: u32,
    texts: Vec<TextCmd>,
}

fn split_layers(scene: &[DrawCmd]) -> Vec<&[DrawCmd]> {
    let mut out = Vec::new();
    let mut start = 0usize;
    for (i, cmd) in scene.iter().enumerate() {
        if matches!(cmd, DrawCmd::Layer) {
            if i > start {
                out.push(&scene[start..i]);
            }
            start = i + 1;
        }
    }
    if start < scene.len() {
        out.push(&scene[start..]);
    }
    if out.is_empty() {
        out.push(scene);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_text(s: &str) -> DrawCmd {
        DrawCmd::Text(TextCmd {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 200.0,
                h: 18.0,
            },
            text: s.into(),
            size: 12.0,
            color: [1.0; 4],
            h_align: Align::Start,
            v_align: Align::Center,
            bold: false,
            monospaced: false,
            clip: None,
        })
    }

    #[test]
    fn overlay_layer_keeps_sidebar_text_cmds() {
        let mut scene = vec![dummy_text("Plugins"), dummy_text("SETTINGS"), DrawCmd::Layer];
        for i in 0..16 {
            scene.push(dummy_text(&format!(
                "ADAT {}/{} - Analog Heat",
                i * 2 + 1,
                i * 2 + 2
            )));
        }
        let layers = split_layers(&scene);
        assert_eq!(layers.len(), 2);
        let base: Vec<&str> = layers[0]
            .iter()
            .filter_map(|c| match c {
                DrawCmd::Text(t) => Some(t.text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(base, ["Plugins", "SETTINGS"]);
        assert_eq!(
            layers[1]
                .iter()
                .filter(|c| matches!(c, DrawCmd::Text(_)))
                .count(),
            16
        );
    }
}

fn upload_vbo(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    vbo: &mut wgpu::Buffer,
    cap: &mut u64,
    bytes: &[u8],
) {
    let needed = bytes.len() as u64;
    if needed == 0 {
        return;
    }
    if needed > *cap {
        let new_cap = needed.next_power_of_two().max((*cap).max(1) * 2);
        *vbo = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("vbo"),
            size: new_cap,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        *cap = new_cap;
    }
    queue.write_buffer(vbo, 0, bytes);
}
