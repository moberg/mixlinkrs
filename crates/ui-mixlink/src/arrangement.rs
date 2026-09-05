use asset::{minmax_for_width, minmax_zoom_points, WaveformCache, WaveformStatus};
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
    let bounds = Rect { x: l.x, y: l.y, w: l.w, h: l.h };
    let timeline = Rect { x: l.x + HEADER_W, y: l.y, w: (l.w - HEADER_W).max(0.0), h: l.h };
    let legend = Rect { x: l.x, y: l.y, w: HEADER_W, h: (l.h - TIME_RULER_H).max(0.0) };

    cmds.push(DrawCmd::Layer);
    cmds.push(DrawCmd::Clip { rect: bounds });
    theme::fill(&mut cmds, bounds, [0.0, 0.0, 0.0, 0.35]);

    cmds.push(DrawCmd::Layer);
    cmds.push(DrawCmd::Clip { rect: timeline });
    paint_bar_ruler(&mut cmds, view);
    for (i, track) in view.tracks.iter().enumerate() {
        let y = l.y + RULER_H + i as f32 * TRACK_H - l.scroll_y;
        if y + TRACK_H < l.y || y > l.y + l.h - TIME_RULER_H {
            continue;
        }
        theme::seam_h(&mut cmds, l.x + HEADER_W, y + TRACK_H - 1.0, l.w - HEADER_W, false);
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

    cmds.push(DrawCmd::Layer);
    theme::fill(&mut cmds, legend, [0.11, 0.11, 0.11, 1.0]);
    theme::hardware_surface(
        &mut cmds,
        Rect { x: l.x, y: l.y, w: HEADER_W, h: RULER_H },
        theme::SurfaceStyle::UpperFaceplate,
    );
    theme::seam_v(&mut cmds, l.x + HEADER_W - 2.0, l.y, legend.h, true);
    for (i, track) in view.tracks.iter().enumerate() {
        let y = l.y + RULER_H + i as f32 * TRACK_H - l.scroll_y;
        if y + TRACK_H < l.y || y > l.y + l.h - TIME_RULER_H {
            continue;
        }
        let selected = view.selected_lane == Some(track.lane);
        let kind: analog::MixLane = track.lane.into();
        let kind_color = theme::lane_kind_color(kind);
        let lane_bot = l.y + l.h - TIME_RULER_H;
        let header = Rect {
            x: l.x,
            y: y.max(l.y + RULER_H),
            w: HEADER_W,
            h: (y + TRACK_H).min(lane_bot) - y.max(l.y + RULER_H),
        };
        if header.h <= 0.5 {
            continue;
        }
        theme::fill(&mut cmds, header, theme::lane_kind_header(kind, selected));
        theme::fill(&mut cmds, Rect { x: l.x, y: header.y, w: 3.0, h: header.h }, kind_color);
        theme::text_clip(
            &mut cmds,
            Rect { x: l.x + 8.0, y, w: HEADER_W - 10.0, h: TRACK_H },
            project::strip_adat_channel(&track.name),
            11.0,
            kind_color,
            selected,
            Some(header),
        );
        theme::seam_h(&mut cmds, l.x, y + TRACK_H - 1.0, HEADER_W, false);
    }

    cmds.push(DrawCmd::Layer);
    cmds.push(DrawCmd::Clip { rect: bounds });
    paint_time_ruler(&mut cmds, view);
    cmds.push(DrawCmd::Layer);
    cmds
}

fn x_of(view: &ArrangementView<'_>, frame: i64) -> f32 {
    x_of_frame(&view.layout, frame, view.tempo, view.sample_rate)
}

pub fn x_of_frame(layout: &ArrangementLayout, frame: i64, tempo: f64, rate: f64) -> f32 {
    let bar = MixTime::bar_of(frame, tempo, rate);
    layout.x + HEADER_W + bar as f32 * layout.pixels_per_bar - layout.scroll_x
}

/// START badge in the bar ruler — the only place the marker is grabbed.
pub fn start_marker_hit(
    layout: &ArrangementLayout,
    origin: i64,
    tempo: f64,
    rate: f64,
    x: f32,
    y: f32,
) -> bool {
    let mx = x_of_frame(layout, origin, tempo, rate);
    x >= mx - 8.0 && x < mx + 42.0 && y >= layout.y && y < layout.y + RULER_H
}

