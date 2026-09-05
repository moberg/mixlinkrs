use render::{Align, Color as Rgba, DrawCmd, Rect};

pub type Color = Rgba;

pub const WINDOW: Color = [0.10, 0.10, 0.11, 1.0];
pub const TEXT: Color = [0.82, 0.82, 0.84, 1.0];
pub const TEXT_DIM: Color = [0.55, 0.55, 0.58, 1.0];
pub const ORANGE: Color = [1.00, 0.45, 0.12, 1.0];
pub const AMBER: Color = [1.00, 0.62, 0.14, 1.0];
pub const BLUE: Color = [0.22, 0.55, 0.95, 1.0];
/// Mix / take browser selection (sampled from MixLink Take 7 highlight).
pub const BROWSER_SELECTED: Color = [42.0 / 255.0, 70.0 / 255.0, 120.0 / 255.0, 1.0];
pub const METER_GREEN: Color = [0.20, 0.82, 0.32, 1.0];
pub const METER_YELLOW: Color = [0.95, 0.82, 0.18, 1.0];
pub const METER_RED: Color = [0.95, 0.22, 0.18, 1.0];
pub const POINTER: Color = [0xEE as f32 / 255.0, 0xEE as f32 / 255.0, 0xEA as f32 / 255.0, 1.0];
pub const PRIMARY_TEXT: Color =
    [0xDE as f32 / 255.0, 0xDF as f32 / 255.0, 0xDC as f32 / 255.0, 1.0];
pub const SECONDARY_TEXT: Color =
    [0xA7 as f32 / 255.0, 0xA9 as f32 / 255.0, 0xA5 as f32 / 255.0, 1.0];
pub const DEEP_SLOT: Color = [0.035, 0.038, 0.035, 1.0];
pub const SEAM_DARK: Color = [0.0, 0.0, 0.0, 0.75];
pub const SEAM_HIGHLIGHT: Color = [1.0, 1.0, 1.0, 0.055];

pub struct Material {
    pub top: Color,
    pub middle: Color,
    pub bottom: Color,
    pub grain_light: f32,
    pub grain_dark: f32,
    pub top_highlight: f32,
    pub bottom_shadow: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceStyle {
    UpperFaceplate,
    FaderBay,
    Sidebar,
    Recessed,
}

/// MixLink `Color.white.opacity` composites in gamma sRGB. wgpu blends the same
/// alpha in linear, which reads as a gray wash — keep fills as MixLink sRGB and
/// scale only white overlay alphas (~⅓ of MixLink for broad washes).
pub const UPPER_FACEPLATE: Material = Material {
    top: [0.26, 0.26, 0.26, 1.0],
    middle: [0.20, 0.20, 0.20, 1.0],
    bottom: [0.16, 0.16, 0.16, 1.0],
    grain_light: 0.007, // MixLink 0.020
    grain_dark: 0.014,
    top_highlight: 0.016, // MixLink 0.055
    bottom_shadow: 0.08,
};
pub const FADER_BAY: Material = Material {
    top: [0x24 as f32 / 255.0, 0x26 as f32 / 255.0, 0x24 as f32 / 255.0, 1.0],
    middle: [0x1B as f32 / 255.0, 0x1D as f32 / 255.0, 0x1B as f32 / 255.0, 1.0],
    bottom: [0x15 as f32 / 255.0, 0x17 as f32 / 255.0, 0x15 as f32 / 255.0, 1.0],
    grain_light: 0.006, // MixLink 0.017
    grain_dark: 0.024,
    top_highlight: 0.008, // MixLink 0.035
    bottom_shadow: 0.55,
};
pub const SIDEBAR_MAT: Material = Material {
    top: [0.130, 0.135, 0.130, 1.0],
    middle: [0.115, 0.120, 0.115, 1.0],
    bottom: [0.100, 0.105, 0.100, 1.0],
    grain_light: 0.005, // MixLink 0.014
    grain_dark: 0.016,
    top_highlight: 0.010, // MixLink 0.04
    bottom_shadow: 0.50,
};
pub const RECESSED: Material = Material {
    top: [0.075, 0.078, 0.075, 1.0],
    middle: [0.065, 0.068, 0.065, 1.0],
    bottom: [0.055, 0.058, 0.055, 1.0],
    grain_light: 0.004, // MixLink 0.010
    grain_dark: 0.014,
    top_highlight: 0.006, // MixLink 0.02
    bottom_shadow: 0.62,
};

pub fn material_for(style: SurfaceStyle) -> &'static Material {
    match style {
        SurfaceStyle::UpperFaceplate => &UPPER_FACEPLATE,
        SurfaceStyle::FaderBay => &FADER_BAY,
        SurfaceStyle::Sidebar => &SIDEBAR_MAT,
        SurfaceStyle::Recessed => &RECESSED,
    }
}

