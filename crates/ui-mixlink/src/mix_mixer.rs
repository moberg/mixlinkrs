//! Mix-page mixer: channel tracks + pinned Main. 220 pt, or 320 pt with knobs.

use analog::AnalogEngine;
use project::{MixDocument, MixLane, MixTrack, KNOB_COUNT};
use render::{DrawCmd, Rect};

use crate::theme::{self, Layout};

pub const HEIGHT: f32 = 220.0;
pub const HEIGHT_KNOBS: f32 = 320.0;

pub struct MixMixerView<'a> {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub mix: Option<&'a MixDocument>,
    pub selected_lane: Option<MixLane>,
    pub show_knobs: bool,
    pub peaks: &'a [f32],
    pub engine: &'a AnalogEngine,
}

pub fn paint(view: &MixMixerView<'_>) -> Vec<DrawCmd> {
    let mut cmds = Vec::new();
    theme::hardware_surface(
        &mut cmds,
        Rect { x: view.x, y: view.y, w: view.w, h: view.h },
        theme::SurfaceStyle::FaderBay,
    );
    let tracks: &[MixTrack] = view.mix.map(|m| m.tracks.as_slice()).unwrap_or(&[]);
    let n = tracks.len().max(1) as f32;
    let ch_w = ((view.w - 8.0) / n).clamp(88.0, 106.0);
    let mut x = view.x + 4.0;
    for (i, track) in tracks.iter().enumerate() {
        let selected = view.selected_lane == Some(track.lane);
        paint_track(&mut cmds, view, track, x, ch_w, selected, i);
        x += ch_w;
    }
    if tracks.iter().all(|t| t.lane != MixLane::Main) {
        paint_main_placeholder(&mut cmds, view, x, ch_w * Layout::MAIN_FACTOR);
    }
    cmds
}

fn paint_track(
    cmds: &mut Vec<DrawCmd>,
    view: &MixMixerView<'_>,
    track: &MixTrack,
    x: f32,
    w: f32,
    selected: bool,
    index: usize,
) {
    if selected {
        theme::fill(
            cmds,
            Rect { x, y: view.y, w, h: view.h },
            [theme::ORANGE[0], theme::ORANGE[1], theme::ORANGE[2], 0.12],
        );
    }
    let mut y = view.y + 8.0;
    if view.show_knobs {
        for k in 0..KNOB_COUNT {
            let v = track.knobs.get(k).copied().unwrap_or(0.0);
            theme::fill(cmds, Rect { x: x + w * 0.5 - 10.0, y, w: 20.0, h: 14.0 }, theme::RECESSED.middle);
            theme::text_center(cmds, Rect { x, y, w, h: 14.0 }, format!("{:.0}", v * 100.0), 8.0, theme::TEXT_DIM, false);
            y += 16.0;
        }
    }
    theme::text_center(
        cmds,
        Rect { x, y, w, h: 16.0 },
        track.name.as_str(),
        9.0,
        if selected { theme::TEXT } else { theme::TEXT_DIM },
        true,
    );
    y += 18.0;
    crate::widgets::knob(
        cmds,
        x + w * 0.5 - 14.0,
        y,
        28.0,
        track.pan,
        crate::widgets::KnobKind::Pan,
        None,
    );
    y += 36.0;
    let peak = view.peaks.get(index).copied().unwrap_or(0.0);
    let meter_h = (view.h - (y - view.y) - 36.0).max(40.0);
    theme::fill(cmds, Rect { x: x + 6.0, y, w: 6.0, h: meter_h }, [0.0, 0.0, 0.0, 0.45]);
    let lit = meter_h * peak;
    theme::fill(
        cmds,
        Rect { x: x + 6.0, y: y + meter_h - lit, w: 6.0, h: lit },
        if peak > 0.95 { theme::METER_RED } else { theme::METER_GREEN },
    );
    let cap_y = y + meter_h * (1.0 - track.fader) - Layout::FADER_CAP_H * 0.5;
    let dest = Rect {
        x: x + w * 0.5 - Layout::FADER_CAP_W * 0.5,
        y: cap_y.clamp(y, y + meter_h - Layout::FADER_CAP_H),
        w: Layout::FADER_CAP_W,
        h: Layout::FADER_CAP_H,
    };
    crate::widgets::fader_cap(cmds, dest);
    crate::widgets::hardware_pad(
        cmds,
        Rect { x: x + 6.0, y: view.y + view.h - 28.0, w: w * 0.5 - 8.0, h: 22.0 },
        "M",
        track.mute,
        theme::METER_RED,
    );
    crate::widgets::hardware_pad(
        cmds,
        Rect { x: x + w * 0.5 + 2.0, y: view.y + view.h - 28.0, w: w * 0.5 - 8.0, h: 22.0 },
        "S",
        track.solo,
        theme::AMBER,
    );
    let _ = view.engine;
}

fn paint_main_placeholder(cmds: &mut Vec<DrawCmd>, view: &MixMixerView<'_>, x: f32, w: f32) {
    theme::text_center(cmds, Rect { x, y: view.y + 8.0, w, h: 16.0 }, "M", 11.0, theme::TEXT, true);
}

pub fn height(show_knobs: bool) -> f32 {
    if show_knobs {
        HEIGHT_KNOBS
    } else {
        HEIGHT
    }
}
