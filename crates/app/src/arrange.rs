//! Arrangement edit helpers used by the Mix page.

use project::{ArrSelection, MixClip, MixLane, MixTime, MixTrack};

pub fn snap_frame_delta(
    delta: i64,
    grid_on: bool,
    bypass: bool,
    step_bars: f64,
    tempo: f64,
    rate: f64,
) -> i64 {
    if !grid_on || bypass {
        return delta;
    }
    MixTime::snap(delta, step_bars, tempo, rate, 0)
}

/// Paste lands on the mix timeline. Take START is only for `startFromTake`.
pub fn mix_paste_frame(
    leaving_take: bool,
    mix_start_frame: i64,
    playing: bool,
    playhead: i64,
    locate: i64,
) -> i64 {
    if leaving_take {
        mix_start_frame.max(0)
    } else if playing {
        playhead.max(0)
    } else {
        locate.max(0)
    }
}

pub fn snap_locate(
    frame: i64,
    grid_on: bool,
    bypass: bool,
    step_bars: f64,
    tempo: f64,
    rate: f64,
    origin: i64,
) -> i64 {
    if !grid_on || bypass {
        return frame.max(0);
    }
    MixTime::snap(frame, step_bars, tempo, rate, origin).max(0)
}

/// Time select follows the visible zoom grid. Option (`free`) skips snap.
pub fn snap_select(
    frame: i64,
    grid_on: bool,
    free: bool,
    pixels_per_bar: f32,
    tempo: f64,
    rate: f64,
    origin: i64,
) -> i64 {
    if !grid_on || free {
        return frame.max(0);
    }
    MixTime::snap(
        frame,
        ui_mixlink::arrangement::zoom_grid_step(pixels_per_bar),
        tempo,
        rate,
        origin,
    )
    .max(0)
}

pub fn lanes_between(tracks: &[MixTrack], a: MixLane, b: MixLane) -> Vec<MixLane> {
    let ia = tracks.iter().position(|t| t.lane == a);
    let ib = tracks.iter().position(|t| t.lane == b);
    match (ia, ib) {
        (Some(i), Some(j)) => {
            let (lo, hi) = if i <= j { (i, j) } else { (j, i) };
            tracks[lo..=hi].iter().map(|t| t.lane).collect()
        }
        _ => vec![a],
    }
}

pub fn all_lanes(tracks: &[MixTrack]) -> Vec<MixLane> {
    tracks.iter().filter(|t| t.lane != MixLane::Main).map(|t| t.lane).collect()
}

pub fn find_clip<'a>(tracks: &'a [MixTrack], id: uuid::Uuid) -> Option<(MixLane, &'a MixClip)> {
    for track in tracks {
        if let Some(clip) = track.clips.iter().find(|c| c.id == id) {
            return Some((track.lane, clip));
        }
    }
    None
}

pub fn shift_lane(tracks: &[MixTrack], lane: MixLane, delta: i32) -> Option<MixLane> {
    let lanes = all_lanes(tracks);
    let i = lanes.iter().position(|&l| l == lane)?;
    let next = (i as i32 + delta).clamp(0, lanes.len() as i32 - 1) as usize;
    lanes.get(next).copied()
}

pub fn clip_readout(clip: &MixClip, tempo: f64, rate: f64) -> String {
    format!(
        "{}  +{}",
        MixTime::format_position(clip.mix_start_frame, tempo, rate),
        MixTime::format_position(clip.source_frame_count, tempo, rate)
    )
}

pub const TEMPO_MIN: f64 = 20.0;
pub const TEMPO_MAX: f64 = 300.0;
const TEMPO_DRAG_PX: f64 = 8.0;

pub fn clamp_tempo(bpm: f64) -> f64 {
    bpm.clamp(TEMPO_MIN, TEMPO_MAX)
}

pub fn format_tempo(bpm: f64) -> String {
    let bpm = clamp_tempo(bpm);
    if (bpm - bpm.round()).abs() < 0.05 {
        format!("{}", bpm.round() as i32)
    } else {
        format!("{:.1}", bpm)
    }
}

