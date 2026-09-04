use asset::{bins_for_width, column_half_pixels, WaveformCache};
use project::{MixDocument, MixGrid, MixLane, MixTime};
use render::{DrawCmd, Rect};

use crate::theme;

pub const TRACK_H: f32 = 80.0;
pub const HEADER_W: f32 = 86.0;
pub const RULER_H: f32 = 22.0;
pub const TIME_RULER_H: f32 = 22.0;

#[derive(Clone, Copy, Debug)]
pub struct ArrangementLayout {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub scroll_x: f32,
    pub scroll_y: f32,
    pub pixels_per_bar: f32,
}

pub struct ArrangementView<'a> {
    pub layout: ArrangementLayout,
    pub mix: Option<&'a MixDocument>,
    pub tracks: &'a [project::MixTrack],
    pub selected_lane: Option<MixLane>,
    pub selected_clips: &'a [uuid::Uuid],
    pub playhead: i64,
    pub origin: i64,
    pub tempo: f64,
    pub sample_rate: f64,
    pub grid: MixGrid,
    pub grid_enabled: bool,
    pub viewing_take: bool,
    pub bar_selection: Option<(MixLane, i64, i64)>,
    pub waveforms: Option<&'a WaveformCache>,
}

pub fn paint(view: &ArrangementView<'_>) -> Vec<DrawCmd> {
    let mut cmds = Vec::with_capacity(512);
    let l = view.layout;
    theme::fill(&mut cmds, Rect { x: l.x, y: l.y, w: l.w, h: l.h }, [0.0, 0.0, 0.0, 0.35]);
    theme::fill(&mut cmds, Rect { x: l.x, y: l.y, w: HEADER_W, h: l.h - TIME_RULER_H }, [0.0, 0.0, 0.0, 0.22]);
    theme::seam_v(&mut cmds, l.x + HEADER_W - 2.0, l.y, l.h - TIME_RULER_H, true);

    theme::material(&mut cmds, Rect { x: l.x, y: l.y, w: HEADER_W, h: RULER_H }, &theme::UPPER_FACEPLATE);
    paint_bar_ruler(&mut cmds, view);
    let _n = view.tracks.len();
    for (i, track) in view.tracks.iter().enumerate() {
        let y = l.y + RULER_H + i as f32 * TRACK_H - l.scroll_y;
        if y + TRACK_H < l.y || y > l.y + l.h - TIME_RULER_H {
            continue;
        }
        let selected = view.selected_lane == Some(track.lane);
        theme::fill(
            &mut cmds,
            Rect { x: l.x, y, w: HEADER_W, h: TRACK_H },
            if selected { [theme::ORANGE[0], theme::ORANGE[1], theme::ORANGE[2], 0.18] } else { [1.0, 1.0, 1.0, 0.03] },
        );
        theme::text(
            &mut cmds,
            Rect { x: l.x + 8.0, y, w: HEADER_W - 10.0, h: TRACK_H },
            track.name.clone(),
            11.0,
            if selected { theme::TEXT } else { theme::TEXT_DIM },
            selected,
        );
        theme::seam_h(&mut cmds, l.x, y + TRACK_H - 1.0, l.w, false);
        paint_lane(&mut cmds, view, track, y);
    }
    if view.tracks.is_empty() {
        theme::text(
            &mut cmds,
            Rect { x: l.x + HEADER_W + 16.0, y: l.y + RULER_H + 16.0, w: 200.0, h: 20.0 },
            "Start from a take",
            12.0,
            theme::TEXT_DIM,
            false,
        );
    }
    paint_start_marker(&mut cmds, view);
    paint_playhead(&mut cmds, view);
    paint_time_ruler(&mut cmds, view);
    cmds
}

fn x_of(view: &ArrangementView<'_>, frame: i64) -> f32 {
    let bar = MixTime::bar_of(frame, view.tempo, view.sample_rate);
    view.layout.x + HEADER_W + bar as f32 * view.layout.pixels_per_bar - view.layout.scroll_x
}