pub fn send_color(lane: analog::ReturnLane) -> Color {
    match lane {
        analog::ReturnLane::SendA => [0.22, 0.78, 0.28, 1.0],
        analog::ReturnLane::SendB => [0.92, 0.48, 0.14, 1.0],
        analog::ReturnLane::SendC => [0.18, 0.62, 0.88, 1.0],
        analog::ReturnLane::SendD => [0.78, 0.38, 0.95, 1.0],
        analog::ReturnLane::SendE => [0.25, 0.85, 0.82, 1.0],
        analog::ReturnLane::SendF => PRIMARY_TEXT,
        analog::ReturnLane::Bus1 | analog::ReturnLane::Bus2 => SECONDARY_TEXT,
    }
}

/// Arrangement / mixer lane family: channels, returns, buses, main.
pub fn lane_kind_color(lane: analog::MixLane) -> Color {
    match lane {
        analog::MixLane::Strip(_) => [0.52, 0.76, 0.98, 1.0],
        analog::MixLane::ReturnLane(lane) if lane.is_send() => [0.38, 0.86, 0.50, 1.0],
        analog::MixLane::ReturnLane(_) => [0.98, 0.74, 0.28, 1.0],
        analog::MixLane::Main => TEXT,
    }
}

pub fn lane_kind_header(lane: analog::MixLane, selected: bool) -> Color {
    let [r, g, b, _] = lane_kind_color(lane);
    if selected {
        [r * 0.22 + 0.10, g * 0.16 + 0.08, b * 0.12 + 0.06, 1.0]
    } else {
        [r * 0.10 + 0.11, g * 0.08 + 0.11, b * 0.06 + 0.11, 1.0]
    }
}

pub fn clip_color_for_lane(take: i32, lane: analog::MixLane) -> Color {
    let take = clip_color(take);
    let kind = lane_kind_color(lane);
    [
        take[0] * 0.55 + kind[0] * 0.45,
        take[1] * 0.55 + kind[1] * 0.45,
        take[2] * 0.55 + kind[2] * 0.45,
        1.0,
    ]
}

pub fn fill(cmds: &mut Vec<DrawCmd>, rect: Rect, color: Color) {
    cmds.push(DrawCmd::Rect { rect, color });
}

/// MixLink `HardwareSurface` — one chassis panel, not a per-strip fill.
pub fn hardware_surface(cmds: &mut Vec<DrawCmd>, rect: Rect, style: SurfaceStyle) {
    if rect.w <= 0.0 || rect.h <= 0.0 {
        return;
    }
    if style == SurfaceStyle::UpperFaceplate {
        upper_faceplate(cmds, rect);
    } else {
        stacked_surface(cmds, rect, material_for(style), style == SurfaceStyle::FaderBay);
    }
}