fn paint_bar_ruler(cmds: &mut Vec<DrawCmd>, view: &ArrangementView<'_>) {
    let l = view.layout;
    theme::hardware_surface(
        cmds,
        Rect { x: l.x + HEADER_W, y: l.y, w: l.w - HEADER_W, h: RULER_H },
        theme::SurfaceStyle::UpperFaceplate,
    );
    let ppb = l.pixels_per_bar;
    let step = if ppb >= 36.0 {
        1.0
    } else if ppb * 4.0 >= 36.0 {
        4.0
    } else {
        8.0
    };
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
                    theme::text_mono(
                        cmds,
                        Rect { x: x + 3.0, y: l.y + 2.0, w: 24.0, h: 14.0 },
                        format!("{n}"),
                        9.0,
                        color,
                        musical == 0.0,
                    );
                }
            }
        }
        bar += step;
    }
}

fn paint_lane(
    cmds: &mut Vec<DrawCmd>,
    view: &ArrangementView<'_>,
    track: &project::MixTrack,
    y: f32,
) {
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
        let mut color = theme::clip_color_for_lane(clip.source_take, track.lane.into());
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
        paint_clip_waveform(cmds, view, clip, rect);
    }
    if let Some((lane, a, b)) = view.bar_selection {
        if lane == track.lane {
            let x0 = x_of(view, a.min(b));
            let x1 = x_of(view, a.max(b)).max(x0 + 2.0);
            theme::fill(
                cmds,
                Rect { x: x0, y, w: x1 - x0, h: TRACK_H },
                [theme::ORANGE[0], theme::ORANGE[1], theme::ORANGE[2], 0.22],
            );
        }
    }
}

/// Visible slice only. Zoomed out: one min/max column per pixel. Zoomed in:
/// one point per LOD bin, spaced in clip pixels, so the strip can slope.
fn paint_clip_waveform(
    cmds: &mut Vec<DrawCmd>,
    view: &ArrangementView<'_>,
    clip: &project::MixClip,
    rect: Rect,
) {
    let Some(cache) = view.waveforms else {
        return;
    };
    let lod = match cache.status(&clip.source_file) {
        WaveformStatus::Ready(lod) => lod,
        WaveformStatus::Loading => {
            paint_clip_loading(cmds, rect);
            return;
        }
        WaveformStatus::Missing => return,
    };
    if lod.min.is_empty() || lod.max.is_empty() {
        return;
    }
    let l = view.layout;
    let vis_x0 = rect.x.max(l.x + HEADER_W);
    let vis_x1 = (rect.x + rect.w).min(l.x + l.w);
    if vis_x1 <= vis_x0 {
        return;
    }
    let full_width = f64::from(rect.w);
    if full_width <= 1.0 {
        return;
    }
    let start_x = f64::from(vis_x0 - rect.x);
    let vis_w = f64::from(vis_x1 - vis_x0);
    let n = lod.min.len().min(lod.max.len());
    let bin_start = (clip.source_start_frame as usize / asset::SAMPLES_PER_BIN).min(n);
    let bin_count = (clip.source_frame_count as usize / asset::SAMPLES_PER_BIN)
        .max(1)
        .min(n.saturating_sub(bin_start));
    if bin_count == 0 {
        return;
    }
    let peak = lod.max_peak.max(1e-5);
    let scale = bin_count as f64 / full_width;
    let (x0, bar_w, mut bins) = if scale >= 1.0 {
        let cols = vis_w.ceil() as usize;
        (
            vis_x0,
            1.0f32,
            minmax_for_width(&lod.min, &lod.max, bin_start, bin_count, cols, start_x, full_width),
        )
    } else {
        let (x_off, spacing, pts) = minmax_zoom_points(
            &lod.min, &lod.max, bin_start, bin_count, start_x, vis_w, full_width,
        );
        (rect.x + x_off as f32, spacing as f32, pts)
    };
    for (mn, mx) in &mut bins {
        *mn = (*mn / peak).clamp(-1.0, 1.0);
        *mx = (*mx / peak).clamp(-1.0, 1.0);
    }
    if bins.is_empty() {
        return;
    }
    let half = (rect.h * 0.5 - 8.0).max(4.0);
    cmds.push(DrawCmd::WaveformBins {
        x0,
        y_center: rect.y + rect.h * 0.55,
        height: half * 2.0,
        bar_w,
        color: [1.0, 1.0, 1.0, 0.78],
        bins,
    });
}