fn paint_bar_ruler(cmds: &mut Vec<DrawCmd>, view: &ArrangementView<'_>) {
    let l = view.layout;
    theme::material(cmds, Rect { x: l.x + HEADER_W, y: l.y, w: l.w - HEADER_W, h: RULER_H }, &theme::UPPER_FACEPLATE);
    let ppb = l.pixels_per_bar;
    let step = if ppb >= 36.0 { 1.0 } else if ppb * 4.0 >= 36.0 { 4.0 } else { 8.0 };
    let start_bar = MixTime::bar_of(-view.origin, view.tempo, view.sample_rate);
    let end_bar = start_bar + ((l.w - HEADER_W + l.scroll_x) / ppb.max(1.0)) as f64 + 4.0;
    let mut bar = (start_bar / step).floor() * step;
    while bar <= end_bar {
        let frame = view.origin + MixTime::frame_from_bar(bar, view.tempo, view.sample_rate);
        let x = x_of(view, frame);
        if x >= l.x + HEADER_W - 2.0 && x < l.x + l.w {
            let musical = bar;
            if (musical - musical.round()).abs() < 0.001 {
                let n = musical.round() as i32;
                if n != 0 {
                    let color = if musical == 0.0 {
                        theme::METER_GREEN
                    } else if n.rem_euclid(4) == 0 {
                        theme::TEXT
                    } else {
                        theme::TEXT_DIM
                    };
                    theme::text(cmds, Rect { x: x + 3.0, y: l.y + 2.0, w: 24.0, h: 14.0 }, format!("{n}"), 9.0, color, musical == 0.0);
                }
            }
        }
        bar += step;
    }
}

fn paint_lane(cmds: &mut Vec<DrawCmd>, view: &ArrangementView<'_>, track: &project::MixTrack, y: f32) {
    let l = view.layout;
    draw_grid(cmds, view, y, TRACK_H);
    for clip in &track.clips {
        let start = if view.viewing_take { 0 } else { clip.mix_start_frame };
        let end = start + clip.source_frame_count.max(0);
        let left = x_of(view, start);
        let right = x_of(view, end).max(left + 4.0);
        let rect = Rect { x: left, y: y + 4.0, w: right - left, h: TRACK_H - 8.0 };
        if rect.x + rect.w < l.x + HEADER_W || rect.x > l.x + l.w {
            continue;
        }
        let selected = view.selected_clips.contains(&clip.id);
        let mut color = theme::clip_color(clip.source_take);
        color[3] = if selected { 0.95 } else { 0.72 };
        cmds.push(DrawCmd::RoundedRect { rect, color, radius: 3.0 });
        cmds.push(DrawCmd::Line {
            a: (rect.x, rect.y),
            b: (rect.x + rect.w, rect.y),
            color: if selected { [1.0, 1.0, 1.0, 0.85] } else { [0.0, 0.0, 0.0, 0.4] },
            thickness: if selected { 1.2 } else { 0.6 },
        });
        theme::text(
            cmds,
            Rect { x: rect.x + 4.0, y: rect.y + 2.0, w: 40.0, h: 12.0 },
            format!("T{}", clip.source_take),
            8.0,
            [1.0, 1.0, 1.0, 0.8],
            true,
        );
        if let Some(cache) = view.waveforms {
            if let Some(lod) = cache.get(&clip.source_file) {
                let cols = rect.w.max(1.0) as usize;
                let start = (clip.source_start_frame as usize / asset::SAMPLES_PER_BIN).min(lod.peaks.len());
                let count = (clip.source_frame_count as usize / asset::SAMPLES_PER_BIN).max(1);
                let cols_peaks = bins_for_width(&lod.peaks, start, count, cols, 0.0, cols as f64);
                let half = (rect.h * 0.5 - 8.0).max(4.0);
                let bins: Vec<(f32, f32)> = cols_peaks
                    .iter()
                    .map(|p| {
                        let mag = column_half_pixels(*p, lod.max_peak, 1.0);
                        (-mag, mag)
                    })
                    .collect();
                if !bins.is_empty() {
                    cmds.push(DrawCmd::WaveformBins {
                        x0: rect.x,
                        y_center: rect.y + rect.h * 0.55,
                        height: half * 2.0,
                        bar_w: 1.0,
                        color: [1.0, 1.0, 1.0, 0.55],
                        bins,
                    });
                }
            }
        }
    }
    if let Some((lane, a, b)) = view.bar_selection {
        if lane == track.lane {
            let x0 = x_of(view, a.min(b));
            let x1 = x_of(view, a.max(b)).max(x0 + 2.0);
            theme::fill(cmds, Rect { x: x0, y, w: x1 - x0, h: TRACK_H }, [theme::ORANGE[0], theme::ORANGE[1], theme::ORANGE[2], 0.22]);
        }
    }
}

