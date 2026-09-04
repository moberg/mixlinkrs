//! Shared MixLink-look widgets. All chrome uses these so Settings/sidebar/mixer match.

use render::{aspect_fill_uv, DrawCmd, Rect, TextureId};

use crate::theme::{self, Color, Layout};

/// MixLink `FaderCapView`: stem, Gaussian-ish drop shadow, then the PNG.
pub fn fader_cap(cmds: &mut Vec<DrawCmd>, rect: Rect) {
    // Stem sits under the cap and peeks out the bottom (MixLink offset y: 5).
    let stem = Rect {
        x: rect.x + rect.w * 0.5 - 2.5,
        y: rect.y + rect.h - 2.0,
        w: 5.0,
        h: 7.0,
    };
    cmds.push(DrawCmd::RoundedRect {
        rect: stem,
        color: [0.62, 0.62, 0.60, 1.0],
        radius: 2.5,
    });

    // MixLink: `.shadow(color: .black.opacity(0.55), radius: 2, x: 1, y: 2)`
    soft_rect_shadow(cmds, rect, 0.70, 4.0, 1.5, 3.0, 3.0);

    cmds.push(DrawCmd::Image {
        rect,
        uv: aspect_fill_uv(30.0, 53.0, rect.w, rect.h),
        texture: TextureId::FaderCap,
    });
}

const FACE_TOP: Color = [0x30 as f32 / 255.0, 0x31 as f32 / 255.0, 0x33 as f32 / 255.0, 1.0];
const FACE_MID: Color = [0x24 as f32 / 255.0, 0x25 as f32 / 255.0, 0x27 as f32 / 255.0, 1.0];
const FACE_BOT: Color = [0x19 as f32 / 255.0, 0x1A as f32 / 255.0, 0x1C as f32 / 255.0, 1.0];

/// MixLink `HardwarePadStyle`: mounting well, 3-stop metal, bevel, drop shadow.
pub fn hardware_pad(cmds: &mut Vec<DrawCmd>, rect: Rect, title: &str, on: bool, accent: Color) {
    pad_face(cmds, rect, on, accent);
    let label = if on {
        [accent[0], accent[1], accent[2], 1.0]
    } else {
        [1.0, 1.0, 1.0, 0.72]
    };
    // MixLink `HardwareButton`: `.font(.system(size: 10.5, weight: .semibold))` + `uppercased()`.
    theme::text_center(cmds, rect, title.to_uppercase(), 10.5, label, true);
}

pub fn icon_pad(cmds: &mut Vec<DrawCmd>, rect: Rect, glyph: &str, enabled: bool) {
    pad_face(cmds, rect, false, theme::PRIMARY_TEXT);
    theme::text_center(
        cmds,
        rect,
        glyph,
        9.5,
        if enabled { [1.0, 1.0, 1.0, 0.78] } else { theme::SECONDARY_TEXT },
        true,
    );
}

/// MixLink `ChannelOnToggle`: hardware pad + green LED + SwiftUI shadows.
pub fn enable_toggle(cmds: &mut Vec<DrawCmd>, rect: Rect, on: bool) {
    let accent = theme::METER_GREEN;
    if on {
        // HardwarePadStyle: `.shadow(color: accent.opacity(0.55), radius: 6)`
        enable_rect_glow(cmds, rect, accent, 0.55, 6.0, Layout::BUTTON_RADIUS);
    }
    pad_face_ex(cmds, rect, on, accent, false);

    let cx = rect.x + rect.w * 0.5;
    let cy = rect.y + rect.h * 0.5;
    let led_d = 5.5;
    if on {
        // HardwarePadStyle on the label: `.shadow(color: accent.opacity(0.9), radius: 2.4)`
        enable_led_glow(cmds, cx, cy, led_d, accent, 0.9, 2.4);
        // ChannelOnToggle: `.shadow(color: meterGreen.opacity(0.7), radius: 1.6)`
        enable_led_glow(cmds, cx, cy, led_d, accent, 0.7, 1.6);
        // Lit face uses `Color.white.opacity(0.28)` over the accent — same mix on the LED core
        // so the 5.5pt disc reads as a hotter yellowish-green hotspot, not a flat chip.
        cmds.push(DrawCmd::RadialDisc {
            cx,
            cy,
            d: led_d,
            center: (0.50, 0.42),
            inner: [
                accent[0] + (1.0 - accent[0]) * 0.28,
                accent[1] + (1.0 - accent[1]) * 0.28,
                accent[2] + (1.0 - accent[2]) * 0.28,
                1.0,
            ],
            mid: accent,
            outer: accent,
        });
    } else {
        // ChannelOnToggle off: `Color(white: 0.16)`
        disc(cmds, cx, cy, led_d, [0.16, 0.16, 0.16, 1.0]);
    }
}

