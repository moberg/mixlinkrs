//! Mix-page mixer: same strip chrome as the Record mixer (pan, fader bay, name, pads).

use analog::AnalogEngine;
use analog::ALL_SEND_LANES;
use project::{MixLane, MixTrack, KNOB_COUNT};
use render::{DrawCmd, Rect};

use crate::mixer::{self, FaderBay};
use crate::theme::{self, Layout};
use crate::widgets::{self, KnobKind};

/// Footer control in the mix browser when the mixer is hidden.
pub const HANDLE_H: f32 = 24.0;
/// Record-like fader travel so names, ticks, and caps stay readable.
const MIN_FADER_BAY: f32 = 200.0;
const PAD_BOTTOM: f32 = 8.0;
pub const HEIGHT: f32 = Layout::SEND_NAME_BAR
    + Layout::PAN_ROW
    + MIN_FADER_BAY
    + Layout::NAME_ROW
    + Layout::BUTTON_H
    + PAD_BOTTOM;
pub const HEIGHT_KNOBS: f32 = HEIGHT + Layout::SEND_NAME_BAR + 80.0;

const KNOB_D: f32 = 28.0;
const KNOB_ROW_H: f32 = 40.0;
const KNOB_SECTION_H: f32 = Layout::SEND_NAME_BAR + KNOB_ROW_H * 2.0;

pub struct MixMixerView<'a> {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub tracks: &'a [MixTrack],
    pub selected_lane: Option<MixLane>,
    pub show_knobs: bool,
    pub visible: bool,
    pub peaks: &'a [f32],
    pub control_room_fader: f32,
    pub control_room_peak: f32,
    pub engine: &'a AnalogEngine,
}

#[derive(Clone, Copy, Debug)]
pub struct MixStripGeom {
    pub knobs: [Rect; KNOB_COUNT],
    pub pan: Rect,
    pub fader_hit: Rect,
    pub rail_top: f32,
    pub rail_bot: f32,
    pub solo: Rect,
    pub mute: Rect,
}

pub fn channel_width(mixer_w: f32, track_count: usize) -> f32 {
    ((mixer_w - 8.0) / track_count.max(1) as f32).clamp(Layout::MIN_CH, Layout::MAX_CH)
}

pub fn strip_geom(
    x0: f32,
    y0: f32,
    h: f32,
    ch_w: f32,
    index: usize,
    show_knobs: bool,
) -> MixStripGeom {
    let x = x0 + 4.0 + index as f32 * ch_w;
    let mut y = y0;
    let mut knobs = [Rect { x: 0.0, y: 0.0, w: 0.0, h: 0.0 }; KNOB_COUNT];
    if show_knobs {
        y += Layout::SEND_NAME_BAR;
        let cell_w = ch_w / 3.0;
        for k in 0..KNOB_COUNT {
            let col = (k % 3) as f32;
            let row = (k / 3) as f32;
            knobs[k] = Rect {
                x: x + col * cell_w + (cell_w - KNOB_D) * 0.5,
                y: y + row * KNOB_ROW_H + (KNOB_ROW_H - KNOB_D) * 0.5,
                w: KNOB_D,
                h: KNOB_D,
            };
        }
        y += KNOB_ROW_H * 2.0;
    }
    y += Layout::SEND_NAME_BAR;
    let pan = mixer::pan_knob_rect(x, y, ch_w);
    y += Layout::PAN_ROW;
    let bay_h = (y0 + h - y - Layout::NAME_ROW - Layout::BUTTON_H - PAD_BOTTOM).max(MIN_FADER_BAY);
    let bay = FaderBay::layout(x, y, ch_w, bay_h);
    let pad_y = y + bay_h + Layout::NAME_ROW;
    let (solo, mute) = mixer::button_pair_rects(x, pad_y, ch_w);
    MixStripGeom {
        knobs,
        pan,
        fader_hit: bay.hit,
        rail_top: bay.rail_top,
        rail_bot: bay.rail_bot,
        solo,
        mute,
    }
}

