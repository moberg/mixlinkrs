use analog::{AnalogEngine, MixAssign, ReturnLane, ALL_SEND_LANES};
use render::{DrawCmd, Rect};

use crate::theme::{self, Layout};
use crate::widgets;

#[derive(Clone, Copy, Debug)]
pub struct MixerLayout {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub ch_w: f32,
    pub main_w: f32,
    pub scroll_x: f32,
}

impl MixerLayout {
    pub fn new(x: f32, y: f32, w: f32, h: f32, send_count: usize) -> Self {
        let n_in = 8.0;
        let n_ret = send_count.max(2).min(6) as f32;
        let n_bus = 2.0;
        let n_main = 1.0;
        let available = (w - Self::gutter()).max(100.0);
        let ch_w = Layout::channel_width(available, n_in + n_ret + n_bus, n_main);
        Self { x, y, w, h, ch_w, main_w: ch_w * Layout::MAIN_FACTOR, scroll_x: 0.0 }
    }

    /// Leading inset plus the three 1pt group dividers.
    fn gutter() -> f32 {
        Layout::MIXER_LEADING + 4.0 + 3.0
    }

    pub fn content_width(&self, send_count: usize) -> f32 {
        let n_send = send_count.max(2).min(6) as f32;
        Self::gutter() + self.ch_w * (8.0 + n_send + 2.0) + self.main_w
    }

    pub fn max_scroll_x(&self, send_count: usize) -> f32 {
        (self.content_width(send_count) - self.w).max(0.0)
    }

    /// Window width that fits `send_count` returns at min channel width with no horizontal scroll.
    pub fn min_window_width(send_count: usize) -> f32 {
        let n_send = send_count.max(2).min(6) as f32;
        let mixer = Self::gutter() + Layout::MIN_CH * (8.0 + n_send + 2.0 + Layout::MAIN_FACTOR);
        (mixer + Layout::SIDEBAR_WIDTH).ceil()
    }
}

#[derive(Clone, Copy, Debug)]
pub enum StripKind {
    Input(usize),
    Return(ReturnLane),
    Main,
}

pub struct MixerView<'a> {
    pub engine: &'a AnalogEngine,
    pub peaks: &'a [f32],
    pub layout: MixerLayout,
}

#[derive(Clone, Copy, Debug)]
pub enum MixerExtraHit {
    AddReturn,
    RemoveReturn,
    ControlWithPan,
}

