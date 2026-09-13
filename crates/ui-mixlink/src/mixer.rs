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
        Layout::MIXER_LEADING + 3.0
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
    pub deck: crate::deck::DeckView,
}

#[derive(Clone, Copy, Debug)]
pub enum MixerExtraHit {
    AddReturn,
    RemoveReturn,
    ControlWithPan,
    Record,
}

pub fn paint(view: &MixerView<'_>) -> (Vec<DrawCmd>, Vec<(Rect, MixerExtraHit)>) {
    let mut cmds = Vec::with_capacity(4096);
    let mut extras = Vec::new();
    let l = view.layout;
    let clip = Rect { x: l.x, y: l.y, w: l.w, h: l.h };
    cmds.push(DrawCmd::Layer);
    cmds.push(DrawCmd::Clip { rect: clip });
    let sends: Vec<ReturnLane> = view.engine.config.visible_send_lanes();
    let mut x = mixer_origin(&l);
    let fx_x = x + l.ch_w * 8.0 + 1.0;
    let bus_x = fx_x + l.ch_w * sends.len() as f32 + 1.0;
    let main_x = bus_x + l.ch_w * 2.0 + 1.0;
    let strips_right = main_x + l.main_w;
    let chassis_w = (strips_right - l.x).clamp(0.0, l.w);
    theme::mixer_chassis(
        &mut cmds,
        Rect { x: l.x, y: l.y, w: chassis_w, h: l.h },
        Layout::upper_faceplate_height(&sends),
    );
    theme::mixer_overflow(
        &mut cmds,
        Rect { x: l.x + chassis_w, y: l.y, w: (l.w - chassis_w).max(0.0), h: l.h },
    );

    paint_name_bar_rails(&mut cmds, x, l.y, l.ch_w, l.main_w, &sends);
    let pan_bar_y = pan_name_bar_y(&l, sends.len());
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
    group_divider(&mut cmds, x, l.y, l.h, pan_bar_y);
    x += 1.0;
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
    group_divider(&mut cmds, x, l.y, l.h, pan_bar_y);
    x += 1.0;
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
    group_divider(&mut cmds, x, l.y, l.h, pan_bar_y);
    x += 1.0;
    paint_strip(&mut cmds, view, StripKind::Main, x, l.main_w, &sends, &mut extras);
    paint_section_title(
        &mut cmds,
        fx_x,
        pan_bar_y,
        l.ch_w * sends.len() as f32,
        "Effect returns",
        Some((view.engine.config.effect_return_count, analog::MAX_SEND_COUNT)),
        &mut extras,
    );
    paint_section_title(&mut cmds, bus_x, pan_bar_y, l.ch_w * 2.0, "Bus returns", None, &mut extras);
    paint_section_title(&mut cmds, main_x, pan_bar_y, l.main_w, "Main", None, &mut extras);
    if let Some(hit) =
        crate::deck::paint(&mut cmds, crate::deck::recorder_bay(&l, sends.len()), &view.deck)
    {
        extras.push(hit);
    }
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

/// Group split (inputs | FX | bus | Main). Starts at the PAN title row so the
/// empty send-stack above returns/Main stays one sheet, then runs through the
/// leftover bay and the faders.
fn group_divider(cmds: &mut Vec<DrawCmd>, x: f32, mixer_y: f32, mixer_h: f32, from_y: f32) {
    let y = from_y.max(mixer_y);
    let h = (mixer_y + mixer_h - y).max(0.0);
    if h > 0.0 {
        theme::fill(cmds, Rect { x, y, w: 1.0, h }, [0.05, 0.05, 0.05, 1.0]);
    }
}

/// Compact steppers on the PAN name bar — not `icon_pad` (well + shadow overflow).
const SECTION_STEPPER: f32 = 10.0;
const SECTION_STEPPER_GAP: f32 = 2.0;
const SECTION_TITLE_STEPPER_GAP: f32 = 3.0;

/// Section labels on the PAN name bar (Effect returns / Bus returns / Main).
fn paint_section_title(
    cmds: &mut Vec<DrawCmd>,
    x: f32,
    y: f32,
    w: f32,
    title: &str,
    plus_minus: Option<(i32, i32)>,
    extras: &mut Vec<(Rect, MixerExtraHit)>,
) {
    let title_x = x + 6.0;
    let title_w = section_label_advance(title).min((w - 8.0).max(8.0));
    theme::text(
        cmds,
        Rect { x: title_x, y, w: title_w, h: Layout::SEND_NAME_BAR },
        title,
        SEND_TITLE_SIZE,
        theme::SECONDARY_TEXT,
        false,
    );
    if let Some((count, max)) = plus_minus {
        let by = y + (Layout::SEND_NAME_BAR - SECTION_STEPPER) * 0.5;
        let mut bx = title_x + title_w + SECTION_TITLE_STEPPER_GAP;
        if count > 2 {
            let r = Rect { x: bx, y: by, w: SECTION_STEPPER, h: SECTION_STEPPER };
            paint_section_stepper(cmds, r, "−");
            extras.push((r, MixerExtraHit::RemoveReturn));
            bx += SECTION_STEPPER + SECTION_STEPPER_GAP;
        }
        if count < max {
            let r = Rect { x: bx, y: by, w: SECTION_STEPPER, h: SECTION_STEPPER };
            paint_section_stepper(cmds, r, "+");
            extras.push((r, MixerExtraHit::AddReturn));
        }
    }
}

fn paint_section_stepper(cmds: &mut Vec<DrawCmd>, rect: Rect, glyph: &str) {
    theme::fill(cmds, rect, [0.11, 0.11, 0.12, 1.0]);
    theme::fill(cmds, Rect { x: rect.x, y: rect.y, w: rect.w, h: 1.0 }, [1.0, 1.0, 1.0, 0.10]);
    theme::fill(
        cmds,
        Rect { x: rect.x, y: rect.y + rect.h - 1.0, w: rect.w, h: 1.0 },
        [0.0, 0.0, 0.0, 0.45],
    );
    theme::text_center(cmds, rect, glyph, 8.0, [1.0, 1.0, 1.0, 0.72], true);
}

/// Tight 9pt run so the steppers sit against the glyphs, not a wide estimate.
fn section_label_advance(s: &str) -> f32 {
    s.chars()
        .map(|ch| {
            SEND_TITLE_SIZE
                * match ch {
                    ' ' => 0.28,
                    'i' | 'l' | 'I' | 'j' | 't' | 'f' | 'r' | '.' | '·' => 0.38,
                    'm' | 'M' | 'w' | 'W' => 0.90,
                    _ => 0.56,
                }
        })
        .sum()
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
    let y0 = l.y;

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
    let first_input = matches!(kind, StripKind::Input(0));
    if has_sends {
        if let StripKind::Input(i) = kind {
            for lane in sends {
                name_bar(cmds, x, y, w, first_input, Some(*lane), view.engine);
                if first_input {
                    paint_control_with_pan(cmds, view, sends, *lane, extras);
                }
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
            y += Layout::send_row_h(*lane);
        }
    }

    name_bar(cmds, x, y, w, first_input, None, view.engine);
    y += Layout::SEND_NAME_BAR;
    theme::faceplate_cell(cmds, Rect { x, y, w, h: Layout::PAN_ROW }, false, true);
    if !matches!(kind, StripKind::Main) {
        paint_enable_toggle(cmds, pan_enable_rect(x, y, w), enabled);
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
    let used_top = FaderBay::layout(x, y, w, bay_h).track_top;
    theme::channel_bay_shading(
        cmds,
        Rect { x, y: used_top, w, h: (l.y + l.h - used_top).max(0.0) },
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

    // Returns / Main have no send knobs — keep that empty faceplate seamless
    // down to the PAN title row. Inputs keep seams through the send stack.
    // Seams continue through the leftover above capped faders.
    let send_area: f32 = sends.iter().copied().map(Layout::send_lane_h).sum();
    let skip_upper = !matches!(kind, StripKind::Input(_));
    let body_y = y0 + if skip_upper { send_area } else { 0.0 };
    let body_h = (l.y + l.h - body_y).max(0.0);
    if body_h > 0.0 {
        if matches!(kind, StripKind::Input(0)) {
            theme::channel_seam(cmds, x, y0, (l.y + l.h - y0).max(0.0), false);
        }
        theme::channel_seam(cmds, x + w - 1.0, body_y, body_h, false);
    }
}

fn mixer_origin(layout: &MixerLayout) -> f32 {
    layout.x + Layout::MIXER_LEADING - layout.scroll_x
}

/// MixLink `SendLaneNameBar` plates as one rail per row (not a fill per strip).
fn paint_enable_toggle(cmds: &mut Vec<DrawCmd>, rect: Rect, enabled: bool) {
    widgets::enable_toggle(cmds, rect, enabled);
}

const PAN_ENABLE_INSET: f32 = 4.0;

/// On/off toggle in the top-right of the pan faceplate (below the name bar).
pub fn pan_enable_rect(strip_x: f32, pan_row_y: f32, strip_w: f32) -> Rect {
    Rect {
        x: strip_x + strip_w - PAN_ENABLE_INSET - Layout::ENABLE_W,
        y: pan_row_y + PAN_ENABLE_INSET,
        w: Layout::ENABLE_W,
        h: Layout::ENABLE_H,
    }
}

fn paint_name_bar_rails(
    cmds: &mut Vec<DrawCmd>,
    origin: f32,
    mixer_y: f32,
    ch_w: f32,
    main_w: f32,
    sends: &[ReturnLane],
) {
    let rail_x = origin;
    let inputs_w = ch_w * 8.0;
    let mut y = mixer_y;
    for lane in sends {
        name_bar_plate(cmds, rail_x, y, inputs_w);
        y += Layout::send_lane_h(*lane);
    }
    // PAN / Effect returns / Bus returns / Main share one rail.
    let pan_w = inputs_w + 1.0 + ch_w * sends.len() as f32 + 1.0 + ch_w * 2.0 + 1.0 + main_w;
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
            send_lane_title(lane, &engine.return_display_name(lane))
        } else {
            "PAN".into()
        };
        theme::text(
            cmds,
            Rect { x: x + 6.0, y, w: w - 8.0, h: Layout::SEND_NAME_BAR },
            label,
            SEND_TITLE_SIZE,
            theme::SECONDARY_TEXT,
            false,
        );
    }
}

fn send_lane_title(lane: ReturnLane, return_name: &str) -> String {
    format!("SEND {} · {}", lane.strip_title(), return_name)
}

fn paint_control_with_pan(
    cmds: &mut Vec<DrawCmd>,
    view: &MixerView<'_>,
    sends: &[ReturnLane],
    lane: ReturnLane,
    extras: &mut Vec<(Rect, MixerExtraHit)>,
) {
    if lane != ReturnLane::SendC {
        return;
    }
    let Some(hit) = control_with_pan_rect(&view.layout, sends.len()) else {
        return;
    };
    let box_r = Rect {
        x: hit.x + hit.w - CONTROL_WITH_PAN_BOX,
        y: hit.y + (hit.h - CONTROL_WITH_PAN_BOX) * 0.5,
        w: CONTROL_WITH_PAN_BOX,
        h: CONTROL_WITH_PAN_BOX,
    };
    widgets::checkbox_box(
        cmds,
        box_r,
        view.engine.config.pan_knobs_control_send_c,
        8.0,
    );
    theme::text_end(
        cmds,
        Rect {
            x: hit.x,
            y: hit.y,
            w: (box_r.x - hit.x - 3.0).max(1.0),
            h: hit.h,
        },
        CONTROL_WITH_PAN_LABEL,
        CONTROL_WITH_PAN_LABEL_SIZE,
        theme::SECONDARY_TEXT,
        false,
    );
    extras.push((hit, MixerExtraHit::ControlWithPan));
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
        let avail = (bay_h - Layout::FADER_BAY_PAD_Y * 2.0).max(Layout::FADER_CAP_H);
        let track_h = Layout::FADER_TRACK_H.min(avail);
        let track_top = strip_y + bay_h - Layout::FADER_BAY_PAD_Y - track_h;
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
    let mut sx = mixer_origin(layout);
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

pub fn send_stack_h(send_count: usize) -> f32 {
    let n = send_count.max(2).min(6);
    ALL_SEND_LANES.iter().take(n).copied().map(Layout::send_lane_h).sum()
}

/// Y of the PAN / Effect returns / Bus returns / Main name bar.
pub fn pan_name_bar_y(layout: &MixerLayout, send_count: usize) -> f32 {
    layout.y + send_stack_h(send_count)
}

pub fn pan_row_y(layout: &MixerLayout, send_count: usize) -> f32 {
    pan_name_bar_y(layout, send_count) + Layout::SEND_NAME_BAR
}

pub fn fader_bay_frame(layout: &MixerLayout, send_count: usize) -> (f32, f32) {
    let y = pan_name_bar_y(layout, send_count) + Layout::SEND_NAME_BAR + Layout::PAN_ROW;
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
    let mut sx = mixer_origin(layout);
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

const CONTROL_WITH_PAN_LABEL: &str = "Control with Pan";
const CONTROL_WITH_PAN_BOX: f32 = 10.0;
const CONTROL_WITH_PAN_LABEL_SIZE: f32 = 8.0;
/// Compact label + gap + trailing checkbox, right-aligned to channel 8.
const CONTROL_WITH_PAN_HIT_W: f32 = 90.0;
const SEND_TITLE_SIZE: f32 = 11.0;

/// Hit rect on the Send C name bar, right-aligned to input channel 8.
/// Only when there are ≥3 effect returns (MixLink: Control with Pan is Send C only).
pub fn control_with_pan_rect(layout: &MixerLayout, send_count: usize) -> Option<Rect> {
    if send_count < 3 {
        return None;
    }
    let n = send_count.max(2).min(6);
    let Some(i) = ALL_SEND_LANES.iter().take(n).position(|l| *l == ReturnLane::SendC) else {
        return None;
    };
    let (x8, w8) = strip_frame(layout, send_count, StripKind::Input(7));
    let y = layout.y + ALL_SEND_LANES.iter().take(i).copied().map(Layout::send_lane_h).sum::<f32>();
    Some(Rect {
        x: x8 + w8 - 6.0 - CONTROL_WITH_PAN_HIT_W,
        y,
        w: CONTROL_WITH_PAN_HIT_W,
        h: Layout::SEND_NAME_BAR,
    })
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
    fn input_channels_share_one_width() {
        let layout = MixerLayout::new(0.0, 0.0, 1600.0, 900.0, 2);
        let (x0, w0) = strip_frame(&layout, 2, StripKind::Input(0));
        let (x1, w1) = strip_frame(&layout, 2, StripKind::Input(1));
        assert_eq!(w0, w1);
        assert!((x1 - (x0 + w0)).abs() < 0.01);
        assert!((x0 - (layout.x + Layout::MIXER_LEADING)).abs() < 0.01);
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

    #[test]
    fn control_with_pan_sits_on_send_c_name_bar() {
        let layout = MixerLayout::new(0.0, 0.0, 1600.0, 900.0, 3);
        let r = control_with_pan_rect(&layout, 3).expect("Send C title-bar hit");
        let bar_y = layout.y
            + Layout::send_lane_h(ReturnLane::SendA)
            + Layout::send_lane_h(ReturnLane::SendB);
        assert!((r.y - bar_y).abs() < 0.01);
        assert!((r.h - Layout::SEND_NAME_BAR).abs() < 0.01);
        let (x8, w8) = strip_frame(&layout, 3, StripKind::Input(7));
        assert!(
            (r.x + r.w - (x8 + w8 - 6.0)).abs() < 0.5,
            "should right-align to channel 8, got right={} want {}",
            r.x + r.w,
            x8 + w8 - 6.0
        );
        let (cx, _) = strip_frame(&layout, 3, StripKind::Return(ReturnLane::SendC));
        assert!(r.x + r.w < cx, "must not land on the Send C return strip");
        let (sx, sw) = strip_frame(&layout, 3, StripKind::Input(0));
        assert!(r.x > sx + sw, "must not sit next to the SEND C title on channel 1");
    }

    #[test]
    fn control_with_pan_hidden_without_send_c() {
        let layout = MixerLayout::new(0.0, 0.0, 1600.0, 900.0, 2);
        assert!(control_with_pan_rect(&layout, 2).is_none());
    }

    #[test]
    fn pan_name_bar_sits_below_send_stack() {
        let layout = MixerLayout::new(0.0, 40.0, 1600.0, 900.0, 3);
        let y = pan_name_bar_y(&layout, 3);
        let expected = 40.0
            + Layout::send_lane_h(ReturnLane::SendA)
            + Layout::send_lane_h(ReturnLane::SendB)
            + Layout::send_lane_h(ReturnLane::SendC);
        assert!((y - expected).abs() < 0.01);
        assert!((pan_row_y(&layout, 3) - (y + Layout::SEND_NAME_BAR)).abs() < 0.01);
        let (bay_y, _) = fader_bay_frame(&layout, 3);
        assert!((bay_y - (y + Layout::SEND_NAME_BAR + Layout::PAN_ROW)).abs() < 0.01);
    }

    #[test]
    fn fader_track_caps_without_collapsing_bay() {
        let layout = MixerLayout::new(0.0, 0.0, 1600.0, 1400.0, 3);
        let (y, h) = fader_bay_frame(&layout, 3);
        assert!(h > Layout::FADER_TRACK_H + 80.0, "fixture should leave leftover bay metal");
        let bay = FaderBay::layout(0.0, y, 100.0, h);
        assert!((bay.track_h - Layout::FADER_TRACK_H).abs() < 0.01);
        assert!((bay.track_top + bay.track_h - (y + h - Layout::FADER_BAY_PAD_Y)).abs() < 0.01);
        assert!(bay.track_top - y > 40.0, "channel bay stays tall above the fixed throw");
        let short = FaderBay::layout(0.0, 0.0, 100.0, 160.0);
        assert!(short.track_h < Layout::FADER_TRACK_H);
        assert!((short.track_top - Layout::FADER_BAY_PAD_Y).abs() < 0.01);
    }

    #[test]
    fn unused_bay_is_one_smooth_sheet() {
        let engine = test_engine();
        let send_count = engine.config.visible_send_lanes().len();
        let layout = MixerLayout::new(0.0, 0.0, 1800.0, 1400.0, send_count);
        let (bay_y, bay_h) = fader_bay_frame(&layout, send_count);
        let used_top = FaderBay::layout(0.0, bay_y, 100.0, bay_h).track_top;
        assert!(used_top - bay_y > 40.0, "fixture should leave unused bay metal");
        let gap0 = bay_y + 8.0;
        let gap1 = used_top - 8.0;
        let peaks = [0.0f32; 17];
        let (cmds, _) = paint(&MixerView {
            engine: &engine,
            peaks: &peaks,
            layout,
            deck: crate::deck::DeckView::default(),
        });
        let seams = cmds.iter().any(|c| {
            let rect = match c {
                DrawCmd::Rect { rect, .. } | DrawCmd::HorzGradient { rect, .. } => rect,
                _ => return false,
            };
            rect.w <= 2.5 && rect.h > 20.0 && rect.y <= gap0 && rect.y + rect.h >= gap1
        });
        assert!(seams, "channel separators should stretch through the leftover bay");
        let grain = cmds.iter().any(|c| match c {
            DrawCmd::Line { a, b, thickness, .. } => {
                (b.0 - a.0).abs() > 200.0
                    && *thickness <= 1.0
                    && a.1 >= gap0
                    && a.1 <= gap1
                    && (b.1 - a.1).abs() < 1.0
            }
            _ => false,
        });
        assert!(!grain, "grain and metal lines must not run through the unused bay");
        let bands = cmds.iter().any(|c| match c {
            DrawCmd::VertGradient { rect, .. } => {
                rect.w > 200.0
                    && rect.h > 4.0
                    && rect.h < 20.0
                    && rect.y >= gap0
                    && rect.y + rect.h <= gap1
            }
            _ => false,
        });
        assert!(!bands, "metal gradient bands must not sit in the unused bay");
    }

    #[test]
    fn overflow_right_of_strips_is_flat_bay() {
        let engine = test_engine();
        let send_count = engine.config.visible_send_lanes().len();
        let layout = MixerLayout::new(0.0, 0.0, 1800.0, 1400.0, send_count);
        let origin = mixer_origin(&layout);
        let main_x = origin
            + layout.ch_w * 8.0
            + 1.0
            + layout.ch_w * send_count as f32
            + 1.0
            + layout.ch_w * 2.0
            + 1.0;
        let strips_right = main_x + layout.main_w;
        assert!(layout.w - strips_right > 80.0, "fixture should leave overflow past Main");
        let peaks = [0.0f32; 17];
        let (cmds, _) = paint(&MixerView {
            engine: &engine,
            peaks: &peaks,
            layout,
            deck: crate::deck::DeckView::default(),
        });
        let dark = theme::material_for(theme::SurfaceStyle::FaderBay).middle;
        let overflow = cmds.iter().any(|c| match c {
            DrawCmd::Rect { rect, color } => {
                (rect.x - strips_right).abs() < 1.0
                    && rect.w > 40.0
                    && (rect.h - layout.h).abs() < 1.0
                    && *color == dark
            }
            _ => false,
        });
        assert!(overflow, "overflow past Main should be the unused-bay fill");
        let grain_past = cmds.iter().any(|c| match c {
            DrawCmd::Line { a, b, thickness, .. } => {
                *thickness <= 1.0 && a.0.min(b.0) < strips_right && a.0.max(b.0) > strips_right + 20.0
            }
            _ => false,
        });
        assert!(!grain_past, "faceplate / fader grain must stop at the last strip");
    }

    fn test_engine() -> AnalogEngine {
        let config = analog::SessionConfig::new();
        let mut mixer = analog::MixerState::new();
        mixer.monitored_output = config.main_output;
        let mut surface = analog::SurfaceState::new();
        surface.load_returns(&config);
        AnalogEngine::new(mixer, surface, config, osc::OscSession::new())
    }

    #[test]
    fn section_titles_live_on_pan_name_bar_not_group_header() {
        let engine = test_engine();
        let send_count = engine.config.visible_send_lanes().len();
        let layout = MixerLayout::new(0.0, 0.0, 1800.0, 900.0, send_count);
        let peaks = [0.0f32; 17];
        let (cmds, extras) = paint(&MixerView {
            engine: &engine,
            peaks: &peaks,
            layout,
            deck: crate::deck::DeckView::default(),
        });
        let texts: Vec<&str> = cmds
            .iter()
            .filter_map(|c| match c {
                DrawCmd::Text(t) => Some(t.text.as_str()),
                _ => None,
            })
            .collect();
        assert!(!texts.contains(&"Channels"), "{texts:?}");
        let bar_y = pan_name_bar_y(&layout, send_count);
        for title in ["PAN", "Effect returns", "Bus returns", "Main"] {
            let on_bar = cmds.iter().any(|c| match c {
                DrawCmd::Text(t) if t.text == title => (t.rect.y - bar_y).abs() < 0.5,
                _ => false,
            });
            assert!(on_bar, "{title} should sit on the PAN name bar (y={bar_y}); texts={texts:?}");
        }
        let plus = extras.iter().find(|(_, h)| matches!(h, MixerExtraHit::AddReturn));
        let Some((plus, _)) = plus else { panic!("effect-return + should sit on the PAN name bar") };
        assert_eq!(plus.h, SECTION_STEPPER);
        let title = cmds.iter().find_map(|c| match c {
            DrawCmd::Text(t) if t.text == "Effect returns" => Some(t),
            _ => None,
        });
        let Some(title) = title else { panic!("Effect returns title") };
        assert!(
            (plus.x - (title.rect.x + title.rect.w + SECTION_TITLE_STEPPER_GAP)).abs() < 1.0,
            "plus x={} should hug the title (title x={} w={})",
            plus.x,
            title.rect.x,
            title.rect.w
        );
        let (bus_x, _) = strip_frame(&layout, send_count, StripKind::Return(ReturnLane::Bus1));
        assert!(plus.x + plus.w < bus_x, "plus must not sit on the Bus returns group");
        let (fx_x, _) = strip_frame(&layout, send_count, StripKind::Return(ReturnLane::SendA));
        let empty_seams = cmds.iter().any(|c| match c {
            DrawCmd::Rect { rect, .. } => {
                rect.w <= 1.5 && rect.x >= fx_x && rect.y + 0.5 < bar_y && rect.h > 8.0
            }
            _ => false,
        });
        assert!(!empty_seams, "no group dividers in the empty send-stack above the title row");
    }
}
