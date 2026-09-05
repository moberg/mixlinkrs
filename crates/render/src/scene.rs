//! Scene graph: immediate-mode list of draw commands the app rebuilds each
//! frame. A scene is converted to vertices by `tessellate()` and uploaded to
//! the GPU.

pub type Color = [f32; 4];

/// MixLink / SwiftUI colors are display-referred sRGB. wgpu sRGB targets treat
/// vertex colors as linear and encode on present, so 0.20 would show as ~0.48.
pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

pub fn srgb_to_gpu(c: Color) -> Color {
    [srgb_to_linear(c[0]), srgb_to_linear(c[1]), srgb_to_linear(c[2]), c[3]]
}

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
    /// MixLink `.font(.system(..., design: .monospaced))`.
    pub monospaced: bool,
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
    HardwareFaderCap,
}

impl TextureId {
    pub fn atlas_index(self) -> usize {
        match self {
            Self::FaderCap => 0,
            Self::HardwareFaderCap => 1,
        }
    }
}

#[derive(Clone, Debug)]
pub enum DrawCmd {
    Rect {
        rect: Rect,
        color: Color,
    },
    RoundedRect {
        rect: Rect,
        color: Color,
        radius: f32,
    },
    VertGradient {
        rect: Rect,
        top: Color,
        bottom: Color,
    },
    HorzGradient {
        rect: Rect,
        left: Color,
        right: Color,
    },
    /// Circle with a top-leading → bottom-trailing gradient (MixLink knob wells).
    LitDisc {
        cx: f32,
        cy: f32,
        d: f32,
        top_leading: Color,
        middle: Color,
        bottom_trailing: Color,
    },
    /// Circle with an offset radial fill (MixLink knob bodies).
    RadialDisc {
        cx: f32,
        cy: f32,
        d: f32,
        /// Highlight center in 0..1 disc space (MixLink uses (0.32, 0.28)).
        center: (f32, f32),
        inner: Color,
        mid: Color,
        outer: Color,
    },
    Line {
        a: (f32, f32),
        b: (f32, f32),
        color: Color,
        thickness: f32,
    },
    /// Textured quad. `uv` is the source rect in 0..1 texture space (aspect-fill crop).
    Image {
        rect: Rect,
        uv: Rect,
        texture: TextureId,
    },
    /// Bipolar `(min, max)` in −1…1. Consecutive pairs are joined as a strip
    /// (trapezoids), so zooming in interpolates instead of repeating a column.
    /// `bins[i]` sits at `x0 + i * bar_w`. Arrangement clips pass linear
    /// min/max divided by the file peak so transients keep their true height.
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
    /// Each layer gets its own glyphon vertex buffer; do not share one
    /// `TextRenderer` across layers in a single encoder.
    Layer,
    /// Clip this layer to `rect` (logical points). First `Clip` in a layer wins.
    Clip {
        rect: Rect,
    },
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
    let px_to_ndc = |x: f32, y: f32| -> [f32; 2] { [2.0 * x / vw - 1.0, 1.0 - 2.0 * y / vh] };
    let mut out = Vec::with_capacity(scene.len() * 6);
    for cmd in scene {
        match cmd {
            DrawCmd::Text(_) | DrawCmd::Image { .. } | DrawCmd::Layer | DrawCmd::Clip { .. } => {
                /* other pipelines */
            }
            DrawCmd::Rect { rect, color } => {
                push_quad(&mut out, rect.x, rect.y, rect.w, rect.h, *color, px_to_ndc);
            }
            DrawCmd::RoundedRect { rect, color, radius } => {
                push_rounded(&mut out, rect, *color, *radius, px_to_ndc);
            }
            DrawCmd::VertGradient { rect, top, bottom } => {
                push_vert_gradient(&mut out, rect, *top, *bottom, px_to_ndc);
            }
            DrawCmd::HorzGradient { rect, left, right } => {
                push_horz_gradient(&mut out, rect, *left, *right, px_to_ndc);
            }
            DrawCmd::LitDisc { cx, cy, d, top_leading, middle, bottom_trailing } => {
                push_lit_disc(
                    &mut out,
                    *cx,
                    *cy,
                    *d,
                    *top_leading,
                    *middle,
                    *bottom_trailing,
                    px_to_ndc,
                );
            }
            DrawCmd::RadialDisc { cx, cy, d, center, inner, mid, outer } => {
                push_radial_disc(&mut out, *cx, *cy, *d, *center, *inner, *mid, *outer, px_to_ndc);
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
            DrawCmd::WaveformBins { x0, y_center, height, bar_w, color, bins } => {
                push_waveform_strip(
                    &mut out, *x0, *y_center, *height, *bar_w, *color, bins, vw, px_to_ndc,
                );
            }
        }
    }
    out
}

fn push_waveform_strip(
    out: &mut Vec<Vertex>,
    x0: f32,
    y_center: f32,
    height: f32,
    bar_w: f32,
    color: Color,
    bins: &[(f32, f32)],
    vw: f32,
    map: impl Fn(f32, f32) -> [f32; 2],
) {
    if bins.is_empty() {
        return;
    }
    let h_half = height * 0.5;
    let y_of = |v: f32| y_center - v * h_half;
    let edge = |mn: f32, mx: f32| -> (f32, f32) {
        let mut top = y_of(mx);
        let mut bot = y_of(mn);
        if bot - top < 1.0 {
            let mid = (top + bot) * 0.5;
            top = mid - 0.5;
            bot = mid + 0.5;
        }
        (top, bot)
    };
    if bins.len() == 1 {
        let x = x0;
        if x + bar_w.max(1.0) < 0.0 || x > vw {
            return;
        }
        let (top, bot) = edge(bins[0].0, bins[0].1);
        push_quad(out, x, top, bar_w.max(1.0), bot - top, color, map);
        return;
    }
    for i in 0..bins.len() - 1 {
        let xa = x0 + i as f32 * bar_w;
        let xb = x0 + (i + 1) as f32 * bar_w;
        if xb < 0.0 || xa > vw {
            continue;
        }
        let (a_top, a_bot) = edge(bins[i].0, bins[i].1);
        let (b_top, b_bot) = edge(bins[i + 1].0, bins[i + 1].1);
        let p0 = map(xa, a_top);
        let p1 = map(xb, b_top);
        let p2 = map(xb, b_bot);
        let p3 = map(xa, a_bot);
        push_tri(out, p0, p1, p2, color);
        push_tri(out, p0, p2, p3, color);
    }
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
    let color = srgb_to_gpu(color);
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
    let top = srgb_to_gpu(top);
    let bottom = srgb_to_gpu(bottom);
    out.push(Vertex { pos: p0, color: top });
    out.push(Vertex { pos: p1, color: top });
    out.push(Vertex { pos: p2, color: bottom });
    out.push(Vertex { pos: p0, color: top });
    out.push(Vertex { pos: p2, color: bottom });
    out.push(Vertex { pos: p3, color: bottom });
}

fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
        a[3] + (b[3] - a[3]) * t,
    ]
}