pub fn paint(view: &MixerView<'_>) -> (Vec<DrawCmd>, Vec<(Rect, MixerExtraHit)>) {
    let mut cmds = Vec::with_capacity(4096);
    let mut extras = Vec::new();
    let l = view.layout;
    let clip = Rect { x: l.x, y: l.y, w: l.w, h: l.h };
    cmds.push(DrawCmd::Layer);
    cmds.push(DrawCmd::Clip { rect: clip });
    let sends: Vec<ReturnLane> = view.engine.config.visible_send_lanes();
    theme::mixer_chassis(
        &mut cmds,
        Rect { x: l.x, y: l.y, w: l.w, h: l.h },
        Layout::upper_faceplate_height(&sends),
    );

    let mut x = l.x + Layout::MIXER_LEADING + 4.0 - l.scroll_x;
    paint_name_bar_rails(&mut cmds, x, l.y, l.ch_w, &sends);
    group_header(&mut cmds, x, l.y, l.ch_w * 8.0, "Channels", None, &mut extras);
    for i in 0..8 {
        paint_strip(
            &mut cmds,
            view,
            StripKind::Input(i),
            x + i as f32 * l.ch_w,
            l.ch_w,
            &sends,
            &mut extras,
        );
    }
    x += l.ch_w * 8.0;
    theme::fill(&mut cmds, Rect { x, y: l.y, w: 1.0, h: l.h }, [0.05, 0.05, 0.05, 1.0]);
    x += 1.0;
    group_header(
        &mut cmds,
        x,
        l.y,
        l.ch_w * sends.len() as f32,
        "Effect returns",
        Some((view.engine.config.effect_return_count, analog::MAX_SEND_COUNT)),
        &mut extras,
    );
    for (i, lane) in sends.iter().enumerate() {
        paint_strip(
            &mut cmds,
            view,
            StripKind::Return(*lane),
            x + i as f32 * l.ch_w,
            l.ch_w,
            &sends,
            &mut extras,
        );
    }
    x += l.ch_w * sends.len() as f32;
    theme::fill(&mut cmds, Rect { x, y: l.y, w: 1.0, h: l.h }, [0.05, 0.05, 0.05, 1.0]);
    x += 1.0;
    group_header(&mut cmds, x, l.y, l.ch_w * 2.0, "Bus returns", None, &mut extras);
    paint_strip(
        &mut cmds,
        view,
        StripKind::Return(ReturnLane::Bus1),
        x,
        l.ch_w,
        &sends,
        &mut extras,
    );
    paint_strip(
        &mut cmds,
        view,
        StripKind::Return(ReturnLane::Bus2),
        x + l.ch_w,
        l.ch_w,
        &sends,
        &mut extras,
    );
    x += l.ch_w * 2.0;
    theme::fill(&mut cmds, Rect { x, y: l.y, w: 1.0, h: l.h }, [0.05, 0.05, 0.05, 1.0]);
    x += 1.0;
    group_header(&mut cmds, x, l.y, l.main_w, "Main", None, &mut extras);
    paint_strip(&mut cmds, view, StripKind::Main, x, l.main_w, &sends, &mut extras);
    extras.retain(|(r, _)| {
        r.x < clip.x + clip.w && r.x + r.w > clip.x && r.y < clip.y + clip.h && r.y + r.h > clip.y
    });
    let max_scroll = l.max_scroll_x(sends.len());
    if max_scroll > 0.5 {
        widgets::scrollbar_h(
            &mut cmds,
            Rect { x: l.x + 6.0, y: l.y + l.h - 10.0, w: (l.w - 12.0).max(16.0), h: 6.0 },
            l.scroll_x,
            max_scroll,
        );
    }
    (cmds, extras)
}

fn group_header(
    cmds: &mut Vec<DrawCmd>,
    x: f32,
    y: f32,
    w: f32,
    title: &str,
    plus_minus: Option<(i32, i32)>,
    extras: &mut Vec<(Rect, MixerExtraHit)>,
) {
    theme::text(
        cmds,
        Rect { x: x + 8.0, y: y + 18.0, w: w - 12.0, h: 22.0 },
        title,
        11.5,
        theme::SECONDARY_TEXT,
        true,
    );
    if let Some((count, max)) = plus_minus {
        let bx = x + w - 52.0;
        if count > 2 {
            widgets::icon_pad(cmds, Rect { x: bx, y: y + 18.0, w: 20.0, h: 20.0 }, "−", true);
            extras
                .push((Rect { x: bx, y: y + 18.0, w: 20.0, h: 20.0 }, MixerExtraHit::RemoveReturn));
        }
        if count < max {
            widgets::icon_pad(
                cmds,
                Rect { x: bx + 24.0, y: y + 18.0, w: 20.0, h: 20.0 },
                "+",
                true,
            );
            extras.push((
                Rect { x: bx + 24.0, y: y + 18.0, w: 20.0, h: 20.0 },
                MixerExtraHit::AddReturn,
            ));
        }
    }
    theme::seam_h(cmds, x, y + Layout::GROUP_HEADER - 2.0, w, false);
}