/// MixLink SwiftUI `.shadow(color:opacity, radius:)` with no offset — Gaussian rings.
fn enable_rect_glow(cmds: &mut Vec<DrawCmd>, rect: Rect, color: Color, opacity: f32, radius: f32, corner: f32) {
    const RINGS: i32 = 12;
    for i in (1..=RINGS).rev() {
        let t = i as f32 / RINGS as f32;
        let grow = radius * t;
        let a = opacity * 0.5 * (-0.5 * t * t).exp();
        if a < 0.008 {
            continue;
        }
        cmds.push(DrawCmd::RoundedRect {
            rect: Rect {
                x: rect.x - grow,
                y: rect.y - grow,
                w: rect.w + grow * 2.0,
                h: rect.h + grow * 2.0,
            },
            color: [color[0], color[1], color[2], a],
            radius: corner + grow,
        });
    }
}

fn enable_led_glow(cmds: &mut Vec<DrawCmd>, cx: f32, cy: f32, diameter: f32, color: Color, opacity: f32, radius: f32) {
    const RINGS: i32 = 8;
    for i in (1..=RINGS).rev() {
        let t = i as f32 / RINGS as f32;
        let grow = radius * t;
        let a = opacity * 0.5 * (-0.5 * t * t).exp();
        if a < 0.008 {
            continue;
        }
        disc(cmds, cx, cy, diameter + grow * 2.0, [color[0], color[1], color[2], a]);
    }
}

/// MixLink `HardwareModuleModifier`: recessed card with inset bevel.
pub fn hardware_module(cmds: &mut Vec<DrawCmd>, rect: Rect) {
    theme::hardware_surface(cmds, rect, theme::SurfaceStyle::Recessed);
    cmds.push(DrawCmd::RoundedRect {
        rect: Rect { x: rect.x, y: rect.y, w: rect.w, h: rect.h },
        color: [0.0, 0.0, 0.0, 0.0],
        radius: 3.0,
    });
    theme::fill(cmds, Rect { x: rect.x, y: rect.y, w: rect.w, h: 1.0 }, [0.0, 0.0, 0.0, 0.55]);
    theme::fill(cmds, Rect { x: rect.x, y: rect.y + rect.h - 1.0, w: rect.w, h: 1.0 }, [1.0, 1.0, 1.0, 0.045]);
    theme::fill(cmds, Rect { x: rect.x, y: rect.y, w: 1.0, h: rect.h }, [0.0, 0.0, 0.0, 0.45]);
    theme::fill(cmds, Rect { x: rect.x + rect.w - 1.0, y: rect.y, w: 1.0, h: rect.h }, [1.0, 1.0, 1.0, 0.04]);
    cmds.push(DrawCmd::RoundedRect {
        rect: Rect { x: rect.x + 1.0, y: rect.y + 1.0, w: rect.w - 2.0, h: rect.h - 2.0 },
        color: [1.0, 1.0, 1.0, 0.025],
        radius: 2.5,
    });
}

fn pad_face(cmds: &mut Vec<DrawCmd>, rect: Rect, lit: bool, accent: Color) {
    pad_face_ex(cmds, rect, lit, accent, true);
}