fn paint_clip_loading(cmds: &mut Vec<DrawCmd>, rect: Rect) {
    let phase = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f32())
        .unwrap_or(0.0)
        * 2.4)
        .fract();
    let bars = 7usize;
    let gap = 3.0;
    let total_w = (rect.w * 0.42).clamp(28.0, 110.0);
    let bar_w = ((total_w - gap * (bars.saturating_sub(1)) as f32) / bars as f32).max(2.0);
    let cluster = bar_w * bars as f32 + gap * (bars.saturating_sub(1)) as f32;
    let x0 = rect.x + (rect.w - cluster) * 0.5;
    let mid_y = rect.y + rect.h * 0.46;
    let max_h = (rect.h * 0.38).max(8.0);
    for i in 0..bars {
        let wave = ((phase + i as f32 * 0.12) * std::f32::consts::TAU).sin() * 0.5 + 0.5;
        let h = 4.0 + max_h * (0.22 + 0.78 * wave);
        cmds.push(DrawCmd::RoundedRect {
            rect: Rect { x: x0 + i as f32 * (bar_w + gap), y: mid_y - h * 0.5, w: bar_w, h },
            color: [1.0, 1.0, 1.0, 0.18 + 0.32 * wave],
            radius: 1.2,
        });
    }
    theme::text_center(
        cmds,
        Rect { x: rect.x + 4.0, y: rect.y + rect.h * 0.64, w: (rect.w - 8.0).max(8.0), h: 14.0 },
        "Loading",
        10.0,
        [1.0, 1.0, 1.0, 0.55],
        false,
    );
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
                cmds.push(DrawCmd::Line {
                    a: (x, y),
                    b: (x, y + h),
                    color: [1.0, 1.0, 1.0, op],
                    thickness: 1.0,
                });
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
                let frame =
                    view.origin + MixTime::frame_from_bar(bar, view.tempo, view.sample_rate);
                let x = x_of(view, frame);
                if x >= l.x + HEADER_W && x <= l.x + l.w {
                    cmds.push(DrawCmd::Line {
                        a: (x, y),
                        b: (x, y + h),
                        color: [1.0, 1.0, 1.0, 0.035],
                        thickness: 1.0,
                    });
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
    theme::text_mono(
        cmds,
        Rect { x: x - 4.0, y: l.y + 1.0, w: 44.0, h: 14.0 },
        "▶ START",
        8.0,
        [0.0, 0.0, 0.0, 0.85],
        true,
    );
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
    theme::hardware_surface(
        cmds,
        Rect { x: l.x, y, w: HEADER_W, h: TIME_RULER_H },
        theme::SurfaceStyle::UpperFaceplate,
    );
    theme::hardware_surface(
        cmds,
        Rect { x: l.x + HEADER_W, y, w: l.w - HEADER_W, h: TIME_RULER_H },
        theme::SurfaceStyle::UpperFaceplate,
    );
    theme::seam_h(cmds, l.x, y, l.w, true);
    theme::seam_v(cmds, l.x + HEADER_W - 2.0, y, TIME_RULER_H, true);
    let clock = MixTime::format_clock(view.playhead, view.sample_rate);
    theme::text_center_mono(
        cmds,
        Rect { x: l.x, y, w: HEADER_W, h: TIME_RULER_H },
        clock,
        10.0,
        theme::TEXT,
        true,
    );
    paint_time_ticks(cmds, view, y);
    let play_x = x_of(view, view.playhead);
    if play_x >= l.x + HEADER_W && play_x <= l.x + l.w {
        cmds.push(DrawCmd::Line {
            a: (play_x, y),
            b: (play_x, y + TIME_RULER_H),
            color: theme::ORANGE,
            thickness: 1.2,
        });
    }
}