fn paint_strip(
    cmds: &mut Vec<DrawCmd>,
    view: &MixerView<'_>,
    kind: StripKind,
    x: f32,
    w: f32,
    sends: &[ReturnLane],
    extras: &mut Vec<(Rect, MixerExtraHit)>,
) {
    let l = view.layout;
    let y0 = l.y + Layout::GROUP_HEADER;

    let (fader, pan, name, id, has_sends, dim, enabled) = match kind {
        StripKind::Input(i) => {
            let s = &view.engine.surface.strips[i];
            let id = view.engine.config.strips[i].channel_id();
            let enabled = view.engine.config.strips[i].enabled;
            (
                s.fader,
                s.pan,
                view.engine.selected_name(id),
                format!("{}", i + 1),
                true,
                !enabled,
                enabled,
            )
        }
        StripKind::Return(lane) => {
            let r = view.engine.surface.returns.iter().find(|r| r.id == lane as i32);
            let f = r.map(|r| r.fader).unwrap_or(0.0);
            let p = r.map(|r| r.pan).unwrap_or(0.5);
            let unused = lane.is_send() && view.engine.config.chain_ref(lane).is_none();
            let enabled = view.engine.config.is_return_enabled(lane);
            (
                f,
                p,
                view.engine.return_display_name(lane),
                lane.strip_title().to_string(),
                false,
                unused || !enabled,
                enabled,
            )
        }
        StripKind::Main => {
            (view.engine.mixer.main_fader, 0.5, "Main".into(), "M".into(), false, false, true)
        }
    };

    // MixLink dims strip chrome via opacity; the MixerChassis stays fully lit.
    // A per-strip black fill would hide the continuous grain.

    let mut y = y0;
    if !matches!(kind, StripKind::Main) {
        widgets::enable_toggle(
            cmds,
            Rect {
                x: x + w * 0.5 - Layout::ENABLE_W * 0.5,
                y: y + (Layout::ENABLE_ROW - Layout::ENABLE_H) * 0.5,
                w: Layout::ENABLE_W,
                h: Layout::ENABLE_H,
            },
            enabled,
        );
    }
    y += Layout::ENABLE_ROW;

    let first_input = matches!(kind, StripKind::Input(0));
    if has_sends {
        if let StripKind::Input(i) = kind {
            for lane in sends {
                name_bar(cmds, x, y, w, first_input, Some(*lane), view.engine);
                y += Layout::SEND_NAME_BAR;
                let aux = view.engine.surface.strips[i].aux(*lane);
                let row_h = Layout::send_row_h(*lane);
                theme::faceplate_cell(
                    cmds,
                    Rect { x, y, w, h: row_h },
                    Some(*lane) == sends.last().copied(),
                    false,
                );
                let knob = send_knob_rect(x, y, w, row_h);
                widgets::knob(
                    cmds,
                    knob.x,
                    knob.y,
                    Layout::SEND_KNOB,
                    aux,
                    widgets::KnobKind::Send(theme::send_color(*lane)),
                    Some(&theme::fader_value_text(aux)),
                );
                y += row_h;
            }
        }
    } else {
        for lane in sends {
            name_bar(cmds, x, y, w, false, None, view.engine);
            y += Layout::SEND_NAME_BAR;
            if matches!(kind, StripKind::Return(ReturnLane::SendC))
                && sends.len() >= 3
                && *lane == ReturnLane::SendC
            {
                let box_r = Rect { x: x + 10.0, y: y + 10.0, w: w - 16.0, h: 24.0 };
                widgets::checkbox(
                    cmds,
                    box_r.x,
                    box_r.y,
                    view.engine.config.pan_knobs_control_send_c,
                    "Control with Pan",
                );
                extras.push((box_r, MixerExtraHit::ControlWithPan));
            }
            y += Layout::send_row_h(*lane);
        }
    }

    name_bar(cmds, x, y, w, first_input, None, view.engine);
    y += Layout::SEND_NAME_BAR;
    theme::faceplate_cell(cmds, Rect { x, y, w, h: Layout::PAN_ROW }, false, true);
    if !matches!(kind, StripKind::Main) {
        let knob = pan_knob_rect(x, y, w);
        widgets::knob(cmds, knob.x, knob.y, Layout::PAN_KNOB, pan, widgets::KnobKind::Pan, None);
    }
    theme::text_center(
        cmds,
        Rect { x, y: y + Layout::PAN_ROW - 18.0, w, h: 16.0 },
        id,
        11.0,
        theme::SECONDARY_TEXT,
        true,
    );
    y += Layout::PAN_ROW;

    let bay_h = (l.y + l.h - y - Layout::NAME_ROW - Layout::BUTTON_STACK).max(80.0);
    theme::channel_bay_shading(
        cmds,
        Rect { x, y, w, h: bay_h + Layout::NAME_ROW + Layout::BUTTON_STACK },
    );
    paint_fader(
        cmds,
        x,
        y,
        w,
        bay_h,
        fader,
        peak_for(view, kind),
        view.engine.config.hardware_strips,
    );
    y += bay_h;

    widgets::strip_name_label(
        cmds,
        Rect { x, y, w, h: Layout::NAME_ROW },
        &name,
        widgets::StripNameStyle { diamond: !matches!(kind, StripKind::Main), dim, color: None },
    );
    y += Layout::NAME_ROW;

    match kind {
        StripKind::Input(i) => {
            let assign = view.engine.surface.strips[i].assign;
            let id = view.engine.config.strips[i].channel_id();
            let muted = view.engine.mixer.channel(id).map(|c| c.mute).unwrap_or(false);
            let soloed = view.engine.mixer.channel(id).map(|c| c.solo).unwrap_or(false);
            let (solo, mute) = button_pair_rects(x, y, w);
            widgets::hardware_pad(cmds, solo, "SOLO", soloed, theme::METER_GREEN);
            widgets::hardware_pad(cmds, mute, "MUTE", muted, theme::METER_RED);
            let (bus1, bus2) =
                button_pair_rects(x, y + Layout::BUTTON_H + Layout::BUTTON_ROW_GAP, w);
            widgets::hardware_pad(cmds, bus1, "BUS 1", assign == MixAssign::Bus1, theme::AMBER);
            widgets::hardware_pad(cmds, bus2, "BUS 2", assign == MixAssign::Bus2, theme::METER_RED);
        }
        StripKind::Return(lane) => {
            let (solo, mute) = button_pair_rects(x, y, w);
            widgets::hardware_pad(
                cmds,
                solo,
                "SOLO",
                view.engine.return_soloed(lane),
                theme::METER_GREEN,
            );
            widgets::hardware_pad(
                cmds,
                mute,
                "MUTE",
                view.engine.return_muted(lane),
                theme::METER_RED,
            );
        }
        StripKind::Main => {}
    }

    // MixLink `ChannelSeam`: enable row always; body skips the send stack on
    // non-last effect/bus returns so the upper faceplate stays one sheet.
    theme::channel_seam(cmds, x + w - 1.0, y0, Layout::ENABLE_ROW, false);
    let send_area: f32 = sends.iter().copied().map(Layout::send_lane_h).sum();
    let skip_upper = match kind {
        StripKind::Return(lane) if lane.is_send() => Some(lane) != sends.last().copied(),
        StripKind::Return(ReturnLane::Bus1) => true,
        _ => false,
    };
    let body_y = y0 + Layout::ENABLE_ROW + if skip_upper { send_area } else { 0.0 };
    let body_h = (l.y + l.h - body_y).max(0.0);
    if body_h > 0.0 {
        theme::channel_seam(cmds, x + w - 1.0, body_y, body_h, false);
    }
}