/// MixLink `MixerChassis`: brushed upper faceplate, seam, fader-bay metal + inset.
pub fn mixer_chassis(cmds: &mut Vec<DrawCmd>, rect: Rect, upper_height: f32) {
    let upper_h = upper_height.clamp(0.0, rect.h);
    hardware_surface(
        cmds,
        Rect { x: rect.x, y: rect.y, w: rect.w, h: upper_h },
        SurfaceStyle::UpperFaceplate,
    );
    faceplate_seam(cmds, rect.x, rect.y + upper_h, rect.w);
    let bay = Rect {
        x: rect.x,
        y: rect.y + upper_h + 2.0,
        w: rect.w,
        h: (rect.h - upper_h - 2.0).max(0.0),
    };
    hardware_surface(cmds, bay, SurfaceStyle::FaderBay);
    fader_bay_inset_shadow(cmds, bay);
}

/// MixLink `MixerUpperPanelBackground`. White stop alphas are lower than MixLink
/// (0.048 / 0.022 / 0.02) so linear blending does not lift the 0.20 fill to gray.
fn upper_faceplate(cmds: &mut Vec<DrawCmd>, rect: Rect) {
    fill(cmds, rect, [0.20, 0.20, 0.20, 1.0]);
    vert_stops(
        cmds,
        rect,
        &[
            (0.00, [1.0, 1.0, 1.0, 0.016]),
            (0.22, [1.0, 1.0, 1.0, 0.007]),
            (0.52, [0.0, 0.0, 0.0, 0.0]),
            (0.78, [0.0, 0.0, 0.0, 0.035]),
            (1.00, [0.0, 0.0, 0.0, 0.07]),
        ],
    );
    horz_stops(
        cmds,
        rect,
        &[
            (0.00, [0.0, 0.0, 0.0, 0.04]),
            (0.20, [0.0, 0.0, 0.0, 0.0]),
            (0.50, [1.0, 1.0, 1.0, 0.007]),
            (0.80, [0.0, 0.0, 0.0, 0.0]),
            (1.00, [0.0, 0.0, 0.0, 0.04]),
        ],
    );
    brushed_texture(cmds, rect);
}

/// MixLink `HardwareSurface.stackedSurface`.
fn stacked_surface(cmds: &mut Vec<DrawCmd>, rect: Rect, mat: &Material, fader_bay: bool) {
    let mid_h = rect.h * 0.45;
    cmds.push(DrawCmd::VertGradient {
        rect: Rect { x: rect.x, y: rect.y, w: rect.w, h: mid_h },
        top: mat.top,
        bottom: mat.middle,
    });
    cmds.push(DrawCmd::VertGradient {
        rect: Rect { x: rect.x, y: rect.y + mid_h, w: rect.w, h: rect.h - mid_h },
        top: mat.middle,
        bottom: mat.bottom,
    });
    stacked_grain(cmds, rect, mat);
    if fader_bay {
        fader_bay_metal(cmds, rect);
    }
    let half = rect.h * 0.5;
    cmds.push(DrawCmd::VertGradient {
        rect: Rect { x: rect.x, y: rect.y, w: rect.w, h: half },
        top: [1.0, 1.0, 1.0, mat.top_highlight],
        bottom: [1.0, 1.0, 1.0, 0.0],
    });
    cmds.push(DrawCmd::VertGradient {
        rect: Rect { x: rect.x, y: rect.y + half, w: rect.w, h: rect.h - half },
        top: [0.0, 0.0, 0.0, 0.0],
        bottom: [0.0, 0.0, 0.0, mat.bottom_shadow * 0.22],
    });
    // MixLink diagonal is white 0.018 at topLeading only. A full-width 0.010
    // white wash is extra heat in linear; keep a faint top sheen + the darken.
    cmds.push(DrawCmd::VertGradient {
        rect,
        top: [1.0, 1.0, 1.0, 0.003],
        bottom: [0.0, 0.0, 0.0, 0.0],
    });
    cmds.push(DrawCmd::HorzGradient {
        rect,
        left: [0.0, 0.0, 0.0, 0.0],
        right: [0.0, 0.0, 0.0, 0.06],
    });
}

