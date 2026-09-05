//! Shared MixLink-look widgets. All chrome uses these so Settings/sidebar/mixer match.

use osc::fader_lin_from_db;
use render::{aspect_fill_uv, DrawCmd, Rect, TextureId};

use crate::theme::{self, Color, Layout};

/// MixLink `FaderCapView`: stem, Gaussian-ish drop shadow, then the PNG.
/// `hardware` uses the icon-style cream cap with a horizontal score only.
pub fn fader_cap(cmds: &mut Vec<DrawCmd>, rect: Rect, hardware: bool) {
    if hardware {
        hardware_fader_cap(cmds, rect);
        return;
    }
    // Stem sits under the cap and peeks out the bottom (MixLink offset y: 5).
    let stem = Rect { x: rect.x + rect.w * 0.5 - 2.5, y: rect.y + rect.h - 2.0, w: 5.0, h: 7.0 };
    cmds.push(DrawCmd::RoundedRect { rect: stem, color: [0.62, 0.62, 0.60, 1.0], radius: 2.5 });

    // MixLink: `.shadow(color: .black.opacity(0.55), radius: 2, x: 1, y: 2)`
    soft_rect_shadow(cmds, rect, 0.70, 4.0, 1.5, 3.0, 3.0);

    cmds.push(DrawCmd::Image {
        rect,
        uv: aspect_fill_uv(30.0, 53.0, rect.w, rect.h),
        texture: TextureId::FaderCap,
    });
}

/// Cap cropped from the app icon (97×192).
const HARDWARE_CAP_SRC: (f32, f32) = (97.0, 192.0);

fn hardware_fader_cap(cmds: &mut Vec<DrawCmd>, rect: Rect) {
    soft_rect_shadow(cmds, rect, 0.85, 6.0, 0.6, 2.6, 9.0);
    cmds.push(DrawCmd::Image {
        rect,
        uv: aspect_fill_uv(HARDWARE_CAP_SRC.0, HARDWARE_CAP_SRC.1, rect.w, rect.h),
        texture: TextureId::HardwareFaderCap,
    });
}

const FACE_TOP: Color = [0x30 as f32 / 255.0, 0x31 as f32 / 255.0, 0x33 as f32 / 255.0, 1.0];
const FACE_MID: Color = [0x24 as f32 / 255.0, 0x25 as f32 / 255.0, 0x27 as f32 / 255.0, 1.0];
const FACE_BOT: Color = [0x19 as f32 / 255.0, 0x1A as f32 / 255.0, 0x1C as f32 / 255.0, 1.0];
/// MixLink `HardwarePadStyle` uses `accent.opacity(0.46)` over metal. wgpu
/// blends in linear after sRGB→linear, so 0.46 composites as a solid brick.
/// 0.24 keeps MUTE red / SOLO green / BUS amber / Rec as a tint the grain
/// still shows through.
const LIT_PAD_WASH: f32 = 0.24;
/// MixLink `Color.white.opacity(0.28)` face sheen, scaled the same way so the
/// top does not linearize into an opaque orange-red slab.
const LIT_PAD_SHEEN: f32 = 0.12;
/// MixLink `HardwarePadStyle` `strokeBorder` is a TL→BR gradient
/// (`white 0.18 / 0.08` off, `white 0.55` lit). MixLinkRs draws 1px bars and
/// blends in linear, so those alphas read as a near-white rim. Use the
/// MixLink inner-highlight look (white 0.045–0.08), ~⅓ of the MixLink stops.
const PAD_BEVEL_TOP: f32 = 0.06; // MixLink 0.18
const PAD_BEVEL_TOP_SUNKEN: f32 = 0.027; // MixLink 0.08
const PAD_BEVEL_TOP_LIT: f32 = 0.18; // MixLink 0.55
const PAD_BEVEL_LEFT: f32 = 0.045; // was 0.08
const PAD_BEVEL_LEFT_LIT: f32 = 0.07; // was 0.22

/// MixLink `HardwarePadStyle`: mounting well, 3-stop metal, bevel, drop shadow.
pub fn hardware_pad(cmds: &mut Vec<DrawCmd>, rect: Rect, title: &str, on: bool, accent: Color) {
    pad_face(cmds, rect, on, accent);
    // MixLink off-state title is `Color.white.opacity(0.72)`. Accent is the face
    // wash (`LIT_PAD_WASH` over metal), not the glyph fill — full-accent type on
    // that tint reads as red-on-red for MUTE / SOLO / BUS.
    let label = [1.0, 1.0, 1.0, 0.72];
    // MixLink `HardwareButton`: `.font(.system(size: 10.5, weight: .semibold))` + `uppercased()`.
    theme::text_center(cmds, rect, title.to_uppercase(), 10.5, label, true);
}

