use analog::ReturnLane;

use crate::arrangement::{self, ArrangementLayout, HEADER_W, RULER_H, TIME_RULER_H, TRACK_H};
use crate::mixer::{self, MixerLayout, StripKind};
use crate::theme::Layout;
use crate::widgets;

#[derive(Clone, Copy, Debug)]
pub enum Hit {
    Fader { kind: StripKind, rail_top: f32, rail_bot: f32 },
    Knob { kind: StripKind, lane: Option<ReturnLane>, start_value: f32 },
    Pad { kind: StripKind, which: Pad },
    Enable { kind: StripKind },
    Name { kind: StripKind },
    Clip { lane: project::MixLane, id: uuid::Uuid },
    ClipEdge { id: uuid::Uuid, left: bool },
    ClipFade { id: uuid::Uuid, left: bool },
    ClipLoop { id: uuid::Uuid },
    ClipSlip { id: uuid::Uuid },
    Lane { track: usize },
    Ruler { viewport_anchored: bool },
    TimeRuler,
    StartMarker,
    Locate,
    MixFader { track: usize, rail_top: f32, rail_bot: f32 },
    MixControlRoomFader { rail_top: f32, rail_bot: f32 },
    MixPan { track: usize },
    MixMute { track: usize },
    MixSolo { track: usize },
    MixKnob { track: usize, knob: usize },
    MixMixerHandle,
}

#[derive(Clone, Copy, Debug)]
pub enum Pad {
    Solo,
    Mute,
    Bus1,
    Bus2,
}

pub fn hit_mixer(layout: &MixerLayout, send_count: usize, x: f32, y: f32) -> Option<Hit> {
    let kind = mixer::strip_at(layout, send_count, x, y)?;
    if y < layout.y + Layout::GROUP_HEADER {
        return None;
    }
    let y0 = layout.y + Layout::GROUP_HEADER;
    if y < y0 + Layout::ENABLE_ROW {
        return Some(Hit::Enable { kind });
    }
    let (sx, sw) = mixer::strip_frame(layout, send_count, kind);
    let mut yy = y0 + Layout::ENABLE_ROW;
    let n = send_count.max(2).min(6);
    for i in 0..n {
        let lane = analog::ALL_SEND_LANES[i];
        let row_h = Layout::send_row_h(lane);
        let lane_h = Layout::SEND_NAME_BAR + row_h;
        if y >= yy && y < yy + lane_h {
            let row_y = yy + Layout::SEND_NAME_BAR;
            if matches!(kind, StripKind::Input(_)) {
                let disc = mixer::send_knob_rect(sx, row_y, sw, row_h);
                if widgets::contains(mixer::knob_hit_rect(disc), x, y) {
                    return Some(Hit::Knob { kind, lane: Some(lane), start_value: 0.0 });
                }
            }
            return None;
        }
        yy += lane_h;
    }
    let pan_lane_h = Layout::SEND_NAME_BAR + Layout::PAN_ROW;
    if y >= yy && y < yy + pan_lane_h {
        if !matches!(kind, StripKind::Main) {
            let disc = mixer::pan_knob_rect(sx, yy + Layout::SEND_NAME_BAR, sw);
            if widgets::contains(mixer::knob_hit_rect(disc), x, y) {
                return Some(Hit::Knob { kind, lane: None, start_value: 0.0 });
            }
        }
        return None;
    }
    let (bay_y, bay_h) = mixer::fader_bay_frame(layout, send_count);
    if y >= bay_y && y < bay_y + bay_h {
        let bay = mixer::FaderBay::layout(sx, bay_y, sw, bay_h);
        if widgets::contains(bay.hit, x, y) {
            return Some(Hit::Fader { kind, rail_top: bay.rail_top, rail_bot: bay.rail_bot });
        }
        return None;
    }
    yy = bay_y + bay_h;
    if y >= yy && y < yy + Layout::NAME_ROW {
        return Some(Hit::Name { kind });
    }
    yy += Layout::NAME_ROW;
    let row1 = yy + Layout::BUTTON_H + Layout::BUTTON_ROW_GAP;
    if y >= yy && y < yy + Layout::BUTTON_H {
        let (solo, mute) = mixer::button_pair_rects(sx, yy, sw);
        if widgets::contains(solo, x, y) {
            return Some(Hit::Pad { kind, which: Pad::Solo });
        }
        if widgets::contains(mute, x, y) {
            return Some(Hit::Pad { kind, which: Pad::Mute });
        }
        return None;
    }
    if matches!(kind, StripKind::Input(_)) && y >= row1 && y < row1 + Layout::BUTTON_H {
        let (bus1, bus2) = mixer::button_pair_rects(sx, row1, sw);
        if widgets::contains(bus1, x, y) {
            return Some(Hit::Pad { kind, which: Pad::Bus1 });
        }
        if widgets::contains(bus2, x, y) {
            return Some(Hit::Pad { kind, which: Pad::Bus2 });
        }
        return None;
    }
    None
}