fn pad_face_ex(cmds: &mut Vec<DrawCmd>, rect: Rect, lit: bool, accent: Color, generic_glow: bool) {
    let r = Layout::BUTTON_RADIUS;
    let sunken = lit;
    let well = Rect {
        x: rect.x - 1.35,
        y: rect.y - 0.75,
        w: rect.w + 2.7,
        h: rect.h + 2.7,
    };
    // MixLink `HardwarePadStyle`:
    // `.shadow(color: .black.opacity(sunken ? 0.28 : 0.62), radius: sunken ? 0.4 : 1.1, x: sunken ? 0 : 0.8, y: sunken ? 0.3 : 1.6)`
    // SwiftUI applies this to the composed pad (face + mounting well).
    soft_rect_shadow(
        cmds,
        well,
        if sunken { 0.28 } else { 0.62 },
        if sunken { 0.4 } else { 1.1 },
        if sunken { 0.0 } else { 0.8 },
        if sunken { 0.3 } else { 1.6 },
        r + 0.6,
    );
    if lit && generic_glow {
        cmds.push(DrawCmd::RoundedRect {
            rect: Rect { x: rect.x - 3.0, y: rect.y - 2.0, w: rect.w + 6.0, h: rect.h + 6.0 },
            color: [accent[0], accent[1], accent[2], 0.22],
            radius: r + 3.0,
        });
    }
    cmds.push(DrawCmd::RoundedRect {
        rect: well,
        color: [0.0, 0.0, 0.0, 0.78],
        radius: r + 0.6,
    });
    theme::fill(
        cmds,
        Rect { x: well.x + 1.0, y: well.y + well.h - 0.6, w: well.w - 2.0, h: 0.6 },
        [1.0, 1.0, 1.0, 0.06],
    );

    let face = if sunken {
        Rect { x: rect.x, y: rect.y + 1.0, w: rect.w, h: rect.h }
    } else {
        rect
    };
    let mid_h = face.h * 0.45;
    cmds.push(DrawCmd::VertGradient {
        rect: Rect { x: face.x, y: face.y, w: face.w, h: mid_h },
        top: if sunken { FACE_MID } else { FACE_TOP },
        bottom: FACE_MID,
    });
    cmds.push(DrawCmd::VertGradient {
        rect: Rect { x: face.x, y: face.y + mid_h, w: face.w, h: face.h - mid_h },
        top: FACE_MID,
        bottom: if sunken { [0.075, 0.075, 0.075, 1.0] } else { FACE_BOT },
    });
    if lit {
        cmds.push(DrawCmd::RoundedRect {
            rect: face,
            color: [accent[0], accent[1], accent[2], 0.46],
            radius: r,
        });
        cmds.push(DrawCmd::VertGradient {
            rect: face,
            top: [1.0, 1.0, 1.0, 0.28],
            bottom: [accent[0], accent[1], accent[2], 0.0],
        });
    }
    theme::fill(
        cmds,
        Rect { x: face.x, y: face.y, w: face.w, h: 1.0 },
        if lit { [1.0, 1.0, 1.0, 0.55] } else { [1.0, 1.0, 1.0, if sunken { 0.08 } else { 0.18 }] },
    );
    theme::fill(
        cmds,
        Rect { x: face.x, y: face.y + face.h - 1.0, w: face.w, h: 1.0 },
        if lit {
            [accent[0], accent[1], accent[2], 0.35]
        } else {
            [0.0, 0.0, 0.0, if sunken { 0.38 } else { 0.58 }]
        },
    );
    theme::fill(cmds, Rect { x: face.x, y: face.y, w: 1.0, h: face.h }, [1.0, 1.0, 1.0, if lit { 0.22 } else { 0.08 }]);
    theme::fill(cmds, Rect { x: face.x + face.w - 1.0, y: face.y, w: 1.0, h: face.h }, [0.0, 0.0, 0.0, 0.40]);
}

/// MixLink `KnobStyle`: send has a colored 300° ring; pan is a silver-rimmed pot.
#[derive(Clone, Copy)]
pub enum KnobKind {
    Send(Color),
    Pan,
}

/// MixLink `KnobView`. Send knobs show a dB label; pan knobs do not.
pub fn knob(cmds: &mut Vec<DrawCmd>, x: f32, y: f32, d: f32, value: f32, kind: KnobKind, label: Option<&str>) {
    let cx = x + d * 0.5;
    let cy = y + d * 0.5;
    let value = value.clamp(0.0, 1.0);
    match kind {
        KnobKind::Send(ring) => paint_send_knob(cmds, cx, cy, d, value, ring),
        KnobKind::Pan => paint_pan_knob(cmds, cx, cy, d, value),
    }
    if let Some(label) = label {
        theme::text_center_mono(
            cmds,
            Rect { x, y: y + d + 2.0, w: d, h: 12.0 },
            label,
            8.5,
            theme::SECONDARY_TEXT,
            false,
        );
    }
}

/// MixLink `sendKnob`: lit well, 300° ring, radial body, short pointer.
fn paint_send_knob(cmds: &mut Vec<DrawCmd>, cx: f32, cy: f32, d: f32, value: f32, ring: Color) {
    let well_d = d + 4.0;
    hardware_elevation(cmds, cx, cy, well_d);
    cmds.push(DrawCmd::LitDisc {
        cx,
        cy,
        d: well_d,
        top_leading: gray(0.29, 1.0),
        middle: gray(0.105, 1.0),
        bottom_trailing: gray(0.04, 1.0),
    });
    rim_stroke(cmds, cx, cy, well_d * 0.5, 0.7, [1.0, 1.0, 1.0, 0.16], [0.0, 0.0, 0.0, 0.55]);

    let track_r = (d - 1.0) * 0.5;
    stroke_arc(cmds, cx, cy, track_r, 120.0, 300.0, [0.0, 0.0, 0.0, 0.85], 3.2);
    let glow = [ring[0], ring[1], ring[2], 0.28];
    stroke_arc(cmds, cx, cy, track_r, 120.0, 300.0, glow, 3.8);
    stroke_arc(cmds, cx, cy, track_r, 120.0, 300.0, ring, 2.0);

    let body_d = (d - 6.0).max(8.0);
    disc(cmds, cx, cy, body_d + 1.0, [0.0, 0.0, 0.0, 0.70]);
    cmds.push(DrawCmd::RadialDisc {
        cx,
        cy,
        d: body_d,
        center: (0.32, 0.28),
        inner: gray(0.21, 1.0),
        mid: gray(0.075, 1.0),
        outer: gray(0.018, 1.0),
    });
    pointer(cmds, cx, cy, value, 13.0, body_d * 0.18, 2.0, theme::POINTER);
}