/// Header project dropdown: RECORD/MIX pad, left-aligned name, trailing chevron.
pub fn chrome_menu_pad(cmds: &mut Vec<DrawCmd>, rect: Rect, title: &str, on: bool) {
    pad_face(cmds, rect, on, theme::PRIMARY_TEXT);
    const PAD_X: f32 = 8.0;
    const CHEVRON_W: f32 = 8.0;
    const GAP: f32 = 4.0;
    let label_color = [1.0, 1.0, 1.0, 0.72];
    let chevron =
        Rect { x: rect.x + rect.w - PAD_X - CHEVRON_W, y: rect.y, w: CHEVRON_W, h: rect.h };
    let label = Rect {
        x: rect.x + PAD_X,
        y: rect.y,
        w: (chevron.x - GAP - rect.x - PAD_X).max(1.0),
        h: rect.h,
    };
    theme::text(cmds, label, title, 11.0, label_color, false);
    chevron_up_chevron_down(cmds, chevron, label_color);
}

pub fn icon_pad(cmds: &mut Vec<DrawCmd>, rect: Rect, glyph: &str, enabled: bool) {
    pad_face(cmds, rect, false, theme::PRIMARY_TEXT);
    let size = (rect.h * 0.72).clamp(7.0, 9.5);
    theme::text_center(
        cmds,
        rect,
        glyph,
        size,
        if enabled { [1.0, 1.0, 1.0, 0.78] } else { theme::SECONDARY_TEXT },
        true,
    );
}

/// Dark header pad with a simple folder glyph (no SF Symbol / emoji in the atlas).
pub fn folder_icon_pad(cmds: &mut Vec<DrawCmd>, rect: Rect, enabled: bool) {
    pad_face(cmds, rect, false, theme::PRIMARY_TEXT);
    let color = if enabled { [1.0, 1.0, 1.0, 0.78] } else { theme::SECONDARY_TEXT };
    folder_glyph(cmds, rect, color);
}

/// Tab + body, ~11pt weight to sit next to the 9.5pt plus.
fn folder_glyph(cmds: &mut Vec<DrawCmd>, pad: Rect, color: Color) {
    let cx = pad.x + pad.w * 0.5;
    let cy = pad.y + pad.h * 0.5 + 0.4;
    let body = Rect { x: cx - 6.0, y: cy - 3.2, w: 12.0, h: 7.4 };
    let tab = Rect { x: body.x, y: body.y - 2.0, w: 4.8, h: 2.6 };
    cmds.push(DrawCmd::RoundedRect { rect: tab, color, radius: 1.0 });
    cmds.push(DrawCmd::RoundedRect { rect: body, color, radius: 1.4 });
}

/// MixLink `ChannelOnToggle`: hardware pad + green LED + SwiftUI shadows.
pub fn enable_toggle(cmds: &mut Vec<DrawCmd>, rect: Rect, on: bool) {
    let accent = theme::METER_GREEN;
    let r = Layout::BUTTON_RADIUS;
    // HardwarePadStyle shadows the composed control (face + mounting well).
    let well = Rect { x: rect.x - 1.35, y: rect.y - 0.75, w: rect.w + 2.7, h: rect.h + 2.7 };
    if on {
        // MixLink HardwarePadStyle: `.shadow(color: accent.opacity(0.55), radius: 6)`.
        // Keep a tight chassis haze — the old 0.32 / 5pt bloom washed the strip.
        gaussian_tint_shadow(cmds, well, accent, 0.12, 2.2, r + 0.6);
    }
    pad_face_ex(cmds, rect, on, accent, false);

    let cx = rect.x + rect.w * 0.5;
    let cy = rect.y + rect.h * 0.5;
    let led_d = 5.5;
    if on {
        let led = Rect { x: cx - led_d * 0.5, y: cy - led_d * 0.5, w: led_d, h: led_d };
        // HardwarePadStyle on the label: `.shadow(color: accent.opacity(0.9), radius: 2.4)`
        gaussian_tint_shadow(cmds, led, accent, 0.40, 1.3, led_d * 0.5);
        // ChannelOnToggle: `.shadow(color: meterGreen.opacity(0.7), radius: 1.6)`
        gaussian_tint_shadow(cmds, led, accent, 0.32, 0.9, led_d * 0.5);
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
        Rect {
            x: rect.x + 1.0,
            y: rect.y + 1.0,
            w: (rect.w - 2.0).max(0.0),
            h: (rect.h - 2.0).max(0.0),
        },
        [1.0, 1.0, 1.0, 0.05],
    );
    theme::fill(cmds, Rect { x: rect.x, y: rect.y, w: rect.w, h: 1.0 }, [0.0, 0.0, 0.0, 0.55]);
    theme::fill(
        cmds,
        Rect { x: rect.x, y: rect.y + rect.h - 1.0, w: rect.w, h: 1.0 },
        [1.0, 1.0, 1.0, 0.045],
    );
    theme::fill(cmds, Rect { x: rect.x, y: rect.y, w: 1.0, h: rect.h }, [0.0, 0.0, 0.0, 0.45]);
    theme::fill(
        cmds,
        Rect { x: rect.x + rect.w - 1.0, y: rect.y, w: 1.0, h: rect.h },
        [1.0, 1.0, 1.0, 0.04],
    );
}