/// MixLink `drawTimeRuler` — major ticks with 9 medium mono clock labels.
fn paint_time_ticks(cmds: &mut Vec<DrawCmd>, view: &ArrangementView<'_>, y: f32) {
    let l = view.layout;
    let rate = view.sample_rate.max(1.0);
    let pps = l.pixels_per_bar as f64 * view.tempo / 240.0;
    if pps <= 0.01 || l.w <= HEADER_W + 1.0 {
        return;
    }
    let (major, minor) = time_tick_steps(pps);
    let start_sec = frame_at(l.x + HEADER_W, &l, view.tempo, rate) as f64 / rate;
    let end_sec = frame_at(l.x + l.w, &l, view.tempo, rate) as f64 / rate;
    let first_minor = ((start_sec / minor).floor() as i32).max(0);
    let last_minor = (end_sec / minor).ceil() as i32;
    if first_minor > last_minor {
        return;
    }
    let majors_every = ((major / minor).round() as i32).max(1);
    let edge = l.x + l.w;
    for i in first_minor..=last_minor {
        let t = i as f64 * minor;
        let frame = (t * rate).round() as i64;
        let x = x_of(view, frame);
        if x < l.x + HEADER_W - 2.0 || x > edge + 2.0 {
            continue;
        }
        if i % majors_every == 0 {
            cmds.push(DrawCmd::Line {
                a: (x, y),
                b: (x, y + 12.0),
                color: [1.0, 1.0, 1.0, 0.42],
                thickness: 1.0,
            });
            if x < edge - 8.0 {
                theme::text_mono(
                    cmds,
                    Rect { x: x + 4.0, y: y + 3.0, w: 48.0, h: 12.0 },
                    MixTime::format_clock_seconds(t),
                    9.0,
                    theme::TEXT_DIM,
                    false,
                );
            }
        } else {
            cmds.push(DrawCmd::Line {
                a: (x, y),
                b: (x, y + 7.0),
                color: [1.0, 1.0, 1.0, 0.22],
                thickness: 1.0,
            });
        }
    }
}

fn time_tick_steps(pixels_per_second: f64) -> (f64, f64) {
    const MIN_MAJOR: f64 = 52.0;
    const STEPS: [(f64, f64); 13] = [
        (0.1, 0.02),
        (0.2, 0.05),
        (0.5, 0.1),
        (1.0, 0.25),
        (2.0, 0.5),
        (5.0, 1.0),
        (10.0, 2.0),
        (15.0, 5.0),
        (30.0, 5.0),
        (60.0, 10.0),
        (120.0, 30.0),
        (300.0, 60.0),
        (600.0, 120.0),
    ];
    for (major, minor) in STEPS {
        if major * pixels_per_second >= MIN_MAJOR {
            return (major, minor);
        }
    }
    (1200.0, 300.0)
}

pub fn visible_bars(last: i64, play: i64, origin: i64, tempo: f64, rate: f64) -> i32 {
    let bars = MixTime::bar_of(last.max(play).max(origin), tempo, rate);
    (32.0_f64).max(bars.ceil() + 8.0) as i32
}