/// MixLink `PanKnobFace` plus the same `hardwareElevation` as send knobs.
fn paint_pan_knob(cmds: &mut Vec<DrawCmd>, cx: f32, cy: f32, d: f32, value: f32) {
    hardware_elevation(cmds, cx, cy, d + 4.0);
    disc(cmds, cx, cy, d, [0.0, 0.0, 0.0, 0.90]);

    let face_d = (d - 4.0).max(8.0);
    cmds.push(DrawCmd::LitDisc {
        cx,
        cy,
        d: face_d + 2.2,
        top_leading: gray(0.92, 1.0),
        middle: gray(0.72, 1.0),
        bottom_trailing: gray(0.48, 1.0),
    });
    cmds.push(DrawCmd::LitDisc {
        cx,
        cy,
        d: (face_d - 2.2).max(6.0),
        top_leading: gray(0.16, 1.0),
        middle: gray(0.11, 1.0),
        bottom_trailing: gray(0.075, 1.0),
    });
    rim_stroke(
        cmds,
        cx,
        cy,
        (d - 8.0).max(4.0) * 0.5,
        0.7,
        [0.0, 0.0, 0.0, 0.55],
        [0.0, 0.0, 0.0, 0.55],
    );
    pointer(cmds, cx, cy, value, 17.0, 8.0, 1.8, [1.0, 1.0, 1.0, 0.95]);
}

fn disc(cmds: &mut Vec<DrawCmd>, cx: f32, cy: f32, d: f32, color: Color) {
    let d = d.max(1.0);
    cmds.push(DrawCmd::RoundedRect {
        rect: Rect { x: cx - d * 0.5, y: cy - d * 0.5, w: d, h: d },
        color,
        radius: d * 0.5,
    });
}

fn gray(v: f32, a: f32) -> Color {
    [v, v, v, a]
}

/// MixLink `hardwareElevation`: SwiftUI-like blurred drop, not a hard disc.
fn hardware_elevation(cmds: &mut Vec<DrawCmd>, cx: f32, cy: f32, diameter: f32) {
    let drop = diameter * 0.5;
    soft_disc_shadow(cmds, cx, cy, diameter, 0.32, drop * 0.42, drop * 0.14, drop * 0.58);
    soft_disc_shadow(cmds, cx, cy, diameter, 0.75, 2.5, 1.0, 2.0);
}

fn soft_disc_shadow(
    cmds: &mut Vec<DrawCmd>,
    cx: f32,
    cy: f32,
    diameter: f32,
    opacity: f32,
    blur: f32,
    ox: f32,
    oy: f32,
) {
    let rings = 10;
    for i in (1..=rings).rev() {
        let t = i as f32 / rings as f32;
        let d = diameter + blur * 2.0 * t;
        let a = opacity * (-3.2 * t * t).exp() * 0.16;
        disc(cmds, cx + ox, cy + oy, d, [0.0, 0.0, 0.0, a]);
    }
}

/// MixLink SwiftUI `.shadow(color:opacity, radius, x, y)`.
///
/// `blur` is the SwiftUI radius (Gaussian σ). Rings use a signed-distance
/// falloff so the umbra's hard edge sits under the control and the visible
/// drop is a wide, smooth penumbra (~3σ) instead of a hard silhouette.
fn soft_rect_shadow(
    cmds: &mut Vec<DrawCmd>,
    rect: Rect,
    opacity: f32,
    blur: f32,
    ox: f32,
    oy: f32,
    corner: f32,
) {
    let sigma = blur.max(0.05);
    let inner = -2.0 * sigma;
    let outer = 3.0 * sigma;
    const RINGS: i32 = 20;
    for i in (0..=RINGS).rev() {
        let t = i as f32 / RINGS as f32;
        let d = inner + (outer - inner) * t;
        let a = opacity * 0.5 * (1.0 - (d / (sigma * std::f32::consts::SQRT_2)).tanh());
        if a < 0.003 {
            continue;
        }
        let grow = d;
        let w = (rect.w + grow * 2.0).max(1.0);
        let h = (rect.h + grow * 2.0).max(1.0);
        cmds.push(DrawCmd::RoundedRect {
            rect: Rect {
                x: rect.x + ox + (rect.w - w) * 0.5,
                y: rect.y + oy + (rect.h - h) * 0.5,
                w,
                h,
            },
            color: [0.0, 0.0, 0.0, a],
            radius: (corner + grow.max(0.0)).max(0.5),
        });
    }
}