/// MixLink `MixerUpperPanelBackground.brushedTexture` — 1.15 pt sin-hash grain.
fn brushed_texture(cmds: &mut Vec<DrawCmd>, rect: Rect) {
    let mut local = 0.0f32;
    while local < rect.h {
        let f = grain_fraction(local);
        let use_light = f > 0.5;
        let opacity = if use_light { 0.003 + f * 0.003 } else { 0.008 + f * 0.012 };
        let color = if use_light { [1.0, 1.0, 1.0, opacity] } else { [0.0, 0.0, 0.0, opacity] };
        cmds.push(DrawCmd::Line {
            a: (rect.x, rect.y + local),
            b: (rect.x + rect.w, rect.y + local),
            color,
            thickness: 0.5,
        });
        local += 1.15;
    }
}

/// MixLink stacked-surface grain — 1.5 pt spacing, light and dark.
fn stacked_grain(cmds: &mut Vec<DrawCmd>, rect: Rect, mat: &Material) {
    let mut local = 0.5f32;
    while local < rect.h {
        let f = grain_fraction(local);
        let use_light = f > 0.48;
        let opacity = if use_light {
            mat.grain_light * (0.5 + f * 0.5)
        } else {
            mat.grain_dark * (0.5 + f * 0.5)
        };
        let color = if use_light { [1.0, 1.0, 1.0, opacity] } else { [0.0, 0.0, 0.0, opacity] };
        cmds.push(DrawCmd::Line {
            a: (rect.x, rect.y + local),
            b: (rect.x + rect.w, rect.y + local),
            color,
            thickness: 0.5,
        });
        local += 1.5;
    }
}

/// MixLink `FaderBayMetalTexture`.
fn fader_bay_metal(cmds: &mut Vec<DrawCmd>, rect: Rect) {
    let mut local = 1.0f32;
    while local < rect.h {
        let wave = (local as f64 * 0.173).sin().abs() as f32;
        let fine = (local as f64 * 2.417).sin().abs() as f32;
        let dark = wave > 0.965;
        let (opacity, thickness, step) = if dark {
            (0.030 + fine * 0.018, 0.7, 2.2)
        } else {
            (0.002 + fine * 0.003, 0.35, 1.15)
        };
        let color = if dark { [0.0, 0.0, 0.0, opacity] } else { [1.0, 1.0, 1.0, opacity] };
        cmds.push(DrawCmd::Line {
            a: (rect.x, rect.y + local),
            b: (rect.x + rect.w, rect.y + local),
            color,
            thickness,
        });
        local += step;
    }
    let band_count = (rect.h / 38.0).max(1.0) as i32;
    for index in 0..band_count {
        let seed = (index + 3) as f64;
        let center = index as f32 * 38.0 + (seed * 4.731).sin().abs() as f32 * 24.0;
        let h = 4.0 + (seed * 1.913).sin().abs() as f32 * 7.0;
        let band = Rect { x: rect.x, y: rect.y + center, w: rect.w, h };
        cmds.push(DrawCmd::VertGradient {
            rect: Rect { x: band.x, y: band.y, w: band.w, h: band.h * 0.5 },
            top: [0.0, 0.0, 0.0, 0.0],
            bottom: [0.0, 0.0, 0.0, 0.012],
        });
        cmds.push(DrawCmd::VertGradient {
            rect: Rect { x: band.x, y: band.y + band.h * 0.5, w: band.w, h: band.h * 0.5 },
            top: [0.0, 0.0, 0.0, 0.012],
            bottom: [0.0, 0.0, 0.0, 0.0],
        });
    }
}

