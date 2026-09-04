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
        let available = (w - Layout::MIXER_LEADING - 4.0).max(100.0);
        let ch_w = Layout::channel_width(available, n_in + n_ret + n_bus, n_main);
        Self { x, y, w, h, ch_w, main_w: ch_w * Layout::MAIN_FACTOR, scroll_x: 0.0 }
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
    let sends: Vec<ReturnLane> = view.engine.config.visible_send_lanes();
    theme::mixer_chassis(
        &mut cmds,
        Rect { x: l.x, y: l.y, w: l.w, h: l.h },
        Layout::upper_faceplate_height(&sends),
    );

    let mut x = l.x + Layout::MIXER_LEADING + 4.0 - l.scroll_x;
    group_header(&mut cmds, x, l.y, l.ch_w * 8.0, "Channels", None, &mut extras);
    for i in 0..8 {
        paint_strip(&mut cmds, view, StripKind::Input(i), x + i as f32 * l.ch_w, l.ch_w, &sends, &mut extras);
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
        paint_strip(&mut cmds, view, StripKind::Return(*lane), x + i as f32 * l.ch_w, l.ch_w, &sends, &mut extras);
    }
    x += l.ch_w * sends.len() as f32;
    theme::fill(&mut cmds, Rect { x, y: l.y, w: 1.0, h: l.h }, [0.05, 0.05, 0.05, 1.0]);
    x += 1.0;
    group_header(&mut cmds, x, l.y, l.ch_w * 2.0, "Bus returns", None, &mut extras);
    paint_strip(&mut cmds, view, StripKind::Return(ReturnLane::Bus1), x, l.ch_w, &sends, &mut extras);
    paint_strip(&mut cmds, view, StripKind::Return(ReturnLane::Bus2), x + l.ch_w, l.ch_w, &sends, &mut extras);
    x += l.ch_w * 2.0;
    theme::fill(&mut cmds, Rect { x, y: l.y, w: 1.0, h: l.h }, [0.05, 0.05, 0.05, 1.0]);
    x += 1.0;
    group_header(&mut cmds, x, l.y, l.main_w, "Main", None, &mut extras);
    paint_strip(&mut cmds, view, StripKind::Main, x, l.main_w, &sends, &mut extras);
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
    theme::text(cmds, Rect { x: x + 8.0, y: y + 18.0, w: w - 12.0, h: 22.0 }, title, 11.5, theme::SECONDARY_TEXT, true);
    if let Some((count, max)) = plus_minus {
        let bx = x + w - 52.0;
        if count > 2 {
            widgets::icon_pad(cmds, Rect { x: bx, y: y + 18.0, w: 20.0, h: 20.0 }, "−", true);
            extras.push((Rect { x: bx, y: y + 18.0, w: 20.0, h: 20.0 }, MixerExtraHit::RemoveReturn));
        }
        if count < max {
            widgets::icon_pad(cmds, Rect { x: bx + 24.0, y: y + 18.0, w: 20.0, h: 20.0 }, "+", true);
            extras.push((Rect { x: bx + 24.0, y: y + 18.0, w: 20.0, h: 20.0 }, MixerExtraHit::AddReturn));
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
            (s.fader, s.pan, view.engine.selected_name(id), format!("{}", i + 1), true, !enabled, enabled)
        }
        StripKind::Return(lane) => {
            let r = view.engine.surface.returns.iter().find(|r| r.id == lane as i32);
            let f = r.map(|r| r.fader).unwrap_or(0.0);
            let p = r.map(|r| r.pan).unwrap_or(0.5);
            let unused = lane.is_send() && view.engine.config.effect_ref(lane).is_none();
            let enabled = view.engine.config.is_return_enabled(lane);
            (f, p, view.engine.return_display_name(lane), lane.strip_title().to_string(), false, unused || !enabled, enabled)
        }
        StripKind::Main => (view.engine.mixer.main_fader, 0.5, "Main".into(), "M".into(), false, false, true),
    };

    if dim {
        theme::fill(cmds, Rect { x, y: y0, w, h: l.h - Layout::GROUP_HEADER }, [0.0, 0.0, 0.0, 0.36]);
    }

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
                name_bar(cmds, x, y, w, first_input, true, Some(*lane), view.engine);
                y += Layout::SEND_NAME_BAR;
                let aux = view.engine.surface.strips[i].aux(*lane);
                let row_h = Layout::send_row_h(*lane);
                theme::faceplate_cell(
                    cmds,
                    Rect { x, y, w, h: row_h },
                    Some(*lane) == sends.last().copied(),
                    false,
                );
                widgets::knob(
                    cmds,
                    x + (w - Layout::SEND_KNOB) * 0.5,
                    y + 4.0,
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
            name_bar(cmds, x, y, w, false, false, None, view.engine);
            y += Layout::SEND_NAME_BAR;
            if matches!(kind, StripKind::Return(ReturnLane::SendC)) && sends.len() >= 3 && *lane == ReturnLane::SendC {
                let box_r = Rect { x: x + 10.0, y: y + 10.0, w: w - 16.0, h: 24.0 };
                widgets::checkbox(cmds, box_r.x, box_r.y, view.engine.config.pan_knobs_control_send_c, "Control with Pan");
                extras.push((box_r, MixerExtraHit::ControlWithPan));
            }
            y += Layout::send_row_h(*lane);
        }
    }

    name_bar(cmds, x, y, w, first_input, !matches!(kind, StripKind::Main), None, view.engine);
    y += Layout::SEND_NAME_BAR;
    theme::faceplate_cell(cmds, Rect { x, y, w, h: Layout::PAN_ROW }, false, true);
    if !matches!(kind, StripKind::Main) {
        widgets::knob(
            cmds,
            x + (w - Layout::PAN_KNOB) * 0.5,
            y + 6.0,
            Layout::PAN_KNOB,
            pan,
            widgets::KnobKind::Pan,
            None,
        );
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
    theme::channel_bay_shading(cmds, Rect { x, y, w, h: bay_h + Layout::NAME_ROW + Layout::BUTTON_STACK });
    paint_fader(cmds, x, y, w, bay_h, fader, peak_for(view, kind), first_input || matches!(kind, StripKind::Main));
    y += bay_h;

    widgets::strip_name_label(
        cmds,
        Rect { x, y, w, h: Layout::NAME_ROW },
        &name,
        widgets::StripNameStyle {
            diamond: !matches!(kind, StripKind::Main),
            dim,
            selected: matches!(kind, StripKind::Main),
        },
    );
    y += Layout::NAME_ROW;

    match kind {
        StripKind::Input(i) => {
            let assign = view.engine.surface.strips[i].assign;
            let id = view.engine.config.strips[i].channel_id();
            let muted = view.engine.mixer.channel(id).map(|c| c.mute).unwrap_or(false);
            let soloed = view.engine.mixer.channel(id).map(|c| c.solo).unwrap_or(false);
            widgets::hardware_pad(cmds, Rect { x: x + 8.0, y, w: w - 16.0, h: Layout::BUTTON_H }, "SOLO", soloed, theme::METER_GREEN);
            widgets::hardware_pad(cmds, Rect { x: x + 8.0, y: y + 31.0, w: w - 16.0, h: Layout::BUTTON_H }, "MUTE", muted, theme::METER_RED);
            widgets::hardware_pad(cmds, Rect { x: x + 8.0, y: y + 62.0, w: w - 16.0, h: Layout::BUTTON_H }, "BUS 1", assign == MixAssign::Bus1, theme::AMBER);
            widgets::hardware_pad(cmds, Rect { x: x + 8.0, y: y + 93.0, w: w - 16.0, h: Layout::BUTTON_H }, "BUS 2", assign == MixAssign::Bus2, theme::METER_RED);
        }
        StripKind::Return(lane) => {
            widgets::hardware_pad(cmds, Rect { x: x + 8.0, y, w: w - 16.0, h: Layout::BUTTON_H }, "SOLO", view.engine.return_soloed(lane), theme::METER_GREEN);
            widgets::hardware_pad(cmds, Rect { x: x + 8.0, y: y + 31.0, w: w - 16.0, h: Layout::BUTTON_H }, "MUTE", view.engine.return_muted(lane), theme::METER_RED);
        }
        StripKind::Main => {}
    }

    theme::channel_seam(cmds, x + w - 1.0, y0, l.h - Layout::GROUP_HEADER, false);
}