fn pointer(cmds: &mut Vec<DrawCmd>, cx: f32, cy: f32, value: f32, length: f32, offset: f32, width: f32, color: Color) {
    // MixLink: capsule pointing up, then `value * 270 - 135`. Screen θ (0 = east, y-down).
    let theta = (value * 270.0 - 225.0).to_radians();
    let (ct, st) = (theta.cos(), theta.sin());
    let inner = (offset - length * 0.5).max(0.5);
    let outer = offset + length * 0.5;
    cmds.push(DrawCmd::Line {
        a: (cx + ct * inner, cy + st * inner),
        b: (cx + ct * outer, cy + st * outer),
        color,
        thickness: width,
    });
}

fn stroke_arc(
    cmds: &mut Vec<DrawCmd>,
    cx: f32,
    cy: f32,
    r: f32,
    start_deg: f32,
    sweep_deg: f32,
    color: Color,
    thickness: f32,
) {
    let start = start_deg.to_radians();
    let sweep = sweep_deg.to_radians();
    let steps = ((r.abs() * 1.6).ceil() as i32).clamp(24, 72);
    for i in 0..steps {
        let a0 = start + sweep * (i as f32 / steps as f32);
        let a1 = start + sweep * ((i + 1) as f32 / steps as f32);
        cmds.push(DrawCmd::Line {
            a: (cx + a0.cos() * r, cy + a0.sin() * r),
            b: (cx + a1.cos() * r, cy + a1.sin() * r),
            color,
            thickness,
        });
    }
}

fn rim_stroke(cmds: &mut Vec<DrawCmd>, cx: f32, cy: f32, r: f32, thickness: f32, hi: Color, lo: Color) {
    let steps = ((r.abs() * 2.0).ceil() as i32).clamp(32, 72);
    let tau = std::f32::consts::TAU;
    for i in 0..steps {
        let a0 = tau * (i as f32 / steps as f32);
        let a1 = tau * ((i + 1) as f32 / steps as f32);
        let mid = (a0 + a1) * 0.5;
        let t = ((mid.cos() + mid.sin()) * 0.5 + 0.5).clamp(0.0, 1.0);
        let color = if t < 0.5 {
            let u = 1.0 - t * 2.0;
            [hi[0], hi[1], hi[2], hi[3] * u]
        } else {
            let u = (t - 0.5) * 2.0;
            [lo[0], lo[1], lo[2], lo[3] * u]
        };
        if color[3] < 0.01 {
            continue;
        }
        cmds.push(DrawCmd::Line {
            a: (cx + a0.cos() * r, cy + a0.sin() * r),
            b: (cx + a1.cos() * r, cy + a1.sin() * r),
            color,
            thickness,
        });
    }
}

pub fn checkbox(cmds: &mut Vec<DrawCmd>, x: f32, y: f32, on: bool, label: &str) {
    hardware_pad(cmds, Rect { x, y, w: 14.0, h: 14.0 }, if on { "✓" } else { "" }, on, theme::METER_RED);
    theme::text(cmds, Rect { x: x + 18.0, y: y - 1.0, w: 110.0, h: 16.0 }, label, 9.0, theme::TEXT_DIM, false);
}

pub fn menu_label(cmds: &mut Vec<DrawCmd>, rect: Rect, text: &str) {
    theme::fill(cmds, rect, [0.0, 0.0, 0.0, 0.22]);
    theme::fill(cmds, Rect { x: rect.x, y: rect.y, w: rect.w, h: 1.0 }, [0.0, 0.0, 0.0, 0.45]);
    theme::fill(cmds, Rect { x: rect.x, y: rect.y + rect.h - 1.0, w: rect.w, h: 1.0 }, [1.0, 1.0, 1.0, 0.04]);
    theme::text(cmds, Rect { x: rect.x, y: rect.y, w: 10.0, h: rect.h }, "◆", 7.0, theme::SECONDARY_TEXT, false);
    theme::text(
        cmds,
        Rect { x: rect.x + 12.0, y: rect.y, w: rect.w - 14.0, h: rect.h },
        text,
        11.5,
        theme::PRIMARY_TEXT,
        false,
    );
}