/// MixLink `SendLaneNameBar` plates as one rail per row (not a fill per strip).
/// Send plates cover the leading gutter + input group (`MixerLeadingNameGutter`).
fn paint_name_bar_rails(
    cmds: &mut Vec<DrawCmd>,
    origin: f32,
    mixer_y: f32,
    ch_w: f32,
    sends: &[ReturnLane],
) {
    let gutter = Layout::MIXER_LEADING + 4.0;
    let rail_x = origin - gutter;
    let inputs_w = gutter + ch_w * 8.0;
    let mut y = mixer_y + Layout::GROUP_HEADER + Layout::ENABLE_ROW;
    for lane in sends {
        name_bar_plate(cmds, rail_x, y, inputs_w);
        y += Layout::send_lane_h(*lane);
    }
    // MixLink `showsPlate: showsPan` — every strip except Main.
    let pan_w = inputs_w + 1.0 + ch_w * sends.len() as f32 + 1.0 + ch_w * 2.0;
    name_bar_plate(cmds, rail_x, y, pan_w);
}

fn name_bar_plate(cmds: &mut Vec<DrawCmd>, x: f32, y: f32, w: f32) {
    // MixLink `SendLaneNameBar`: `Color(white: 0.105)` + 1pt black/white edges.
    theme::fill(cmds, Rect { x, y, w, h: Layout::SEND_NAME_BAR }, [0.105, 0.105, 0.105, 1.0]);
    theme::fill(cmds, Rect { x, y, w, h: 1.0 }, [0.0, 0.0, 0.0, 0.40]);
    theme::fill(
        cmds,
        Rect { x, y: y + Layout::SEND_NAME_BAR - 1.0, w, h: 1.0 },
        [1.0, 1.0, 1.0, 0.04],
    );
}

