//! cosmic-text / glyphon integration for the renderer.
//!
//! The DAW UI draws in *logical points* (1 point == 1 px at 1× DPR). We keep
//! glyph coordinates in the same space here; the only place the physical scale
//! factor enters is in the viewport `Resolution` and the per-`TextArea`
//! `scale` multiplier we hand to glyphon, so that rasterized glyph quads land
//! at the correct physical pixel location.

use std::sync::Arc;

use glyphon::{
    Attrs, Buffer, Cache, Color as GColor, Family, FontSystem, Metrics, Resolution,
    Shaping, Style, SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer, Viewport,
    Weight,
};

use crate::scene::{Align, Color, Rect, TextCmd};

/// Owns all GPU+CPU resources required to rasterize and draw text.
/// One instance lives per `Renderer`.
pub struct TextSystem {
    font_system: FontSystem,
    swash_cache: SwashCache,
    atlas: TextAtlas,
    viewport: Viewport,
    renderer: TextRenderer,
    /// Per-frame scratch: keeps `Buffer`s alive while they are referenced by
    /// `TextArea`s passed to `prepare`.
    frame_buffers: Vec<Buffer>,
}

impl TextSystem {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
    ) -> Self {
        let mut font_system = FontSystem::new();
        configure_system_ui_font(&mut font_system);
        configure_system_mono_font(&mut font_system);
        let swash_cache = SwashCache::new();
        let cache = Cache::new(device);
        let viewport = Viewport::new(device, &cache);
        let mut atlas = TextAtlas::new(device, queue, &cache, format);
        let renderer = TextRenderer::new(
            &mut atlas,
            device,
            wgpu::MultisampleState::default(),
            None,
        );
        Self {
            font_system,
            swash_cache,
            atlas,
            viewport,
            renderer,
            frame_buffers: Vec::new(),
        }
    }

    /// Build glyphon `TextArea`s from the DAW's scene `TextCmd`s and hand them
    /// to the text renderer for preparation. Must be called before
    /// `render_pass(...)` on the same frame.
    ///
    /// `logical_size` is the UI coordinate space (points), `physical_size` is
    /// the actual surface resolution, and `scale_factor` is
    /// physical_size / logical_size.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        physical_size: (u32, u32),
        scale_factor: f32,
        texts: &[TextCmd],
    ) -> Result<(), glyphon::PrepareError> {
        self.viewport.update(
            queue,
            Resolution { width: physical_size.0, height: physical_size.1 },
        );

        self.frame_buffers.clear();
        self.frame_buffers.reserve(texts.len());

        // Work entirely in physical pixels so glyphon rasterizes glyphs at
        // the correct on-screen size. (cosmic-text's `Metrics::font_size` is a
        // *raster* size in pixels — not in logical units — so on HiDPI displays
        // we must multiply by the DPR or glyphs come out half-size.)
        let s = scale_factor.max(1.0);
        for cmd in texts {
            let px = cmd.size * s;
            let metrics = Metrics::new(px, px * 1.25);
            let mut buffer = Buffer::new(&mut self.font_system, metrics);
            buffer.set_size(
                &mut self.font_system,
                Some((cmd.rect.w * s).max(1.0)),
                Some((cmd.rect.h * s).max(1.0)),
            );
            let attrs = ui_text_attrs(cmd.bold, cmd.monospaced);
            buffer.set_text(
                &mut self.font_system,
                &cmd.text,
                attrs,
                Shaping::Advanced,
            );
            buffer.shape_until_scroll(&mut self.font_system, false);
            self.frame_buffers.push(buffer);
        }

        // Build TextArea list. Positions and metrics are in physical pixels,
        // so scale=1.0 — glyphon treats buffer coords and text_area coords
        // uniformly here.
        let mut areas: Vec<TextArea> = Vec::with_capacity(texts.len());
        for (cmd, buffer) in texts.iter().zip(self.frame_buffers.iter()) {
            let rect_px_x = cmd.rect.x * s;
            let rect_px_y = cmd.rect.y * s;
            let rect_px_w = cmd.rect.w * s;
            let rect_px_h = cmd.rect.h * s;

            let line_w = longest_line_w(buffer);
            let hx = match cmd.h_align {
                Align::Start => rect_px_x,
                Align::Center => rect_px_x + (rect_px_w - line_w) * 0.5,
                Align::End => rect_px_x + rect_px_w - line_w - 2.0 * s,
            };
            let line_count = buffer.layout_runs().count().max(1) as f32;
            let text_h = cmd.size * 1.25 * s * line_count;
            let vy = match cmd.v_align {
                Align::Start => rect_px_y,
                Align::Center => rect_px_y + (rect_px_h - text_h) * 0.5,
                Align::End => rect_px_y + rect_px_h - text_h - 1.0 * s,
            };

            let c = color_to_glyphon(cmd.color);
            // Visibility bounds default to the layout rect, but callers can
            // opt in to a tighter clip (e.g. to prevent a column's text from
            // spilling past a sticky scroll viewport).
            let clip = cmd.clip.unwrap_or(cmd.rect);
            let clip_left = clip.x * s;
            let clip_top = clip.y * s;
            let clip_right = (clip.x + clip.w) * s;
            let clip_bot = (clip.y + clip.h) * s;
            areas.push(TextArea {
                buffer,
                left: hx,
                top: vy,
                scale: 1.0,
                bounds: TextBounds {
                    left: clip_left.floor() as i32,
                    top: clip_top.floor() as i32,
                    right: clip_right.ceil() as i32,
                    bottom: clip_bot.ceil() as i32,
                },
                default_color: c,
                custom_glyphs: &[],
            });
        }

        self.renderer.prepare(
            device,
            queue,
            &mut self.font_system,
            &mut self.atlas,
            &self.viewport,
            areas,
            &mut self.swash_cache,
        )
    }

    /// Issue the text draw inside an existing render pass.
    pub fn render_pass<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
    ) -> Result<(), glyphon::RenderError> {
        self.renderer.render(&self.atlas, &self.viewport, pass)
    }

    /// Trim the glyph atlas LRU. Call once per frame.
    pub fn end_frame(&mut self) {
        self.atlas.trim();
    }
}