/// MixLink `StripNameLabel` — recessed selector cell under the fader track.
pub struct StripNameStyle {
    pub diamond: bool,
    pub dim: bool,
    pub selected: bool,
}

pub fn strip_name_label(cmds: &mut Vec<DrawCmd>, rect: Rect, text: &str, style: StripNameStyle) {
    if rect.w <= 0.0 || rect.h <= 0.0 {
        return;
    }
    let fill = if style.selected {
        [0.165, 0.168, 0.165, 1.0]
    } else {
        [0.070, 0.072, 0.070, 1.0]
    };
    theme::fill(cmds, rect, fill);
    name_rail_grain(cmds, rect);
    cmds.push(DrawCmd::VertGradient {
        rect: Rect { x: rect.x, y: rect.y, w: rect.w, h: 7.0 },
        top: [0.0, 0.0, 0.0, 0.42],
        bottom: [0.0, 0.0, 0.0, 0.0],
    });
    cmds.push(DrawCmd::VertGradient {
        rect: Rect { x: rect.x, y: rect.y + rect.h - 6.0, w: rect.w, h: 6.0 },
        top: [0.0, 0.0, 0.0, 0.0],
        bottom: [1.0, 1.0, 1.0, 0.045],
    });
    theme::fill(cmds, Rect { x: rect.x, y: rect.y, w: rect.w, h: 1.0 }, [0.0, 0.0, 0.0, 0.55]);
    theme::fill(
        cmds,
        Rect { x: rect.x + 6.0, y: rect.y + rect.h - 1.0, w: (rect.w - 12.0).max(0.0), h: 1.0 },
        theme::SEAM_DARK,
    );

    let label = if style.dim {
        [theme::SECONDARY_TEXT[0], theme::SECONDARY_TEXT[1], theme::SECONDARY_TEXT[2], 0.42]
    } else {
        theme::PRIMARY_TEXT
    };
    let gem = if style.dim {
        [theme::SECONDARY_TEXT[0], theme::SECONDARY_TEXT[1], theme::SECONDARY_TEXT[2], 0.32]
    } else {
        theme::SECONDARY_TEXT
    };
    let font = 11.5;
    let text_w = (text.chars().count() as f32 * font * 0.56).clamp(8.0, (rect.w - 18.0).max(8.0));
    let dia = 6.5;
    let gap = 4.0;
    let cluster = if style.diamond { dia + gap + text_w } else { text_w };
    let x0 = rect.x + (rect.w - cluster) * 0.5;
    if style.diamond {
        hollow_diamond(cmds, x0 + dia * 0.5, rect.y + rect.h * 0.5, dia, gem);
        theme::text(
            cmds,
            Rect { x: x0 + dia + gap, y: rect.y, w: text_w, h: rect.h },
            text,
            font,
            label,
            false,
        );
    } else {
        theme::text_center(cmds, rect, text, font, label, false);
    }
}

fn name_rail_grain(cmds: &mut Vec<DrawCmd>, rect: Rect) {
    let mut local = 0.5f32;
    while local < rect.h {
        let n = (local as f64 * 12.9898).sin() * 43758.5453;
        let f = (n - n.floor()) as f32;
        let use_light = f > 0.5;
        let opacity = if use_light { 0.010 + f * 0.012 } else { 0.016 + f * 0.018 };
        cmds.push(DrawCmd::Line {
            a: (rect.x, rect.y + local),
            b: (rect.x + rect.w, rect.y + local),
            color: if use_light { [1.0, 1.0, 1.0, opacity] } else { [0.0, 0.0, 0.0, opacity] },
            thickness: 0.5,
        });
        local += 1.15;
    }
}

fn hollow_diamond(cmds: &mut Vec<DrawCmd>, cx: f32, cy: f32, size: f32, color: Color) {
    let h = size * 0.52;
    let w = size * 0.42;
    let n = (cx, cy - h);
    let e = (cx + w, cy);
    let s = (cx, cy + h);
    let west = (cx - w, cy);
    for (a, b) in [(n, e), (e, s), (s, west), (west, n)] {
        cmds.push(DrawCmd::Line { a, b, color, thickness: 1.0 });
    }
}

pub fn text_field(cmds: &mut Vec<DrawCmd>, rect: Rect, text: &str, focused: bool, caret: bool) {
    text_field_styled(cmds, rect, text, focused, caret, 12.0, false, false);
}

/// MixLink tempo value: 13 semibold mono (`SessionHeaderView`).
pub fn text_field_tempo(cmds: &mut Vec<DrawCmd>, rect: Rect, text: &str, focused: bool, caret: bool) {
    text_field_styled(cmds, rect, text, focused, caret, 13.0, true, true);
}

