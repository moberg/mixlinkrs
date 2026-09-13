//! Recorder motion graphic in the empty send-stack above returns / Main.

use render::{DrawCmd, Rect};

use crate::theme::{self, Color, Layout};
use crate::widgets;

/// Muted line-art gray — matches the wireframe reel, not a lit white.
const LINE: Color = [1.0, 1.0, 1.0, 0.28];
const LINE_LIVE: Color = [1.0, 1.0, 1.0, 0.42];
const STROKE: f32 = 1.5;
/// Hub diameter as a fraction of the reel.
const HUB: f32 = 0.22;
const REC_DISC: Color = [0.86, 0.20, 0.17, 1.0];
const REC_DISC_LIVE: Color = [0.92, 0.22, 0.18, 1.0];

#[derive(Clone, Debug)]
pub struct DeckView {
    pub recording: bool,
    pub take_number: i32,
    pub take_title: String,
    pub elapsed: f32,
    pub tracks: usize,
    pub sample_rate: u32,
    pub ready: bool,
}

impl Default for DeckView {
    fn default() -> Self {
        Self {
            recording: false,
            take_number: 1,
            take_title: "Take 1".into(),
            elapsed: 0.0,
            tracks: 0,
            sample_rate: 48_000,
            ready: true,
        }
    }
}

pub fn format_elapsed(secs: f32) -> String {
    let s = secs.max(0.0);
    let m = (s / 60.0) as u32;
    format!("{m:02}:{:04.1}", s % 60.0)
}

/// Record button in the deck bay, if the bay is large enough to paint.
pub fn paint(
    cmds: &mut Vec<DrawCmd>,
    bay: Rect,
    view: &DeckView,
) -> Option<(Rect, crate::mixer::MixerExtraHit)> {
    if bay.w < 220.0 || bay.h < 120.0 {
        return None;
    }
    cmds.push(DrawCmd::Layer);
    cmds.push(DrawCmd::Clip { rect: bay });

    let inset = 22.0;
    let lower_h = 92.0;
    let col_w = ((bay.w - inset * 2.0) * 0.5).max(110.0);
    let left_cx = bay.x + inset + col_w * 0.5;
    let right_cx = bay.x + bay.w - inset - col_w * 0.5;
    let reel_d = (bay.h - inset - lower_h - 8.0).min(col_w * 0.78).clamp(88.0, 188.0);
    let reel_cy = bay.y + inset + reel_d * 0.5;
    let left = (left_cx, reel_cy);
    let right = (right_cx, reel_cy);
    let rest = -std::f32::consts::FRAC_PI_2;
    let spin = if view.recording { view.elapsed * 1.35 } else { 0.0 };

    paint_tape_path(cmds, left, right, reel_d * 0.5, view);
    paint_reel(cmds, left.0, left.1, reel_d, rest + spin, view.recording);
    paint_reel(cmds, right.0, right.1, reel_d, rest + spin, view.recording);

    let lower_top = reel_cy + reel_d * 0.5 + 12.0;
    let lower_mid = (lower_top + bay.y + bay.h - 16.0) * 0.5;
    let btn_w = (col_w * 0.72).clamp(108.0, 168.0);
    let btn_h = (lower_h * 0.62).clamp(52.0, 68.0);
    let btn = Rect {
        x: left_cx - btn_w * 0.5,
        y: lower_mid - btn_h * 0.5,
        w: btn_w,
        h: btn_h,
    };
    paint_rec_button(cmds, btn, view.recording);
    paint_readout(cmds, right_cx, lower_mid, (col_w * 0.92).max(160.0), view);

    Some((btn, crate::mixer::MixerExtraHit::Record))
}

fn paint_reel(cmds: &mut Vec<DrawCmd>, cx: f32, cy: f32, d: f32, angle: f32, _live: bool) {
    widgets::send_knob_well(cmds, cx, cy, d);
    widgets::send_knob_body(cmds, cx, cy, d);
    let body_d = (d - 6.0).max(8.0);
    let spoke_in = body_d * 0.12;
    let spoke_out = body_d * 0.42;
    for i in 0..3 {
        let a = angle + i as f32 * std::f32::consts::TAU / 3.0;
        let (s, c) = a.sin_cos();
        cmds.push(DrawCmd::Line {
            a: (cx + c * spoke_in, cy + s * spoke_in),
            b: (cx + c * spoke_out, cy + s * spoke_out),
            color: theme::POINTER,
            thickness: 2.0,
        });
    }
    let hub = body_d * 0.14;
    cmds.push(DrawCmd::RadialDisc {
        cx,
        cy,
        d: hub,
        center: (0.32, 0.28),
        inner: [0.28, 0.28, 0.28, 1.0],
        mid: [0.10, 0.10, 0.10, 1.0],
        outer: [0.04, 0.04, 0.04, 1.0],
    });
}

