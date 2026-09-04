//! One wgpu overlay at a time: menu popup, Settings, or Channels.

use analog::{AnalogEngine, ChannelID, MixerBus};
use render::{DrawCmd, Rect};

use crate::theme::{self, Layout};
use crate::widgets::{self, MenuItem};

#[derive(Clone, Debug)]
pub enum Overlay {
    Menu {
        rect: Rect,
        items: Vec<MenuItem>,
        action: MenuAction,
    },
    Settings,
    Channels,
}

#[derive(Clone, Debug)]
pub enum MenuAction {
    StripSource { strip: usize },
    ReturnEffect { lane: analog::ReturnLane },
    HardwareOutput { id: i32 },
    HardwareInput { id: i32 },
    MixOut,
    AudioDevice,
    AudioBuffer,
    PluginBundle { id: i32 },
    PluginPlayback { id: i32 },
    InsertBundle { insert: uuid::Uuid },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextFocus {
    None,
    Tempo,
    ProjectName,
    HardwareName(i32),
    PluginName(i32),
    GearAlias(ChannelID),
    OscHost,
    OscSend,
    OscListen,
    MidiNeedle,
}

pub fn paint_menu(overlay: &Overlay, hover: Option<usize>) -> Vec<DrawCmd> {
    let mut cmds = Vec::new();
    if let Overlay::Menu { rect, items, .. } = overlay {
        widgets::popup_menu(&mut cmds, *rect, items, hover);
    }
    cmds
}

pub fn paint_settings(engine: &AnalogEngine, w: f32, h: f32, focus: &TextFocus, caret: bool) -> (Vec<DrawCmd>, Rect) {
    let mut cmds = Vec::new();
    let rect = Rect { x: w * 0.5 - 220.0, y: 64.0, w: 440.0, h: (h - 120.0).min(420.0) };
    theme::fill(&mut cmds, Rect { x: 0.0, y: 0.0, w, h }, [0.0, 0.0, 0.0, 0.62]);
    theme::fill(&mut cmds, rect, [0.12, 0.125, 0.12, 1.0]);
    theme::hardware_surface(&mut cmds, rect, theme::SurfaceStyle::Sidebar);
    theme::text_center(
        &mut cmds,
        Rect { x: rect.x, y: rect.y + 10.0, w: rect.w, h: 22.0 },
        "Settings",
        14.0,
        theme::TEXT,
        true,
    );
    let mut y = rect.y + 40.0;
    field_label(&mut cmds, rect.x + 16.0, y, "Post-fader sends");
    widgets::checkbox(
        &mut cmds,
        rect.x + 160.0,
        y + 2.0,
        engine.config.sends_post_fader,
        if engine.config.sends_post_fader { "On" } else { "Off" },
    );
    y += 32.0;
    field_label(&mut cmds, rect.x + 16.0, y, "OSC host");
    widgets::text_field(
        &mut cmds,
        Rect { x: rect.x + 140.0, y, w: 260.0, h: 24.0 },
        &engine.config.osc_host,
        *focus == TextFocus::OscHost,
        caret,
    );
    y += 32.0;
    field_label(&mut cmds, rect.x + 16.0, y, "Send port");
    widgets::text_field(
        &mut cmds,
        Rect { x: rect.x + 140.0, y, w: 260.0, h: 24.0 },
        &engine.config.osc_send_port.to_string(),
        *focus == TextFocus::OscSend,
        caret,
    );
    y += 32.0;
    field_label(&mut cmds, rect.x + 16.0, y, "Listen port");
    widgets::text_field(
        &mut cmds,
        Rect { x: rect.x + 140.0, y, w: 260.0, h: 24.0 },
        &engine.config.osc_listen_port.to_string(),
        *focus == TextFocus::OscListen,
        caret,
    );
    y += 32.0;
    field_label(&mut cmds, rect.x + 16.0, y, "MIDI contains");
    widgets::text_field(
        &mut cmds,
        Rect { x: rect.x + 140.0, y, w: 260.0, h: 24.0 },
        &engine.config.midi_device_contains,
        *focus == TextFocus::MidiNeedle,
        caret,
    );
    y += 40.0;
    widgets::hardware_pad(
        &mut cmds,
        Rect { x: rect.x + 140.0, y, w: 160.0, h: 26.0 },
        "Apply & reconnect",
        false,
        theme::PRIMARY_TEXT,
    );
    (cmds, rect)
}

pub fn paint_channels(engine: &AnalogEngine, w: f32, h: f32, focus: &TextFocus, caret: bool) -> (Vec<DrawCmd>, Vec<(Rect, ChannelID)>) {
    let mut cmds = Vec::new();
    let body_w = (w - Layout::SIDEBAR_WIDTH - 48.0).min(720.0);
    let rect = Rect { x: 24.0, y: 48.0, w: body_w, h: h - 96.0 };
    theme::fill(&mut cmds, Rect { x: 0.0, y: 0.0, w, h }, [0.0, 0.0, 0.0, 0.62]);
    theme::fill(&mut cmds, rect, [0.12, 0.125, 0.12, 1.0]);
    theme::hardware_surface(&mut cmds, rect, theme::SurfaceStyle::Sidebar);
    theme::text(&mut cmds, Rect { x: rect.x + 16.0, y: rect.y + 10.0, w: 200.0, h: 20.0 }, "Channels", 14.0, theme::TEXT, true);
    let col_w = (rect.w - 24.0) * 0.5;
    let mut fields = Vec::new();
    paint_channel_col(
        &mut cmds,
        &mut fields,
        engine,
        Rect { x: rect.x + 12.0, y: rect.y + 36.0, w: col_w - 8.0, h: rect.h - 48.0 },
        MixerBus::Input,
        "Inputs",
        focus,
        caret,
    );
    paint_channel_col(
        &mut cmds,
        &mut fields,
        engine,
        Rect { x: rect.x + 12.0 + col_w, y: rect.y + 36.0, w: col_w - 8.0, h: rect.h - 48.0 },
        MixerBus::Output,
        "Outputs",
        focus,
        caret,
    );
    (cmds, fields)
}

fn paint_channel_col(
    cmds: &mut Vec<DrawCmd>,
    fields: &mut Vec<(Rect, ChannelID)>,
    engine: &AnalogEngine,
    rect: Rect,
    bus: MixerBus,
    title: &str,
    focus: &TextFocus,
    caret: bool,
) {
    theme::text(cmds, Rect { x: rect.x, y: rect.y, w: rect.w, h: 18.0 }, title, 12.0, theme::TEXT_DIM, true);
    let mut y = rect.y + 22.0;
    for ch in engine.mixer.strips(bus) {
        if y + 26.0 > rect.y + rect.h {
            break;
        }
        theme::text_clip(
            cmds,
            Rect { x: rect.x, y, w: 140.0, h: 24.0 },
            AnalogEngine::hardware_label(&ch),
            12.0,
            theme::TEXT_DIM,
            false,
            Some(rect),
        );
        let field = Rect { x: rect.x + 148.0, y, w: rect.w - 148.0, h: 24.0 };
        let name = engine.config.gear_name(ch.id);
        widgets::text_field(cmds, field, &name, *focus == TextFocus::GearAlias(ch.id), caret);
        fields.push((field, ch.id));
        y += 28.0;
    }
}

fn field_label(cmds: &mut Vec<DrawCmd>, x: f32, y: f32, s: &str) {
    theme::text(cmds, Rect { x, y, w: 120.0, h: 24.0 }, s, 14.0, theme::TEXT_DIM, false);
}

pub fn settings_hits(w: f32, h: f32) -> SettingsHits {
    let rect = Rect { x: w * 0.5 - 220.0, y: 64.0, w: 440.0, h: (h - 120.0).min(420.0) };
    SettingsHits {
        panel: rect,
        post_fader: Rect { x: rect.x + 160.0, y: rect.y + 40.0, w: 80.0, h: 20.0 },
        osc_host: Rect { x: rect.x + 140.0, y: rect.y + 72.0, w: 260.0, h: 24.0 },
        osc_send: Rect { x: rect.x + 140.0, y: rect.y + 104.0, w: 260.0, h: 24.0 },
        osc_listen: Rect { x: rect.x + 140.0, y: rect.y + 136.0, w: 260.0, h: 24.0 },
        midi: Rect { x: rect.x + 140.0, y: rect.y + 168.0, w: 260.0, h: 24.0 },
        apply: Rect { x: rect.x + 140.0, y: rect.y + 208.0, w: 160.0, h: 26.0 },
    }
}

pub struct SettingsHits {
    pub panel: Rect,
    pub post_fader: Rect,
    pub osc_host: Rect,
    pub osc_send: Rect,
    pub osc_listen: Rect,
    pub midi: Rect,
    pub apply: Rect,
}

pub fn contains(rect: Rect, x: f32, y: f32) -> bool {
    widgets::contains(rect, x, y)
}

pub fn menu_at(rect: Rect, items: &[MenuItem], y: f32) -> Option<usize> {
    widgets::item_at(rect, items, y)
}

pub fn layout_popup(anchor: Rect, items: &[MenuItem], window_h: f32) -> Rect {
    let h = widgets::menu_height(items).min(window_h - 16.0);
    let gap = 4.0;
    let y = (anchor.y + anchor.h + gap).min(window_h - h - 8.0).max(8.0);
    Rect {
        x: anchor.x.max(8.0),
        y,
        w: anchor.w.max(240.0).min(320.0),
        h,
    }
}