/// MixLink `MenuLabel` / project-name well: recessed field, not a gray overlay.
pub fn recessed_field(cmds: &mut Vec<DrawCmd>, rect: Rect) {
    theme::hardware_surface(cmds, rect, theme::SurfaceStyle::Recessed);
    // MixLink MenuLabel: `strokeBorder(Color.black.opacity(0.72), lineWidth: 0.7)` r=2.5
    stroke_border(cmds, rect, [0.0, 0.0, 0.0, 0.72]);
    // MixLink: `strokeBorder(Color.white.opacity(0.045), lineWidth: 0.5)` inset 1
    stroke_border(
        cmds,
        Rect {
            x: rect.x + 1.0,
            y: rect.y + 1.0,
            w: (rect.w - 2.0).max(0.0),
            h: (rect.h - 2.0).max(0.0),
        },
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
    // Hardware SOLO/MUTE/BUS pads: MixLink lights the metal with an accent wash,
    // not a hard expanded halo. `enable_toggle` draws its own Gaussian chassis glow.
    pad_face_ex(cmds, rect, lit, accent, false);
}

fn pad_face_ex(cmds: &mut Vec<DrawCmd>, rect: Rect, lit: bool, accent: Color, generic_glow: bool) {
    let r = Layout::BUTTON_RADIUS;
    let sunken = lit;
    let well = Rect { x: rect.x - 1.35, y: rect.y - 0.75, w: rect.w + 2.7, h: rect.h + 2.7 };
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
    cmds.push(DrawCmd::RoundedRect { rect: well, color: [0.0, 0.0, 0.0, 0.78], radius: r + 0.6 });
    theme::fill(
        cmds,
        Rect { x: well.x + 1.0, y: well.y + well.h - 0.6, w: well.w - 2.0, h: 0.6 },
        [1.0, 1.0, 1.0, 0.06],
    );

    let face =
        if sunken { Rect { x: rect.x, y: rect.y + 1.0, w: rect.w, h: rect.h } } else { rect };
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
            color: [accent[0], accent[1], accent[2], LIT_PAD_WASH],
            radius: r,
        });
        // White→clear sheen (not white→accent): a full-face accent gradient is
        // what linearized into the loud red slab.
        cmds.push(DrawCmd::VertGradient {
            rect: face,
            top: [1.0, 1.0, 1.0, LIT_PAD_SHEEN],
            bottom: [1.0, 1.0, 1.0, 0.0],
        });
    }
    theme::fill(
        cmds,
        Rect { x: face.x, y: face.y, w: face.w, h: 1.0 },
        [
            1.0,
            1.0,
            1.0,
            if lit {
                PAD_BEVEL_TOP_LIT
            } else if sunken {
                PAD_BEVEL_TOP_SUNKEN
            } else {
                PAD_BEVEL_TOP
            },
        ],
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
    theme::fill(
        cmds,
        Rect { x: face.x, y: face.y, w: 1.0, h: face.h },
        [1.0, 1.0, 1.0, if lit { PAD_BEVEL_LEFT_LIT } else { PAD_BEVEL_LEFT }],
    );
    theme::fill(
        cmds,
        Rect { x: face.x + face.w - 1.0, y: face.y, w: 1.0, h: face.h },
        [0.0, 0.0, 0.0, 0.40],
    );
}

/// MixLink `KnobStyle`: send has a colored 300° ring; pan is a silver-rimmed pot.
#[derive(Clone, Copy)]
pub enum KnobKind {
    Send(Color),
    Pan,
}

