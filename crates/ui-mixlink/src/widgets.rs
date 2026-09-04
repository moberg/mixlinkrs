//! Shared MixLink-look widgets. All chrome uses these so Settings/sidebar/mixer match.

use osc::fader_lin_from_db;
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
    // MixLink off-state title is `Color.white.opacity(0.72)`. Accent is the face
    // wash (`accent.opacity(0.46)`), not the glyph fill — full-accent type on that
    // tint reads as red-on-red for MUTE / SOLO / BUS.
    let label = [1.0, 1.0, 1.0, 0.72];
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
    let r = Layout::BUTTON_RADIUS;
    // HardwarePadStyle shadows the composed control (face + mounting well).
    let well = Rect {
        x: rect.x - 1.35,
        y: rect.y - 0.75,
        w: rect.w + 2.7,
        h: rect.h + 2.7,
    };
    if on {
        // MixLink HardwarePadStyle: `.shadow(color: accent.opacity(0.55), radius: 6)`.
        // wgpu additive rings read hotter than SwiftUI's Gaussian, so this is
        // ~0.58 of that opacity with a slightly tighter radius.
        gaussian_tint_shadow(cmds, well, accent, 0.32, 5.0, r + 0.6);
    }
    pad_face_ex(cmds, rect, on, accent, false);

    let cx = rect.x + rect.w * 0.5;
    let cy = rect.y + rect.h * 0.5;
    let led_d = 5.5;
    if on {
        let led = Rect {
            x: cx - led_d * 0.5,
            y: cy - led_d * 0.5,
            w: led_d,
            h: led_d,
        };
        // HardwarePadStyle on the label: `.shadow(color: accent.opacity(0.9), radius: 2.4)`
        gaussian_tint_shadow(cmds, led, accent, 0.9, 2.4, led_d * 0.5);
        // ChannelOnToggle: `.shadow(color: meterGreen.opacity(0.7), radius: 1.6)`
        gaussian_tint_shadow(cmds, led, accent, 0.7, 1.6, led_d * 0.5);
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

/// MixLink `HardwareModuleModifier`: `HardwareSurface(.recessed)` + inset strokes.
/// Recessed middle is sRGB (0.065, 0.068, 0.065). Do not fill the body with a
/// translucent white well — that linearizes to medium gray.
pub fn hardware_module(cmds: &mut Vec<DrawCmd>, rect: Rect) {
    theme::hardware_surface(cmds, rect, theme::SurfaceStyle::Recessed);
    // MixLink: `strokeBorder(Color.black.opacity(0.72), lineWidth: 0.8)` r=3
    stroke_border(cmds, rect, [0.0, 0.0, 0.0, 0.72]);
    // MixLink: `strokeBorder(Color.white.opacity(0.05), lineWidth: 0.55)` inset 1
    stroke_border(
        cmds,
        Rect { x: rect.x + 1.0, y: rect.y + 1.0, w: (rect.w - 2.0).max(0.0), h: (rect.h - 2.0).max(0.0) },
        [1.0, 1.0, 1.0, 0.05],
    );
    theme::fill(cmds, Rect { x: rect.x, y: rect.y, w: rect.w, h: 1.0 }, [0.0, 0.0, 0.0, 0.55]);
    theme::fill(cmds, Rect { x: rect.x, y: rect.y + rect.h - 1.0, w: rect.w, h: 1.0 }, [1.0, 1.0, 1.0, 0.045]);
    theme::fill(cmds, Rect { x: rect.x, y: rect.y, w: 1.0, h: rect.h }, [0.0, 0.0, 0.0, 0.45]);
    theme::fill(cmds, Rect { x: rect.x + rect.w - 1.0, y: rect.y, w: 1.0, h: rect.h }, [1.0, 1.0, 1.0, 0.04]);
}

/// MixLink `MenuLabel` / project-name well: recessed field, not a gray overlay.
pub fn recessed_field(cmds: &mut Vec<DrawCmd>, rect: Rect) {
    theme::hardware_surface(cmds, rect, theme::SurfaceStyle::Recessed);
    // MixLink MenuLabel: `strokeBorder(Color.black.opacity(0.72), lineWidth: 0.7)` r=2.5
    stroke_border(cmds, rect, [0.0, 0.0, 0.0, 0.72]);
    // MixLink: `strokeBorder(Color.white.opacity(0.045), lineWidth: 0.5)` inset 1
    stroke_border(
        cmds,
        Rect { x: rect.x + 1.0, y: rect.y + 1.0, w: (rect.w - 2.0).max(0.0), h: (rect.h - 2.0).max(0.0) },
        [1.0, 1.0, 1.0, 0.045],
    );
    theme::fill(cmds, Rect { x: rect.x, y: rect.y, w: rect.w, h: 1.0 }, [0.0, 0.0, 0.0, 0.48]);
}

fn stroke_border(cmds: &mut Vec<DrawCmd>, rect: Rect, color: Color) {
    if rect.w <= 0.0 || rect.h <= 0.0 {
        return;
    }
    theme::fill(cmds, Rect { x: rect.x, y: rect.y, w: rect.w, h: 1.0 }, color);
    theme::fill(cmds, Rect { x: rect.x, y: rect.y + rect.h - 1.0, w: rect.w, h: 1.0 }, color);
    theme::fill(cmds, Rect { x: rect.x, y: rect.y, w: 1.0, h: rect.h }, color);
    theme::fill(cmds, Rect { x: rect.x + rect.w - 1.0, y: rect.y, w: 1.0, h: rect.h }, color);
}

fn pad_face(cmds: &mut Vec<DrawCmd>, rect: Rect, lit: bool, accent: Color) {
    // Hardware SOLO/MUTE/BUS pads: MixLink lights the metal with `accent.opacity(0.46)`,
    // not a hard expanded halo. `enable_toggle` draws its own Gaussian chassis glow.
    pad_face_ex(cmds, rect, lit, accent, false);
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
    sdf_shadow_rings(cmds, rect, [0.0, 0.0, 0.0, 1.0], opacity, blur, ox, oy, corner, false);
}

/// SwiftUI `.shadow(color: tint.opacity, radius)` with no offset.
///
/// Same 20-stop SDF as [`soft_rect_shadow`], but each ring is the *increment*
/// of the Gaussian (not the absolute profile). Stacking the absolute profile
/// in a bright tint reads as a neon plate; the increment stays a chassis haze.
fn gaussian_tint_shadow(cmds: &mut Vec<DrawCmd>, rect: Rect, color: Color, opacity: f32, blur: f32, corner: f32) {
    sdf_shadow_rings(cmds, rect, color, opacity, blur, 0.0, 0.0, corner, true);
}

fn sdf_shadow_rings(
    cmds: &mut Vec<DrawCmd>,
    rect: Rect,
    color: Color,
    opacity: f32,
    blur: f32,
    ox: f32,
    oy: f32,
    corner: f32,
    glow: bool,
) {
    let sigma = blur.max(0.05);
    let inner = -2.0 * sigma;
    let outer = 3.0 * sigma;
    const RINGS: i32 = 20;
    let profile = |d: f32| opacity * 0.5 * (1.0 - (d / (sigma * std::f32::consts::SQRT_2)).tanh());
    for i in (0..=RINGS).rev() {
        let t = i as f32 / RINGS as f32;
        let d = inner + (outer - inner) * t;
        let a = if glow {
            // Umbra sits under the pad. Drawing it would leak through the
            // 78%-opaque well and paint a second green rectangle.
            if d < 0.0 {
                continue;
            }
            let g = profile(d);
            let g_out = if i == RINGS {
                0.0
            } else {
                let t_out = (i + 1) as f32 / RINGS as f32;
                profile(inner + (outer - inner) * t_out)
            };
            if g_out >= 0.999 {
                continue;
            }
            ((g - g_out) / (1.0 - g_out)).max(0.0)
        } else {
            profile(d)
        };
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
            color: [color[0], color[1], color[2], a],
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
    recessed_field(cmds, rect);
    // MixLink MenuLabel: `.padding(.vertical, 5) .padding(.horizontal, 6)`,
    // 11 medium `MixerTheme.primaryText`, `chevron.up.chevron.down`.
    theme::text(
        cmds,
        Rect { x: rect.x + 6.0, y: rect.y, w: (rect.w - 22.0).max(0.0), h: rect.h },
        text,
        11.0,
        theme::PRIMARY_TEXT,
        false,
    );
    theme::text(
        cmds,
        Rect { x: rect.x + rect.w - 16.0, y: rect.y, w: 12.0, h: rect.h },
        "↕",
        8.0,
        theme::PRIMARY_TEXT,
        false,
    );
}

/// MixLink `StripNameLabel` — diamond + text on the fader bay, bottom hairline.
/// No fill and no grain: MixLink sits this row on the continuous bay metal.
pub struct StripNameStyle {
    pub diamond: bool,
    pub dim: bool,
}

pub fn strip_name_label(cmds: &mut Vec<DrawCmd>, rect: Rect, text: &str, style: StripNameStyle) {
    if rect.w <= 0.0 || rect.h <= 0.0 {
        return;
    }
    // MixLink: `.overlay(alignment: .bottom) { Rectangle().fill(MixerTheme.seam).padding(.horizontal, 6) }`
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

/// MixLink `DecibelScaleView.Placement`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScalePlacement {
    Leading,
    Trailing,
}

/// MixLink `Layout.capCenterY`: tick Y follows cap travel, not a linear track.
pub fn cap_center_y(lin: f32, height: f32, cap_h: f32) -> f32 {
    cap_h * 0.5 + (1.0 - lin.clamp(0.0, 1.0)) * (height - cap_h).max(0.0)
}

/// MixLink `LevelMeterView`: recessed housing, LED stack inset 2×3.
pub fn level_meter(cmds: &mut Vec<DrawCmd>, rect: Rect, peak: f32) {
    cmds.push(DrawCmd::RoundedRect {
        rect,
        color: [0.045, 0.045, 0.045, 1.0],
        radius: 2.0,
    });
    theme::fill(cmds, Rect { x: rect.x, y: rect.y, w: rect.w, h: 1.0 }, [0.0, 0.0, 0.0, 0.82]);
    theme::fill(cmds, Rect { x: rect.x, y: rect.y, w: 1.0, h: rect.h }, [0.0, 0.0, 0.0, 0.72]);
    theme::fill(
        cmds,
        Rect { x: rect.x, y: rect.y + rect.h - 0.7, w: rect.w, h: 0.7 },
        [1.0, 1.0, 1.0, 0.04],
    );
    theme::fill(
        cmds,
        Rect { x: rect.x + rect.w - 0.7, y: rect.y, w: 0.7, h: rect.h },
        [1.0, 1.0, 1.0, 0.04],
    );
    let inner = Rect {
        x: rect.x + 2.0,
        y: rect.y + 3.0,
        w: Layout::METER_W,
        h: (rect.h - 6.0).max(1.0),
    };
    theme::fill(cmds, inner, [0.018, 0.018, 0.018, 1.0]);
    let segs = Layout::METER_SEGMENTS;
    let fill_n = (peak * segs as f32).round() as i32;
    let seg_h = inner.h / segs as f32;
    for i in 0..segs {
        if i >= fill_n {
            continue;
        }
        let t = i as f32 / segs as f32;
        let color = if t > 0.9 {
            theme::METER_RED
        } else if t > 0.7 {
            theme::METER_YELLOW
        } else {
            theme::METER_GREEN
        };
        theme::fill(
            cmds,
            Rect {
                x: inner.x,
                y: inner.y + inner.h - (i as f32 + 1.0) * seg_h,
                w: inner.w,
                h: (seg_h - 0.4).max(0.2),
            },
            color,
        );
    }
}

/// MixLink dB scale: ticks at `capCenterY(faderLin(fromDb:))`. Trailing draws −∞…+6.
pub fn decibel_scale(cmds: &mut Vec<DrawCmd>, x: f32, top: f32, h: f32, placement: ScalePlacement) {
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
    let col_w = match placement {
        ScalePlacement::Leading => Layout::SCALE_LEADING,
        ScalePlacement::Trailing => Layout::SCALE_WIDTH,
    };
    for (db, label, major) in MARKS {
        let lin = fader_lin_from_db(db);
        let y = top + cap_center_y(lin, h, Layout::FADER_CAP_H);
        let tick_w = if major { 10.0 } else { 5.0 };
        let tick_h = if major { 0.75 } else { 0.5 };
        let tick_x = match placement {
            ScalePlacement::Leading => x + col_w - tick_w,
            ScalePlacement::Trailing => x,
        };
        theme::fill(
            cmds,
            Rect { x: tick_x, y: y - tick_h * 0.5, w: tick_w, h: tick_h.max(0.5) },
            if major { [1.0, 1.0, 1.0, 0.23] } else { [1.0, 1.0, 1.0, 0.11] },
        );
        if placement == ScalePlacement::Trailing {
            if let Some(label) = label {
                theme::text_mono(
                    cmds,
                    Rect { x: x + tick_w + 2.0, y: y - 6.0, w: 22.0, h: 12.0 },
                    label,
                    8.0,
                    [1.0, 1.0, 1.0, 0.56],
                    false,
                );
            }
        }
    }
}