fn name_bar(
    cmds: &mut Vec<DrawCmd>,
    x: f32,
    y: f32,
    w: f32,
    show_title: bool,
    lane: Option<ReturnLane>,
    engine: &AnalogEngine,
) {
    if show_title {
        let label = if let Some(lane) = lane {
            format!("SEND {} · {}", lane.strip_title(), engine.return_display_name(lane))
        } else {
            "PAN".into()
        };
        theme::text(
            cmds,
            Rect { x: x + 6.0, y, w: w - 8.0, h: Layout::SEND_NAME_BAR },
            label,
            9.0,
            theme::SECONDARY_TEXT,
            false,
        );
    }
}

fn peak_for(view: &MixerView<'_>, kind: StripKind) -> f32 {
    match kind {
        StripKind::Input(i) => view.peaks.get(i).copied().unwrap_or(0.0),
        StripKind::Return(lane) => view.peaks.get(8 + lane as usize).copied().unwrap_or(0.0),
        StripKind::Main => view.peaks.get(16).copied().unwrap_or(0.0),
    }
}

/// MixLink `ChannelStripView.faderBay`:
/// `[meter + leading ticks | slot reserve | trailing ticks+labels]`, cap overlayed on the slot.
#[derive(Clone, Copy, Debug)]
pub struct FaderBay {
    pub track_top: f32,
    pub track_h: f32,
    pub meter: Rect,
    pub leading_x: f32,
    pub trailing_x: f32,
    pub cap_cx: f32,
    pub hit: Rect,
    pub rail_top: f32,
    pub rail_bot: f32,
}

impl FaderBay {
    pub fn layout(strip_x: f32, strip_y: f32, strip_w: f32, bay_h: f32) -> Self {
        let inner_w = (strip_w - Layout::FADER_BAY_PAD_X * 2.0).max(1.0);
        let slot_reserve = Layout::FADER_SLOT + 8.0;
        let flex = ((inner_w - slot_reserve) * 0.5).max(0.0);
        let origin = strip_x + Layout::FADER_BAY_PAD_X;
        let meter_group_w = Layout::METER_HOUSING + 2.0 + Layout::SCALE_LEADING;
        let meter_group_right = origin + flex;
        let meter_x = meter_group_right - meter_group_w;
        let leading_x = meter_group_right - Layout::SCALE_LEADING;
        let slot_left = origin + flex;
        let cap_cx = slot_left + slot_reserve * 0.5;
        let trailing_x = slot_left + slot_reserve;
        let track_top = strip_y + Layout::FADER_BAY_PAD_Y;
        let track_h = (bay_h - Layout::FADER_BAY_PAD_Y * 2.0).max(Layout::FADER_CAP_H);
        let travel = (track_h - Layout::FADER_CAP_H).max(0.0);
        Self {
            track_top,
            track_h,
            meter: Rect { x: meter_x, y: track_top, w: Layout::METER_HOUSING, h: track_h },
            leading_x,
            trailing_x,
            cap_cx,
            hit: Rect {
                x: cap_cx - Layout::FADER_HIT_W * 0.5,
                y: track_top,
                w: Layout::FADER_HIT_W,
                h: track_h,
            },
            rail_top: track_top,
            rail_bot: track_top + travel,
        }
    }
}

