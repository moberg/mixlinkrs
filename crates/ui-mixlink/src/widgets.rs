//! Shared MixLink-look widgets. All chrome uses these so Settings/sidebar/mixer match.

use render::{DrawCmd, Rect};

use crate::theme::{self, Color, Layout};

pub fn hardware_pad(cmds: &mut Vec<DrawCmd>, rect: Rect, title: &str, on: bool, accent: Color) {
    let bg = if on {
        [accent[0], accent[1], accent[2], 0.46]
    } else {
        theme::RECESSED.middle
    };
    cmds.push(DrawCmd::RoundedRect { rect, color: bg, radius: Layout::BUTTON_RADIUS });
    theme::fill(cmds, Rect { x: rect.x, y: rect.y, w: rect.w, h: 1.0 }, [1.0, 1.0, 1.0, 0.06]);
    theme::fill(
        cmds,
        Rect { x: rect.x, y: rect.y + rect.h - 1.0, w: rect.w, h: 1.0 },
        [0.0, 0.0, 0.0, 0.55],
    );
    theme::text_center(cmds, rect, title, 11.0, theme::TEXT, true);
}

pub fn icon_pad(cmds: &mut Vec<DrawCmd>, rect: Rect, glyph: &str, enabled: bool) {
    let bg = if enabled { theme::RECESSED.middle } else { [0.04, 0.04, 0.04, 1.0] };
    cmds.push(DrawCmd::RoundedRect { rect, color: bg, radius: Layout::BUTTON_RADIUS });
    theme::text_center(
        cmds,
        rect,
        glyph,
        12.0,
        if enabled { theme::TEXT } else { theme::TEXT_DIM },
        true,
    );
}

/// Metal knob + colored pointer. Optional value label under the disc.
pub fn knob(cmds: &mut Vec<DrawCmd>, x: f32, y: f32, d: f32, value: f32, ring: Color, label: Option<&str>) {
    let cx = x + d * 0.5;
    let cy = y + d * 0.5;
    cmds.push(DrawCmd::RoundedRect {
        rect: Rect { x: x - 2.0, y: y - 2.0, w: d + 4.0, h: d + 4.0 },
        color: [0.04, 0.04, 0.04, 1.0],
        radius: (d + 4.0) * 0.5,
    });
    cmds.push(DrawCmd::RoundedRect {
        rect: Rect { x, y, w: d, h: d },
        color: [0.22, 0.22, 0.22, 1.0],
        radius: d * 0.5,
    });
    cmds.push(DrawCmd::RoundedRect {
        rect: Rect { x: x + d * 0.08, y: y + d * 0.08, w: d * 0.84, h: d * 0.84 },
        color: [0.10, 0.10, 0.10, 1.0],
        radius: d * 0.42,
    });
    paint_arc(cmds, cx, cy, d * 0.42, value, ring);
    let t = (0.75 + value.clamp(0.0, 1.0) * 1.5) * std::f32::consts::PI;
    let r = d * 0.34;
    cmds.push(DrawCmd::Line {
        a: (cx, cy),
        b: (cx + t.cos() * r, cy + t.sin() * r),
        color: theme::POINTER,
        thickness: 2.0,
    });
    if let Some(label) = label {
        theme::text_center(
            cmds,
            Rect { x, y: y + d + 2.0, w: d, h: 12.0 },
            label,
            8.5,
            theme::SECONDARY_TEXT,
            false,
        );
    }
}

fn paint_arc(cmds: &mut Vec<DrawCmd>, cx: f32, cy: f32, r: f32, value: f32, color: Color) {
    let start = 0.75 * std::f32::consts::PI;
    let end = start + value.clamp(0.0, 1.0) * 1.5 * std::f32::consts::PI;
    let steps = ((value * 28.0).ceil() as i32).max(1);
    let mut a = start;
    let da = (end - start) / steps as f32;
    for _ in 0..steps {
        let b = a + da;
        cmds.push(DrawCmd::Line {
            a: (cx + a.cos() * r, cy + a.sin() * r),
            b: (cx + b.cos() * r, cy + b.sin() * r),
            color,
            thickness: 2.2,
        });
        a = b;
    }
}