/// MixLink `KnobView`. Send knobs show a dB label; pan knobs do not.
pub fn knob(
    cmds: &mut Vec<DrawCmd>,
    x: f32,
    y: f32,
    d: f32,
    value: f32,
    kind: KnobKind,
    label: Option<&str>,
) {
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
    // MixLink: 2pt `ringColor` stroke + `.shadow(color: ringColor.opacity(0.24), radius: 0.9)`.
    gaussian_arc_shadow(cmds, cx, cy, track_r, 120.0, 300.0, ring, 0.24, 0.9, 2.0);
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

/// MixLink `PanKnobFace` — black well, silver rim, white pointer, no dB label.
/// Disc contact shadow matches send: `hardwareElevation(diameter: size + 4)`.
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

/// MixLink `hardwareElevation(diameter:)` on send and pan disc wells (`size + 4`):
/// `.shadow(color: .black.opacity(0.75), radius: 2, x: 1, y: 2)`
/// `.shadow(color: .black.opacity(0.32), radius: drop * 0.42, x: drop * 0.14, y: drop * 0.58)`
/// where `drop = diameter * 0.5`. Pan uses this body drop only — no send ring glow.
fn hardware_elevation(cmds: &mut Vec<DrawCmd>, cx: f32, cy: f32, diameter: f32) {
    let drop = diameter * 0.5;
    gaussian_disc_shadow(cmds, cx, cy, diameter, 0.32, drop * 0.42, drop * 0.14, drop * 0.58);
    gaussian_disc_shadow(cmds, cx, cy, diameter, 0.75, 2.0, 1.0, 2.0);
}

/// MixLink SwiftUI `.shadow(color: .black.opacity, radius, x, y)` on a disc.
/// Incremental Gaussian (same SDF rings as pads), not stacked neon silhouettes.
fn gaussian_disc_shadow(
    cmds: &mut Vec<DrawCmd>,
    cx: f32,
    cy: f32,
    diameter: f32,
    opacity: f32,
    blur: f32,
    ox: f32,
    oy: f32,
) {
    let d = diameter.max(1.0);
    sdf_shadow_rings(
        cmds,
        Rect { x: cx - d * 0.5, y: cy - d * 0.5, w: d, h: d },
        [0.0, 0.0, 0.0, 1.0],
        opacity,
        blur,
        ox,
        oy,
        d * 0.5,
        true,
        false,
    );
}

/// MixLink send-ring `.shadow(color: ring.opacity, radius)` on a 2pt stroke.
fn gaussian_arc_shadow(
    cmds: &mut Vec<DrawCmd>,
    cx: f32,
    cy: f32,
    r: f32,
    start_deg: f32,
    sweep_deg: f32,
    color: Color,
    opacity: f32,
    blur: f32,
    stroke_w: f32,
) {
    let sigma = blur.max(0.05);
    let inner = -2.0 * sigma;
    let outer = 3.0 * sigma;
    const RINGS: i32 = 20;
    let profile = |d: f32| opacity * 0.5 * (1.0 - (d / (sigma * std::f32::consts::SQRT_2)).tanh());
    for i in (0..=RINGS).rev() {
        let t = i as f32 / RINGS as f32;
        let d = inner + (outer - inner) * t;
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
        let a = ((g - g_out) / (1.0 - g_out)).max(0.0);
        if a < 0.003 {
            continue;
        }
        stroke_arc(
            cmds,
            cx,
            cy,
            r,
            start_deg,
            sweep_deg,
            [color[0], color[1], color[2], a],
            (stroke_w + d * 2.0).max(0.5),
        );
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
    sdf_shadow_rings(cmds, rect, [0.0, 0.0, 0.0, 1.0], opacity, blur, ox, oy, corner, false, false);
}

/// SwiftUI `.shadow(color: tint.opacity, radius)` with no offset.
///
/// Same 20-stop SDF as [`soft_rect_shadow`], but each ring is the *increment*
/// of the Gaussian (not the absolute profile). Stacking the absolute profile
/// in a bright tint reads as a neon plate; the increment stays a chassis haze.
fn gaussian_tint_shadow(
    cmds: &mut Vec<DrawCmd>,
    rect: Rect,
    color: Color,
    opacity: f32,
    blur: f32,
    corner: f32,
) {
    sdf_shadow_rings(cmds, rect, color, opacity, blur, 0.0, 0.0, corner, true, true);
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
    skip_umbra: bool,
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
            // Pad tints skip the umbra so it does not leak through a translucent well.
            // Opaque knobs keep it: the offset crescent is the drop-shadow silhouette.
            if skip_umbra && d < 0.0 {
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

fn pointer(
    cmds: &mut Vec<DrawCmd>,
    cx: f32,
    cy: f32,
    value: f32,
    length: f32,
    offset: f32,
    width: f32,
    color: Color,
) {
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

fn rim_stroke(
    cmds: &mut Vec<DrawCmd>,
    cx: f32,
    cy: f32,
    r: f32,
    thickness: f32,
    hi: Color,
    lo: Color,
) {
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

/// Thin horizontal hardware scrollbar. Hidden when `max_scroll` is ~0.
pub fn scrollbar_h(cmds: &mut Vec<DrawCmd>, track: Rect, scroll: f32, max_scroll: f32) {
    if max_scroll < 0.5 || track.w < 16.0 {
        return;
    }
    let pad = 2.0;
    let inner_w = (track.w - pad * 2.0).max(8.0);
    let view_ratio = track.w / (track.w + max_scroll);
    let thumb_w = (inner_w * view_ratio).clamp(18.0, inner_w);
    let t = (scroll / max_scroll).clamp(0.0, 1.0);
    let thumb_x = track.x + pad + t * (inner_w - thumb_w);
    theme::fill(cmds, track, [0.0, 0.0, 0.0, 0.28]);
    cmds.push(DrawCmd::RoundedRect {
        rect: Rect { x: thumb_x, y: track.y + 1.0, w: thumb_w, h: (track.h - 2.0).max(2.0) },
        color: [0.42, 0.42, 0.40, 0.85],
        radius: 2.0,
    });
}

/// Thin hardware scrollbar. Hidden when `max_scroll` is ~0.
pub fn scrollbar(cmds: &mut Vec<DrawCmd>, track: Rect, scroll: f32, max_scroll: f32) {
    if max_scroll < 0.5 || track.h < 16.0 {
        return;
    }
    let pad = 2.0;
    let inner_h = (track.h - pad * 2.0).max(8.0);
    let view_ratio = track.h / (track.h + max_scroll);
    let thumb_h = (inner_h * view_ratio).clamp(18.0, inner_h);
    let t = (scroll / max_scroll).clamp(0.0, 1.0);
    let thumb_y = track.y + pad + t * (inner_h - thumb_h);
    theme::fill(cmds, track, [0.0, 0.0, 0.0, 0.28]);
    cmds.push(DrawCmd::RoundedRect {
        rect: Rect { x: track.x + 1.0, y: thumb_y, w: (track.w - 2.0).max(2.0), h: thumb_h },
        color: [0.42, 0.42, 0.40, 0.85],
        radius: 2.0,
    });
}

/// MixLink Settings checkbox: dark square, red border + check when on, “On”/“Off”.
pub fn checkbox(cmds: &mut Vec<DrawCmd>, x: f32, y: f32, on: bool, label: &str) {
    let box_r = Rect { x, y, w: 14.0, h: 14.0 };
    theme::fill(cmds, box_r, theme::DEEP_SLOT);
    theme::stroke_rect(
        cmds,
        box_r,
        if on { theme::METER_RED } else { [0.22, 0.22, 0.24, 1.0] },
        1.0,
    );
    if on {
        theme::text_center(cmds, box_r, "✓", 11.0, [1.0, 1.0, 1.0, 0.95], true);
    }
    theme::text(
        cmds,
        Rect { x: x + 20.0, y: y - 1.0, w: 110.0, h: 16.0 },
        label,
        11.0,
        theme::TEXT,
        false,
    );
}

/// Charcoal brushed window + inset recessed card (MixLink Settings / Channels).
/// Fills to `(0, 0)` so dark chrome paints under a transparent macOS titlebar.
pub fn document_window(cmds: &mut Vec<DrawCmd>, w: f32, h: f32, panel: Rect) {
    theme::hardware_surface(cmds, Rect { x: 0.0, y: 0.0, w, h }, theme::SurfaceStyle::Sidebar);
    hardware_module(cmds, panel);
}

/// Beveled Close pad, bottom-right of a document window.
pub fn window_close(cmds: &mut Vec<DrawCmd>, rect: Rect) {
    hardware_pad(cmds, rect, "Close", false, theme::PRIMARY_TEXT);
}

/// MixLink closed sidebar picker (`ChannelPicker` / `MenuLabel` as it paints on
/// a hardware module): `chevron.up.chevron.down` + value on the card, no second
/// recessed well and no diamond. MixLink Swift still wraps `MenuLabel` in a
/// recessed surface; on the card that well disappears — MixLinkRs draws the
/// screenshot: flat type, MixLink `.padding(.vertical, 5) .padding(.horizontal, 6)`.
#[derive(Clone, Copy)]
pub struct ChannelPickerStyle {
    pub size: f32,
    pub color: Color,
    /// Pack the chevron+text cluster to the trailing edge (plugin 3/4).
    pub trailing: bool,
}

impl ChannelPickerStyle {
    /// Heat Output/Input, Mix Out, Audio Device — white, a
    /// point larger than the 11pt `textDim` field label.
    pub fn value() -> Self {
        Self { size: 12.0, color: theme::PRIMARY_TEXT, trailing: false }
    }

    /// MixLink `MenuLabel` 11 medium `MixerTheme.primaryText` — plugin bundle
    /// reads as regular light gray under the bold slot title.
    pub fn plugin() -> Self {
        Self { size: 11.0, color: theme::TEXT, trailing: false }
    }

    /// PluginSlotView footer: `HStack` + `ChannelPicker(title: nil)` trailing.
    pub fn playback() -> Self {
        Self { size: 11.0, color: theme::PRIMARY_TEXT, trailing: true }
    }
}

pub fn channel_picker(cmds: &mut Vec<DrawCmd>, rect: Rect, text: &str, style: ChannelPickerStyle) {
    if rect.w <= 0.0 || rect.h <= 0.0 {
        return;
    }
    // MixLink MenuLabel: `.padding(.vertical, 5) .padding(.horizontal, 6)`,
    // `HStack(spacing: 3)`. Screenshot Heat/EffectRack: leading chevron, not
    // the Swift `Text` then trailing symbol order.
    const PAD_X: f32 = 6.0;
    const GAP: f32 = 3.0;
    const CHEVRON_W: f32 = 8.0;
    let inner = (rect.w - PAD_X * 2.0).max(0.0);
    let x0 = rect.x + PAD_X;
    let chevron_box = Rect { x: x0, y: rect.y, w: CHEVRON_W, h: rect.h };
    chevron_up_chevron_down(cmds, chevron_box, style.color);
    let label_x = x0 + CHEVRON_W + GAP;
    let label_w = (rect.x + PAD_X + inner - label_x).max(1.0);
    theme::text(
        cmds,
        Rect { x: label_x, y: rect.y, w: label_w, h: rect.h },
        text,
        style.size,
        style.color,
        false,
    );
}

/// Sidebar pickers use [`channel_picker`]; kept so other agents can call the
/// old name. No recessed well.
pub fn menu_label(cmds: &mut Vec<DrawCmd>, rect: Rect, text: &str) {
    channel_picker(cmds, rect, text, ChannelPickerStyle::value());
}

/// MixLink `Image(systemName: "chevron.up.chevron.down")` ~11pt semibold.
fn chevron_up_chevron_down(cmds: &mut Vec<DrawCmd>, rect: Rect, color: Color) {
    let cx = rect.x + rect.w * 0.5;
    let cy = rect.y + rect.h * 0.5;
    let w = 5.2;
    let h = 2.6;
    let gap = 1.4;
    let up_cy = cy - (h + gap) * 0.5;
    let dn_cy = cy + (h + gap) * 0.5;
    let stroke = 1.15;
    chevron_arm(cmds, cx, up_cy, w, h, true, color, stroke);
    chevron_arm(cmds, cx, dn_cy, w, h, false, color, stroke);
}

fn chevron_arm(
    cmds: &mut Vec<DrawCmd>,
    cx: f32,
    cy: f32,
    w: f32,
    h: f32,
    up: bool,
    color: Color,
    thickness: f32,
) {
    let peak_y = if up { cy - h * 0.5 } else { cy + h * 0.5 };
    let base_y = if up { cy + h * 0.5 } else { cy - h * 0.5 };
    cmds.push(DrawCmd::Line { a: (cx - w * 0.5, base_y), b: (cx, peak_y), color, thickness });
    cmds.push(DrawCmd::Line { a: (cx, peak_y), b: (cx + w * 0.5, base_y), color, thickness });
}

/// MixLink `StripNameLabel` — diamond + text on the fader bay, bottom hairline.
/// No fill and no grain: MixLink sits this row on the continuous bay metal.
pub struct StripNameStyle {
    pub diamond: bool,
    pub dim: bool,
    pub color: Option<theme::Color>,
}

/// MixLink `StripNameLabel` metrics: 11.5 medium, diamond 6.5, `HStack(spacing: 4)`,
/// horizontal pad 2, then `.frame(maxWidth: .infinity)` so the cluster is centered.
const STRIP_NAME_FONT: f32 = 11.5;
const STRIP_NAME_LINE_H: f32 = 17.0;
const STRIP_NAME_DIA: f32 = 6.5;
const STRIP_NAME_GAP: f32 = 4.0;
const STRIP_NAME_PAD: f32 = 2.0;

/// Positions for a diamond+name (or name-only) cluster centered in `col_w`.
/// Offsets are relative to the strip's left edge.
#[derive(Clone, Copy, Debug)]
struct StripNameLayout {
    diamond_cx: f32,
    text_x: f32,
    text_w: f32,
    group_x: f32,
    group_w: f32,
}

impl StripNameLayout {
    fn new(col_w: f32, text: &str, diamond: bool) -> Self {
        let inner_x = STRIP_NAME_PAD;
        let inner_w = (col_w - STRIP_NAME_PAD * 2.0).max(0.0);
        let icon_w = if diamond { STRIP_NAME_DIA + STRIP_NAME_GAP } else { 0.0 };
        let text_max = (inner_w - icon_w).max(1.0);
        let text_w = approx_ui_advance(text, STRIP_NAME_FONT).min(text_max);
        let group_w = icon_w + text_w;
        let group_x = inner_x + (inner_w - group_w) * 0.5;
        let text_x = group_x + icon_w;
        Self {
            diamond_cx: group_x + STRIP_NAME_DIA * 0.5,
            text_x,
            text_w: (inner_x + inner_w - text_x).max(1.0),
            group_x,
            group_w,
        }
    }
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
        style.color.unwrap_or(theme::PRIMARY_TEXT)
    };
    let gem = if style.dim {
        [theme::SECONDARY_TEXT[0], theme::SECONDARY_TEXT[1], theme::SECONDARY_TEXT[2], 0.32]
    } else {
        style.color.unwrap_or(theme::SECONDARY_TEXT)
    };
    let text_y = rect.y + (rect.h - STRIP_NAME_LINE_H) * 0.5;
    let layout = StripNameLayout::new(rect.w, text, style.diamond);
    if style.diamond {
        hollow_diamond(
            cmds,
            rect.x + layout.diamond_cx,
            rect.y + rect.h * 0.5,
            STRIP_NAME_DIA,
            gem,
        );
        theme::text(
            cmds,
            Rect { x: rect.x + layout.text_x, y: text_y, w: layout.text_w, h: STRIP_NAME_LINE_H },
            text,
            STRIP_NAME_FONT,
            label,
            false,
        );
    } else {
        // MixLink Main: `Text.frame(maxWidth: .infinity)`. Center the name as
        // the same cluster (no diamond) so a wide strip does not left-align it.
        theme::text_center(
            cmds,
            Rect {
                x: rect.x + layout.group_x,
                y: text_y,
                w: layout.group_w.max(1.0),
                h: STRIP_NAME_LINE_H,
            },
            text,
            STRIP_NAME_FONT,
            label,
            false,
        );
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
pub fn text_field_tempo(
    cmds: &mut Vec<DrawCmd>,
    rect: Rect,
    text: &str,
    focused: bool,
    caret: bool,
) {
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
    theme::fill(cmds, rect, [0.0, 0.0, 0.0, 0.22]);
    theme::fill(cmds, Rect { x: rect.x, y: rect.y, w: rect.w, h: 1.0 }, [1.0, 1.0, 1.0, 0.16]);
    theme::fill(
        cmds,
        Rect { x: rect.x, y: rect.y + rect.h - 1.0, w: rect.w, h: 1.0 },
        [0.0, 0.0, 0.0, 0.62],
    );
    theme::fill(cmds, Rect { x: rect.x, y: rect.y, w: 1.0, h: rect.h }, [1.0, 1.0, 1.0, 0.06]);
    theme::fill(
        cmds,
        Rect { x: rect.x + rect.w - 1.0, y: rect.y, w: 1.0, h: rect.h },
        [0.0, 0.0, 0.0, 0.45],
    );
    cmds.push(DrawCmd::RoundedRect {
        rect,
        color: if focused { [1.0, 1.0, 1.0, 0.08] } else { [0.0, 0.0, 0.0, 0.0] },
        radius: 3.0,
    });
    let shown = if caret && focused { format!("{text}|") } else { text.to_string() };
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
        rect: Rect { x: rect.x - 1.0, y: rect.y - 1.0, w: rect.w + 2.0, h: rect.h + 2.0 },
        color: MENU_BORDER,
        radius: MENU_RADIUS + 1.0,
    });
    cmds.push(DrawCmd::RoundedRect { rect, color: MENU_FILL, radius: MENU_RADIUS });

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
                Rect { x: rect.x + 8.0, y, w: MENU_CHECK_W, h: MENU_ITEM_H },
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

/// Width needed to draw labels without clipping, including the check column.
pub fn menu_content_width(items: &[MenuItem]) -> f32 {
    const LABEL_SIZE: f32 = 13.0;
    const SECTION_SIZE: f32 = 11.0;
    const PAD_RIGHT: f32 = 12.0;
    let mut inner = 0.0_f32;
    for item in items {
        if let Some(sec) = item.section.as_deref() {
            inner = inner.max(approx_ui_advance(sec, SECTION_SIZE));
        }
        inner = inner.max(approx_ui_advance(&item.label, LABEL_SIZE));
    }
    MENU_LABEL_X + inner + PAD_RIGHT
}

/// Slightly wide vs SF Pro so a menu is never sized to a short glyph run
/// ("Digifa") when the sidebar field is already ~230pt.
fn approx_ui_advance(s: &str, size: f32) -> f32 {
    s.chars()
        .map(|ch| {
            size * match ch {
                ' ' | '.' | ',' | ':' | ';' | 'i' | 'l' | 'I' | 'j' | 't' | 'f' | '\'' => 0.36,
                'm' | 'M' | 'w' | 'W' => 1.00,
                _ => 0.74,
            }
        })
        .sum()
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
    cmds.push(DrawCmd::RoundedRect { rect, color: [0.045, 0.045, 0.045, 1.0], radius: 2.0 });
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
    let inner =
        Rect { x: rect.x + 2.0, y: rect.y + 3.0, w: Layout::METER_W, h: (rect.h - 6.0).max(1.0) };
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

#[cfg(test)]
mod tests {
    use super::*;

    fn col_mid(col_w: f32) -> f32 {
        STRIP_NAME_PAD + (col_w - STRIP_NAME_PAD * 2.0) * 0.5
    }

    #[test]
    fn document_window_paints_under_the_titlebar() {
        let mut cmds = Vec::new();
        document_window(
            &mut cmds,
            400.0,
            300.0,
            Rect { x: 18.0, y: 36.0, w: 364.0, h: 200.0 },
        );
        let covers_top = cmds.iter().any(|c| match c {
            DrawCmd::VertGradient { rect, .. } => {
                rect.x == 0.0 && rect.y == 0.0 && rect.w >= 399.0
            }
            DrawCmd::Rect { rect, .. } => rect.x == 0.0 && rect.y == 0.0 && rect.w >= 399.0,
            _ => false,
        });
        assert!(covers_top, "document chrome must fill from the top edge");
    }

    #[test]
    fn diamond_and_name_cluster_is_centered() {
        let col = Layout::CHANNEL_WIDTH;
        let layout = StripNameLayout::new(col, "Rytm", true);
        assert!(
            (layout.group_x + layout.group_w * 0.5 - col_mid(col)).abs() < 0.05,
            "cluster mid {} vs column mid {}",
            layout.group_x + layout.group_w * 0.5,
            col_mid(col)
        );
        // Icon then text, 4pt gap — not a diamond on the column centerline.
        assert!(
            (layout.text_x - (layout.diamond_cx + STRIP_NAME_DIA * 0.5) - STRIP_NAME_GAP).abs()
                < 0.05
        );
        assert!(layout.group_x > STRIP_NAME_PAD + 8.0, "short name must not sit on the left pad");
        assert!(layout.diamond_cx < layout.text_x);
    }

    #[test]
    fn main_name_cluster_is_centered_without_diamond() {
        let col = Layout::CHANNEL_WIDTH * Layout::MAIN_FACTOR;
        let layout = StripNameLayout::new(col, "Main", false);
        assert!(
            (layout.group_x + layout.group_w * 0.5 - col_mid(col)).abs() < 0.05,
            "Main mid {} vs column mid {}",
            layout.group_x + layout.group_w * 0.5,
            col_mid(col)
        );
        assert_eq!(layout.text_x, layout.group_x);
        assert!(layout.group_x > STRIP_NAME_PAD + 8.0);
    }

    #[test]
    fn long_name_uses_full_inner_width() {
        let col = Layout::CHANNEL_WIDTH;
        let layout = StripNameLayout::new(col, "EffectRack 2", true);
        let inner = col - STRIP_NAME_PAD * 2.0;
        assert!(layout.group_x >= STRIP_NAME_PAD - 0.05);
        assert!(layout.group_x + layout.group_w <= STRIP_NAME_PAD + inner + 0.05);
        assert!(layout.text_w + STRIP_NAME_DIA + STRIP_NAME_GAP <= inner + 0.05);
    }

    #[test]
    fn strip_name_label_paints_centered_diamond_cluster() {
        let mut cmds = Vec::new();
        let col = Layout::CHANNEL_WIDTH;
        strip_name_label(
            &mut cmds,
            Rect { x: 40.0, y: 10.0, w: col, h: Layout::NAME_ROW },
            "EMU",
            StripNameStyle { diamond: true, dim: false, color: None },
        );
        let layout = StripNameLayout::new(col, "EMU", true);
        let diamond = cmds.iter().find_map(|c| match c {
            DrawCmd::Line { a, .. } => Some(a.0),
            _ => None,
        });
        let text_x = cmds.iter().find_map(|c| match c {
            DrawCmd::Text(t) => Some(t.rect.x),
            _ => None,
        });
        let Some(line_x) = diamond else { panic!("expected diamond strokes") };
        let Some(text_x) = text_x else { panic!("expected name text") };
        assert!((line_x - (40.0 + layout.diamond_cx)).abs() < STRIP_NAME_DIA);
        assert!((text_x - (40.0 + layout.text_x)).abs() < 0.05);
        assert!(text_x > 40.0 + STRIP_NAME_PAD + STRIP_NAME_DIA);
    }
}
