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
    Clip { track: usize, clip: usize },
    Lane { track: usize },
    Ruler { viewport_anchored: bool },
    TimeRuler,
    StartMarker,
    Locate,
    MixFader { track: usize, rail_top: f32, rail_bot: f32 },
    MixPan { track: usize },
    MixMute { track: usize },
    MixSolo { track: usize },
    MixKnob { track: usize, knob: usize },
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
            return Some(Hit::Fader {
                kind,
                rail_top: bay.rail_top,
                rail_bot: bay.rail_bot,
            });
        }
        return None;
    }
    yy = bay_y + bay_h;
    if y >= yy && y < yy + Layout::NAME_ROW {
        return Some(Hit::Name { kind });
    }
    yy += Layout::NAME_ROW;
    if y >= yy {
        let rel = y - yy;
        let which = if rel < 31.0 {
            Pad::Solo
        } else if rel < 62.0 {
            Pad::Mute
        } else if rel < 93.0 {
            Pad::Bus1
        } else {
            Pad::Bus2
        };
        return Some(Hit::Pad { kind, which });
    }
    Some(Hit::Name { kind })
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
    fn main_name_row_hits_name_but_is_not_a_menu_kind() {
        let layout = MixerLayout::new(0.0, 0.0, 1600.0, 900.0, 2);
        let (sx, sw) = mixer::strip_frame(&layout, 2, StripKind::Main);
        let y = name_y(&layout, 2);
        match hit_mixer(&layout, 2, sx + sw * 0.5, y) {
            Some(Hit::Name { kind: StripKind::Main }) => {}
            other => panic!("Main name row should be Hit::Name(Main), got {other:?}"),
        }
    }
}

pub fn hit_arrangement(layout: &ArrangementLayout, track_count: usize, x: f32, y: f32) -> Option<Hit> {
    if y >= layout.y + layout.h - TIME_RULER_H {
        return Some(Hit::TimeRuler);
    }
    if y < layout.y + RULER_H && x >= layout.x + HEADER_W {
        return Some(Hit::Ruler { viewport_anchored: false });
    }
    if x < layout.x + HEADER_W {
        let idx = ((y - layout.y - RULER_H + layout.scroll_y) / TRACK_H) as i32;
        if idx >= 0 && (idx as usize) < track_count {
            return Some(Hit::Lane { track: idx as usize });
        }
        return None;
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
    x: f32,
    y: f32,
) -> Option<Hit> {
    if x < x0 || y < y0 || x > x0 + w || y > y0 + h || track_count == 0 {
        return None;
    }
    let ch_w = ((w - 8.0) / track_count as f32).clamp(88.0, 106.0);
    let i = ((x - x0 - 4.0) / ch_w) as i32;
    if i < 0 || i as usize >= track_count {
        return None;
    }
    let i = i as usize;
    let mut yy = y0 + 8.0;
    if show_knobs {
        for k in 0..project::KNOB_COUNT {
            if y >= yy && y < yy + 16.0 {
                return Some(Hit::MixKnob { track: i, knob: k });
            }
            yy += 16.0;
        }
    }
    yy += 18.0;
    if y >= yy && y < yy + 36.0 {
        return Some(Hit::MixPan { track: i });
    }
    yy += 36.0;
    let meter_h = (h - (yy - y0) - 36.0).max(40.0);
    if y >= yy && y < yy + meter_h {
        return Some(Hit::MixFader {
            track: i,
            rail_top: yy,
            rail_bot: yy + meter_h - crate::theme::Layout::FADER_CAP_H,
        });
    }
    if y >= y0 + h - 28.0 {
        if x < x0 + 4.0 + i as f32 * ch_w + ch_w * 0.5 {
            return Some(Hit::MixMute { track: i });
        }
        return Some(Hit::MixSolo { track: i });
    }
    None
}