fn name_bar(
    cmds: &mut Vec<DrawCmd>,
    x: f32,
    y: f32,
    w: f32,
    show_title: bool,
    show_plate: bool,
    lane: Option<ReturnLane>,
    engine: &AnalogEngine,
) {
    if show_plate {
        theme::fill(cmds, Rect { x, y, w, h: Layout::SEND_NAME_BAR }, [0.08, 0.08, 0.08, 1.0]);
        theme::fill(cmds, Rect { x, y, w, h: 1.0 }, [0.0, 0.0, 0.0, 0.40]);
        theme::fill(cmds, Rect { x, y: y + Layout::SEND_NAME_BAR - 1.0, w, h: 1.0 }, [1.0, 1.0, 1.0, 0.04]);
    }
    if show_title {
        let label = if let Some(lane) = lane {
            format!("SEND {} · {}", lane.strip_title(), engine.return_display_name(lane))
        } else {
            "PAN".into()
        };
        theme::text(cmds, Rect { x: x + 6.0, y, w: w - 8.0, h: Layout::SEND_NAME_BAR }, label, 9.0, theme::SECONDARY_TEXT, false);
    }
}

fn peak_for(view: &MixerView<'_>, kind: StripKind) -> f32 {
    match kind {
        StripKind::Input(i) => view.peaks.get(i).copied().unwrap_or(0.0),
        StripKind::Return(lane) => view.peaks.get(8 + lane as usize).copied().unwrap_or(0.0),
        StripKind::Main => view.peaks.get(16).copied().unwrap_or(0.0),
    }
}