/// Workaround for private field access: glyphon keeps Arc's internally, but we
/// still want to keep type inference simple in render.rs.
pub type SharedFontSystem = Arc<FontSystem>;

fn color_to_glyphon(c: Color) -> GColor {
    let r = (c[0].clamp(0.0, 1.0) * 255.0) as u8;
    let g = (c[1].clamp(0.0, 1.0) * 255.0) as u8;
    let b = (c[2].clamp(0.0, 1.0) * 255.0) as u8;
    let a = (c[3].clamp(0.0, 1.0) * 255.0) as u8;
    GColor::rgba(r, g, b, a)
}

fn longest_line_w(buffer: &Buffer) -> f32 {
    let mut w = 0.0f32;
    for run in buffer.layout_runs() {
        w = w.max(run.line_w);
    }
    w
}

/// MixLink `.font(.system(size:weight:design:))` — SF via SansSerif, SF Mono
/// via Monospace. Semibold (not extra-bold) when `bold` is set.
fn ui_text_attrs(bold: bool, monospaced: bool) -> Attrs<'static> {
    Attrs::new()
        .family(if monospaced {
            Family::Monospace
        } else {
            Family::SansSerif
        })
        .weight(if bold {
            Weight::SEMIBOLD
        } else {
            Weight::NORMAL
        })
}

