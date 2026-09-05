//! Recorder motion graphic in the empty send-stack above returns / Main.

use render::{DrawCmd, Rect};

use crate::theme::{self, Color, Layout};

const RING: Color = [1.0, 1.0, 1.0, 0.16];
const RING_LIVE: Color = [1.0, 1.0, 1.0, 0.34];
const SPOKE: Color = [1.0, 1.0, 1.0, 0.14];
const SPOKE_LIVE: Color = [1.0, 1.0, 1.0, 0.28];
const HUB: Color = [0.22, 0.22, 0.23, 1.0];
const TAPE: Color = [1.0, 1.0, 1.0, 0.10];
const TAPE_LIVE: Color = [1.0, 1.0, 1.0, 0.22];

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

    let pad = 28.0;
    let readout_w = 200.0;
    let btn_d = (bay.h * 0.22).clamp(52.0, 64.0);
    let reel_budget = (bay.w - readout_w - btn_d - 96.0).max(160.0);
    let reel_d = (bay.h - 56.0).min(reel_budget * 0.44).clamp(96.0, 168.0);
    let gap = (reel_d * 0.46).clamp(40.0, 72.0);
    let machine_w = reel_d * 2.0 + gap + 28.0 + btn_d + 20.0 + readout_w;
    let origin_x = bay.x + ((bay.w - machine_w) * 0.5).max(pad);
    let cy = bay.y + bay.h * 0.50;

    let left = (origin_x + reel_d * 0.5, cy);
    let right = (left.0 + reel_d + gap, cy);
    let spin = if view.recording { view.elapsed * 1.35 } else { 0.0 };

    paint_tape_path(cmds, left, right, reel_d * 0.5, view);
    paint_head(cmds, (left.0 + right.0) * 0.5, cy + reel_d * 0.18, view.recording);
    paint_reel(cmds, left.0, left.1, reel_d, -spin * 0.85, view.recording);
    paint_reel(cmds, right.0, right.1, reel_d, spin, view.recording);

    let btn = Rect {
        x: right.0 + reel_d * 0.5 + 28.0,
        y: cy - btn_d * 0.5,
        w: btn_d,
        h: btn_d,
    };
    paint_rec_button(cmds, btn, view.recording, view.ready);
    paint_readout(
        cmds,
        btn.x + btn.w + 22.0,
        cy,
        (btn.x + btn.w + 22.0 + readout_w).min(bay.x + bay.w - 20.0),
        view,
    );

    Some((btn, crate::mixer::MixerExtraHit::Record))
}

fn paint_reel(cmds: &mut Vec<DrawCmd>, cx: f32, cy: f32, d: f32, angle: f32, live: bool) {
    let ring = if live { RING_LIVE } else { RING };
    let spoke = if live { SPOKE_LIVE } else { SPOKE };

    cmds.push(DrawCmd::RadialDisc {
        cx,
        cy,
        d,
        center: (0.5, 0.5),
        inner: [0.14, 0.14, 0.145, 0.35],
        mid: [0.12, 0.12, 0.125, 0.22],
        outer: [0.10, 0.10, 0.105, 0.08],
    });
    cmds.push(DrawCmd::LitDisc {
        cx,
        cy,
        d,
        top_leading: ring,
        middle: [0.0, 0.0, 0.0, 0.0],
        bottom_trailing: ring,
    });
    cmds.push(DrawCmd::LitDisc {
        cx,
        cy,
        d: d * 0.58,
        top_leading: [1.0, 1.0, 1.0, if live { 0.10 } else { 0.05 }],
        middle: [0.0, 0.0, 0.0, 0.0],
        bottom_trailing: [1.0, 1.0, 1.0, if live { 0.08 } else { 0.04 }],
    });

    let r_in = d * 0.08;
    let r_out = d * 0.46;
    for i in 0..4 {
        let a = angle + i as f32 * std::f32::consts::FRAC_PI_2;
        let (s, c) = a.sin_cos();
        cmds.push(DrawCmd::Line {
            a: (cx + c * r_in, cy + s * r_in),
            b: (cx + c * r_out, cy + s * r_out),
            color: spoke,
            thickness: 1.25,
        });
    }

    cmds.push(DrawCmd::RadialDisc {
        cx,
        cy,
        d: d * 0.12,
        center: (0.5, 0.5),
        inner: if live { [0.38, 0.38, 0.40, 1.0] } else { HUB },
        mid: HUB,
        outer: [0.10, 0.10, 0.11, 1.0],
    });
}

fn paint_tape_path(
    cmds: &mut Vec<DrawCmd>,
    left: (f32, f32),
    right: (f32, f32),
    r: f32,
    view: &DeckView,
) {
    let wobble = if view.recording { (view.elapsed * 9.0).sin() * 0.8 } else { 0.0 };
    let a = (left.0 + r * 0.78, left.1 + r * 0.22);
    let mid = ((left.0 + right.0) * 0.5, left.1 + r * 0.30 + wobble);
    let b = (right.0 - r * 0.78, right.1 + r * 0.22);
    let color = if view.recording { TAPE_LIVE } else { TAPE };
    cmds.push(DrawCmd::Line { a, b: mid, color, thickness: 1.25 });
    cmds.push(DrawCmd::Line { a: mid, b, color, thickness: 1.25 });
}

