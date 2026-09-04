//! Scene graph: immediate-mode list of draw commands the app rebuilds each
//! frame. A scene is converted to vertices by `tessellate()` and uploaded to
//! the GPU.

pub type Color = [f32; 4];

#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Horizontal or vertical alignment within a text box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Start,
    Center,
    End,
}

/// A single text draw. Separated from `DrawCmd` because text rendering goes
/// through glyphon and has very different lifetime rules.
#[derive(Clone, Debug)]
pub struct TextCmd {
    pub rect: Rect,
    pub text: String,
    /// Font size in logical points.
    pub size: f32,
    pub color: Color,
    pub h_align: Align,
    pub v_align: Align,
    pub bold: bool,
    /// Optional tighter clip rectangle. When set, the text renderer uses this
    /// as glyphon's visibility bounds while still laying text out relative to
    /// `rect`. Useful when a widget emits text inside a larger rect that
    /// extends past a scroll viewport — the layout position stays stable and
    /// only glyphs falling outside `clip` get culled.
    pub clip: Option<Rect>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextureId {
    FaderCap,
}

#[derive(Clone, Debug)]
pub enum DrawCmd {
    Rect { rect: Rect, color: Color },
    RoundedRect { rect: Rect, color: Color, radius: f32 },
    VertGradient { rect: Rect, top: Color, bottom: Color },
    Line { a: (f32, f32), b: (f32, f32), color: Color, thickness: f32 },
    /// Textured quad. `uv` is the source rect in 0..1 texture space (aspect-fill crop).
    Image {
        rect: Rect,
        uv: Rect,
        texture: TextureId,
    },
    /// A sequence of `(min, max)` pairs, each drawn as a vertical bar of width
    /// `bar_w` starting at `x0 + i * bar_w`.
    ///
    /// MixLink waveforms use per-file `maxPeak` normalisation and
    /// `pow(peak / maxPeak, 0.45)` gamma. Callers should apply that before
    /// filling `bins` rather than relying on linear rustest scaling.
    WaveformBins {
        x0: f32,
        y_center: f32,
        height: f32,
        bar_w: f32,
        color: Color,
        bins: Vec<(f32, f32)>,
    },
    /// Text drawn via glyphon / cosmic-text.
    Text(TextCmd),
    /// Start a new paint layer. Geometry and text before this command are
    /// flushed so later commands (menus, Settings, Channels) draw on top.
    Layer,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Vertex {
    pub pos: [f32; 2],
    pub color: [f32; 4],
}

unsafe impl bytemuck::Pod for Vertex {}
unsafe impl bytemuck::Zeroable for Vertex {}

/// Tessellate a scene into a vertex list in NDC. `viewport_size` is in pixels.
/// Text commands are skipped (they are drawn by the glyphon path).
pub fn tessellate(scene: &[DrawCmd], viewport_size: (f32, f32)) -> Vec<Vertex> {
    let (vw, vh) = viewport_size;
    let px_to_ndc = |x: f32, y: f32| -> [f32; 2] {
        [2.0 * x / vw - 1.0, 1.0 - 2.0 * y / vh]
    };
    let mut out = Vec::with_capacity(scene.len() * 6);
    for cmd in scene {
        match cmd {
            DrawCmd::Text(_) | DrawCmd::Image { .. } | DrawCmd::Layer => { /* other pipelines */ }
            DrawCmd::Rect { rect, color } => {
                push_quad(&mut out, rect.x, rect.y, rect.w, rect.h, *color, px_to_ndc);
            }
            DrawCmd::RoundedRect { rect, color, radius } => {
                push_rounded(&mut out, rect, *color, *radius, px_to_ndc);
            }
            DrawCmd::VertGradient { rect, top, bottom } => {
                push_vert_gradient(&mut out, rect, *top, *bottom, px_to_ndc);
            }
            DrawCmd::Line { a, b, color, thickness } => {
                let (ax, ay) = *a;
                let (bx, by) = *b;
                let dx = bx - ax;
                let dy = by - ay;
                let len = (dx * dx + dy * dy).sqrt().max(1e-6);
                let nx = -dy / len * thickness * 0.5;
                let ny = dx / len * thickness * 0.5;
                let p0 = px_to_ndc(ax + nx, ay + ny);
                let p1 = px_to_ndc(bx + nx, by + ny);
                let p2 = px_to_ndc(bx - nx, by - ny);
                let p3 = px_to_ndc(ax - nx, ay - ny);
                push_tri(&mut out, p0, p1, p2, *color);
                push_tri(&mut out, p0, p2, p3, *color);
            }
            DrawCmd::WaveformBins {
                x0,
                y_center,
                height,
                bar_w,
                color,
                bins,
            } => {
                let h_half = *height * 0.5;
                for (i, (mn, mx)) in bins.iter().enumerate() {
                    let x = x0 + i as f32 * *bar_w;
                    let y_top = y_center - mx * h_half;
                    let y_bot = y_center - mn * h_half;
                    push_quad(
                        &mut out,
                        x,
                        y_top,
                        *bar_w,
                        (y_bot - y_top).max(1.0),
                        *color,
                        px_to_ndc,
                    );
                }
            }
        }
    }
    out
}

fn push_quad(
    out: &mut Vec<Vertex>,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: [f32; 4],
    map: impl Fn(f32, f32) -> [f32; 2],
) {
    let p0 = map(x, y);
    let p1 = map(x + w, y);
    let p2 = map(x + w, y + h);
    let p3 = map(x, y + h);
    push_tri(out, p0, p1, p2, color);
    push_tri(out, p0, p2, p3, color);
}

fn push_tri(out: &mut Vec<Vertex>, a: [f32; 2], b: [f32; 2], c: [f32; 2], color: [f32; 4]) {
    out.push(Vertex { pos: a, color });
    out.push(Vertex { pos: b, color });
    out.push(Vertex { pos: c, color });
}

fn push_vert_gradient(
    out: &mut Vec<Vertex>,
    rect: &Rect,
    top: Color,
    bottom: Color,
    map: impl Fn(f32, f32) -> [f32; 2],
) {
    let p0 = map(rect.x, rect.y);
    let p1 = map(rect.x + rect.w, rect.y);
    let p2 = map(rect.x + rect.w, rect.y + rect.h);
    let p3 = map(rect.x, rect.y + rect.h);
    out.push(Vertex { pos: p0, color: top });
    out.push(Vertex { pos: p1, color: top });
    out.push(Vertex { pos: p2, color: bottom });
    out.push(Vertex { pos: p0, color: top });
    out.push(Vertex { pos: p2, color: bottom });
    out.push(Vertex { pos: p3, color: bottom });
}

fn push_rounded(
    out: &mut Vec<Vertex>,
    rect: &Rect,
    color: Color,
    radius: f32,
    map: impl Fn(f32, f32) -> [f32; 2],
) {
    let r = radius.min(rect.w * 0.5).min(rect.h * 0.5).max(0.0);
    if r < 0.5 {
        push_quad(out, rect.x, rect.y, rect.w, rect.h, color, map);
        return;
    }
    // Centre + four edge quads, then fan the corners.
    push_quad(out, rect.x + r, rect.y, rect.w - 2.0 * r, rect.h, color, &map);
    push_quad(out, rect.x, rect.y + r, r, rect.h - 2.0 * r, color, &map);
    push_quad(out, rect.x + rect.w - r, rect.y + r, r, rect.h - 2.0 * r, color, &map);
    const STEPS: i32 = 6;
    let corners = [
        (rect.x + r, rect.y + r, std::f32::consts::PI, std::f32::consts::PI * 1.5),
        (rect.x + rect.w - r, rect.y + r, std::f32::consts::PI * 1.5, std::f32::consts::PI * 2.0),
        (rect.x + rect.w - r, rect.y + rect.h - r, 0.0, std::f32::consts::PI * 0.5),
        (rect.x + r, rect.y + rect.h - r, std::f32::consts::PI * 0.5, std::f32::consts::PI),
    ];
    for (cx, cy, a0, a1) in corners {
        let origin = map(cx, cy);
        for i in 0..STEPS {
            let t0 = i as f32 / STEPS as f32;
            let t1 = (i + 1) as f32 / STEPS as f32;
            let a = a0 + (a1 - a0) * t0;
            let b = a0 + (a1 - a0) * t1;
            let p1 = map(cx + a.cos() * r, cy + a.sin() * r);
            let p2 = map(cx + b.cos() * r, cy + b.sin() * r);
            push_tri(out, origin, p1, p2, color);
        }
    }
}

/// Aspect-fill UV for a dest rect of `dest_w × dest_h` over a bitmap of `src_w × src_h`.
/// MixLink's FaderCap is 30×53 drawn into 30×48 — ~5 px of height is cropped.
pub fn aspect_fill_uv(src_w: f32, src_h: f32, dest_w: f32, dest_h: f32) -> Rect {
    let src_aspect = src_w / src_h.max(1e-6);
    let dest_aspect = dest_w / dest_h.max(1e-6);
    if src_aspect > dest_aspect {
        let vis_w = dest_aspect * src_h / src_w;
        Rect { x: (1.0 - vis_w) * 0.5, y: 0.0, w: vis_w, h: 1.0 }
    } else {
        let vis_h = (src_w / dest_aspect) / src_h;
        Rect { x: 0.0, y: (1.0 - vis_h) * 0.5, w: 1.0, h: vis_h }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_produces_two_triangles() {
        let v = tessellate(
            &[DrawCmd::Rect {
                rect: Rect { x: 0.0, y: 0.0, w: 100.0, h: 50.0 },
                color: [1.0, 1.0, 1.0, 1.0],
            }],
            (200.0, 100.0),
        );
        assert_eq!(v.len(), 6);
    }

    #[test]
    fn waveform_bins_expand() {
        let v = tessellate(
            &[DrawCmd::WaveformBins {
                x0: 10.0,
                y_center: 50.0,
                height: 40.0,
                bar_w: 2.0,
                color: [1.0; 4],
                bins: vec![(-0.5, 0.5); 8],
            }],
            (200.0, 100.0),
        );
        assert_eq!(v.len(), 8 * 6);
    }
}