/// cosmic-text defaults sans-serif to "Fira Sans". On macOS MixLink uses the
/// system UI font (`.font(.system)` → SF Pro / System Font). SFNS.ttf is a
/// variable face registered only as Regular (400); requesting Bold/Semibold
/// then skips it (`font_weight_diff == 0` required) and falls through to
/// Menlo. Point SansSerif at that family and alias a 600-weight OS/2 copy so
/// MixLink's `.semibold` stays on SF.
fn configure_system_ui_font(font_system: &mut FontSystem) {
    #[cfg(target_os = "macos")]
    {
        const CANDIDATES: &[&str] = &[
            ".AppleSystemUIFont",
            "SF Pro Text",
            "SF Pro Display",
            "System Font",
            ".SF NS",
        ];
        let (family, regular_id) = {
            let db = font_system.db();
            let family = CANDIDATES
                .iter()
                .copied()
                .find(|name| db.faces().any(|face| face.families.iter().any(|(n, _)| n == name)))
                .map(str::to_string);
            let regular_id = family.as_deref().and_then(|fam| {
                db.faces()
                    .find(|face| {
                        face.weight == Weight::NORMAL
                            && face.style == Style::Normal
                            && face.families.iter().any(|(n, _)| n == fam)
                    })
                    .map(|face| face.id)
            });
            (family, regular_id)
        };

        if let Some(ref family) = family {
            font_system.db_mut().set_sans_serif_family(family.clone());
        } else if font_system
            .db_mut()
            .load_font_file("/System/Library/Fonts/SFNS.ttf")
            .is_ok()
        {
            font_system.db_mut().set_sans_serif_family("System Font");
        }

        let id = regular_id.or_else(|| {
            font_system.db().faces().find(|face| {
                face.weight == Weight::NORMAL
                    && face.style == Style::Normal
                    && face.families.iter().any(|(n, _)| n == "System Font" || n == ".SF NS")
            }).map(|face| face.id)
        });
        if let Some(id) = id {
            if let Some(bytes) = font_system.db().with_face_data(id, |data, _| data.to_vec()) {
                if let Some(semibold) = patch_ttf_os2_weight(&bytes, 600) {
                    font_system.db_mut().load_font_data(semibold);
                }
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = font_system;
    }
}

/// MixLink `.design(.monospaced)` / `NSFont.monospacedDigitSystemFont` — SF Mono,
/// then Menlo, then the system monospaced UI font. Do not pick a random mono.
fn configure_system_mono_font(font_system: &mut FontSystem) {
    #[cfg(target_os = "macos")]
    {
        const CANDIDATES: &[&str] = &[
            "SF Mono",
            "SFMono",
            ".AppleSystemUIFontMonospaced",
            ".SF NS Mono",
            "Menlo",
        ];
        let (family, source_id) = {
            let db = font_system.db();
            let family = CANDIDATES
                .iter()
                .copied()
                .find(|name| db.faces().any(|face| face.families.iter().any(|(n, _)| n == name)))
                .map(str::to_string);
            let source_id = family.as_deref().and_then(|fam| {
                db.faces()
                    .find(|face| {
                        face.weight == Weight::NORMAL
                            && face.style == Style::Normal
                            && face.families.iter().any(|(n, _)| n == fam)
                    })
                    .map(|face| face.id)
                    .or_else(|| {
                        db.faces()
                            .find(|face| {
                                face.style == Style::Normal
                                    && face.families.iter().any(|(n, _)| n == fam)
                            })
                            .map(|face| face.id)
                    })
            });
            (family, source_id)
        };

        if let Some(ref family) = family {
            font_system.db_mut().set_monospace_family(family.clone());
        } else if font_system
            .db_mut()
            .load_font_file("/System/Library/Fonts/SFNSMono.ttf")
            .is_ok()
        {
            font_system.db_mut().set_monospace_family(".SF NS Mono");
        }

        let id = source_id.or_else(|| {
            font_system.db().faces().find(|face| {
                face.style == Style::Normal
                    && face.families.iter().any(|(n, _)| {
                        n == ".SF NS Mono"
                            || n == ".AppleSystemUIFontMonospaced"
                            || n == "SF Mono"
                    })
            }).map(|face| face.id)
        });
        if let Some(id) = id {
            if let Some(bytes) = font_system.db().with_face_data(id, |data, _| data.to_vec()) {
                // SFNSMono.ttf is a Light (295) variable face; alias Regular + Semibold
                // so MixLink medium/regular and `.semibold` stay on SF Mono.
                for weight in [400_u16, 600] {
                    if let Some(patched) = patch_ttf_os2_weight(&bytes, weight) {
                        font_system.db_mut().load_font_data(patched);
                    }
                }
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = font_system;
    }
}

/// Rewrite `OS/2.usWeightClass` so fontdb/cosmic-text will match `weight`.
fn patch_ttf_os2_weight(font: &[u8], weight: u16) -> Option<Vec<u8>> {
    if font.len() < 12 || &font[0..4] == b"ttcf" {
        return None;
    }
    let num_tables = u16::from_be_bytes(font[4..6].try_into().ok()?);
    let mut os2_off = None;
    for i in 0..num_tables as usize {
        let rec = 12 + i * 16;
        if rec + 16 > font.len() {
            return None;
        }
        if &font[rec..rec + 4] == b"OS/2" {
            os2_off = Some(u32::from_be_bytes(font[rec + 8..rec + 12].try_into().ok()?) as usize);
            break;
        }
    }
    let off = os2_off?;
    if off + 6 > font.len() {
        return None;
    }
    let mut data = font.to_vec();
    data[off + 4..off + 6].copy_from_slice(&weight.to_be_bytes());
    Some(data)
}

/// Helper for anyone building a `TextCmd`: approximate advance width for a
/// single-line piece of text at the given font size. Uses cosmic-text's
/// shaper so it's accurate for the exact font stack glyphon loads.
pub fn measure_text(system: &mut TextSystem, text: &str, size: f32) -> f32 {
    let metrics = Metrics::new(size, size * 1.25);
    let mut buffer = Buffer::new(&mut system.font_system, metrics);
    buffer.set_size(&mut system.font_system, Some(10_000.0), Some(size * 2.0));
    buffer.set_text(
        &mut system.font_system,
        text,
        ui_text_attrs(false, false),
        Shaping::Advanced,
    );
    buffer.shape_until_scroll(&mut system.font_system, false);
    longest_line_w(&buffer)
}

impl Rect {
    /// Shrink the rect by `dx` on each horizontal edge and `dy` on each
    /// vertical edge. Used extensively by widget layout.
    pub fn inset(&self, dx: f32, dy: f32) -> Rect {
        Rect {
            x: self.x + dx,
            y: self.y + dy,
            w: (self.w - 2.0 * dx).max(0.0),
            h: (self.h - 2.0 * dy).max(0.0),
        }
    }
}