#[cfg(test)]
mod tests {
    use analog::ReturnLane;

    use super::*;
    use crate::mixer::{self, MixerLayout, StripKind};

    fn name_y(layout: &MixerLayout, send_count: usize) -> f32 {
        let (bay_y, bay_h) = mixer::fader_bay_frame(layout, send_count);
        bay_y + bay_h + Layout::NAME_ROW * 0.5
    }

    #[test]
    fn bus_and_send_name_rows_hit_name() {
        let layout = MixerLayout::new(0.0, 0.0, 1600.0, 900.0, 2);
        let y = name_y(&layout, 2);
        for kind in [
            StripKind::Return(ReturnLane::SendA),
            StripKind::Return(ReturnLane::Bus1),
            StripKind::Return(ReturnLane::Bus2),
        ] {
            let (sx, sw) = mixer::strip_frame(&layout, 2, kind);
            match hit_mixer(&layout, 2, sx + sw * 0.5, y) {
                Some(Hit::Name { kind: got }) => assert!(matches!(
                    (kind, got),
                    (StripKind::Return(a), StripKind::Return(b)) if a == b
                )),
                other => panic!("{kind:?} name row should be Hit::Name, got {other:?}"),
            }
        }
    }

    #[test]
    fn paired_pads_hit_left_and_right() {
        let layout = MixerLayout::new(0.0, 0.0, 1600.0, 900.0, 2);
        let (sx, sw) = mixer::strip_frame(&layout, 2, StripKind::Input(0));
        let (bay_y, bay_h) = mixer::fader_bay_frame(&layout, 2);
        let stack_y = bay_y + bay_h + Layout::NAME_ROW;
        let (solo, mute) = mixer::button_pair_rects(sx, stack_y, sw);
        match hit_mixer(&layout, 2, solo.x + solo.w * 0.5, solo.y + 8.0) {
            Some(Hit::Pad { kind: StripKind::Input(0), which: Pad::Solo }) => {}
            other => panic!("solo pad, got {other:?}"),
        }
        match hit_mixer(&layout, 2, mute.x + mute.w * 0.5, mute.y + 8.0) {
            Some(Hit::Pad { kind: StripKind::Input(0), which: Pad::Mute }) => {}
            other => panic!("mute pad, got {other:?}"),
        }
        let (bus1, bus2) =
            mixer::button_pair_rects(sx, stack_y + Layout::BUTTON_H + Layout::BUTTON_ROW_GAP, sw);
        match hit_mixer(&layout, 2, bus1.x + bus1.w * 0.5, bus1.y + 8.0) {
            Some(Hit::Pad { kind: StripKind::Input(0), which: Pad::Bus1 }) => {}
            other => panic!("bus1 pad, got {other:?}"),
        }
        match hit_mixer(&layout, 2, bus2.x + bus2.w * 0.5, bus2.y + 8.0) {
            Some(Hit::Pad { kind: StripKind::Input(0), which: Pad::Bus2 }) => {}
            other => panic!("bus2 pad, got {other:?}"),
        }
    }

    #[test]
    fn main_name_row_hits_name_but_is_not_a_menu_kind() {
        let layout = MixerLayout::new(0.0, 0.0, 1600.0, 900.0, 2);
        let (sx, sw) = mixer::strip_frame(&layout, 2, StripKind::Main);
        let y = name_y(&layout, 2);
        match hit_mixer(&layout, 2, sx + sw * 0.5, y) {
            Some(Hit::Name { kind: StripKind::Main }) => {}
            other => panic!("Main name row should be Hit::Name(Main), got {other:?}"),
        }
    }