pub fn paint_fader(
    cmds: &mut Vec<DrawCmd>,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    lin: f32,
    peak: f32,
    hardware: bool,
) {
    let bay = FaderBay::layout(x, y, w, h);
    widgets::level_meter(cmds, bay.meter, peak);
    widgets::decibel_scale(
        cmds,
        bay.leading_x,
        bay.track_top,
        bay.track_h,
        widgets::ScalePlacement::Leading,
    );

    let slot_outer = Layout::FADER_SLOT + 3.0;
    let slot_x = bay.cap_cx - Layout::FADER_SLOT * 0.5;
    cmds.push(DrawCmd::RoundedRect {
        rect: Rect {
            x: bay.cap_cx - slot_outer * 0.5,
            y: bay.track_top,
            w: slot_outer,
            h: bay.track_h,
        },
        color: [0.04, 0.04, 0.04, 1.0],
        radius: slot_outer * 0.5,
    });
    cmds.push(DrawCmd::RoundedRect {
        rect: Rect { x: slot_x, y: bay.track_top, w: Layout::FADER_SLOT, h: bay.track_h },
        color: [0.01, 0.01, 0.01, 1.0],
        radius: Layout::FADER_SLOT * 0.5,
    });
    theme::fill(
        cmds,
        Rect { x: slot_x, y: bay.track_top + 1.0, w: 1.2, h: bay.track_h - 2.0 },
        [0.0, 0.0, 0.0, 0.55],
    );
    theme::fill(
        cmds,
        Rect {
            x: slot_x + Layout::FADER_SLOT - 0.8,
            y: bay.track_top + 1.0,
            w: 0.8,
            h: bay.track_h - 2.0,
        },
        [1.0, 1.0, 1.0, 0.08],
    );

    widgets::decibel_scale(
        cmds,
        bay.trailing_x,
        bay.track_top,
        bay.track_h,
        widgets::ScalePlacement::Trailing,
    );

    let travel = (bay.track_h - Layout::FADER_CAP_H).max(0.0);
    let cap_y = bay.track_top + travel * (1.0 - lin.clamp(0.0, 1.0));
    widgets::fader_cap(
        cmds,
        Rect {
            x: bay.cap_cx - Layout::FADER_CAP_W * 0.5,
            y: cap_y,
            w: Layout::FADER_CAP_W,
            h: Layout::FADER_CAP_H,
        },
        hardware,
    );
}

/// MixLink `KnobView` VStack spacing (2) + dB `Text` (MixLinkRs paints 12pt).
const SEND_KNOB_LABEL_STACK: f32 = 2.0 + 12.0;
/// MixLink `panSlot` `.padding(.top, 8)`.
const PAN_KNOB_TOP_PAD: f32 = 8.0;
/// MixLink `KnobView` `.frame(width: size+8, height: size+8)` — disc inset.
const KNOB_FRAME_INSET: f32 = 4.0;

/// Send disc in the control row (below the name bar).
///
/// MixLink `SendControlView` has no extra padding; `.frame(height: sendRowHeight)`
/// default-centers the `KnobView` VStack (disc frame `size+8`, spacing 2, dB).
/// MixLinkRs paints the 48pt disc (not the +8 frame), so
/// `knob_y = row_y + (row_h - 48 - 14) * 0.5` equals MixLink's disc top
/// `(row_h - 70) / 2 + 4` — send A 14, other lanes 15. The dB sits in the
/// lower padding without un-centering that stack.
pub fn send_knob_rect(strip_x: f32, row_y: f32, strip_w: f32, row_h: f32) -> Rect {
    let d = Layout::SEND_KNOB;
    Rect {
        x: strip_x + (strip_w - d) * 0.5,
        y: row_y + (row_h - d - SEND_KNOB_LABEL_STACK) * 0.5,
        w: d,
        h: d,
    }
}

/// Pan disc in the pan control row (below the PAN name bar).
///
/// MixLink pins `KnobView` under `.padding(.top, 8)` then centers the 40pt disc
/// in the `size+8` frame (4pt inset). Remaining space is the channel ID
/// (`maxHeight: .infinity`) — the disc is not shifted by the ID. MixLinkRs
/// `knob_y = row_y + 8 + 4` (was `row_y + 6`).
pub fn pan_knob_rect(strip_x: f32, row_y: f32, strip_w: f32) -> Rect {
    let d = Layout::PAN_KNOB;
    Rect {
        x: strip_x + (strip_w - d) * 0.5,
        y: row_y + PAN_KNOB_TOP_PAD + KNOB_FRAME_INSET,
        w: d,
        h: d,
    }
}