fn text_field_styled(
    cmds: &mut Vec<DrawCmd>,
    rect: Rect,
    text: &str,
    focused: bool,
    caret: bool,
    size: f32,
    bold: bool,
    monospaced: bool,
) {
    theme::hardware_surface(cmds, rect, theme::SurfaceStyle::Recessed);
    theme::fill(cmds, Rect { x: rect.x, y: rect.y, w: rect.w, h: 1.0 }, [0.0, 0.0, 0.0, 0.55]);
    theme::fill(cmds, Rect { x: rect.x, y: rect.y + rect.h - 1.0, w: rect.w, h: 1.0 }, [1.0, 1.0, 1.0, 0.05]);
    cmds.push(DrawCmd::RoundedRect {
        rect,
        color: if focused { [1.0, 1.0, 1.0, 0.08] } else { [0.0, 0.0, 0.0, 0.0] },
        radius: 3.0,
    });
    let shown = if caret && focused {
        format!("{text}|")
    } else {
        text.to_string()
    };
    let box_rect = Rect { x: rect.x + 6.0, y: rect.y, w: rect.w - 12.0, h: rect.h };
    if monospaced {
        theme::text_mono(cmds, box_rect, shown, size, theme::TEXT, bold);
    } else {
        theme::text(cmds, box_rect, shown, size, theme::TEXT, bold);
    }
}

#[derive(Clone, Debug)]
pub struct MenuItem {
    pub id: String,
    pub label: String,
    pub checked: bool,
    pub section: Option<String>,
}

/// MixLink `StripSourceMenu` / `ReturnEffectMenu` is SwiftUI `Menu` +
/// `.menuStyle(.borderlessButton)` — native AppKit dark NSMenu. wgpu recreates that chrome.
///
/// Sampled from MixLink screenshots + AppKit dark-menu fallback (no vibrancy):
/// fill `#383838`, 9pt corners, 1px white@0.16 rim, shadow black@0.45 blur 20 y+8,
/// items 13pt, section 11pt `#888888`, row 26pt, check column 22pt.
const MENU_RADIUS: f32 = 9.0;
const MENU_PAD_Y: f32 = 6.0;
const MENU_ITEM_H: f32 = 26.0;
const MENU_SECTION_H: f32 = 22.0;
const MENU_CHECK_W: f32 = 22.0;
const MENU_LABEL_X: f32 = 28.0;
const MENU_HIGHLIGHT_INSET: f32 = 5.0;
const MENU_FILL: Color = [0x38 as f32 / 255.0, 0x38 as f32 / 255.0, 0x38 as f32 / 255.0, 1.0];
const MENU_BORDER: Color = [1.0, 1.0, 1.0, 0.16];
const MENU_SECTION: Color = [0x88 as f32 / 255.0, 0x88 as f32 / 255.0, 0x88 as f32 / 255.0, 1.0];
const MENU_ITEM: Color = [1.0, 1.0, 1.0, 0.92];
const MENU_ITEM_SELECTED: Color = [1.0, 1.0, 1.0, 1.0];

pub fn popup_menu(cmds: &mut Vec<DrawCmd>, rect: Rect, items: &[MenuItem], hover: Option<usize>) {
    menu_drop_shadow(cmds, rect);
    cmds.push(DrawCmd::RoundedRect {
        rect: Rect {
            x: rect.x - 1.0,
            y: rect.y - 1.0,
            w: rect.w + 2.0,
            h: rect.h + 2.0,
        },
        color: MENU_BORDER,
        radius: MENU_RADIUS + 1.0,
    });
    cmds.push(DrawCmd::RoundedRect {
        rect,
        color: MENU_FILL,
        radius: MENU_RADIUS,
    });

    let clip = Rect {
        x: rect.x + 4.0,
        y: rect.y + 2.0,
        w: (rect.w - 8.0).max(0.0),
        h: (rect.h - 4.0).max(0.0),
    };
    let mut y = rect.y + MENU_PAD_Y;
    let mut last_section: Option<&str> = None;
    for (i, item) in items.iter().enumerate() {
        if let Some(sec) = item.section.as_deref() {
            if last_section != Some(sec) {
                theme::text_clip(
                    cmds,
                    Rect {
                        x: rect.x + MENU_LABEL_X,
                        y,
                        w: (rect.w - MENU_LABEL_X - 10.0).max(0.0),
                        h: MENU_SECTION_H,
                    },
                    sec,
                    11.0,
                    MENU_SECTION,
                    false,
                    Some(clip),
                );
                y += MENU_SECTION_H;
                last_section = Some(sec);
            }
        }
        if hover == Some(i) {
            cmds.push(DrawCmd::RoundedRect {
                rect: Rect {
                    x: rect.x + MENU_HIGHLIGHT_INSET,
                    y,
                    w: (rect.w - MENU_HIGHLIGHT_INSET * 2.0).max(0.0),
                    h: MENU_ITEM_H,
                },
                color: [1.0, 1.0, 1.0, 0.12],
                radius: 6.0,
            });
        }
        if item.checked {
            theme::text(
                cmds,
                Rect {
                    x: rect.x + 8.0,
                    y,
                    w: MENU_CHECK_W,
                    h: MENU_ITEM_H,
                },
                "✓",
                12.0,
                MENU_ITEM_SELECTED,
                false,
            );
        }
        theme::text_clip(
            cmds,
            Rect {
                x: rect.x + MENU_LABEL_X,
                y,
                w: (rect.w - MENU_LABEL_X - 10.0).max(0.0),
                h: MENU_ITEM_H,
            },
            item.label.as_str(),
            13.0,
            if item.checked { MENU_ITEM_SELECTED } else { MENU_ITEM },
            item.checked,
            Some(clip),
        );
        y += MENU_ITEM_H;
    }
}