fn paint_head(cmds: &mut Vec<DrawCmd>, cx: f32, cy: f32, live: bool) {
    cmds.push(DrawCmd::RoundedRect {
        rect: Rect { x: cx - 9.0, y: cy - 5.0, w: 18.0, h: 3.0 },
        color: if live { [1.0, 1.0, 1.0, 0.28] } else { [1.0, 1.0, 1.0, 0.12] },
        radius: 1.5,
    });
}

fn paint_rec_button(cmds: &mut Vec<DrawCmd>, rect: Rect, recording: bool, ready: bool) {
    let cx = rect.x + rect.w * 0.5;
    let cy = rect.y + rect.h * 0.5;
    let d = rect.w.min(rect.h);
    let accent = if recording {
        theme::METER_RED
    } else if ready {
        [0.72, 0.20, 0.18, 1.0]
    } else {
        [0.28, 0.28, 0.30, 1.0]
    };

    cmds.push(DrawCmd::LitDisc {
        cx,
        cy,
        d: d + 2.0,
        top_leading: [1.0, 1.0, 1.0, 0.08],
        middle: [0.0, 0.0, 0.0, 0.0],
        bottom_trailing: [1.0, 1.0, 1.0, 0.05],
    });
    if recording {
        cmds.push(DrawCmd::LitDisc {
            cx,
            cy,
            d: d + 16.0,
            top_leading: [accent[0], accent[1], accent[2], 0.0],
            middle: [accent[0], accent[1], accent[2], 0.16],
            bottom_trailing: [accent[0], accent[1], accent[2], 0.0],
        });
    }
    cmds.push(DrawCmd::RadialDisc {
        cx,
        cy,
        d,
        center: (0.5, 0.5),
        inner: [0.16, 0.16, 0.17, 1.0],
        mid: [0.13, 0.13, 0.14, 1.0],
        outer: [0.10, 0.10, 0.11, 1.0],
    });

    let glyph = d * 0.36;
    if recording {
        cmds.push(DrawCmd::RoundedRect {
            rect: Rect { x: cx - glyph * 0.38, y: cy - glyph * 0.38, w: glyph * 0.76, h: glyph * 0.76 },
            color: accent,
            radius: 3.0,
        });
    } else {
        cmds.push(DrawCmd::RadialDisc {
            cx,
            cy,
            d: glyph,
            center: (0.5, 0.5),
            inner: accent,
            mid: accent,
            outer: [accent[0] * 0.7, accent[1] * 0.7, accent[2] * 0.7, 1.0],
        });
    }
}

fn paint_readout(cmds: &mut Vec<DrawCmd>, x: f32, cy: f32, right: f32, view: &DeckView) {
    let w = (right - x).max(80.0);
    let time = format_elapsed(if view.recording { view.elapsed } else { 0.0 });
    theme::text(cmds, Rect { x, y: cy - 44.0, w, h: 18.0 }, &view.take_title, 15.0, theme::TEXT, false);
    theme::text_mono(
        cmds,
        Rect { x, y: cy - 20.0, w, h: 32.0 },
        time,
        28.0,
        if view.recording { theme::METER_RED } else { theme::PRIMARY_TEXT },
        false,
    );
    let khz = view.sample_rate as f32 / 1000.0;
    let rate = if khz.fract() == 0.0 {
        format!("{khz:.0} kHz")
    } else {
        format!("{khz:.1} kHz")
    };
    let meta = if view.tracks == 1 {
        format!("1 track  ·  {rate}")
    } else {
        format!("{} tracks  ·  {rate}", view.tracks)
    };
    theme::text(cmds, Rect { x, y: cy + 16.0, w, h: 14.0 }, meta, 11.0, theme::SECONDARY_TEXT, false);
    let (status, color) = if view.recording {
        ("Recording", theme::METER_RED)
    } else if !view.ready {
        ("Set a projects folder", theme::TEXT_DIM)
    } else if view.tracks == 0 {
        ("No tracks armed", theme::TEXT_DIM)
    } else {
        ("Ready", theme::SECONDARY_TEXT)
    };
    theme::text(cmds, Rect { x, y: cy + 32.0, w, h: 14.0 }, status, 11.0, color, false);
}

pub fn recorder_bay(layout: &crate::mixer::MixerLayout, send_count: usize) -> Rect {
    let origin = layout.x + Layout::MIXER_LEADING - layout.scroll_x;
    let n = send_count.max(2).min(6) as f32;
    let fx_x = origin + layout.ch_w * 8.0 + 1.0;
    let w = layout.ch_w * n + 1.0 + layout.ch_w * 2.0 + 1.0 + layout.main_w;
    let h = (crate::mixer::pan_name_bar_y(layout, send_count) - layout.y).max(0.0);
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
    fn bay_sits_above_the_pan_title_row() {
        let layout = MixerLayout::new(0.0, 40.0, 1800.0, 900.0, 3);
        let bay = recorder_bay(&layout, 3);
        let bar = crate::mixer::pan_name_bar_y(&layout, 3);
        assert!((bay.y - 40.0).abs() < 0.01);
        assert!((bay.y + bay.h - bar).abs() < 0.01);
        assert!(bay.w > 200.0);
    }

    #[test]
    fn paints_a_record_button_in_a_roomy_bay() {
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
        assert!(cmds.iter().any(|c| matches!(c, DrawCmd::RadialDisc { .. })));
        let texts: Vec<&str> = cmds
            .iter()
            .filter_map(|c| match c {
                DrawCmd::Text(t) => Some(t.text.as_str()),
                _ => None,
            })
            .collect();
        assert!(!texts.iter().any(|t| t.contains("TAPE") || t.contains("IPS")));
    }
}