fn push_lit_disc(
    out: &mut Vec<Vertex>,
    cx: f32,
    cy: f32,
    d: f32,
    top_leading: Color,
    middle: Color,
    bottom_trailing: Color,
    map: impl Fn(f32, f32) -> [f32; 2],
) {
    let r = (d * 0.5).max(0.5);
    let segs = 36;
    let origin = map(cx, cy);
    for i in 0..segs {
        let a0 = std::f32::consts::TAU * (i as f32 / segs as f32);
        let a1 = std::f32::consts::TAU * ((i + 1) as f32 / segs as f32);
        let color_at = |a: f32| {
            let t = (a.cos() + a.sin() + 2.0) * 0.25;
            if t < 0.5 {
                lerp_color(top_leading, middle, t * 2.0)
            } else {
                lerp_color(middle, bottom_trailing, (t - 0.5) * 2.0)
            }
        };
        let p1 = map(cx + a0.cos() * r, cy + a0.sin() * r);
        let p2 = map(cx + a1.cos() * r, cy + a1.sin() * r);
        out.push(Vertex { pos: origin, color: srgb_to_gpu(middle) });
        out.push(Vertex { pos: p1, color: srgb_to_gpu(color_at(a0)) });
        out.push(Vertex { pos: p2, color: srgb_to_gpu(color_at(a1)) });
    }
}

