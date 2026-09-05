use asset::{minmax_for_width, minmax_zoom_points, WaveformCache, WaveformStatus};
use project::{ArrSelection, MixClip, MixDocument, MixGrid, MixLane, MixTime};
use render::{DrawCmd, Rect};

use crate::theme;

pub const TRACK_H: f32 = 80.0;
pub const HEADER_W: f32 = 86.0;
pub const RULER_H: f32 = 22.0;
pub const TIME_RULER_H: f32 = 22.0;
pub const TITLE_H: f32 = 14.0;
pub const CLIP_EDGE_PX: f32 = 6.0;
/// Title-bar trim hit zone (Ableton-style). Wider than the body edge.
pub const CLIP_MARK_PX: f32 = 12.0;
pub const FADE_HANDLE: f32 = 10.0;
pub const LOOP_CORNER: f32 = 10.0;

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

impl ArrangementLayout {
    /// Vertical travel while any lane is still below the time ruler.
    pub fn max_scroll_y(&self, track_count: usize) -> f32 {
        let visible = (self.h - RULER_H - TIME_RULER_H).max(0.0);
        (track_count as f32 * TRACK_H - visible).max(0.0)
    }
}

pub struct ArrangementView<'a> {
    pub layout: ArrangementLayout,
    pub mix: Option<&'a MixDocument>,
    pub tracks: &'a [project::MixTrack],
    pub selected_lane: Option<MixLane>,
    pub selection: &'a ArrSelection,
    pub playhead: i64,
    pub origin: i64,
    pub tempo: f64,
    pub sample_rate: f64,
    pub grid: MixGrid,
    pub grid_enabled: bool,
    pub viewing_take: bool,
    pub home_take: Option<i32>,
    pub drag_preview: Option<&'a [(MixLane, MixClip)]>,
    pub readout: Option<&'a str>,
    pub waveforms: Option<&'a WaveformCache>,
    /// Time-range and clip-outline chrome. Off while moving or resizing.
    pub show_selection: bool,
    /// When set, the live preview replaces these clips instead of ghosting over them.
    pub hide_clip_ids: &'a [uuid::Uuid],
}

#[derive(Clone, Copy, Debug)]
pub enum ClipHit {
    Body { lane: MixLane, id: uuid::Uuid },
    Edge { id: uuid::Uuid, left: bool },
    Fade { id: uuid::Uuid, left: bool },
    Loop { id: uuid::Uuid },
    Slip { id: uuid::Uuid },
}