fn fader_bay_inset_shadow(cmds: &mut Vec<DrawCmd>, bay: Rect) {
    let h = Layout::FADER_BAY_INSET.min(bay.h);
    if h <= 0.0 {
        return;
    }
    cmds.push(DrawCmd::VertGradient {
        rect: Rect { x: bay.x, y: bay.y, w: bay.w, h: h * 0.45 },
        top: [0.0, 0.0, 0.0, 0.28],
        bottom: [0.0, 0.0, 0.0, 0.08],
    });
    cmds.push(DrawCmd::VertGradient {
        rect: Rect { x: bay.x, y: bay.y + h * 0.45, w: bay.w, h: h * 0.55 },
        top: [0.0, 0.0, 0.0, 0.08],
        bottom: [0.0, 0.0, 0.0, 0.0],
    });
}

/// MixLink `sin(y * 12.9898) * 43758.5453` hash. `y` is local to the surface.
fn grain_fraction(y: f32) -> f32 {
    let n = (y as f64 * 12.9898).sin() * 43758.5453;
    (n - n.floor()) as f32
}

fn vert_stops(cmds: &mut Vec<DrawCmd>, rect: Rect, stops: &[(f32, Color)]) {
    for pair in stops.windows(2) {
        let (t0, c0) = pair[0];
        let (t1, c1) = pair[1];
        let y0 = rect.y + rect.h * t0;
        let y1 = rect.y + rect.h * t1;
        cmds.push(DrawCmd::VertGradient {
            rect: Rect { x: rect.x, y: y0, w: rect.w, h: (y1 - y0).max(0.5) },
            top: c0,
            bottom: c1,
        });
    }
}

fn horz_stops(cmds: &mut Vec<DrawCmd>, rect: Rect, stops: &[(f32, Color)]) {
    for pair in stops.windows(2) {
        let (t0, c0) = pair[0];
        let (t1, c1) = pair[1];
        let x0 = rect.x + rect.w * t0;
        let x1 = rect.x + rect.w * t1;
        cmds.push(DrawCmd::HorzGradient {
            rect: Rect { x: x0, y: rect.y, w: (x1 - x0).max(0.5), h: rect.h },
            left: c0,
            right: c1,
        });
    }
}

pub fn material(cmds: &mut Vec<DrawCmd>, rect: Rect, mat: &Material) {
    stacked_surface(cmds, rect, mat, false);
}

pub fn material_grain(cmds: &mut Vec<DrawCmd>, rect: Rect, _mat: &Material) {
    hardware_surface(cmds, rect, SurfaceStyle::Sidebar);
}

pub fn seam_h(cmds: &mut Vec<DrawCmd>, x: f32, y: f32, w: f32, strong: bool) {
    fill(
        cmds,
        Rect { x, y, w, h: 1.0 },
        if strong { [0.07, 0.07, 0.07, 1.0] } else { [0.09, 0.09, 0.09, 1.0] },
    );
    fill(
        cmds,
        Rect { x, y: y + 1.0, w, h: 1.0 },
        [1.0, 1.0, 1.0, if strong { 0.055 } else { 0.035 }],
    );
}

pub fn seam_v(cmds: &mut Vec<DrawCmd>, x: f32, y: f32, h: f32, strong: bool) {
    channel_seam(cmds, x, y, h, strong);
}

/// MixLink `FaceplateSeam`.
pub fn faceplate_seam(cmds: &mut Vec<DrawCmd>, x: f32, y: f32, w: f32) {
    fill(cmds, Rect { x, y, w, h: 1.0 }, SEAM_DARK);
    fill(cmds, Rect { x, y: y + 1.0, w, h: 1.0 }, SEAM_HIGHLIGHT);
}

/// MixLink `ChannelSeam`: 2 pt falloff + two 1 pt lines.
pub fn channel_seam(cmds: &mut Vec<DrawCmd>, x: f32, y: f32, h: f32, strong: bool) {
    cmds.push(DrawCmd::HorzGradient {
        rect: Rect { x: x - 2.0, y, w: 2.0, h },
        left: [0.0, 0.0, 0.0, 0.0],
        right: [0.0, 0.0, 0.0, if strong { 0.07 } else { 0.04 }],
    });
    fill(
        cmds,
        Rect { x, y, w: 1.0, h },
        if strong { [0.055, 0.055, 0.055, 1.0] } else { [0.07, 0.07, 0.07, 1.0] },
    );
    fill(
        cmds,
        Rect { x: x + 1.0, y, w: 1.0, h },
        if strong { [0.045, 0.045, 0.045, 1.0] } else { [0.06, 0.06, 0.06, 1.0] },
    );
}