    #[test]
    fn start_badge_hits_marker_not_ruler() {
        let layout = ArrangementLayout {
            x: 148.0,
            y: 40.0,
            w: 800.0,
            h: 400.0,
            scroll_x: 0.0,
            scroll_y: 0.0,
            pixels_per_bar: 48.0,
        };
        let origin = 0;
        let mx = arrangement::x_of_frame(&layout, origin, 120.0, 48_000.0);
        match hit_arrangement(&layout, &[], false, origin, 120.0, 48_000.0, mx, layout.y + 8.0) {
            Some(Hit::StartMarker) => {}
            other => panic!("START badge, got {other:?}"),
        }
        match hit_arrangement(
            &layout,
            &[],
            false,
            origin,
            120.0,
            48_000.0,
            mx + 80.0,
            layout.y + 8.0,
        ) {
            Some(Hit::Ruler { .. }) => {}
            other => panic!("empty ruler, got {other:?}"),
        }
        let line_y = layout.y + arrangement::RULER_H + 20.0;
        match hit_arrangement(&layout, &[], false, origin, 120.0, 48_000.0, mx + 4.0, line_y) {
            Some(Hit::StartMarker) => {}
            other => panic!("START line in the lane body should drag, got {other:?}"),
        }
        match hit_arrangement(&layout, &[], false, origin, 120.0, 48_000.0, mx + 30.0, line_y) {
            Some(Hit::Locate) => {}
            other => panic!("beside START should time-select, got {other:?}"),
        }
    }

    #[test]
    fn clip_body_and_edge_hits() {
        let layout = ArrangementLayout {
            x: 0.0,
            y: 0.0,
            w: 800.0,
            h: 400.0,
            scroll_x: 0.0,
            scroll_y: 0.0,
            pixels_per_bar: 48.0,
        };
        let mut track = project::MixTrack::empty(project::MixLane::Strip(0), Some("Ch 1".into()));
        let mut clip =
            project::MixClip::new(1, project::MixLane::Strip(0), "clip.wav", 0, 96_000, 0, 96_000);
        clip.id = uuid::Uuid::from_u128(7);
        track.clips.push(clip);
        let tracks = [track];
        let left = arrangement::x_of_frame(&layout, 0, 120.0, 48_000.0);
        let right = arrangement::x_of_frame(&layout, 96_000, 120.0, 48_000.0);
        let y = layout.y + arrangement::RULER_H + arrangement::TRACK_H * 0.5;
        let title_y = layout.y + arrangement::RULER_H + 8.0;
        match hit_arrangement(&layout, &tracks, false, 0, 120.0, 48_000.0, left + 20.0, title_y) {
            Some(Hit::Clip { id, .. }) => assert_eq!(id, uuid::Uuid::from_u128(7)),
            other => panic!("clip title bar, got {other:?}"),
        }
        match hit_arrangement(&layout, &tracks, false, 0, 120.0, 48_000.0, left + 20.0, y) {
            Some(Hit::Locate) => {}
            other => panic!("clip waveform should time-select, got {other:?}"),
        }
        match hit_arrangement(&layout, &tracks, false, 0, 120.0, 48_000.0, right - 2.0, y) {
            Some(Hit::ClipEdge { id, left: false }) => assert_eq!(id, uuid::Uuid::from_u128(7)),
            other => panic!("right edge, got {other:?}"),
        }
        match hit_arrangement(&layout, &tracks, false, 0, 120.0, 48_000.0, right - 8.0, title_y) {
            Some(Hit::ClipEdge { id, left: false }) => assert_eq!(id, uuid::Uuid::from_u128(7)),
            other => panic!("title-bar ] mark should trim, got {other:?}"),
        }
        match hit_arrangement(&layout, &tracks, false, 0, 120.0, 48_000.0, left + 8.0, title_y) {
            Some(Hit::ClipEdge { id, left: true }) => assert_eq!(id, uuid::Uuid::from_u128(7)),
            other => panic!("title-bar [ mark should trim, got {other:?}"),
        }
        match hit_arrangement(
            &layout,
            &tracks,
            false,
            0,
            120.0,
            48_000.0,
            left + 80.0,
            layout.y + arrangement::RULER_H + arrangement::TRACK_H + 10.0,
        ) {
            Some(Hit::Locate) => {}
            other => panic!("empty timeline, got {other:?}"),
        }
        match hit_arrangement(&layout, &tracks, false, 0, 120.0, 48_000.0, 10.0, y) {
            Some(Hit::Lane { track: 0 }) => {}
            other => panic!("header, got {other:?}"),
        }
        match hit_arrangement(&layout, &tracks, true, 0, 120.0, 48_000.0, left + 20.0, title_y) {
            Some(Hit::Clip { id, .. }) => assert_eq!(id, uuid::Uuid::from_u128(7)),
            other => panic!("take view clip title, got {other:?}"),
        }
    }
}