pub fn checkbox(cmds: &mut Vec<DrawCmd>, x: f32, y: f32, on: bool, label: &str) {
    hardware_pad(cmds, Rect { x, y, w: 14.0, h: 14.0 }, if on { "✓" } else { "" }, on, theme::METER_RED);
    theme::text(cmds, Rect { x: x + 18.0, y: y - 1.0, w: 110.0, h: 16.0 }, label, 9.0, theme::TEXT_DIM, false);
}

pub fn menu_label(cmds: &mut Vec<DrawCmd>, rect: Rect, text: &str) {
    theme::text(cmds, Rect { x: rect.x, y: rect.y, w: 10.0, h: rect.h }, "◆", 7.0, theme::SECONDARY_TEXT, false);
    theme::text(
        cmds,
        Rect { x: rect.x + 12.0, y: rect.y, w: rect.w - 14.0, h: rect.h },
        text,
        11.5,
        theme::PRIMARY_TEXT,
        false,
    );
    theme::fill(
        cmds,
        Rect { x: rect.x + 6.0, y: rect.y + rect.h - 1.0, w: rect.w - 12.0, h: 1.0 },
        theme::SEAM_DARK,
    );
}

pub fn text_field(cmds: &mut Vec<DrawCmd>, rect: Rect, text: &str, focused: bool, caret: bool) {
    theme::fill(cmds, rect, theme::RECESSED.middle);
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
    theme::text(
        cmds,
        Rect { x: rect.x + 6.0, y: rect.y, w: rect.w - 12.0, h: rect.h },
        shown,
        12.0,
        theme::TEXT,
        false,
    );
}

#[derive(Clone, Debug)]
pub struct MenuItem {
    pub id: String,
    pub label: String,
    pub checked: bool,
    pub section: Option<String>,
}

pub fn popup_menu(cmds: &mut Vec<DrawCmd>, rect: Rect, items: &[MenuItem], hover: Option<usize>) {
    theme::fill(cmds, Rect { x: rect.x + 2.0, y: rect.y + 3.0, w: rect.w, h: rect.h }, [0.0, 0.0, 0.0, 0.45]);
    theme::fill(cmds, rect, [0.12, 0.125, 0.12, 1.0]);
    theme::seam_h(cmds, rect.x, rect.y, rect.w, true);
    let mut y = rect.y + 4.0;
    let mut last_section: Option<&str> = None;
    for (i, item) in items.iter().enumerate() {
        if let Some(sec) = item.section.as_deref() {
            if last_section != Some(sec) {
                theme::text(
                    cmds,
                    Rect { x: rect.x + 8.0, y, w: rect.w - 16.0, h: 16.0 },
                    sec,
                    9.0,
                    theme::TEXT_DIM,
                    true,
                );
                y += 16.0;
                last_section = Some(sec);
            }
        }
        if hover == Some(i) {
            theme::fill(cmds, Rect { x: rect.x + 2.0, y, w: rect.w - 4.0, h: 22.0 }, [1.0, 1.0, 1.0, 0.08]);
        }
        let mark = if item.checked { "✓  " } else { "    " };
        theme::text(
            cmds,
            Rect { x: rect.x + 8.0, y, w: rect.w - 16.0, h: 22.0 },
            format!("{mark}{}", item.label),
            12.0,
            theme::TEXT,
            item.checked,
        );
        y += 22.0;
    }
}

pub fn item_at(rect: Rect, items: &[MenuItem], y: f32) -> Option<usize> {
    let mut yy = rect.y + 4.0;
    let mut last_section: Option<&str> = None;
    for (i, item) in items.iter().enumerate() {
        if let Some(sec) = item.section.as_deref() {
            if last_section != Some(sec) {
                yy += 16.0;
                last_section = Some(sec);
            }
        }
        if y >= yy && y < yy + 22.0 {
            return Some(i);
        }
        yy += 22.0;
    }
    None
}

pub fn menu_height(items: &[MenuItem]) -> f32 {
    let mut h = 8.0;
    let mut last_section: Option<&str> = None;
    for item in items {
        if let Some(sec) = item.section.as_deref() {
            if last_section != Some(sec) {
                h += 16.0;
                last_section = Some(sec);
            }
        }
        h += 22.0;
    }
    h
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
                theme::text(cmds, Rect { x: x + 9.0, y: y - 6.0, w: 22.0, h: 12.0 }, label, 8.0, [1.0, 1.0, 1.0, 0.56], false);
            }
        }
    }
}