/// MixLink `UpperFaceplateCellChrome` on a send or pan cell.
pub fn faceplate_cell(cmds: &mut Vec<DrawCmd>, rect: Rect, last_send: bool, pan: bool) {
    horz_stops(
        cmds,
        rect,
        &[
            (0.00, [0.0, 0.0, 0.0, 0.035]),
            (0.18, [0.0, 0.0, 0.0, 0.0]),
            (0.50, [1.0, 1.0, 1.0, 0.008]),
            (0.82, [0.0, 0.0, 0.0, 0.0]),
            (1.00, [0.0, 0.0, 0.0, 0.04]),
        ],
    );
    if pan {
        fill(cmds, Rect { x: rect.x, y: rect.y, w: rect.w, h: 1.0 }, [1.0, 1.0, 1.0, 0.025]);
        cmds.push(DrawCmd::VertGradient {
            rect: Rect { x: rect.x, y: rect.y + 1.0, w: rect.w, h: 3.0 },
            top: [0.0, 0.0, 0.0, 0.018],
            bottom: [0.0, 0.0, 0.0, 0.0],
        });
        cmds.push(DrawCmd::VertGradient {
            rect: Rect { x: rect.x, y: rect.y + rect.h - 5.0, w: rect.w, h: 5.0 },
            top: [0.0, 0.0, 0.0, 0.0],
            bottom: [0.0, 0.0, 0.0, 0.05],
        });
    } else {
        cmds.push(DrawCmd::VertGradient {
            rect: Rect { x: rect.x, y: rect.y + rect.h - 5.0, w: rect.w, h: 4.0 },
            top: [0.0, 0.0, 0.0, 0.0],
            bottom: [0.0, 0.0, 0.0, if last_send { 0.025 } else { 0.02 }],
        });
        if last_send {
            fill(
                cmds,
                Rect { x: rect.x, y: rect.y + rect.h - 1.0, w: rect.w, h: 1.0 },
                [0.0, 0.0, 0.0, 0.30],
            );
        } else {
            seam_h(cmds, rect.x, rect.y + rect.h - 2.0, rect.w, false);
        }
    }
}

/// MixLink `ChannelBayShading`: soft left/right black falloff on fader + name + pads.
/// MixLink's extra white stop at 0.10 is omitted — `horz_stops` would start there
/// and draw a 1px highlight left of the meter.
pub fn channel_bay_shading(cmds: &mut Vec<DrawCmd>, rect: Rect) {
    horz_stops(
        cmds,
        rect,
        &[
            (0.00, [0.0, 0.0, 0.0, 0.11]),
            (0.09, [0.0, 0.0, 0.0, 0.0]),
            (0.87, [0.0, 0.0, 0.0, 0.0]),
            (1.00, [0.0, 0.0, 0.0, 0.13]),
        ],
    );
}

pub fn text(
    cmds: &mut Vec<DrawCmd>,
    rect: Rect,
    s: impl Into<String>,
    size: f32,
    color: Color,
    bold: bool,
) {
    emit_text(cmds, rect, s, size, color, bold, false, Align::Start, None);
}

pub fn text_mono(
    cmds: &mut Vec<DrawCmd>,
    rect: Rect,
    s: impl Into<String>,
    size: f32,
    color: Color,
    bold: bool,
) {
    emit_text(cmds, rect, s, size, color, bold, true, Align::Start, None);
}

pub fn text_clip(
    cmds: &mut Vec<DrawCmd>,
    rect: Rect,
    s: impl Into<String>,
    size: f32,
    color: Color,
    bold: bool,
    clip: Option<Rect>,
) {
    emit_text(cmds, rect, s, size, color, bold, false, Align::Start, clip);
}