pub fn frame_at(x: f32, layout: &ArrangementLayout, tempo: f64, rate: f64) -> i64 {
    let local = ((x - layout.x - HEADER_W) + layout.scroll_x).max(0.0);
    MixTime::frame_from_bar((local / layout.pixels_per_bar.max(1.0)) as f64, tempo, rate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use asset::WaveformLod;
    use project::{MixClip, MixLane, MixTrack};

    #[test]
    fn zoomed_in_waveform_stays_viewport_sized() {
        let cache = WaveformCache::new();
        cache.insert(
            "clip.wav",
            WaveformLod {
                min: vec![-0.5; 10_000],
                max: vec![0.5; 10_000],
                peaks: vec![0.5; 10_000],
                max_peak: 1.0,
            },
        );
        let mut track = MixTrack::empty(MixLane::Strip(0), Some("Ch 1".into()));
        track.clips = vec![MixClip {
            id: uuid::Uuid::nil(),
            source_take: 1,
            source_lane: MixLane::Strip(0),
            source_file: "clip.wav".into(),
            source_start_frame: 0,
            source_frame_count: 10_000 * asset::SAMPLES_PER_BIN as i64,
            mix_start_frame: 0,
        }];
        let view = ArrangementView {
            layout: ArrangementLayout {
                x: 0.0,
                y: 0.0,
                w: 800.0,
                h: 400.0,
                scroll_x: 0.0,
                scroll_y: 0.0,
                pixels_per_bar: 16_000.0,
            },
            mix: None,
            tracks: std::slice::from_ref(&track),
            selected_lane: None,
            selected_clips: &[],
            playhead: 0,
            origin: 0,
            tempo: 120.0,
            sample_rate: 48_000.0,
            grid: MixGrid::Bar1,
            grid_enabled: false,
            viewing_take: false,
            bar_selection: None,
            waveforms: Some(&cache),
        };
        let cmds = paint(&view);
        let (bins, bar_w) = cmds
            .iter()
            .find_map(|c| match c {
                DrawCmd::WaveformBins { bins, bar_w, .. } => Some((bins.len(), *bar_w)),
                _ => None,
            })
            .expect("waveform");
        assert!(bar_w > 1.0, "zoomed-in strip spaces LOD bins, got bar_w={bar_w}");
        assert!(bins < 800, "must not emit one column per pixel, got {bins}");
        assert!(bins > 20, "visible slice should still cover the lane");
    }

    #[test]
    fn loading_clip_shows_indicator() {
        let cache = WaveformCache::new();
        cache.mark_loading("clip.wav");
        let mut track = MixTrack::empty(MixLane::Strip(0), Some("Ch 1".into()));
        track.clips = vec![MixClip {
            id: uuid::Uuid::nil(),
            source_take: 1,
            source_lane: MixLane::Strip(0),
            source_file: "clip.wav".into(),
            source_start_frame: 0,
            source_frame_count: 48_000,
            mix_start_frame: 0,
        }];
        let cmds = paint(&ArrangementView {
            layout: ArrangementLayout {
                x: 0.0,
                y: 0.0,
                w: 800.0,
                h: 400.0,
                scroll_x: 0.0,
                scroll_y: 0.0,
                pixels_per_bar: 120.0,
            },
            mix: None,
            tracks: std::slice::from_ref(&track),
            selected_lane: None,
            selected_clips: &[],
            playhead: 0,
            origin: 0,
            tempo: 120.0,
            sample_rate: 48_000.0,
            grid: MixGrid::Bar1,
            grid_enabled: false,
            viewing_take: false,
            bar_selection: None,
            waveforms: Some(&cache),
        });
        let loading = cmds.iter().any(|c| match c {
            DrawCmd::Text(t) => t.text == "Loading",
            _ => false,
        });
        assert!(loading, "pending waveform should show a loading label");
        assert!(
            !cmds.iter().any(|c| matches!(c, DrawCmd::WaveformBins { .. })),
            "must not draw bins while the file is still decoding"
        );
    }

    #[test]
    fn arrangement_clips_to_its_bounds() {
        let track = MixTrack::empty(MixLane::Strip(0), Some("Ch 1".into()));
        let layout = ArrangementLayout {
            x: 148.0,
            y: 40.0,
            w: 800.0,
            h: 400.0,
            scroll_x: 0.0,
            scroll_y: 0.0,
            pixels_per_bar: 48.0,
        };
        let cmds = paint(&ArrangementView {
            layout,
            mix: None,
            tracks: std::slice::from_ref(&track),
            selected_lane: None,
            selected_clips: &[],
            playhead: 0,
            origin: 0,
            tempo: 120.0,
            sample_rate: 48_000.0,
            grid: MixGrid::Bar1,
            grid_enabled: false,
            viewing_take: false,
            bar_selection: None,
            waveforms: None,
        });
        let clips: Vec<Rect> = cmds
            .iter()
            .filter_map(|c| match c {
                DrawCmd::Clip { rect } => Some(*rect),
                _ => None,
            })
            .collect();
        assert!(!clips.is_empty());
        assert!(clips.iter().all(|r| r.x >= 147.9 && r.x + r.w <= 948.1));
    }
}