fn draw_grid(cmds: &mut Vec<DrawCmd>, view: &ArrangementView<'_>, y: f32, h: f32) {
    let l = view.layout;
    let ppb = l.pixels_per_bar;
    let layers = [(4.0, 0.08), (1.0, 0.04)];
    for (step, op) in layers {
        if step * ppb < 5.0 {
            continue;
        }
        let start = MixTime::bar_of(-view.origin, view.tempo, view.sample_rate);
        let end = start + ((l.w + l.scroll_x) / ppb.max(1.0)) as f64 + 2.0;
        let step = f64::from(step);
        let mut bar = (start / step).floor() * step;
        while bar <= end {
            let frame = view.origin + MixTime::frame_from_bar(bar, view.tempo, view.sample_rate);
            let x = x_of(view, frame);
            if x >= l.x + HEADER_W && x <= l.x + l.w {
                cmds.push(DrawCmd::Line { a: (x, y), b: (x, y + h), color: [1.0, 1.0, 1.0, op], thickness: 1.0 });
            }
            bar += step;
        }
    }
    if view.grid_enabled {
        let step = view.grid.raw();
        if step < 1.0 && step * ppb as f64 >= 5.0 {
            let start = MixTime::bar_of(-view.origin, view.tempo, view.sample_rate);
            let end = start + ((l.w + l.scroll_x) / ppb.max(1.0)) as f64 + 2.0;
            let mut bar = (start / step).floor() * step;
            while bar <= end {
                let frame = view.origin + MixTime::frame_from_bar(bar, view.tempo, view.sample_rate);
                let x = x_of(view, frame);
                if x >= l.x + HEADER_W && x <= l.x + l.w {
                    cmds.push(DrawCmd::Line { a: (x, y), b: (x, y + h), color: [1.0, 1.0, 1.0, 0.035], thickness: 1.0 });
                }
                bar += step;
            }
        }
    }
}

fn paint_start_marker(cmds: &mut Vec<DrawCmd>, view: &ArrangementView<'_>) {
    let x = x_of(view, view.origin);
    let l = view.layout;
    let h = RULER_H + view.tracks.len() as f32 * TRACK_H;
    cmds.push(DrawCmd::Line {
        a: (x + 8.0, l.y),
        b: (x + 8.0, l.y + h),
        color: theme::METER_GREEN,
        thickness: 1.4,
    });
    theme::fill(cmds, Rect { x: x - 8.0, y: l.y + 1.0, w: 50.0, h: 14.0 }, theme::METER_GREEN);
    theme::text(cmds, Rect { x: x - 4.0, y: l.y + 1.0, w: 44.0, h: 14.0 }, "▶ START", 8.0, [0.0, 0.0, 0.0, 0.85], true);
}

fn paint_playhead(cmds: &mut Vec<DrawCmd>, view: &ArrangementView<'_>) {
    let x = x_of(view, view.playhead);
    let l = view.layout;
    let h = RULER_H + view.tracks.len() as f32 * TRACK_H;
    cmds.push(DrawCmd::Line { a: (x, l.y), b: (x, l.y + h), color: theme::ORANGE, thickness: 1.2 });
}

fn paint_time_ruler(cmds: &mut Vec<DrawCmd>, view: &ArrangementView<'_>) {
    let l = view.layout;
    let y = l.y + l.h - TIME_RULER_H;
    theme::material(cmds, Rect { x: l.x, y, w: HEADER_W, h: TIME_RULER_H }, &theme::UPPER_FACEPLATE);
    theme::material(cmds, Rect { x: l.x + HEADER_W, y, w: l.w - HEADER_W, h: TIME_RULER_H }, &theme::UPPER_FACEPLATE);
    theme::seam_h(cmds, l.x, y, l.w, true);
    theme::seam_v(cmds, l.x + HEADER_W - 2.0, y, TIME_RULER_H, true);
    let clock = MixTime::format_clock(view.playhead, view.sample_rate);
    theme::text_center(cmds, Rect { x: l.x, y, w: HEADER_W, h: TIME_RULER_H }, clock, 10.0, theme::TEXT, true);
    let play_x = x_of(view, view.playhead);
    if play_x >= l.x + HEADER_W && play_x <= l.x + l.w {
        cmds.push(DrawCmd::Line { a: (play_x, y), b: (play_x, y + TIME_RULER_H), color: theme::ORANGE, thickness: 1.2 });
    }
}

pub fn visible_bars(last: i64, play: i64, origin: i64, tempo: f64, rate: f64) -> i32 {
    let bars = MixTime::bar_of(last.max(play).max(origin), tempo, rate);
    (32.0_f64).max(bars.ceil() + 8.0) as i32
}

pub fn frame_at(x: f32, layout: &ArrangementLayout, tempo: f64, rate: f64) -> i64 {
    let local = ((x - layout.x - HEADER_W) + layout.scroll_x).max(0.0);
    MixTime::frame_from_bar((local / layout.pixels_per_bar.max(1.0)) as f64, tempo, rate)
}