pub fn text_center(
    cmds: &mut Vec<DrawCmd>,
    rect: Rect,
    s: impl Into<String>,
    size: f32,
    color: Color,
    bold: bool,
) {
    emit_text(cmds, rect, s, size, color, bold, false, Align::Center, None);
}

pub fn text_center_mono(
    cmds: &mut Vec<DrawCmd>,
    rect: Rect,
    s: impl Into<String>,
    size: f32,
    color: Color,
    bold: bool,
) {
    emit_text(cmds, rect, s, size, color, bold, true, Align::Center, None);
}

fn emit_text(
    cmds: &mut Vec<DrawCmd>,
    rect: Rect,
    s: impl Into<String>,
    size: f32,
    color: Color,
    bold: bool,
    monospaced: bool,
    h_align: Align,
    clip: Option<Rect>,
) {
    cmds.push(DrawCmd::Text(render::TextCmd {
        rect,
        text: s.into(),
        size,
        color,
        h_align,
        v_align: Align::Center,
        bold,
        monospaced,
        clip,
    }));
}

pub struct Layout;

impl Layout {
    pub const CHANNEL_WIDTH: f32 = 100.0;
    pub const MAIN_CHANNEL_WIDTH: f32 = 116.0;
    pub const MIXER_LEADING: f32 = 10.0;
    pub const LEGEND_OVERLAY: f32 = 112.0;
    pub const SIDEBAR_WIDTH: f32 = 248.0;
    pub const GROUP_HEADER: f32 = 58.0;
    pub const ENABLE_ROW: f32 = 20.0;
    pub const ENABLE_W: f32 = 16.0;
    pub const ENABLE_H: f32 = 10.0;
    pub const SEND_KNOB: f32 = 48.0;
    pub const PAN_KNOB: f32 = 40.0;
    pub const SEND_ROW_A: f32 = 90.0;
    pub const SEND_ROW: f32 = 92.0;
    pub const SEND_NAME_BAR: f32 = 18.0;
    pub const PAN_ROW: f32 = 80.0;
    pub const FADER_TROUGH: f32 = 14.0;
    pub const FADER_SLOT: f32 = 7.5;
    pub const FADER_CAP_W: f32 = 30.0;
    pub const FADER_CAP_H: f32 = 48.0;
    /// MixLink `DecibelScaleView` leading column (ticks only).
    pub const SCALE_LEADING: f32 = 12.0;
    /// MixLink `Layout.scaleWidth` trailing column (ticks + numeric labels).
    pub const SCALE_WIDTH: f32 = 28.0;
    pub const FADER_BAY_PAD_X: f32 = 2.0;
    pub const FADER_BAY_PAD_Y: f32 = 8.0;
    /// MixLink `FaderView` hit width: `faderCapWidth + 12`.
    pub const FADER_HIT_W: f32 = 42.0;
    pub const METER_W: f32 = 6.0;
    pub const METER_HOUSING: f32 = 10.0;
    pub const NAME_ROW: f32 = 42.0;
    pub const BUTTON_H: f32 = 26.0;
    /// MixLink `HardwareButton.Style.compact` height is `buttonHeight + 1`.
    pub const COMPACT_BUTTON_H: f32 = 27.0;
    /// MixLink `Layout.headerButtonSize` / `HardwareIconButton`.
    pub const HEADER_BUTTON: f32 = 22.0;
    /// MixLink `HardwareModuleModifier` `.padding(7)`.
    pub const MODULE_PAD: f32 = 7.0;
    /// Gap between the two pads on a shared row (Solo/Mute, Bus 1/Bus 2).
    pub const BUTTON_PAIR_GAP: f32 = 4.0;
    /// Vertical gap between the Solo/Mute row and the Bus row.
    pub const BUTTON_ROW_GAP: f32 = 5.0;
    /// Two shared rows + bottom pad (was four stacked full-width pads).
    pub const BUTTON_STACK: f32 = 65.0;
    pub const BUTTON_RADIUS: f32 = 1.5;
    pub const KNOB_DRAG_PX: f32 = 180.0;
    pub const METER_SEGMENTS: i32 = 64;
    pub const FOOTER_H: f32 = 44.0;
    pub const MIN_CH: f32 = 88.0;
    pub const MAX_CH: f32 = 106.0;
    pub const MAIN_FACTOR: f32 = 1.16;
    pub const FACEPLATE_SEAM: f32 = 2.0;
    pub const FADER_BAY_INSET: f32 = 10.0;