fn push_radial_disc(
    out: &mut Vec<Vertex>,
    cx: f32,
    cy: f32,
    d: f32,
    center: (f32, f32),
    inner: Color,
    mid: Color,
    outer: Color,
    map: impl Fn(f32, f32) -> [f32; 2],
) {
    let r = (d * 0.5).max(0.5);
    let hx = cx + (center.0 - 0.5) * d;
    let hy = cy + (center.1 - 0.5) * d;
    let max_d = r * 1.44;
    let color_at = |x: f32, y: f32| {
        let t = ((x - hx).hypot(y - hy) / max_d).clamp(0.0, 1.0);
        if t < 0.45 {
            lerp_color(inner, mid, t / 0.45)
        } else {
            lerp_color(mid, outer, (t - 0.45) / 0.55)
        }
    };
    let rings = 6;
    let segs = 32;
    for ring in 0..rings {
        let r0 = r * (ring as f32 / rings as f32);
        let r1 = r * ((ring + 1) as f32 / rings as f32);
        for s in 0..segs {
            let a0 = std::f32::consts::TAU * (s as f32 / segs as f32);
            let a1 = std::f32::consts::TAU * ((s + 1) as f32 / segs as f32);
            let x00 = cx + a0.cos() * r0;
            let y00 = cy + a0.sin() * r0;
            let x01 = cx + a1.cos() * r0;
            let y01 = cy + a1.sin() * r0;
            let x10 = cx + a0.cos() * r1;
            let y10 = cy + a0.sin() * r1;
            let x11 = cx + a1.cos() * r1;
            let y11 = cy + a1.sin() * r1;
            let p00 = map(x00, y00);
            let p01 = map(x01, y01);
            let p10 = map(x10, y10);
            let p11 = map(x11, y11);
            let c00 = color_at(x00, y00);
            let c01 = color_at(x01, y01);
            let c10 = color_at(x10, y10);
            let c11 = color_at(x11, y11);
            out.push(Vertex { pos: p00, color: srgb_to_gpu(c00) });
            out.push(Vertex { pos: p10, color: srgb_to_gpu(c10) });
            out.push(Vertex { pos: p11, color: srgb_to_gpu(c11) });
            out.push(Vertex { pos: p00, color: srgb_to_gpu(c00) });
            out.push(Vertex { pos: p11, color: srgb_to_gpu(c11) });
            out.push(Vertex { pos: p01, color: srgb_to_gpu(c01) });
        }
    }
}

fn push_horz_gradient(
    out: &mut Vec<Vertex>,
    rect: &Rect,
    left: Color,
    right: Color,
    map: impl Fn(f32, f32) -> [f32; 2],
) {
    let p0 = map(rect.x, rect.y);
    let p1 = map(rect.x + rect.w, rect.y);
    let p2 = map(rect.x + rect.w, rect.y + rect.h);
    let p3 = map(rect.x, rect.y + rect.h);
    let left = srgb_to_gpu(left);
    let right = srgb_to_gpu(right);
    out.push(Vertex { pos: p0, color: left });
    out.push(Vertex { pos: p1, color: right });
    out.push(Vertex { pos: p2, color: right });
    out.push(Vertex { pos: p0, color: left });
    out.push(Vertex { pos: p2, color: right });
    out.push(Vertex { pos: p3, color: left });
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
        assert_eq!(v.len(), 7 * 6);
    }

    #[test]
    fn waveform_strip_slopes_between_bins() {
        let v = tessellate(
            &[DrawCmd::WaveformBins {
                x0: 0.0,
                y_center: 50.0,
                height: 40.0,
                bar_w: 20.0,
                color: [1.0; 4],
                bins: vec![(-0.2, 0.2), (-0.8, 0.8)],
            }],
            (200.0, 100.0),
        );
        assert_eq!(v.len(), 6);
        let mut ys: Vec<i32> = v.iter().map(|p| (p.pos[1] * 1000.0).round() as i32).collect();
        ys.sort_unstable();
        ys.dedup();
        assert!(ys.len() >= 4, "strip must use both bin amplitudes, got {ys:?}");
    }

    #[test]
    fn waveform_bins_skip_offscreen() {
        let v = tessellate(
            &[DrawCmd::WaveformBins {
                x0: -50.0,
                y_center: 50.0,
                height: 40.0,
                bar_w: 10.0,
                color: [1.0; 4],
                bins: vec![(-0.5, 0.5); 20],
            }],
            (100.0, 100.0),
        );
        assert!(v.len() < 20 * 6);
        assert!(!v.is_empty());
    }

    #[test]
    fn mixlink_srgb_faceplate_is_uploaded_as_linear() {
        let linear = srgb_to_linear(0.20);
        assert!((linear - 0.0331).abs() < 0.002);
        let v = tessellate(
            &[DrawCmd::Rect {
                rect: Rect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 },
                color: [0.20, 0.20, 0.20, 1.0],
            }],
            (10.0, 10.0),
        );
        assert!((v[0].color[0] - linear).abs() < 1e-6);
        assert_eq!(v[0].color[3], 1.0);
    }
}