pub fn paint(view: &MixMixerView<'_>) -> Vec<DrawCmd> {
    if !view.visible {
        return Vec::new();
    }
    let mut cmds = Vec::new();
    let upper = if view.show_knobs {
        KNOB_SECTION_H + Layout::SEND_NAME_BAR + Layout::PAN_ROW
    } else {
        Layout::SEND_NAME_BAR + Layout::PAN_ROW
    };
    theme::mixer_chassis(&mut cmds, Rect { x: view.x, y: view.y, w: view.w, h: view.h }, upper);
    let tracks: &[MixTrack] = view.tracks;
    let n = tracks.len() + 1;
    let ch_w = channel_width(view.w, n);
    let mut x = view.x + 4.0;
    for (i, track) in tracks.iter().enumerate() {
        let selected = view.selected_lane == Some(track.lane);
        paint_track(&mut cmds, view, track, x, ch_w, selected, i);
        x += ch_w;
    }
    if tracks.iter().all(|t| t.lane != MixLane::Main) {
        paint_bus_strip(&mut cmds, view, x, ch_w, "M", "Main", 0.0, 0.0);
        x += ch_w;
    }
    paint_bus_strip(
        &mut cmds,
        view,
        x,
        ch_w,
        "CR",
        "Control Room",
        view.control_room_fader,
        view.control_room_peak,
    );
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
    let geom = strip_geom(view.x, view.y, view.h, w, index, view.show_knobs);
    let mut y = view.y;
    if selected {
        theme::fill(
            cmds,
            Rect { x, y, w, h: 2.0 },
            [theme::ORANGE[0], theme::ORANGE[1], theme::ORANGE[2], 0.85],
        );
    }

    if view.show_knobs {
        if index == 0 {
            theme::text(
                cmds,
                Rect { x: x + 6.0, y, w: w - 8.0, h: Layout::SEND_NAME_BAR },
                "KNOBS",
                9.0,
                theme::SECONDARY_TEXT,
                false,
            );
        }
        y += Layout::SEND_NAME_BAR;
        for row in 0..2 {
            let last = row == 1;
            theme::faceplate_cell(cmds, Rect { x, y, w, h: KNOB_ROW_H }, last, false);
            y += KNOB_ROW_H;
        }
        for (k, rect) in geom.knobs.iter().enumerate() {
            let v = track.knobs.get(k).copied().unwrap_or(0.0);
            let color = ALL_SEND_LANES
                .get(k)
                .map(|lane| theme::send_color(*lane))
                .unwrap_or(theme::SECONDARY_TEXT);
            widgets::knob(cmds, rect.x, rect.y, rect.w, v, KnobKind::Send(color), None);
        }
    }

    if index == 0 {
        theme::text(
            cmds,
            Rect { x: x + 6.0, y, w: w - 8.0, h: Layout::SEND_NAME_BAR },
            "PAN",
            9.0,
            theme::SECONDARY_TEXT,
            false,
        );
    }
    y += Layout::SEND_NAME_BAR;
    theme::faceplate_cell(cmds, Rect { x, y, w, h: Layout::PAN_ROW }, false, true);
    if track.lane != MixLane::Main {
        widgets::knob(
            cmds,
            geom.pan.x,
            geom.pan.y,
            Layout::PAN_KNOB,
            track.pan,
            KnobKind::Pan,
            None,
        );
    }
    let kind_color = theme::lane_kind_color(track.lane.into());
    theme::text_center(
        cmds,
        Rect { x, y: y + Layout::PAN_ROW - 18.0, w, h: 16.0 },
        track.lane.short_title(),
        11.0,
        kind_color,
        true,
    );
    y += Layout::PAN_ROW;

    let bay_h =
        (view.y + view.h - y - Layout::NAME_ROW - Layout::BUTTON_H - PAD_BOTTOM).max(MIN_FADER_BAY);
    theme::channel_bay_shading(
        cmds,
        Rect { x, y, w, h: bay_h + Layout::NAME_ROW + Layout::BUTTON_H + PAD_BOTTOM },
    );
    mixer::paint_fader(
        cmds,
        x,
        y,
        w,
        bay_h,
        track.fader,
        view.peaks.get(index).copied().unwrap_or(0.0),
        view.engine.config.hardware_strips,
    );
    y += bay_h;

    widgets::strip_name_label(
        cmds,
        Rect { x, y, w, h: Layout::NAME_ROW },
        &project::strip_adat_channel(&track.name),
        widgets::StripNameStyle {
            diamond: false,
            dim: track.is_unused_template_strip(),
            color: Some(kind_color),
        },
    );

    if track.lane != MixLane::Main {
        widgets::hardware_pad(cmds, geom.solo, "SOLO", track.solo, theme::METER_GREEN);
        widgets::hardware_pad(cmds, geom.mute, "MUTE", track.mute, theme::METER_RED);
    }

    theme::channel_seam(cmds, x + w - 1.0, view.y, view.h, false);
}

fn paint_bus_strip(
    cmds: &mut Vec<DrawCmd>,
    view: &MixMixerView<'_>,
    x: f32,
    w: f32,
    short: &str,
    name: &str,
    fader: f32,
    peak: f32,
) {
    let mut y = view.y;
    if view.show_knobs {
        y += KNOB_SECTION_H;
    }
    y += Layout::SEND_NAME_BAR;
    theme::faceplate_cell(cmds, Rect { x, y, w, h: Layout::PAN_ROW }, false, true);
    theme::text_center(
        cmds,
        Rect { x, y: y + Layout::PAN_ROW - 18.0, w, h: 16.0 },
        short,
        11.0,
        theme::SECONDARY_TEXT,
        true,
    );
    y += Layout::PAN_ROW;
    let bay_h =
        (view.y + view.h - y - Layout::NAME_ROW - Layout::BUTTON_H - PAD_BOTTOM).max(MIN_FADER_BAY);
    theme::channel_bay_shading(
        cmds,
        Rect { x, y, w, h: bay_h + Layout::NAME_ROW + Layout::BUTTON_H + PAD_BOTTOM },
    );
    mixer::paint_fader(cmds, x, y, w, bay_h, fader, peak, view.engine.config.hardware_strips);
    y += bay_h;
    widgets::strip_name_label(
        cmds,
        Rect { x, y, w, h: Layout::NAME_ROW },
        name,
        widgets::StripNameStyle { diamond: false, dim: false, color: None },
    );
    theme::channel_seam(cmds, x + w - 1.0, view.y, view.h, false);
}

/// Footer pad spanning the mix browser; diamond points toward the mixer.
pub fn paint_collapsed_handle(x: f32, y: f32, w: f32) -> Vec<DrawCmd> {
    let mut cmds = Vec::new();
    theme::seam_h(&mut cmds, x, y, w, true);
    // Leave the browser's trailing seam (1 px) so the pad meets the divider.
    widgets::hardware_pad(
        &mut cmds,
        Rect { x, y: y + 2.0, w: (w - 1.0).max(0.0), h: HANDLE_H - 4.0 },
        "Mixer  ▶",
        false,
        theme::PRIMARY_TEXT,
    );
    cmds
}

pub fn height(show_knobs: bool, visible: bool) -> f32 {
    if !visible {
        0.0
    } else if show_knobs {
        HEIGHT_KNOBS
    } else {
        HEIGHT
    }
}