    pub fn send_row_h(lane: analog::ReturnLane) -> f32 {
        if lane == analog::ReturnLane::SendA {
            Self::SEND_ROW_A
        } else {
            Self::SEND_ROW
        }
    }

    pub fn send_lane_h(lane: analog::ReturnLane) -> f32 {
        Self::SEND_NAME_BAR + Self::send_row_h(lane)
    }

    pub fn upper_faceplate_height(sends: &[analog::ReturnLane]) -> f32 {
        Self::GROUP_HEADER
            + Self::ENABLE_ROW
            + sends.iter().map(|lane| Self::send_lane_h(*lane)).sum::<f32>()
            + Self::PAN_ROW
            + Self::SEND_NAME_BAR
    }

    pub fn channel_width(available: f32, n_channels: f32, n_main: f32) -> f32 {
        let w = available / (n_channels + n_main * Self::MAIN_FACTOR);
        w.clamp(Self::MIN_CH, Self::MAX_CH)
    }
}

pub fn clip_color(take: i32) -> Color {
    const HUES: [f32; 6] = [0.08, 0.18, 0.33, 0.55, 0.62, 0.78];
    let h = HUES[(take.rem_euclid(6)) as usize];
    hsv(h, 0.45, 0.42)
}

fn hsv(h: f32, s: f32, v: f32) -> Color {
    let i = (h * 6.0).floor();
    let f = h * 6.0 - i;
    let p = v * (1.0 - s);
    let q = v * (1.0 - f * s);
    let t = v * (1.0 - (1.0 - f) * s);
    let (r, g, b) = match (i as i32) % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    [r, g, b, 1.0]
}

pub fn fader_value_text(lin: f32) -> String {
    if lin <= 0.001 {
        return "−∞".into();
    }
    let db = osc::fader_db(lin);
    if db <= -120.0 {
        return "−∞".into();
    }
    let rounded = db.round() as i32;
    if rounded == 0 {
        "0".into()
    } else if rounded > 0 {
        format!("+{rounded}")
    } else {
        format!("{rounded}")
    }
}

pub fn pan_text(unit: f32) -> String {
    let n = ((unit - 0.5) * 200.0).round() as i32;
    if n == 0 {
        "C".into()
    } else if n < 0 {
        format!("L{}", -n)
    } else {
        format!("R{n}")
    }
}

#[cfg(test)]
mod tests {
    use analog::{MixLane, ReturnLane};

    use super::*;

    #[test]
    fn lane_kinds_use_distinct_colors() {
        let ch = lane_kind_color(MixLane::Strip(0));
        let ret = lane_kind_color(MixLane::ReturnLane(ReturnLane::SendA));
        let bus = lane_kind_color(MixLane::ReturnLane(ReturnLane::Bus1));
        let main = lane_kind_color(MixLane::Main);
        assert_ne!(ch, ret);
        assert_ne!(ch, bus);
        assert_ne!(ret, bus);
        assert_ne!(ch, main);
        assert_eq!(ch, lane_kind_color(MixLane::Strip(7)));
        assert_eq!(ret, lane_kind_color(MixLane::ReturnLane(ReturnLane::SendC)));
        assert_eq!(bus, lane_kind_color(MixLane::ReturnLane(ReturnLane::Bus2)));
    }
}