fn paint_fader(cmds: &mut Vec<DrawCmd>, x: f32, y: f32, w: f32, h: f32, lin: f32, peak: f32, labels: bool) {
    let trough_x = x + w * 0.42;
    let meter_x = trough_x + Layout::FADER_TROUGH + 6.0;
    let track_top = y + 8.0;
    let track_h = (h - 16.0).max(Layout::FADER_CAP_H);
    widgets::decibel_scale(cmds, meter_x + Layout::METER_HOUSING + 2.0, track_top, track_h, labels);
    let slot_x = trough_x + (Layout::FADER_TROUGH - Layout::FADER_SLOT) * 0.5;
    cmds.push(DrawCmd::RoundedRect {
        rect: Rect { x: slot_x - 1.5, y: track_top, w: Layout::FADER_SLOT + 3.0, h: track_h },
        color: [0.04, 0.04, 0.04, 1.0],
        radius: (Layout::FADER_SLOT + 3.0) * 0.5,
    });
    cmds.push(DrawCmd::RoundedRect {
        rect: Rect { x: slot_x, y: track_top, w: Layout::FADER_SLOT, h: track_h },
        color: [0.01, 0.01, 0.01, 1.0],
        radius: Layout::FADER_SLOT * 0.5,
    });
    theme::fill(cmds, Rect { x: slot_x, y: track_top + 1.0, w: 1.2, h: track_h - 2.0 }, [0.0, 0.0, 0.0, 0.55]);
    theme::fill(
        cmds,
        Rect { x: slot_x + Layout::FADER_SLOT - 0.8, y: track_top + 1.0, w: 0.8, h: track_h - 2.0 },
        [1.0, 1.0, 1.0, 0.08],
    );
    theme::fill(cmds, Rect { x: meter_x, y: track_top, w: Layout::METER_HOUSING, h: track_h }, theme::DEEP_SLOT);
    let segs = Layout::METER_SEGMENTS;
    let fill_n = (peak * segs as f32).round() as i32;
    let seg_h = track_h / segs as f32;
    for i in 0..segs {
        if i >= fill_n {
            continue;
        }
        let t = i as f32 / segs as f32;
        let color = if t > 0.9 { theme::METER_RED } else if t > 0.7 { theme::METER_YELLOW } else { theme::METER_GREEN };
        theme::fill(
            cmds,
            Rect { x: meter_x + 2.0, y: track_top + track_h - (i as f32 + 1.0) * seg_h, w: Layout::METER_W, h: seg_h - 0.4 },
            color,
        );
    }
    let travel = (track_h - Layout::FADER_CAP_H).max(0.0);
    let cap_y = track_top + travel * (1.0 - lin.clamp(0.0, 1.0));
    let cap_x = trough_x + Layout::FADER_TROUGH * 0.5 - Layout::FADER_CAP_W * 0.5;
    widgets::fader_cap(
        cmds,
        Rect { x: cap_x, y: cap_y, w: Layout::FADER_CAP_W, h: Layout::FADER_CAP_H },
    );
}

pub fn fader_rail(layout: &MixerLayout, _strip_x: f32, _strip_w: f32) -> (f32, f32) {
    let y0 = layout.y + Layout::GROUP_HEADER + Layout::ENABLE_ROW;
    (y0 + 200.0, y0 + 200.0 + 220.0 - Layout::FADER_CAP_H)
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