pub fn item_at(rect: Rect, items: &[MenuItem], y: f32) -> Option<usize> {
    let mut yy = rect.y + MENU_PAD_Y;
    let mut last_section: Option<&str> = None;
    for (i, item) in items.iter().enumerate() {
        if let Some(sec) = item.section.as_deref() {
            if last_section != Some(sec) {
                yy += MENU_SECTION_H;
                last_section = Some(sec);
            }
        }
        if y >= yy && y < yy + MENU_ITEM_H {
            return Some(i);
        }
        yy += MENU_ITEM_H;
    }
    None
}

pub fn menu_height(items: &[MenuItem]) -> f32 {
    let mut h = MENU_PAD_Y * 2.0;
    let mut last_section: Option<&str> = None;
    for item in items {
        if let Some(sec) = item.section.as_deref() {
            if last_section != Some(sec) {
                h += MENU_SECTION_H;
                last_section = Some(sec);
            }
        }
        h += MENU_ITEM_H;
    }
    h
}

/// AppKit menu window shadow: ambient `black.opacity(0.45), radius 20, y 8`
/// plus a short contact drop. Radius follows the 9pt panel.
fn menu_drop_shadow(cmds: &mut Vec<DrawCmd>, rect: Rect) {
    let rings = 12;
    let opacity = 0.48;
    let blur = 20.0;
    let oy = 8.0;
    for i in (1..=rings).rev() {
        let t = i as f32 / rings as f32;
        let grow = blur * t;
        let a = opacity * (-2.6 * t * t).exp() * 0.13;
        cmds.push(DrawCmd::RoundedRect {
            rect: Rect {
                x: rect.x - grow,
                y: rect.y + oy - grow * 0.25,
                w: rect.w + grow * 2.0,
                h: rect.h + grow * 2.0,
            },
            color: [0.0, 0.0, 0.0, a],
            radius: MENU_RADIUS + grow * 0.55,
        });
    }
}

pub fn contains(rect: Rect, x: f32, y: f32) -> bool {
    x >= rect.x && x < rect.x + rect.w && y >= rect.y && y < rect.y + rect.h
}

/// MixLink dB scale: major ticks with labels, minor ticks without.
pub fn decibel_scale(cmds: &mut Vec<DrawCmd>, x: f32, top: f32, h: f32, labels: bool) {
    const MARKS: [(f32, Option<&str>, bool); 16] = [
        (6.0, Some("6"), true),
        (5.0, None, false),
        (2.0, None, false),
        (0.0, Some("0"), true),
        (-2.0, None, false),
        (-5.0, Some("−5"), true),
        (-10.0, Some("−10"), true),
        (-15.0, None, false),
        (-20.0, Some("−20"), true),
        (-25.0, None, false),
        (-30.0, Some("−30"), true),
        (-35.0, None, false),
        (-40.0, Some("−40"), true),
        (-50.0, None, false),
        (-60.0, None, false),
        (-65.0, Some("−∞"), true),
    ];
    for (db, label, major) in MARKS {
        let t = (6.0 - db) / 71.0;
        let y = top + t * h;
        let w = if major { 8.0 } else { 4.0 };
        theme::fill(
            cmds,
            Rect { x, y, w, h: 1.0 },
            if major { [1.0, 1.0, 1.0, 0.28] } else { [1.0, 1.0, 1.0, 0.12] },
        );
        if labels {
            if let Some(label) = label {
                theme::text_mono(cmds, Rect { x: x + 9.0, y: y - 6.0, w: 22.0, h: 12.0 }, label, 8.0, [1.0, 1.0, 1.0, 0.56], false);
            }
        }
    }
}