pub fn button_pair_rects(strip_x: f32, row_y: f32, strip_w: f32) -> (Rect, Rect) {
    let inset = 8.0;
    let gap = Layout::BUTTON_PAIR_GAP;
    let inner = (strip_w - inset * 2.0).max(gap + 4.0);
    let bw = ((inner - gap) * 0.5).max(2.0);
    (
        Rect { x: strip_x + inset, y: row_y, w: bw, h: Layout::BUTTON_H },
        Rect { x: strip_x + inset + bw + gap, y: row_y, w: bw, h: Layout::BUTTON_H },
    )
}

/// MixLink `KnobView` `.contentShape` on the `size+8` frame around the disc.
pub fn knob_hit_rect(disc: Rect) -> Rect {
    Rect {
        x: disc.x - KNOB_FRAME_INSET,
        y: disc.y - KNOB_FRAME_INSET,
        w: disc.w + KNOB_FRAME_INSET * 2.0,
        h: disc.h + KNOB_FRAME_INSET * 2.0,
    }
}

pub fn strip_frame(layout: &MixerLayout, send_count: usize, kind: StripKind) -> (f32, f32) {
    let n_send = send_count.max(2).min(6);
    let mut sx = layout.x + Layout::MIXER_LEADING + 4.0 - layout.scroll_x;
    match kind {
        StripKind::Input(i) => (sx + i as f32 * layout.ch_w, layout.ch_w),
        StripKind::Return(lane) if lane.is_send() => {
            sx += layout.ch_w * 8.0 + 1.0;
            let idx = ALL_SEND_LANES.iter().take(n_send).position(|&l| l == lane).unwrap_or(0);
            (sx + idx as f32 * layout.ch_w, layout.ch_w)
        }
        StripKind::Return(ReturnLane::Bus1) => {
            sx += layout.ch_w * 8.0 + 1.0 + layout.ch_w * n_send as f32 + 1.0;
            (sx, layout.ch_w)
        }
        StripKind::Return(ReturnLane::Bus2) => {
            sx += layout.ch_w * 8.0 + 1.0 + layout.ch_w * n_send as f32 + 1.0;
            (sx + layout.ch_w, layout.ch_w)
        }
        StripKind::Return(_) => (sx, layout.ch_w),
        StripKind::Main => {
            sx += layout.ch_w * 8.0
                + 1.0
                + layout.ch_w * n_send as f32
                + 1.0
                + layout.ch_w * 2.0
                + 1.0;
            (sx, layout.main_w)
        }
    }
}

pub fn fader_bay_frame(layout: &MixerLayout, send_count: usize) -> (f32, f32) {
    let n = send_count.max(2).min(6);
    let y = layout.y
        + Layout::GROUP_HEADER
        + Layout::ENABLE_ROW
        + ALL_SEND_LANES.iter().take(n).copied().map(Layout::send_lane_h).sum::<f32>()
        + Layout::SEND_NAME_BAR
        + Layout::PAN_ROW;
    let h = (layout.y + layout.h - y - Layout::NAME_ROW - Layout::BUTTON_STACK).max(80.0);
    (y, h)
}

pub fn fader_rail(
    layout: &MixerLayout,
    send_count: usize,
    strip_x: f32,
    strip_w: f32,
) -> (f32, f32) {
    let (y, h) = fader_bay_frame(layout, send_count);
    let bay = FaderBay::layout(strip_x, y, strip_w, h);
    (bay.rail_top, bay.rail_bot)
}

