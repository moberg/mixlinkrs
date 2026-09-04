use render::{Color as Rgba, DrawCmd, Rect};

pub type Color = Rgba;

pub const WINDOW: Color = [0.10, 0.10, 0.11, 1.0];
pub const TEXT: Color = [0.82, 0.82, 0.84, 1.0];
pub const TEXT_DIM: Color = [0.55, 0.55, 0.58, 1.0];
pub const ORANGE: Color = [1.00, 0.45, 0.12, 1.0];
pub const AMBER: Color = [1.00, 0.62, 0.14, 1.0];
pub const BLUE: Color = [0.22, 0.55, 0.95, 1.0];
pub const METER_GREEN: Color = [0.20, 0.82, 0.32, 1.0];
pub const METER_YELLOW: Color = [0.95, 0.82, 0.18, 1.0];
pub const METER_RED: Color = [0.95, 0.22, 0.18, 1.0];
pub const POINTER: Color = [0xEE as f32 / 255.0, 0xEE as f32 / 255.0, 0xEA as f32 / 255.0, 1.0];
pub const PRIMARY_TEXT: Color = [0xDE as f32 / 255.0, 0xDF as f32 / 255.0, 0xDC as f32 / 255.0, 1.0];
pub const SECONDARY_TEXT: Color = [0xA7 as f32 / 255.0, 0xA9 as f32 / 255.0, 0xA5 as f32 / 255.0, 1.0];
pub const DEEP_SLOT: Color = [0.035, 0.038, 0.035, 1.0];
pub const SEAM_DARK: Color = [0.0, 0.0, 0.0, 0.75];
pub const SEAM_HIGHLIGHT: Color = [1.0, 1.0, 1.0, 0.055];

pub struct Material {
    pub top: Color,
    pub middle: Color,
    pub bottom: Color,
}

pub const UPPER_FACEPLATE: Material = Material {
    top: [0.26, 0.26, 0.26, 1.0],
    middle: [0.20, 0.20, 0.20, 1.0],
    bottom: [0.16, 0.16, 0.16, 1.0],
};
pub const FADER_BAY: Material = Material {
    top: [0x24 as f32 / 255.0, 0x26 as f32 / 255.0, 0x24 as f32 / 255.0, 1.0],
    middle: [0x1B as f32 / 255.0, 0x1D as f32 / 255.0, 0x1B as f32 / 255.0, 1.0],
    bottom: [0x15 as f32 / 255.0, 0x17 as f32 / 255.0, 0x15 as f32 / 255.0, 1.0],
};
pub const SIDEBAR_MAT: Material = Material {
    top: [0.130, 0.135, 0.130, 1.0],
    middle: [0.115, 0.120, 0.115, 1.0],
    bottom: [0.100, 0.105, 0.100, 1.0],
};
pub const RECESSED: Material = Material {
    top: [0.075, 0.078, 0.075, 1.0],
    middle: [0.065, 0.068, 0.065, 1.0],
    bottom: [0.055, 0.058, 0.055, 1.0],
};

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

pub fn fill(cmds: &mut Vec<DrawCmd>, rect: Rect, color: Color) {
    cmds.push(DrawCmd::Rect { rect, color });
}

pub fn material(cmds: &mut Vec<DrawCmd>, rect: Rect, mat: &Material) {
    cmds.push(DrawCmd::VertGradient { rect, top: mat.top, bottom: mat.bottom });
}

/// Subtle grain for chrome only (header / footer / sidebar), not every strip.
pub fn material_grain(cmds: &mut Vec<DrawCmd>, rect: Rect, mat: &Material) {
    material(cmds, rect, mat);
    let mut y = rect.y;
    while y < rect.y + rect.h {
        cmds.push(DrawCmd::Line {
            a: (rect.x, y),
            b: (rect.x + rect.w, y),
            color: [1.0, 1.0, 1.0, 0.018],
            thickness: 0.5,
        });
        y += 3.0;
    }
}

pub fn seam_h(cmds: &mut Vec<DrawCmd>, x: f32, y: f32, w: f32, strong: bool) {
    fill(cmds, Rect { x, y, w, h: 1.0 }, if strong { SEAM_DARK } else { [0.0, 0.0, 0.0, 0.45] });
    fill(cmds, Rect { x, y: y + 1.0, w, h: 1.0 }, SEAM_HIGHLIGHT);
}

pub fn seam_v(cmds: &mut Vec<DrawCmd>, x: f32, y: f32, h: f32, strong: bool) {
    fill(cmds, Rect { x, y, w: 1.0, h }, if strong { SEAM_DARK } else { [0.0, 0.0, 0.0, 0.45] });
    fill(cmds, Rect { x: x + 1.0, y, w: 1.0, h }, SEAM_HIGHLIGHT);
}

pub fn text(cmds: &mut Vec<DrawCmd>, rect: Rect, s: impl Into<String>, size: f32, color: Color, bold: bool) {
    text_clip(cmds, rect, s, size, color, bold, None);
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
    cmds.push(DrawCmd::Text(render::TextCmd {
        rect,
        text: s.into(),
        size,
        color,
        h_align: render::Align::Start,
        v_align: render::Align::Center,
        bold,
        clip,
    }));
}

pub fn text_center(cmds: &mut Vec<DrawCmd>, rect: Rect, s: impl Into<String>, size: f32, color: Color, bold: bool) {
    cmds.push(DrawCmd::Text(render::TextCmd {
        rect,
        text: s.into(),
        size,
        color,
        h_align: render::Align::Center,
        v_align: render::Align::Center,
        bold,
        clip: None,
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
    pub const METER_W: f32 = 6.0;
    pub const METER_HOUSING: f32 = 10.0;
    pub const NAME_ROW: f32 = 42.0;
    pub const BUTTON_H: f32 = 26.0;
    pub const BUTTON_STACK: f32 = 130.0;
    pub const BUTTON_RADIUS: f32 = 1.5;
    pub const KNOB_DRAG_PX: f32 = 180.0;
    pub const METER_SEGMENTS: i32 = 64;
    pub const FOOTER_H: f32 = 44.0;
    pub const MIN_CH: f32 = 88.0;
    pub const MAX_CH: f32 = 106.0;
    pub const MAIN_FACTOR: f32 = 1.16;

    pub fn send_row_h(lane: analog::ReturnLane) -> f32 {
        if lane == analog::ReturnLane::SendA { Self::SEND_ROW_A } else { Self::SEND_ROW }
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