pub fn hit_arrangement(
    layout: &ArrangementLayout,
    tracks: &[project::MixTrack],
    viewing_take: bool,
    origin: i64,
    tempo: f64,
    rate: f64,
    x: f32,
    y: f32,
) -> Option<Hit> {
    if arrangement::start_marker_hit(layout, origin, tempo, rate, x, y) {
        return Some(Hit::StartMarker);
    }
    if y >= layout.y + layout.h - TIME_RULER_H {
        return Some(Hit::TimeRuler);
    }
    if y < layout.y + RULER_H && x >= layout.x + HEADER_W {
        return Some(Hit::Ruler { viewport_anchored: false });
    }
    if x < layout.x + HEADER_W {
        if let Some(idx) = arrangement::track_index_at(layout, y, tracks.len()) {
            return Some(Hit::Lane { track: idx });
        }
        return None;
    }
    if let Some(hit) = arrangement::hit_clip(layout, tracks, viewing_take, tempo, rate, x, y) {
        return Some(match hit {
            arrangement::ClipHit::Body { lane, id } => Hit::Clip { lane, id },
            arrangement::ClipHit::Edge { id, left } => Hit::ClipEdge { id, left },
            arrangement::ClipHit::Fade { id, left } => Hit::ClipFade { id, left },
            arrangement::ClipHit::Loop { id } => Hit::ClipLoop { id },
            arrangement::ClipHit::Slip { id } => Hit::ClipSlip { id },
        });
    }
    Some(Hit::Locate)
}

pub fn frame_at_x(layout: &ArrangementLayout, x: f32, tempo: f64, rate: f64) -> i64 {
    arrangement::frame_at(x, layout, tempo, rate)
}

pub fn hit_mix_mixer(
    x0: f32,
    y0: f32,
    w: f32,
    h: f32,
    track_count: usize,
    show_knobs: bool,
    visible: bool,
    x: f32,
    y: f32,
) -> Option<Hit> {
    if x < x0 || y < y0 || x > x0 + w || y > y0 + h {
        return None;
    }
    if !visible {
        return Some(Hit::MixMixerHandle);
    }
    if track_count == 0 {
        return None;
    }
    let strip_count = track_count + 1;
    let ch_w = crate::mix_mixer::channel_width(w, strip_count);
    let i = ((x - x0 - 4.0) / ch_w) as i32;
    if i < 0 || i as usize >= strip_count {
        return None;
    }
    let i = i as usize;
    let geom = crate::mix_mixer::strip_geom(x0, y0, h, ch_w, i, show_knobs);
    if i == track_count {
        if widgets::contains(geom.fader_hit, x, y) {
            return Some(Hit::MixControlRoomFader {
                rail_top: geom.rail_top,
                rail_bot: geom.rail_bot,
            });
        }
        return None;
    }
    if show_knobs {
        for (k, rect) in geom.knobs.iter().enumerate() {
            if widgets::contains(*rect, x, y) {
                return Some(Hit::MixKnob { track: i, knob: k });
            }
        }
    }
    if widgets::contains(mixer::knob_hit_rect(geom.pan), x, y) {
        return Some(Hit::MixPan { track: i });
    }
    if widgets::contains(geom.fader_hit, x, y) {
        return Some(Hit::MixFader { track: i, rail_top: geom.rail_top, rail_bot: geom.rail_bot });
    }
    if widgets::contains(geom.solo, x, y) {
        return Some(Hit::MixSolo { track: i });
    }
    if widgets::contains(geom.mute, x, y) {
        return Some(Hit::MixMute { track: i });
    }
    None
}