/// Vertical drag: up raises tempo. Default 1 BPM; Option is 0.1.
pub fn tempo_from_drag(start_bpm: f64, start_y: f32, y: f32, fine: bool) -> f64 {
    let step = if fine { 0.1 } else { 1.0 };
    let steps = (-((y - start_y) as f64) / TEMPO_DRAG_PX).round();
    let next = clamp_tempo(start_bpm) + steps * step;
    if fine { (next * 10.0).round() / 10.0 } else { next.round() }.clamp(TEMPO_MIN, TEMPO_MAX)
}

pub fn selection_for_clip(
    lane: MixLane,
    clip: &MixClip,
    add: bool,
    current: &ArrSelection,
) -> ArrSelection {
    if add {
        let mut next = current.clone();
        if !next.clips.contains(&clip.id) {
            next.clips.push(clip.id);
        }
        if !next.lanes.contains(&lane) {
            next.lanes.push(lane);
        }
        next.start = next.start.min(clip.mix_start_frame);
        next.end = next.end.max(clip.mix_end_frame());
        next
    } else {
        let mut next = ArrSelection::default();
        next.select_clip(lane, clip);
        next
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(lane: MixLane) -> MixTrack {
        MixTrack::empty(lane, None)
    }

    #[test]
    fn paste_from_a_take_ignores_that_takes_start() {
        let take_start = 49_203;
        assert_eq!(mix_paste_frame(true, 0, false, take_start, take_start), 0);
        assert_eq!(mix_paste_frame(true, 0, true, take_start, take_start), 0);
        assert_eq!(mix_paste_frame(false, 0, false, 0, 96_000), 96_000);
        assert_eq!(mix_paste_frame(false, 0, true, 24_000, 0), 24_000);
    }

    #[test]
    fn tempo_drag_uses_whole_and_tenth_steps() {
        assert_eq!(tempo_from_drag(120.0, 40.0, 40.0 - 8.0, false), 121.0);
        assert_eq!(tempo_from_drag(120.0, 40.0, 40.0 + 16.0, false), 118.0);
        assert_eq!(tempo_from_drag(120.0, 40.0, 40.0 - 8.0, true), 120.1);
        assert_eq!(format_tempo(120.0), "120");
        assert_eq!(format_tempo(120.4), "120.4");
    }

    #[test]
    fn locate_snaps_to_the_grid() {
        // 120 BPM / 48 kHz → 1/4 bar = 24_000 frames.
        assert_eq!(snap_locate(10_000, true, false, 0.25, 120.0, 48_000.0, 0), 0);
        assert_eq!(snap_locate(13_000, true, false, 0.25, 120.0, 48_000.0, 0), 24_000);
        assert_eq!(snap_locate(13_000, true, true, 0.25, 120.0, 48_000.0, 0), 13_000);
        assert_eq!(snap_locate(13_000, false, false, 0.25, 120.0, 48_000.0, 0), 13_000);
    }

    #[test]
    fn select_snaps_to_zoom_not_the_grid_menu() {
        // Close zoom paints 16ths (6_000 frames). A 1-bar GRID must not win.
        assert_eq!(snap_select(7_000, true, false, 256.0, 120.0, 48_000.0, 0), 6_000);
        assert_eq!(snap_select(7_000, true, true, 256.0, 120.0, 48_000.0, 0), 7_000);
        assert_eq!(snap_select(7_000, false, false, 256.0, 120.0, 48_000.0, 0), 7_000);
        // Default zoom only paints bar lines (96_000 frames).
        assert_eq!(snap_select(50_000, true, false, 48.0, 120.0, 48_000.0, 0), 96_000);
    }

    #[test]
    fn lanes_between_includes_every_channel_in_order() {
        let tracks = [track(MixLane::Strip(0)), track(MixLane::Strip(1)), track(MixLane::Strip(2))];
        assert_eq!(
            lanes_between(&tracks, MixLane::Strip(0), MixLane::Strip(2)),
            vec![MixLane::Strip(0), MixLane::Strip(1), MixLane::Strip(2)]
        );
        assert_eq!(
            lanes_between(&tracks, MixLane::Strip(2), MixLane::Strip(0)),
            vec![MixLane::Strip(0), MixLane::Strip(1), MixLane::Strip(2)]
        );
    }
}