pub fn strip_at(layout: &MixerLayout, send_count: usize, x: f32, _y: f32) -> Option<StripKind> {
    let mut sx = layout.x + Layout::MIXER_LEADING + 4.0 - layout.scroll_x;
    if x < sx {
        return None;
    }
    for i in 0..8 {
        if x >= sx + i as f32 * layout.ch_w && x < sx + (i + 1) as f32 * layout.ch_w {
            return Some(StripKind::Input(i));
        }
    }
    sx += layout.ch_w * 8.0 + 1.0;
    let sends = ALL_SEND_LANES.iter().copied().take(send_count.max(2).min(6)).collect::<Vec<_>>();
    for (i, lane) in sends.iter().enumerate() {
        if x >= sx + i as f32 * layout.ch_w && x < sx + (i + 1) as f32 * layout.ch_w {
            return Some(StripKind::Return(*lane));
        }
    }
    sx += layout.ch_w * sends.len() as f32 + 1.0;
    if x >= sx && x < sx + layout.ch_w {
        return Some(StripKind::Return(ReturnLane::Bus1));
    }
    if x >= sx + layout.ch_w && x < sx + layout.ch_w * 2.0 {
        return Some(StripKind::Return(ReturnLane::Bus2));
    }
    sx += layout.ch_w * 2.0 + 1.0;
    if x >= sx && x < sx + layout.main_w {
        return Some(StripKind::Main);
    }
    None
}

pub fn control_with_pan_rect(layout: &MixerLayout, send_count: usize) -> Option<Rect> {
    if send_count < 3 {
        return None;
    }
    let mut sx = layout.x + Layout::MIXER_LEADING + 4.0 - layout.scroll_x;
    sx += layout.ch_w * 8.0 + 1.0;
    let sends = ALL_SEND_LANES.iter().copied().take(send_count.max(2).min(6)).collect::<Vec<_>>();
    let Some(i) = sends.iter().position(|l| *l == ReturnLane::SendC) else {
        return None;
    };
    let x = sx + i as f32 * layout.ch_w;
    let mut y = layout.y + Layout::GROUP_HEADER + Layout::ENABLE_ROW;
    for lane in &sends {
        if *lane == ReturnLane::SendC {
            y += Layout::SEND_NAME_BAR;
            return Some(Rect { x: x + 8.0, y: y + 6.0, w: layout.ch_w - 12.0, h: 24.0 });
        }
        y += Layout::SEND_NAME_BAR + Layout::send_row_h(*lane);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_knob_y_matches_mixlink_centered_stack() {
        // MixLink: no send padding; disc top = (row_h - 70) / 2 + 4.
        assert_eq!(send_knob_rect(0.0, 0.0, 100.0, Layout::SEND_ROW_A).y, 14.0);
        assert_eq!(send_knob_rect(0.0, 0.0, 100.0, Layout::SEND_ROW).y, 15.0);
        assert_eq!(
            send_knob_rect(0.0, 0.0, 100.0, Layout::SEND_ROW_A).x,
            (100.0 - Layout::SEND_KNOB) * 0.5
        );
    }

    #[test]
    fn pan_knob_y_matches_mixlink_top_pad_plus_frame_inset() {
        // MixLink: padding(.top, 8) + disc centered in size+8 → 12.
        assert_eq!(pan_knob_rect(0.0, 0.0, 100.0).y, 12.0);
        assert_eq!(pan_knob_rect(0.0, 0.0, 100.0).x, (100.0 - Layout::PAN_KNOB) * 0.5);
    }

    #[test]
    fn narrow_mixer_can_scroll_horizontally() {
        let layout = MixerLayout::new(0.0, 0.0, 400.0, 600.0, 2);
        assert!(layout.content_width(2) > layout.w);
        assert!(layout.max_scroll_x(2) > 0.5);
        let wide = MixerLayout::new(0.0, 0.0, 2400.0, 600.0, 2);
        assert_eq!(wide.max_scroll_x(2), 0.0);
    }

    #[test]
    fn three_sends_fit_at_min_window_width() {
        let w = MixerLayout::min_window_width(3);
        let body = w - Layout::SIDEBAR_WIDTH;
        let layout = MixerLayout::new(0.0, 0.0, body, 600.0, 3);
        assert!(
            layout.max_scroll_x(3) < 0.5,
            "max_scroll={} at window {w}",
            layout.max_scroll_x(3)
        );
    }
}