pub fn paint(view: &ArrangementView<'_>) -> Vec<DrawCmd> {
    let mut cmds = Vec::with_capacity(512);
    let l = view.layout;
    let bounds = Rect { x: l.x, y: l.y, w: l.w, h: l.h };
    let timeline = Rect { x: l.x + HEADER_W, y: l.y, w: (l.w - HEADER_W).max(0.0), h: l.h };
    let lanes = Rect {
        x: l.x + HEADER_W,
        y: l.y + RULER_H,
        w: (l.w - HEADER_W).max(0.0),
        h: (l.h - RULER_H - TIME_RULER_H).max(0.0),
    };
    let legend = Rect { x: l.x, y: l.y, w: HEADER_W, h: (l.h - TIME_RULER_H).max(0.0) };

    cmds.push(DrawCmd::Layer);
    cmds.push(DrawCmd::Clip { rect: bounds });
    theme::fill(&mut cmds, bounds, [0.0, 0.0, 0.0, 0.35]);

    cmds.push(DrawCmd::Layer);
    cmds.push(DrawCmd::Clip { rect: timeline });
    paint_bar_ruler(&mut cmds, view);
    paint_readout(&mut cmds, view);

    cmds.push(DrawCmd::Layer);
    cmds.push(DrawCmd::Clip { rect: lanes });
    paint_bar_bands(&mut cmds, view);
    paint_bar_grid(&mut cmds, view);
    for (i, track) in view.tracks.iter().enumerate() {
        let y = l.y + RULER_H + i as f32 * TRACK_H - l.scroll_y;
        if y + TRACK_H < l.y + RULER_H || y > l.y + l.h - TIME_RULER_H {
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

    cmds.push(DrawCmd::Layer);
    cmds.push(DrawCmd::Clip { rect: timeline });
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
        if y + TRACK_H < l.y + RULER_H || y > l.y + l.h - TIME_RULER_H {
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
        if track.solo {
            theme::text_clip(
                &mut cmds,
                Rect { x: l.x + HEADER_W - 16.0, y: header.y + 2.0, w: 14.0, h: 12.0 },
                "S",
                9.0,
                theme::ORANGE,
                true,
                Some(header),
            );
        }
        theme::seam_h(&mut cmds, l.x, y + TRACK_H - 1.0, HEADER_W, false);
    }

    cmds.push(DrawCmd::Layer);
    cmds.push(DrawCmd::Clip { rect: bounds });
    paint_start_marker(&mut cmds, view);
    paint_time_ruler(&mut cmds, view);
    cmds.push(DrawCmd::Layer);
    cmds
}

fn x_of(view: &ArrangementView<'_>, frame: i64) -> f32 {
    x_of_frame(&view.layout, frame, view.tempo, view.sample_rate)
}

pub fn x_of_frame(layout: &ArrangementLayout, frame: i64, tempo: f64, rate: f64) -> f32 {
    let bar = MixTime::bar_of(frame, tempo, rate);
    (f64::from(layout.x) + f64::from(HEADER_W) + bar * f64::from(layout.pixels_per_bar)
        - f64::from(layout.scroll_x)) as f32
}

pub fn clip_display_start(clip: &MixClip, viewing_take: bool) -> i64 {
    if viewing_take {
        0
    } else {
        clip.mix_start_frame
    }
}

pub fn clip_rect(
    layout: &ArrangementLayout,
    clip: &MixClip,
    lane_y: f32,
    viewing_take: bool,
    tempo: f64,
    rate: f64,
) -> Rect {
    let start = clip_display_start(clip, viewing_take);
    let end = start + clip.source_frame_count.max(0);
    let left = x_of_frame(layout, start, tempo, rate);
    let right = x_of_frame(layout, end, tempo, rate).max(left + 4.0);
    Rect { x: left, y: lane_y + 4.0, w: right - left, h: TRACK_H - 8.0 }
}

fn track_index(layout: &ArrangementLayout, y: f32) -> i32 {
    ((y - layout.y - RULER_H + layout.scroll_y) / TRACK_H).floor() as i32
}

pub fn track_index_at(layout: &ArrangementLayout, y: f32, track_count: usize) -> Option<usize> {
    let idx = track_index(layout, y);
    if idx >= 0 && (idx as usize) < track_count {
        Some(idx as usize)
    } else {
        None
    }
}

/// Same as `track_index_at`, but a drag that leaves the lane stack keeps the
/// nearest channel so a time selection can grow across every lane.
pub fn track_index_clamped(
    layout: &ArrangementLayout,
    y: f32,
    track_count: usize,
) -> Option<usize> {
    if track_count == 0 {
        return None;
    }
    Some(track_index(layout, y).clamp(0, track_count as i32 - 1) as usize)
}

pub fn hit_clip(
    layout: &ArrangementLayout,
    tracks: &[project::MixTrack],
    viewing_take: bool,
    tempo: f64,
    rate: f64,
    x: f32,
    y: f32,
) -> Option<ClipHit> {
    let idx = track_index_at(layout, y, tracks.len())?;
    let track = &tracks[idx];
    let lane_y = layout.y + RULER_H + idx as f32 * TRACK_H - layout.scroll_y;
    for clip in track.clips.iter().rev() {
        let rect = clip_rect(layout, clip, lane_y, viewing_take, tempo, rate);
        if x < rect.x || x > rect.x + rect.w || y < rect.y || y > rect.y + rect.h {
            continue;
        }
        let edge = CLIP_EDGE_PX.min(rect.w * 0.25);
        let mark = CLIP_MARK_PX.min(rect.w * 0.28);
        let on_title = y <= rect.y + TITLE_H.min(rect.h);
        if on_title && x <= rect.x + mark {
            return Some(ClipHit::Edge { id: clip.id, left: true });
        }
        if on_title && x >= rect.x + rect.w - mark {
            return Some(ClipHit::Edge { id: clip.id, left: false });
        }
        if x <= rect.x + edge {
            return Some(ClipHit::Edge { id: clip.id, left: true });
        }
        if x >= rect.x + rect.w - edge {
            return Some(ClipHit::Edge { id: clip.id, left: false });
        }
        if on_title {
            if clip.fade_in_frames > 0 && x <= rect.x + mark + FADE_HANDLE {
                return Some(ClipHit::Fade { id: clip.id, left: true });
            }
            if clip.fade_out_frames > 0 && x >= rect.x + rect.w - mark - FADE_HANDLE {
                return Some(ClipHit::Fade { id: clip.id, left: false });
            }
            return Some(ClipHit::Body { lane: track.lane, id: clip.id });
        }
        if y >= rect.y + rect.h - LOOP_CORNER && x >= rect.x + rect.w - LOOP_CORNER {
            return Some(ClipHit::Loop { id: clip.id });
        }
        // Waveform is time-select, not move — fall through to Locate.
        return None;
    }
    None
}

pub fn clip_at(
    layout: &ArrangementLayout,
    tracks: &[project::MixTrack],
    viewing_take: bool,
    tempo: f64,
    rate: f64,
    x: f32,
    y: f32,
) -> Option<(project::MixLane, uuid::Uuid)> {
    let idx = track_index_at(layout, y, tracks.len())?;
    let track = &tracks[idx];
    let lane_y = layout.y + RULER_H + idx as f32 * TRACK_H - layout.scroll_y;
    for clip in track.clips.iter().rev() {
        let rect = clip_rect(layout, clip, lane_y, viewing_take, tempo, rate);
        if x >= rect.x && x <= rect.x + rect.w && y >= rect.y && y <= rect.y + rect.h {
            return Some((track.lane, clip.id));
        }
    }
    None
}

/// MixLink START badge in the bar ruler (58×14). The lane body only hits the line
/// so a time selection can start next to bar 0.
pub const START_HIT_W: f32 = 58.0;
pub const START_HIT_INSET: f32 = 8.0;
pub const START_LINE_HIT_W: f32 = 12.0;

pub fn start_marker_hit(
    layout: &ArrangementLayout,
    origin: i64,
    tempo: f64,
    rate: f64,
    x: f32,
    y: f32,
) -> bool {
    let mx = x_of_frame(layout, origin, tempo, rate);
    let top = layout.y;
    let bot = layout.y + layout.h - TIME_RULER_H;
    if y < top || y >= bot {
        return false;
    }
    if y < layout.y + RULER_H {
        let left = mx - START_HIT_INSET;
        return x >= left && x < left + START_HIT_W;
    }
    let half = START_LINE_HIT_W * 0.5;
    x >= mx - half && x < mx + half
}

fn x_of_bar(view: &ArrangementView<'_>, bar: f64) -> f32 {
    x_of(view, view.origin + MixTime::frame_from_bar(bar, view.tempo, view.sample_rate))
}

fn visible_musical_bars(view: &ArrangementView<'_>) -> (f64, f64) {
    let l = view.layout;
    let rate = view.sample_rate.max(1.0);
    let left = frame_at(l.x + HEADER_W, &l, view.tempo, rate);
    let right = frame_at(l.x + l.w, &l, view.tempo, rate);
    (
        MixTime::bar_of(left - view.origin, view.tempo, rate),
        MixTime::bar_of(right - view.origin, view.tempo, rate),
    )
}

/// Live-style truncation: `3`, `3.2`, `3.2.4` — never `3.1` or `3.2.1`.
fn format_bar_label(bar: f64) -> String {
    let sign = if bar < -1e-9 { "-" } else { "" };
    let sixteenths = (bar.abs() * 16.0).round() as i64;
    let bars = sixteenths / 16;
    let rem = sixteenths.rem_euclid(16);
    let beat = rem / 4 + 1;
    let tick = rem % 4 + 1;
    if beat == 1 && tick == 1 {
        format!("{sign}{bars}")
    } else if tick == 1 {
        format!("{sign}{bars}.{beat}")
    } else {
        format!("{sign}{bars}.{beat}.{tick}")
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct BarTickScale {
    label: f64,
    mid: f64,
    minor: f64,
}

/// Adaptive bar/beat/16th cadence — same zoom steps as Live's arrangement ruler.
fn bar_tick_steps(pixels_per_bar: f32) -> BarTickScale {
    let ppb = f64::from(pixels_per_bar);
    // (label, mid, minor, min pixels per labeled step)
    const STEPS: [(f64, f64, f64, f64); 7] = [
        (0.0625, 0.0625, 0.0625, 36.0),
        (0.25, 0.0625, 0.0625, 30.0),
        (1.0, 0.25, 0.25, 40.0),
        (2.0, 1.0, 0.25, 40.0),
        (4.0, 1.0, 1.0, 36.0),
        (8.0, 4.0, 1.0, 36.0),
        (16.0, 8.0, 4.0, 36.0),
    ];
    for (label, mid, minor, min_px) in STEPS {
        if label * ppb >= min_px {
            return BarTickScale { label, mid, minor };
        }
    }
    BarTickScale { label: 32.0, mid: 16.0, minor: 8.0 }
}

/// Finest painted arrangement line — Live adaptive-grid select, not `MixGrid`.
pub fn zoom_grid_step(pixels_per_bar: f32) -> f64 {
    let ppb = f64::from(pixels_per_bar.max(1.0));
    if 0.0625 * ppb >= 16.0 {
        0.0625
    } else if 0.25 * ppb >= 18.0 {
        0.25
    } else if ppb >= 16.0 {
        1.0
    } else {
        bar_tick_steps(pixels_per_bar).label
    }
}

fn shade_step(scale: BarTickScale) -> f64 {
    if scale.label <= 0.0625 + 1e-9 {
        0.25
    } else if scale.label <= 1.0 + 1e-9 {
        2.0
    } else {
        scale.label.max(2.0)
    }
}

fn paint_bar_bands(cmds: &mut Vec<DrawCmd>, view: &ArrangementView<'_>) {
    let l = view.layout;
    let y = l.y + RULER_H;
    let h = (l.h - RULER_H - TIME_RULER_H).max(0.0);
    if h < 1.0 || l.pixels_per_bar < 1.0 {
        return;
    }
    let step = shade_step(bar_tick_steps(l.pixels_per_bar));
    if step * f64::from(l.pixels_per_bar) < 20.0 {
        return;
    }
    let (start, end) = visible_musical_bars(view);
    let first = (start / step).floor() as i32 - 1;
    let last = (end / step).ceil() as i32 + 1;
    let left = l.x + HEADER_W;
    let right = l.x + l.w;
    for i in first..=last {
        if i.rem_euclid(2) != 0 {
            continue;
        }
        let x0 = x_of_bar(view, f64::from(i) * step).max(left);
        let x1 = x_of_bar(view, f64::from(i + 1) * step).min(right);
        if x1 - x0 < 1.0 {
            continue;
        }
        theme::fill(cmds, Rect { x: x0, y, w: x1 - x0, h }, [1.0, 1.0, 1.0, 0.028]);
    }
}

fn paint_bar_ruler(cmds: &mut Vec<DrawCmd>, view: &ArrangementView<'_>) {
    let l = view.layout;
    theme::hardware_surface(
        cmds,
        Rect { x: l.x + HEADER_W, y: l.y, w: l.w - HEADER_W, h: RULER_H },
        theme::SurfaceStyle::UpperFaceplate,
    );
    let ppb = l.pixels_per_bar;
    if ppb < 1.0 || l.w <= HEADER_W + 1.0 {
        return;
    }
    let scale = bar_tick_steps(ppb);
    let (start, end) = visible_musical_bars(view);
    let first = (start / scale.minor).floor() as i32 - 1;
    let last = (end / scale.minor).ceil() as i32 + 1;
    if first > last {
        return;
    }
    let labels_every = ((scale.label / scale.minor).round() as i32).max(1);
    let mids_every = ((scale.mid / scale.minor).round() as i32).max(1);
    let draw_mid = scale.mid * f64::from(ppb) >= 16.0;
    let draw_minor = scale.minor * f64::from(ppb) >= 28.0 && scale.minor + 1e-9 < scale.mid;
    let edge = l.x + l.w;
    let origin_x = x_of(view, view.origin);
    for i in first..=last {
        let bar = f64::from(i) * scale.minor;
        let x = x_of_bar(view, bar);
        if x < l.x + HEADER_W - 2.0 || x > edge + 2.0 {
            continue;
        }
        let on_start = (x - origin_x).abs() < 8.0;
        if i % labels_every == 0 {
            if !on_start {
                cmds.push(DrawCmd::Line {
                    a: (x, l.y + 1.0),
                    b: (x, l.y + 6.0),
                    color: [1.0, 1.0, 1.0, 0.28],
                    thickness: 1.0,
                });
            }
            if !on_start && bar.abs() > 1e-6 && x < edge - 10.0 {
                let label = format_bar_label(bar);
                let wide = label.contains('.');
                theme::text_mono(
                    cmds,
                    Rect { x: x + 3.0, y: l.y + 8.0, w: if wide { 46.0 } else { 28.0 }, h: 12.0 },
                    label,
                    8.0,
                    if wide { [1.0, 1.0, 1.0, 0.32] } else { [1.0, 1.0, 1.0, 0.46] },
                    false,
                );
            }
        } else if draw_mid && i % mids_every == 0 {
            cmds.push(DrawCmd::Line {
                a: (x, l.y + 1.0),
                b: (x, l.y + 4.0),
                color: [1.0, 1.0, 1.0, 0.14],
                thickness: 1.0,
            });
        } else if draw_minor {
            cmds.push(DrawCmd::Line {
                a: (x, l.y + 1.0),
                b: (x, l.y + 2.5),
                color: [1.0, 1.0, 1.0, 0.08],
                thickness: 1.0,
            });
        }
    }
}

fn paint_lane(
    cmds: &mut Vec<DrawCmd>,
    view: &ArrangementView<'_>,
    track: &project::MixTrack,
    y: f32,
) {
    for clip in &track.clips {
        if view.hide_clip_ids.contains(&clip.id) {
            continue;
        }
        paint_one_clip(cmds, view, clip, y, 1.0);
    }
    if let Some(preview) = view.drag_preview {
        let alpha = if view.hide_clip_ids.is_empty() { 0.55 } else { 1.0 };
        for (lane, clip) in preview {
            if *lane == track.lane {
                paint_one_clip(cmds, view, clip, y, alpha);
            }
        }
    }
    if view.show_selection
        && view.selection.has_range()
        && view.selection.lanes.contains(&track.lane)
    {
        let (a, b) = view.selection.range();
        let x0 = x_of(view, a);
        let x1 = x_of(view, b).max(x0 + 2.0);
        let rect = Rect { x: x0, y, w: x1 - x0, h: TRACK_H };
        theme::fill(
            cmds,
            rect,
            [theme::COPY_SELECT[0], theme::COPY_SELECT[1], theme::COPY_SELECT[2], 0.20],
        );
        theme::stroke_rect(
            cmds,
            rect,
            [theme::COPY_SELECT[0], theme::COPY_SELECT[1], theme::COPY_SELECT[2], 0.40],
            1.0,
        );
        let top_lane =
            view.tracks.iter().find(|t| view.selection.lanes.contains(&t.lane)).map(|t| t.lane);
        if top_lane == Some(track.lane) {
            paint_copy_ticks(cmds, x0, x1, y);
        }
    }
}

fn paint_copy_ticks(cmds: &mut Vec<DrawCmd>, x0: f32, x1: f32, y: f32) {
    let s = 5.0;
    let color = [1.0, 1.0, 1.0, 0.92];
    let tick = |cmds: &mut Vec<DrawCmd>, tip_x: f32, inward: f32| {
        cmds.push(DrawCmd::Line { a: (tip_x, y), b: (tip_x + inward, y), color, thickness: 1.2 });
        cmds.push(DrawCmd::Line {
            a: (tip_x, y),
            b: (tip_x + inward * 0.5, y + s),
            color,
            thickness: 1.2,
        });
        cmds.push(DrawCmd::Line {
            a: (tip_x + inward, y),
            b: (tip_x + inward * 0.5, y + s),
            color,
            thickness: 1.2,
        });
    };
    tick(cmds, x0, s);
    tick(cmds, x1 - s, s);
}

fn paint_one_clip(
    cmds: &mut Vec<DrawCmd>,
    view: &ArrangementView<'_>,
    clip: &MixClip,
    lane_y: f32,
    alpha: f32,
) {
    let l = view.layout;
    let rect = clip_rect(&l, clip, lane_y, view.viewing_take, view.tempo, view.sample_rate);
    if rect.x + rect.w < l.x + HEADER_W || rect.x > l.x + l.w {
        return;
    }
    let selected = view.show_selection && view.selection.clips.contains(&clip.id);
    let mut color = theme::clip_color(clip.source_take);
    color[3] = if selected { 0.95 * alpha } else { 0.62 * alpha };
    cmds.push(DrawCmd::RoundedRect { rect, color, radius: 3.0 });
    let title = Rect { x: rect.x, y: rect.y, w: rect.w, h: TITLE_H.min(rect.h) };
    let mut title_color = color;
    title_color[0] *= 0.55;
    title_color[1] *= 0.55;
    title_color[2] *= 0.55;
    title_color[3] = if selected { 0.95 * alpha } else { 0.82 * alpha };
    theme::fill(cmds, title, title_color);
    cmds.push(DrawCmd::Line {
        a: (rect.x, rect.y),
        b: (rect.x, rect.y + rect.h),
        color: if selected { [1.0, 1.0, 1.0, 0.85 * alpha] } else { [0.0, 0.0, 0.0, 0.55 * alpha] },
        thickness: 1.0,
    });
    cmds.push(DrawCmd::Line {
        a: (rect.x + rect.w, rect.y),
        b: (rect.x + rect.w, rect.y + rect.h),
        color: if selected { [1.0, 1.0, 1.0, 0.85 * alpha] } else { [0.0, 0.0, 0.0, 0.55 * alpha] },
        thickness: 1.0,
    });
    if selected {
        cmds.push(DrawCmd::Line {
            a: (rect.x, rect.y),
            b: (rect.x + rect.w, rect.y),
            color: [1.0, 1.0, 1.0, 0.85 * alpha],
            thickness: 1.2,
        });
        cmds.push(DrawCmd::Line {
            a: (rect.x, rect.y + rect.h),
            b: (rect.x + rect.w, rect.y + rect.h),
            color: [1.0, 1.0, 1.0, 0.7 * alpha],
            thickness: 1.0,
        });
    }
    theme::text_clip(
        cmds,
        Rect { x: rect.x + 4.0, y: rect.y + 1.0, w: (rect.w - 8.0).max(4.0), h: 12.0 },
        clip.title(view.home_take),
        8.0,
        [1.0, 1.0, 1.0, 0.88 * alpha],
        selected,
        Some(title),
    );
    let body = Rect {
        x: rect.x,
        y: rect.y + TITLE_H.min(rect.h),
        w: rect.w,
        h: (rect.h - TITLE_H).max(0.0),
    };
    if body.h > 2.0 {
        paint_clip_waveform(cmds, view, clip, body);
    }
    paint_fade_handles(cmds, clip, rect, alpha);
}

fn paint_fade_handles(cmds: &mut Vec<DrawCmd>, clip: &MixClip, rect: Rect, alpha: f32) {
    if clip.fade_in_frames > 0 && clip.source_frame_count > 0 {
        let t = (clip.fade_in_frames as f32 / clip.source_frame_count as f32).clamp(0.0, 0.5);
        cmds.push(DrawCmd::Line {
            a: (rect.x, rect.y + rect.h),
            b: (rect.x + rect.w * t, rect.y + TITLE_H.min(rect.h)),
            color: [1.0, 1.0, 1.0, 0.45 * alpha],
            thickness: 1.0,
        });
    }
    if clip.fade_out_frames > 0 && clip.source_frame_count > 0 {
        let t = (clip.fade_out_frames as f32 / clip.source_frame_count as f32).clamp(0.0, 0.5);
        cmds.push(DrawCmd::Line {
            a: (rect.x + rect.w, rect.y + rect.h),
            b: (rect.x + rect.w * (1.0 - t), rect.y + TITLE_H.min(rect.h)),
            color: [1.0, 1.0, 1.0, 0.45 * alpha],
            thickness: 1.0,
        });
    }
}

fn paint_readout(cmds: &mut Vec<DrawCmd>, view: &ArrangementView<'_>) {
    let Some(text) = view.readout else {
        return;
    };
    let l = view.layout;
    theme::fill(
        cmds,
        Rect { x: l.x + HEADER_W + 8.0, y: l.y + 2.0, w: 220.0, h: 16.0 },
        [0.0, 0.0, 0.0, 0.55],
    );
    theme::text_mono(
        cmds,
        Rect { x: l.x + HEADER_W + 10.0, y: l.y + 2.0, w: 216.0, h: 14.0 },
        text,
        9.0,
        theme::ORANGE,
        true,
    );
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
    let peak = asset::WAVEFORM_REF_PEAK;
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

fn paint_bar_grid(cmds: &mut Vec<DrawCmd>, view: &ArrangementView<'_>) {
    let l = view.layout;
    let y = l.y + RULER_H;
    let h = (l.h - RULER_H - TIME_RULER_H).max(0.0);
    if h < 1.0 || l.pixels_per_bar < 1.0 {
        return;
    }
    let scale = bar_tick_steps(l.pixels_per_bar);
    let ppb = f64::from(l.pixels_per_bar);
    let (start, end) = visible_musical_bars(view);
    let first = (start / scale.minor).floor() as i32 - 1;
    let last = (end / scale.minor).ceil() as i32 + 1;
    let left = l.x + HEADER_W;
    let right = l.x + l.w;
    for i in first..=last {
        let bar = f64::from(i) * scale.minor;
        let x = x_of_bar(view, bar);
        if x < left || x > right {
            continue;
        }
        let is_bar = on_bar_step(bar, 1.0);
        let is_beat = on_bar_step(bar, 0.25);
        let (op, draw) = if is_bar && ppb >= 16.0 {
            (0.070, true)
        } else if is_beat && 0.25 * ppb >= 18.0 {
            (0.038, true)
        } else if 0.0625 * ppb >= 16.0 {
            (0.020, true)
        } else {
            (0.0, false)
        };
        if draw {
            cmds.push(DrawCmd::Line {
                a: (x, y),
                b: (x, y + h),
                color: [1.0, 1.0, 1.0, op],
                thickness: 1.0,
            });
        }
    }
}

fn on_bar_step(bar: f64, step: f64) -> bool {
    let q = bar / step;
    (q - q.round()).abs() < 1e-6
}

/// MixLink `StartMarkerView`: 58×14 rounded flag at `origin − 8`, line at `+8`.
fn paint_start_marker(cmds: &mut Vec<DrawCmd>, view: &ArrangementView<'_>) {
    let x = x_of(view, view.origin);
    let l = view.layout;
    let h = (l.h - TIME_RULER_H).max(RULER_H);
    cmds.push(DrawCmd::Line {
        a: (x, l.y),
        b: (x, l.y + h),
        color: theme::METER_GREEN,
        thickness: 1.4,
    });
    cmds.push(DrawCmd::RoundedRect {
        rect: Rect { x: x - START_HIT_INSET, y: l.y + 1.0, w: START_HIT_W, h: 14.0 },
        color: theme::METER_GREEN,
        radius: 2.0,
    });
    theme::text_mono(
        cmds,
        Rect { x: x - 5.0, y: l.y + 1.0, w: 50.0, h: 14.0 },
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

/// Ableton-style clock ruler: short ticks along the top edge, labels below,
/// and majors kept far enough apart that tenths never form a comb.
fn paint_time_ticks(cmds: &mut Vec<DrawCmd>, view: &ArrangementView<'_>, y: f32) {
    let l = view.layout;
    let rate = view.sample_rate.max(1.0);
    let pps = l.pixels_per_bar as f64 * view.tempo / 240.0;
    if pps <= 0.01 || l.w <= HEADER_W + 1.0 {
        return;
    }
    let scale = time_tick_steps(pps);
    let start_sec = frame_at(l.x + HEADER_W, &l, view.tempo, rate) as f64 / rate;
    let end_sec = frame_at(l.x + l.w, &l, view.tempo, rate) as f64 / rate;
    let first_minor = ((start_sec / scale.minor).floor() as i32).max(0);
    let last_minor = (end_sec / scale.minor).ceil() as i32;
    if first_minor > last_minor {
        return;
    }
    let majors_every = ((scale.major / scale.minor).round() as i32).max(1);
    let mids_every = ((scale.mid / scale.minor).round() as i32).max(1);
    let draw_mid = scale.mid * pps >= 16.0;
    let draw_minor = scale.minor * pps >= 40.0;
    let edge = l.x + l.w;
    for i in first_minor..=last_minor {
        let t = i as f64 * scale.minor;
        let frame = (t * rate).round() as i64;
        let x = x_of(view, frame);
        if x < l.x + HEADER_W - 2.0 || x > edge + 2.0 {
            continue;
        }
        if i % majors_every == 0 {
            cmds.push(DrawCmd::Line {
                a: (x, y + 1.0),
                b: (x, y + 6.0),
                color: [1.0, 1.0, 1.0, 0.28],
                thickness: 1.0,
            });
            if x < edge - 8.0 {
                theme::text_mono(
                    cmds,
                    Rect { x: x + 3.0, y: y + 8.0, w: 48.0, h: 12.0 },
                    MixTime::format_clock_seconds(t),
                    8.0,
                    [1.0, 1.0, 1.0, 0.40],
                    false,
                );
            }
        } else if draw_mid && i % mids_every == 0 {
            cmds.push(DrawCmd::Line {
                a: (x, y + 1.0),
                b: (x, y + 4.0),
                color: [1.0, 1.0, 1.0, 0.14],
                thickness: 1.0,
            });
        } else if draw_minor {
            cmds.push(DrawCmd::Line {
                a: (x, y + 1.0),
                b: (x, y + 2.5),
                color: [1.0, 1.0, 1.0, 0.08],
                thickness: 1.0,
            });
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct TimeTickScale {
    major: f64,
    mid: f64,
    minor: f64,
}

/// 1–2–5 majors with ~90px of label room — same cadence as Live's time ruler.
fn time_tick_steps(pixels_per_second: f64) -> TimeTickScale {
    // (major, mid, minor, min pixels per labeled major)
    const STEPS: [(f64, f64, f64, f64); 12] = [
        (0.1, 0.05, 0.01, 160.0),
        (0.5, 0.1, 0.05, 320.0),
        (1.0, 0.25, 0.05, 90.0),
        (2.0, 0.5, 0.1, 88.0),
        (5.0, 1.0, 0.5, 88.0),
        (10.0, 2.0, 1.0, 88.0),
        (15.0, 5.0, 1.0, 88.0),
        (30.0, 10.0, 5.0, 88.0),
        (60.0, 15.0, 5.0, 88.0),
        (120.0, 30.0, 10.0, 88.0),
        (300.0, 60.0, 15.0, 88.0),
        (600.0, 120.0, 30.0, 88.0),
    ];
    for (major, mid, minor, min_px) in STEPS {
        if major * pixels_per_second >= min_px {
            return TimeTickScale { major, mid, minor };
        }
    }
    TimeTickScale { major: 1200.0, mid: 300.0, minor: 60.0 }
}

pub fn visible_bars(last: i64, play: i64, origin: i64, tempo: f64, rate: f64) -> i32 {
    let bars = MixTime::bar_of(last.max(play).max(origin), tempo, rate);
    (32.0_f64).max(bars.ceil() + 8.0) as i32
}

pub fn frame_at(x: f32, layout: &ArrangementLayout, tempo: f64, rate: f64) -> i64 {
    let local =
        f64::from(x) - f64::from(layout.x) - f64::from(HEADER_W) + f64::from(layout.scroll_x);
    MixTime::frame_from_bar(
        (local / f64::from(layout.pixels_per_bar.max(1.0))).max(0.0),
        tempo,
        rate,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use asset::WaveformLod;
    use project::{ArrSelection, MixClip, MixLane, MixTrack};

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
        let mut clip = MixClip::new(
            1,
            MixLane::Strip(0),
            "clip.wav",
            0,
            10_000 * asset::SAMPLES_PER_BIN as i64,
            0,
            10_000 * asset::SAMPLES_PER_BIN as i64,
        );
        clip.id = uuid::Uuid::nil();
        track.clips = vec![clip];
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
            selection: &ArrSelection::default(),
            playhead: 0,
            origin: 0,
            tempo: 120.0,
            sample_rate: 48_000.0,
            grid: MixGrid::Bar1,
            grid_enabled: false,
            viewing_take: false,
            home_take: None,
            drag_preview: None,
            readout: None,
            waveforms: Some(&cache),
            show_selection: true,
            hide_clip_ids: &[],
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
        let mut clip = MixClip::new(1, MixLane::Strip(0), "clip.wav", 0, 48_000, 0, 48_000);
        clip.id = uuid::Uuid::nil();
        track.clips = vec![clip];
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
            selection: &ArrSelection::default(),
            playhead: 0,
            origin: 0,
            tempo: 120.0,
            sample_rate: 48_000.0,
            grid: MixGrid::Bar1,
            grid_enabled: false,
            viewing_take: false,
            home_take: None,
            drag_preview: None,
            readout: None,
            waveforms: Some(&cache),
            show_selection: true,
            hide_clip_ids: &[],
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
            selection: &ArrSelection::default(),
            playhead: 0,
            origin: 0,
            tempo: 120.0,
            sample_rate: 48_000.0,
            grid: MixGrid::Bar1,
            grid_enabled: false,
            viewing_take: false,
            home_take: None,
            drag_preview: None,
            readout: None,
            waveforms: None,
            show_selection: true,
            hide_clip_ids: &[],
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
        assert!(
            clips.iter().any(|r| (r.y - (layout.y + RULER_H)).abs() < 0.1
                && (r.h - (layout.h - RULER_H - TIME_RULER_H)).abs() < 0.1),
            "lane body must clip below the bar ruler"
        );
    }

    #[test]
    fn max_scroll_y_stops_at_the_last_lane() {
        let tall = ArrangementLayout {
            x: 0.0,
            y: 0.0,
            w: 800.0,
            h: RULER_H + TRACK_H * 2.0 + TIME_RULER_H + 40.0,
            scroll_x: 0.0,
            scroll_y: 0.0,
            pixels_per_bar: 48.0,
        };
        assert_eq!(tall.max_scroll_y(2), 0.0);
        let short = ArrangementLayout { h: RULER_H + TRACK_H + TIME_RULER_H, ..tall };
        assert_eq!(short.max_scroll_y(3), TRACK_H * 2.0);
        assert_eq!(short.max_scroll_y(0), 0.0);
    }

    #[test]
    fn time_select_keeps_the_nearest_lane_when_the_cursor_leaves() {
        let layout = ArrangementLayout {
            x: 0.0,
            y: 0.0,
            w: 800.0,
            h: 400.0,
            scroll_x: 0.0,
            scroll_y: 0.0,
            pixels_per_bar: 48.0,
        };
        assert_eq!(track_index_clamped(&layout, -20.0, 4), Some(0));
        assert_eq!(track_index_clamped(&layout, layout.y + RULER_H + TRACK_H * 10.0, 4), Some(3));
        assert_eq!(track_index_at(&layout, -20.0, 4), None);
    }

    #[test]
    fn start_line_and_playhead_share_x_when_located_at_origin() {
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
        let origin = 48_000;
        let cmds = paint(&ArrangementView {
            layout,
            mix: None,
            tracks: std::slice::from_ref(&track),
            selected_lane: None,
            selection: &ArrSelection::default(),
            playhead: origin,
            origin,
            tempo: 120.0,
            sample_rate: 48_000.0,
            grid: MixGrid::Bar1,
            grid_enabled: false,
            viewing_take: false,
            home_take: None,
            drag_preview: None,
            readout: None,
            waveforms: None,
            show_selection: true,
            hide_clip_ids: &[],
        });
        let start_x = cmds.iter().find_map(|c| match c {
            DrawCmd::Line { a, color, thickness, .. }
                if *color == theme::METER_GREEN && *thickness > 1.3 =>
            {
                Some(a.0)
            }
            _ => None,
        });
        let play_x = cmds.iter().find_map(|c| match c {
            DrawCmd::Line { a, color, thickness, .. }
                if *color == theme::ORANGE && *thickness > 1.1 =>
            {
                Some(a.0)
            }
            _ => None,
        });
        assert_eq!(start_x, play_x);
        assert!(start_x.is_some());
    }

    #[test]
    fn start_badge_hangs_into_the_lane_header() {
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
            selection: &ArrSelection::default(),
            playhead: 0,
            origin: 0,
            tempo: 120.0,
            sample_rate: 48_000.0,
            grid: MixGrid::Bar1,
            grid_enabled: false,
            viewing_take: false,
            home_take: None,
            drag_preview: None,
            readout: None,
            waveforms: None,
            show_selection: true,
            hide_clip_ids: &[],
        });
        let badge = cmds.iter().find_map(|c| match c {
            DrawCmd::RoundedRect { rect, color, radius }
                if *color == theme::METER_GREEN && *radius > 1.0 =>
            {
                Some(*rect)
            }
            _ => None,
        });
        let badge = badge.expect("START flag");
        let origin_x = x_of_frame(&layout, 0, 120.0, 48_000.0);
        assert!((badge.x - (origin_x - START_HIT_INSET)).abs() < 0.1);
        assert!((badge.w - START_HIT_W).abs() < 0.1);
        assert!(badge.x < layout.x + HEADER_W, "flag should overlap the lane headers");
    }

    #[test]
    fn frame_x_round_trips_at_wide_and_narrow_zoom() {
        let layout_at = |ppb: f32| ArrangementLayout {
            x: 148.0,
            y: 40.0,
            w: 800.0,
            h: 400.0,
            scroll_x: 120.0,
            scroll_y: 0.0,
            pixels_per_bar: ppb,
        };
        for ppb in [10.0, 48.0, 240.0, 4_000.0] {
            let layout = layout_at(ppb);
            for frame in [0, 24_000, 48_000, 192_000] {
                let x = x_of_frame(&layout, frame, 120.0, 48_000.0);
                let back = frame_at(x, &layout, 120.0, 48_000.0);
                assert!((back - frame).abs() <= 1, "ppb={ppb} frame={frame} x={x} back={back}");
            }
        }
    }

    #[test]
    fn time_ruler_matches_ableton_label_cadence() {
        // Live at ~2.5 min across ~1400px (~9 px/s) labels every 10 seconds.
        assert_eq!(time_tick_steps(9.3).major, 10.0);
        // Close zoom like the MixLink screenshot (~1.6s across ~1000px): 1s, not tenths.
        let close = time_tick_steps(625.0);
        assert_eq!(close.major, 1.0);
        assert!(close.mid >= 0.2);
        // Tenths stay unlabeled until each tenth is ~160px.
        assert!(time_tick_steps(1_200.0).major >= 0.5);
        assert_eq!(time_tick_steps(2_000.0).major, 0.1);
    }

    #[test]
    fn time_ruler_stays_sparse_at_close_zoom() {
        let track = MixTrack::empty(MixLane::Strip(0), Some("Ch 1".into()));
        // 120 BPM, 1 bar = 2s. 1250 px/bar → 625 px/s, ~1.6s of timeline.
        let cmds = paint(&ArrangementView {
            layout: ArrangementLayout {
                x: 0.0,
                y: 0.0,
                w: 1_100.0,
                h: 400.0,
                scroll_x: 0.0,
                scroll_y: 0.0,
                pixels_per_bar: 1_250.0,
            },
            mix: None,
            tracks: std::slice::from_ref(&track),
            selected_lane: None,
            selection: &ArrSelection::default(),
            playhead: 0,
            origin: 0,
            tempo: 120.0,
            sample_rate: 48_000.0,
            grid: MixGrid::Bar1,
            grid_enabled: false,
            viewing_take: false,
            home_take: None,
            drag_preview: None,
            readout: None,
            waveforms: None,
            show_selection: true,
            hide_clip_ids: &[],
        });
        let ruler_top = 400.0 - TIME_RULER_H;
        let labels: Vec<&str> = cmds
            .iter()
            .filter_map(|c| match c {
                DrawCmd::Text(t)
                    if t.monospaced && t.rect.y >= ruler_top + 6.0 && t.rect.x >= HEADER_W =>
                {
                    Some(t.text.as_str())
                }
                _ => None,
            })
            .collect();
        assert!(labels.len() <= 4, "close zoom should label seconds, not tenths: {labels:?}");
        assert!(labels.iter().any(|s| *s == "0:00"), "{labels:?}");
        assert!(labels.iter().all(|s| !s.contains('.')), "no tenth labels: {labels:?}");
        let ticks = cmds
            .iter()
            .filter(|c| match c {
                DrawCmd::Line { a, b, color, thickness } => {
                    *thickness <= 1.01
                        && (a.1 - (ruler_top + 1.0)).abs() < 0.2
                        && b.1 - a.1 <= 5.5
                        && color[3] <= 0.30
                }
                _ => false,
            })
            .count();
        assert!(ticks <= 12, "tick comb: {ticks}");
    }

    fn bar_labels(cmds: &[DrawCmd], layout_y: f32) -> Vec<String> {
        cmds.iter()
            .filter_map(|c| match c {
                DrawCmd::Text(t)
                    if t.monospaced
                        && t.rect.y >= layout_y + 6.0
                        && t.rect.y < layout_y + RULER_H
                        && t.rect.x >= HEADER_W
                        && t.text != "▶ START" =>
                {
                    Some(t.text.clone())
                }
                _ => None,
            })
            .collect()
    }

    fn paint_bars(ppb: f32, w: f32) -> Vec<DrawCmd> {
        let track = MixTrack::empty(MixLane::Strip(0), Some("Ch 1".into()));
        paint(&ArrangementView {
            layout: ArrangementLayout {
                x: 0.0,
                y: 0.0,
                w,
                h: 400.0,
                scroll_x: 0.0,
                scroll_y: 0.0,
                pixels_per_bar: ppb,
            },
            mix: None,
            tracks: std::slice::from_ref(&track),
            selected_lane: None,
            selection: &ArrSelection::default(),
            playhead: 0,
            origin: 0,
            tempo: 120.0,
            sample_rate: 48_000.0,
            grid: MixGrid::Bar1,
            grid_enabled: false,
            viewing_take: false,
            home_take: None,
            drag_preview: None,
            readout: None,
            waveforms: None,
            show_selection: true,
            hide_clip_ids: &[],
        })
    }

    #[test]
    fn bar_labels_follow_ableton_truncation() {
        assert_eq!(format_bar_label(0.0), "0");
        assert_eq!(format_bar_label(1.0), "1");
        assert_eq!(format_bar_label(2.25), "2.2");
        assert_eq!(format_bar_label(2.5), "2.3");
        assert_eq!(format_bar_label(2.75), "2.4");
        assert_eq!(format_bar_label(2.3125), "2.2.2");
        assert_eq!(format_bar_label(3.0), "3");
        assert_eq!(format_bar_label(-1.25), "-1.2");
    }

    #[test]
    fn bar_ruler_picks_ableton_zoom_steps() {
        assert_eq!(bar_tick_steps(2_000.0).label, 0.0625);
        assert_eq!(bar_tick_steps(180.0).label, 0.25);
        assert_eq!(bar_tick_steps(48.0).label, 1.0);
        assert!(bar_tick_steps(18.0).label >= 2.0);
    }

    #[test]
    fn zoom_grid_matches_painted_lines() {
        assert_eq!(zoom_grid_step(256.0), 0.0625);
        assert_eq!(zoom_grid_step(80.0), 0.25);
        assert_eq!(zoom_grid_step(48.0), 1.0);
        assert!(zoom_grid_step(10.0) >= 2.0);
    }

    #[test]
    fn bar_ruler_shows_beats_then_sixteenths() {
        let beat = bar_labels(&paint_bars(180.0, 1_100.0), 0.0);
        assert!(beat.iter().any(|s| s == "1"), "{beat:?}");
        assert!(beat.iter().any(|s| s == "1.2"), "{beat:?}");
        assert!(beat.iter().all(|s| s != "0" && !s.ends_with(".1")), "{beat:?}");

        let bars_only = bar_labels(&paint_bars(48.0, 800.0), 0.0);
        assert!(bars_only.iter().any(|s| s == "1"), "{bars_only:?}");
        assert!(bars_only.iter().all(|s| !s.contains('.')), "{bars_only:?}");

        let sixteenths = bar_labels(&paint_bars(1_600.0, 1_100.0), 0.0);
        assert!(sixteenths.iter().any(|s| s.contains(".2.")), "{sixteenths:?}");
    }
}