fn paint_tape_path(
    cmds: &mut Vec<DrawCmd>,
    left: (f32, f32),
    right: (f32, f32),
    r: f32,
    view: &DeckView,
) {
    let line = if view.recording { LINE_LIVE } else { LINE };
    let hub_r = r * HUB;
    let wobble = if view.recording { (view.elapsed * 8.0).sin() * 0.6 } else { 0.0 };
    let a = (left.0 + hub_r * 0.2, left.1 + hub_r);
    let b = (right.0 - hub_r * 0.2, right.1 + hub_r);
    let sag = ((b.0 - a.0) * 0.10).clamp(10.0, 26.0);
    let ctrl = ((a.0 + b.0) * 0.5, a.1 + sag + wobble);
    stroke_quad(cmds, a, ctrl, b, 12, line);
    // Quadratic midpoint sits on the curve, not at the control point.
    let mid = (
        0.25 * a.0 + 0.5 * ctrl.0 + 0.25 * b.0,
        0.25 * a.1 + 0.5 * ctrl.1 + 0.25 * b.1,
    );
    cmds.push(DrawCmd::RoundedRect {
        rect: Rect { x: mid.0 - 11.0, y: mid.1 - 3.0, w: 22.0, h: 6.0 },
        color: line,
        radius: 1.8,
    });
}

fn stroke_quad(
    cmds: &mut Vec<DrawCmd>,
    a: (f32, f32),
    c: (f32, f32),
    b: (f32, f32),
    n: u32,
    color: Color,
) {
    let mut prev = a;
    for i in 1..=n {
        let t = i as f32 / n as f32;
        let u = 1.0 - t;
        let p = (
            u * u * a.0 + 2.0 * u * t * c.0 + t * t * b.0,
            u * u * a.1 + 2.0 * u * t * c.1 + t * t * b.1,
        );
        cmds.push(DrawCmd::Line { a: prev, b: p, color, thickness: STROKE });
        prev = p;
    }
}

fn paint_rec_button(cmds: &mut Vec<DrawCmd>, rect: Rect, recording: bool) {
    let r = 5.0;
    cmds.push(DrawCmd::RoundedRect {
        rect: Rect { x: rect.x - 1.0, y: rect.y + 1.0, w: rect.w + 2.0, h: rect.h + 1.0 },
        color: [0.0, 0.0, 0.0, 0.38],
        radius: r + 0.5,
    });
    cmds.push(DrawCmd::RoundedRect {
        rect,
        color: [0.07, 0.07, 0.075, 1.0],
        radius: r,
    });
    cmds.push(DrawCmd::RoundedRect {
        rect: Rect { x: rect.x + 2.0, y: rect.y + 1.0, w: rect.w - 4.0, h: 1.0 },
        color: [1.0, 1.0, 1.0, 0.05],
        radius: 0.5,
    });
    let cx = rect.x + rect.w * 0.5;
    let cy = rect.y + rect.h * 0.5;
    let accent = if recording { REC_DISC_LIVE } else { REC_DISC };
    let glyph = (rect.h * 0.42).clamp(22.0, 32.0);
    if recording {
        let s = glyph * 0.78;
        cmds.push(DrawCmd::RoundedRect {
            rect: Rect { x: cx - s * 0.5, y: cy - s * 0.5, w: s, h: s },
            color: accent,
            radius: 2.5,
        });
    } else {
        cmds.push(DrawCmd::RoundedRect {
            rect: Rect { x: cx - glyph * 0.5, y: cy - glyph * 0.5, w: glyph, h: glyph },
            color: accent,
            radius: glyph * 0.5,
        });
    }
}

fn paint_readout(cmds: &mut Vec<DrawCmd>, cx: f32, cy: f32, col_w: f32, view: &DeckView) {
    let w = col_w;
    let x = cx - w * 0.5;
    let time = format_elapsed(if view.recording { view.elapsed } else { 0.0 });
    let time_color = if view.recording { REC_DISC_LIVE } else { theme::PRIMARY_TEXT };
    theme::text_center(
        cmds,
        Rect { x, y: cy - 38.0, w, h: 16.0 },
        &view.take_title,
        12.0,
        theme::SECONDARY_TEXT,
        false,
    );
    theme::text_center_mono(cmds, Rect { x, y: cy - 18.0, w, h: 34.0 }, time, 32.0, time_color, false);
    let khz = view.sample_rate as f32 / 1000.0;
    let rate = if khz.fract() == 0.0 {
        format!("{khz:.0} kHz")
    } else {
        format!("{khz:.1} kHz")
    };
    let meta = if view.tracks == 1 {
        format!("1 track armed  •  {rate}")
    } else {
        format!("{} tracks armed  •  {rate}", view.tracks)
    };
    theme::text_center(
        cmds,
        Rect { x, y: cy + 20.0, w, h: 14.0 },
        meta,
        10.0,
        theme::TEXT_DIM,
        false,
    );
}

pub fn recorder_bay(layout: &crate::mixer::MixerLayout, send_count: usize) -> Rect {
    let origin = layout.x + Layout::MIXER_LEADING - layout.scroll_x;
    let n = send_count.max(2).min(6) as f32;
    let fx_x = origin + layout.ch_w * 8.0 + 1.0;
    let w = layout.ch_w * n + 1.0 + layout.ch_w * 2.0 + 1.0 + layout.main_w;
    let h = crate::mixer::send_stack_h(send_count);
    Rect { x: fx_x, y: layout.y, w, h }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mixer::MixerLayout;
    use crate::widgets;

    #[test]
    fn elapsed_formats_minutes_and_tenths() {
        assert_eq!(format_elapsed(0.0), "00:00.0");
        assert_eq!(format_elapsed(12.4), "00:12.4");
        assert_eq!(format_elapsed(75.0), "01:15.0");
    }

    #[test]
    fn bay_sits_in_the_send_stack() {
        let layout = MixerLayout::new(0.0, 40.0, 1800.0, 900.0, 3);
        let bay = recorder_bay(&layout, 3);
        assert!((bay.y - 40.0).abs() < 0.01);
        assert!((bay.h - crate::mixer::send_stack_h(3)).abs() < 0.01);
        assert!(bay.w > 200.0);
    }

    #[test]
    fn record_sits_under_the_left_reel() {
        let layout = MixerLayout::new(0.0, 0.0, 1800.0, 900.0, 3);
        let bay = recorder_bay(&layout, 3);
        let mut cmds = Vec::new();
        let hit = paint(&mut cmds, bay, &DeckView::default());
        let Some((btn, crate::mixer::MixerExtraHit::Record)) = hit else {
            panic!("expected Record hit, bay={bay:?}");
        };
        assert!(widgets::contains(
            Rect { x: bay.x, y: bay.y, w: bay.w, h: bay.h },
            btn.x + btn.w * 0.5,
            btn.y + btn.h * 0.5
        ));
        assert!(
            btn.x + btn.w * 0.5 < bay.x + bay.w * 0.5,
            "record button should sit in the left column"
        );
        assert!(
            btn.y > bay.y + bay.h * 0.35,
            "record button should sit under the reels"
        );
        assert!(btn.w > btn.h, "record control is a horizontal pad");
        assert!(
            cmds.iter().any(|c| matches!(c, DrawCmd::LitDisc { .. })),
            "reels reuse the send-knob well"
        );
        assert!(
            cmds.iter().any(|c| matches!(c, DrawCmd::RadialDisc { .. })),
            "reels reuse the send-knob body"
        );
        let texts: Vec<&str> = cmds
            .iter()
            .filter_map(|c| match c {
                DrawCmd::Text(t) => Some(t.text.as_str()),
                _ => None,
            })
            .collect();
        assert!(texts.iter().any(|t| t.contains("tracks armed")));
        assert!(texts.contains(&"00:00.0"));
        assert!(!texts.iter().any(|t| {
            t.contains("READY")
                || t.contains("Ready")
                || t.contains("TAPE")
                || t.contains("IPS")
                || t.contains("RECORDER")
                || *t == "REC"
                || *t == "STOP"
        }));
    }
}
