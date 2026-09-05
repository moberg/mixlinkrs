//! wgpu overlays on the mixer (menu popup) plus Settings / Channels windows.

use analog::{AnalogEngine, ChannelID, MixerBus, RoutingSlot};
use render::{DrawCmd, Rect};

use crate::chrome::HEADER_H;
use crate::theme::{self, Layout};
use crate::widgets::{self, MenuItem};

#[derive(Clone, Debug)]
pub enum Overlay {
    Menu { rect: Rect, items: Vec<MenuItem> },
}

/// MixLink `Settings { .frame(width: 520, height: 640) }` — dedicated wgpu window.
pub const SETTINGS_WINDOW_W: f32 = 520.0;
pub const SETTINGS_WINDOW_H: f32 = 560.0;

/// MixLink `Window("Channels")` `.defaultSize` / `.frame(minWidth:minHeight:)`.
pub const CHANNELS_WINDOW_W: f32 = 780.0;
pub const CHANNELS_WINDOW_H: f32 = 520.0;

/// Beveled Close pad, bottom-right of Settings / Channels.
pub const WINDOW_CLOSE_W: f32 = 72.0;
pub const WINDOW_CLOSE_H: f32 = 26.0;
pub const WINDOW_CLOSE_PAD: f32 = 16.0;

/// Shared Settings / Channels document card (inset, title band, field grid).
const DOC_PANEL_INSET: f32 = 18.0;
const DOC_TITLE_Y: f32 = 10.0;
const DOC_TITLE_H: f32 = 22.0;
const DOC_CONTENT_Y: f32 = 40.0;
const DOC_ROW_H: f32 = 24.0;
const DOC_ROW_STRIDE: f32 = 32.0;
const DOC_PAD_X: f32 = 16.0;
const DOC_FIELD_X: f32 = 140.0;
const DOC_FIELD_W: f32 = 260.0;
const DOC_LABEL_W: f32 = 140.0;
const DOC_LABEL_SIZE: f32 = 14.0;
const DOC_SECTION_GAP: f32 = 40.0;
const DOC_CLOSE_RESERVE: f32 = WINDOW_CLOSE_PAD + WINDOW_CLOSE_H + 10.0;

const CHANNELS_PAD_X: f32 = DOC_PAD_X;
const CHANNELS_ROW_H: f32 = DOC_ROW_H;
const CHANNELS_ROW_STRIDE: f32 = DOC_ROW_STRIDE;
const CHANNELS_LABEL_W: f32 = DOC_LABEL_W;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChannelsHit {
    GearAlias(ChannelID),
    MixOut,
    Close,
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
    MixName(uuid::Uuid),
    TakeName(i32),
}

pub fn paint_menu(overlay: &Overlay, hover: Option<usize>) -> Vec<DrawCmd> {
    let mut cmds = Vec::new();
    let Overlay::Menu { rect, items } = overlay;
    widgets::popup_menu(&mut cmds, *rect, items, hover);
    cmds
}

pub fn paint_settings(
    engine: &AnalogEngine,
    w: f32,
    h: f32,
    focus: &TextFocus,
    caret: bool,
    projects_folder: &str,
) -> (Vec<DrawCmd>, Rect) {
    let mut cmds = Vec::new();
    let rect = settings_panel(w, h);
    widgets::document_window(&mut cmds, w, h, rect);
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
    field_label(&mut cmds, rect.x + 16.0, y, "Hardware strips");
    widgets::checkbox(
        &mut cmds,
        rect.x + 160.0,
        y + 2.0,
        engine.config.hardware_strips,
        if engine.config.hardware_strips { "On" } else { "Off" },
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
    y += 32.0;
    field_label(&mut cmds, rect.x + 16.0, y, "Projects folder");
    widgets::channel_picker(
        &mut cmds,
        Rect { x: rect.x + 140.0, y, w: 260.0, h: 24.0 },
        projects_folder,
        widgets::ChannelPickerStyle::value(),
    );
    y += 40.0;
    widgets::hardware_pad(
        &mut cmds,
        Rect { x: rect.x + 140.0, y, w: 160.0, h: 26.0 },
        "Apply & reconnect",
        false,
        theme::PRIMARY_TEXT,
    );
    widgets::window_close(&mut cmds, window_close_rect(w, h));
    (cmds, rect)
}

/// Full-window Channels chrome — same document card / title / field grid as Settings.
pub fn paint_channels(
    engine: &AnalogEngine,
    w: f32,
    h: f32,
    focus: &TextFocus,
    caret: bool,
    scroll: f32,
) -> (Vec<DrawCmd>, Vec<(Rect, ChannelsHit)>) {
    let mut cmds = Vec::new();
    let panel = channels_panel(w, h);
    widgets::document_window(&mut cmds, w, h, panel);
    theme::text_center(
        &mut cmds,
        Rect { x: panel.x, y: panel.y + DOC_TITLE_Y, w: panel.w, h: DOC_TITLE_H },
        "Channels",
        DOC_LABEL_SIZE,
        theme::TEXT,
        true,
    );
    let mut fields = Vec::new();
    paint_mix_out(&mut cmds, &mut fields, engine, w, h);
    let (left, right) = channels_columns(w, h);
    let title_y = channels_section_y(h);
    let list_top = channels_list_top(h);
    let list_bottom = channels_list_bottom(h);
    paint_channel_col(
        &mut cmds,
        &mut fields,
        engine,
        left,
        MixerBus::Input,
        "Inputs",
        focus,
        caret,
        scroll,
        title_y,
        list_top,
        list_bottom,
    );
    paint_channel_col(
        &mut cmds,
        &mut fields,
        engine,
        right,
        MixerBus::Output,
        "Outputs",
        focus,
        caret,
        scroll,
        title_y,
        list_top,
        list_bottom,
    );
    let close = window_close_rect(w, h);
    widgets::window_close(&mut cmds, close);
    fields.push((close, ChannelsHit::Close));
    (cmds, fields)
}

pub fn window_close_rect(w: f32, h: f32) -> Rect {
    Rect {
        x: w - WINDOW_CLOSE_PAD - WINDOW_CLOSE_W,
        y: h - WINDOW_CLOSE_PAD - WINDOW_CLOSE_H,
        w: WINDOW_CLOSE_W,
        h: WINDOW_CLOSE_H,
    }
}

fn channels_panel(w: f32, h: f32) -> Rect {
    document_panel(w, h)
}

fn channels_section_y(h: f32) -> f32 {
    document_panel(0.0, h).y + DOC_CONTENT_Y + DOC_SECTION_GAP
}

fn channels_list_top(h: f32) -> f32 {
    channels_section_y(h) + DOC_ROW_STRIDE
}

fn channels_list_bottom(h: f32) -> f32 {
    let panel = document_panel(0.0, h);
    panel.y + panel.h
}

/// Mix Out picker — Settings projects-folder field (label + `channel_picker`).
pub fn channels_mix_out_rect(w: f32) -> Rect {
    let panel = document_panel(w, 0.0);
    Rect { x: panel.x + DOC_FIELD_X, y: panel.y + DOC_CONTENT_Y, w: DOC_FIELD_W, h: DOC_ROW_H }
}

/// Empty band under the transparent titlebar — `Window::drag_window` from the shell.
/// Control hits (Mix Out, fields, Close) must be tested first.
pub fn document_chrome_drag(x: f32, y: f32) -> bool {
    x >= 0.0 && y >= 0.0 && y < HEADER_H
}

pub fn hit_channels(hits: &[(Rect, ChannelsHit)], x: f32, y: f32) -> Option<ChannelsHit> {
    hits.iter().rev().find(|(r, _)| contains(*r, x, y)).map(|(_, h)| h.clone())
}

fn paint_mix_out(
    cmds: &mut Vec<DrawCmd>,
    fields: &mut Vec<(Rect, ChannelsHit)>,
    engine: &AnalogEngine,
    w: f32,
    h: f32,
) {
    let panel = channels_panel(w, h);
    field_label(cmds, panel.x + DOC_PAD_X, panel.y + DOC_CONTENT_Y, RoutingSlot::Mix.title());
    let picker = channels_mix_out_rect(w);
    widgets::channel_picker(
        cmds,
        picker,
        &engine.output_name(engine.config.main_output),
        widgets::ChannelPickerStyle::value(),
    );
    fields.push((picker, ChannelsHit::MixOut));
}

pub fn channels_columns(w: f32, h: f32) -> (Rect, Rect) {
    let panel = channels_panel(w, h);
    let mid = (panel.x + panel.w * 0.5).floor();
    (
        Rect { x: panel.x, y: panel.y, w: (mid - panel.x).max(0.0), h: panel.h },
        Rect { x: mid, y: panel.y, w: (panel.x + panel.w - mid).max(0.0), h: panel.h },
    )
}

pub fn channels_max_scroll(engine: &AnalogEngine, h: f32) -> f32 {
    let n =
        engine.mixer.strips(MixerBus::Input).len().max(engine.mixer.strips(MixerBus::Output).len());
    let visible = (channels_list_bottom(h) - channels_list_top(h)).max(0.0);
    (n as f32 * CHANNELS_ROW_STRIDE - visible).max(0.0)
}

fn paint_channel_col(
    cmds: &mut Vec<DrawCmd>,
    fields: &mut Vec<(Rect, ChannelsHit)>,
    engine: &AnalogEngine,
    rect: Rect,
    bus: MixerBus,
    title: &str,
    focus: &TextFocus,
    caret: bool,
    scroll: f32,
    title_y: f32,
    list_top: f32,
    list_bottom: f32,
) {
    field_label(cmds, rect.x + CHANNELS_PAD_X, title_y, title);
    let list_h = (list_bottom - list_top).max(0.0);
    let clip = Rect { x: rect.x, y: list_top, w: rect.w, h: list_h };
    let mut y = list_top - scroll;
    for ch in engine.mixer.strips(bus) {
        let visible = y + CHANNELS_ROW_H > list_top && y < list_top + list_h;
        if visible {
            theme::text_clip(
                cmds,
                Rect { x: rect.x + CHANNELS_PAD_X, y, w: CHANNELS_LABEL_W, h: CHANNELS_ROW_H },
                AnalogEngine::hardware_label(&ch),
                DOC_LABEL_SIZE,
                theme::TEXT_DIM,
                false,
                Some(clip),
            );
            let field = Rect {
                x: rect.x + DOC_FIELD_X,
                y,
                w: (rect.w - DOC_FIELD_X - CHANNELS_PAD_X).max(0.0),
                h: CHANNELS_ROW_H,
            };
            let name = engine.config.gear_name(ch.id);
            widgets::text_field(cmds, field, &name, *focus == TextFocus::GearAlias(ch.id), caret);
            fields.push((field, ChannelsHit::GearAlias(ch.id)));
        }
        y += CHANNELS_ROW_STRIDE;
    }
}

fn field_label(cmds: &mut Vec<DrawCmd>, x: f32, y: f32, s: &str) {
    theme::text(
        cmds,
        Rect { x, y, w: DOC_LABEL_W, h: DOC_ROW_H },
        s,
        DOC_LABEL_SIZE,
        theme::TEXT_DIM,
        false,
    );
}

pub fn settings_hits(w: f32, h: f32) -> SettingsHits {
    let rect = settings_panel(w, h);
    SettingsHits {
        panel: rect,
        post_fader: Rect { x: rect.x + 160.0, y: rect.y + 40.0, w: 80.0, h: 20.0 },
        hardware_strips: Rect { x: rect.x + 160.0, y: rect.y + 72.0, w: 80.0, h: 20.0 },
        osc_host: Rect { x: rect.x + 140.0, y: rect.y + 104.0, w: 260.0, h: 24.0 },
        osc_send: Rect { x: rect.x + 140.0, y: rect.y + 136.0, w: 260.0, h: 24.0 },
        osc_listen: Rect { x: rect.x + 140.0, y: rect.y + 168.0, w: 260.0, h: 24.0 },
        midi: Rect { x: rect.x + 140.0, y: rect.y + 200.0, w: 260.0, h: 24.0 },
        projects_folder: Rect { x: rect.x + 140.0, y: rect.y + 232.0, w: 260.0, h: 24.0 },
        apply: Rect { x: rect.x + 140.0, y: rect.y + 272.0, w: 160.0, h: 26.0 },
        close: window_close_rect(w, h),
    }
}

pub struct SettingsHits {
    pub panel: Rect,
    pub post_fader: Rect,
    pub hardware_strips: Rect,
    pub osc_host: Rect,
    pub osc_send: Rect,
    pub osc_listen: Rect,
    pub midi: Rect,
    pub apply: Rect,
    pub projects_folder: Rect,
    pub close: Rect,
}

pub fn contains(rect: Rect, x: f32, y: f32) -> bool {
    widgets::contains(rect, x, y)
}

pub fn menu_at(rect: Rect, items: &[MenuItem], x: f32, y: f32) -> Option<usize> {
    if !widgets::contains(rect, x, y) {
        return None;
    }
    widgets::item_at(rect, items, y)
}

/// Inset from the window edge (and above the footer) for popups and modal panels.
pub const OVERLAY_MARGIN: f32 = 8.0;
const MENU_GAP: f32 = 4.0;

fn client_bottom(window_h: f32) -> f32 {
    (window_h - Layout::FOOTER_H - OVERLAY_MARGIN).max(OVERLAY_MARGIN)
}

/// Shift `rect` fully into the mixer+sidebar client area.
pub fn clamp_to_window(rect: Rect, window_w: f32, window_h: f32) -> Rect {
    let m = OVERLAY_MARGIN;
    let max_w = (window_w - m * 2.0).max(0.0);
    let max_h = (client_bottom(window_h) - m).max(0.0);
    let w = rect.w.min(max_w);
    let h = rect.h.min(max_h);
    let x = rect.x.min(window_w - w - m).max(m);
    let y = rect.y.min(client_bottom(window_h) - h).max(m);
    Rect { x, y, w, h }
}

pub fn settings_panel(window_w: f32, window_h: f32) -> Rect {
    document_panel(window_w, window_h)
}

/// Recessed card shared by Settings and Channels: clears the traffic-light
/// band, 18pt inset, Close reserved at the bottom.
pub fn document_panel(window_w: f32, window_h: f32) -> Rect {
    let inset = DOC_PANEL_INSET;
    let top = HEADER_H.max(inset);
    Rect {
        x: inset,
        y: top,
        w: (window_w - inset * 2.0).max(0.0),
        h: (window_h - top - DOC_CLOSE_RESERVE).max(0.0),
    }
}

pub fn settings_text_focus(focus: &TextFocus) -> bool {
    matches!(
        focus,
        TextFocus::OscHost | TextFocus::OscSend | TextFocus::OscListen | TextFocus::MidiNeedle
    )
}

/// Place a menu aligned to `anchor`, then keep it fully on-screen.
///
/// Horizontal: **shift** (never flip). Align to the field, then slide so the
/// wider menu stays in the window — a right-edge sidebar field grows left.
/// Vertical: **flip** above the anchor when it would cross the footer, then
/// **shift** if it still overflows.
pub fn layout_popup(anchor: Rect, items: &[MenuItem], window_w: f32, window_h: f32) -> Rect {
    let m = OVERLAY_MARGIN;
    let max_w = (window_w - m * 2.0).max(0.0);
    let w = anchor.w.max(widgets::menu_content_width(items)).min(max_w);

    let max_h = (client_bottom(window_h) - m).max(0.0);
    let h = widgets::menu_height(items).min(max_h);

    let x = anchor.x.min(window_w - w - m).max(m);

    let bottom = client_bottom(window_h);
    let below = anchor.y + anchor.h + MENU_GAP;
    let above = anchor.y - MENU_GAP - h;
    let y = if below + h <= bottom + 0.5 {
        below
    } else if above >= m {
        above
    } else {
        below
    };
    let y = y.min(bottom - h).max(m);

    Rect { x, y, w, h }
}

/// Same as [`layout_popup`] but the Channels window has no mixer footer.
pub fn layout_popup_window(anchor: Rect, items: &[MenuItem], window_w: f32, window_h: f32) -> Rect {
    let m = OVERLAY_MARGIN;
    let max_w = (window_w - m * 2.0).max(0.0);
    let w = anchor.w.max(widgets::menu_content_width(items)).min(max_w);

    let max_h = (window_h - m * 2.0).max(0.0);
    let h = widgets::menu_height(items).min(max_h);

    let x = anchor.x.min(window_w - w - m).max(m);

    let bottom = (window_h - m).max(m);
    let below = anchor.y + anchor.h + MENU_GAP;
    let above = anchor.y - MENU_GAP - h;
    let y = if below + h <= bottom + 0.5 {
        below
    } else if above >= m {
        above
    } else {
        below
    };
    let y = y.min(bottom - h).max(m);

    Rect { x, y, w, h }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrome::HEADER_TRAFFIC_INSET;
    use crate::widgets::MenuItem;

    fn item(label: &str) -> MenuItem {
        MenuItem { id: label.into(), label: label.into(), checked: false, section: None }
    }

    #[test]
    fn popup_shifts_left_when_past_right_edge() {
        let anchor = Rect { x: 670.0, y: 200.0, w: 232.0, h: 22.0 };
        let items = vec![item("Digiface USB (24145053)  18/18")];
        let r = layout_popup(anchor, &items, 900.0, 700.0);
        assert!(r.x + r.w <= 900.0 - OVERLAY_MARGIN + 0.5);
        assert!(r.x >= OVERLAY_MARGIN);
        assert!(r.w >= 232.0);
        assert!(r.x < anchor.x);
    }

    #[test]
    fn popup_sizes_to_content_not_short_string() {
        let anchor = Rect { x: 20.0, y: 80.0, w: 230.0, h: 22.0 };
        let items = vec![item("Digiface USB (24145053)  18/18")];
        let r = layout_popup(anchor, &items, 1200.0, 800.0);
        assert!(r.w > 230.0, "menu should grow to the device name, got {}", r.w);
        assert!(r.x + r.w <= 1200.0 - OVERLAY_MARGIN + 0.5);
    }

    #[test]
    fn popup_flips_above_near_footer() {
        let anchor = Rect { x: 40.0, y: 640.0, w: 200.0, h: 22.0 };
        let items = vec![item("A"), item("B"), item("C"), item("D")];
        let r = layout_popup(anchor, &items, 900.0, 700.0);
        assert!(r.y + r.h <= anchor.y + 0.5, "expected flip above the field, y={} h={}", r.y, r.h);
        assert!(r.y >= OVERLAY_MARGIN);
    }

    #[test]
    fn popup_opens_below_when_there_is_room() {
        let anchor = Rect { x: 40.0, y: 100.0, w: 200.0, h: 22.0 };
        let items = vec![item("A")];
        let r = layout_popup(anchor, &items, 900.0, 700.0);
        assert!(r.y >= anchor.y + anchor.h);
        assert!(r.x >= OVERLAY_MARGIN);
        assert!(r.x + r.w <= 900.0 - OVERLAY_MARGIN + 0.5);
    }

    #[test]
    fn settings_stays_in_window() {
        for (w, h) in [(400.0, 300.0), (SETTINGS_WINDOW_W, SETTINGS_WINDOW_H), (320.0, 240.0)] {
            let s = settings_panel(w, h);
            assert!(s.x >= 0.0, "settings x {s:?} in {w}x{h}");
            assert!(s.x + s.w <= w + 0.5);
            assert!(s.y >= HEADER_H, "settings title/card clear the traffic-light band");
            assert!(s.y + s.h <= h + 0.5);
            let close = window_close_rect(w, h);
            assert!(s.y + s.h <= close.y + 0.5, "panel should sit above Close");
        }
    }

    #[test]
    fn settings_projects_folder_hit_sits_between_midi_and_apply() {
        let hits = settings_hits(900.0, 700.0);
        assert!(hits.projects_folder.y >= hits.midi.y + hits.midi.h);
        assert!(hits.projects_folder.y + hits.projects_folder.h <= hits.apply.y);
        assert!(contains(hits.panel, hits.projects_folder.x + 4.0, hits.projects_folder.y + 4.0));
    }

    #[test]
    fn settings_paints_projects_folder_label_and_path() {
        let engine = test_engine();
        let (cmds, _) = paint_settings(
            &engine,
            SETTINGS_WINDOW_W,
            SETTINGS_WINDOW_H,
            &TextFocus::None,
            false,
            "/tmp/MixLink Projects",
        );
        let texts: Vec<&str> = cmds
            .iter()
            .filter_map(|c| match c {
                DrawCmd::Text(t) => Some(t.text.as_str()),
                _ => None,
            })
            .collect();
        assert!(texts.contains(&"Projects folder"), "{texts:?}");
        assert!(texts.contains(&"/tmp/MixLink Projects"), "{texts:?}");
        assert!(texts.iter().any(|t| t.eq_ignore_ascii_case("close")), "{texts:?}");
        assert!(texts.contains(&"Settings"), "{texts:?}");
    }

    #[test]
    fn settings_close_is_bottom_right() {
        let hits = settings_hits(SETTINGS_WINDOW_W, SETTINGS_WINDOW_H);
        let close = window_close_rect(SETTINGS_WINDOW_W, SETTINGS_WINDOW_H);
        assert!((hits.close.x - close.x).abs() < 0.5);
        assert!((hits.close.y - close.y).abs() < 0.5);
        assert!((hits.close.w - close.w).abs() < 0.5);
        assert!(close.x + close.w <= SETTINGS_WINDOW_W - 8.0);
        assert!(close.y + close.h <= SETTINGS_WINDOW_H - 8.0);
        assert!(close.x > SETTINGS_WINDOW_W * 0.5);
        assert!(hits.apply.y + hits.apply.h <= close.y);
    }

    #[test]
    fn settings_has_no_mixer_dimmer() {
        let engine = test_engine();
        let (cmds, _) = paint_settings(
            &engine,
            SETTINGS_WINDOW_W,
            SETTINGS_WINDOW_H,
            &TextFocus::None,
            false,
            "/tmp",
        );
        let dimmer = cmds.iter().any(|c| match c {
            DrawCmd::Rect { rect, color: [0.0, 0.0, 0.0, a] } => {
                *a > 0.5
                    && *a < 0.7
                    && rect.w >= SETTINGS_WINDOW_W - 1.0
                    && rect.h >= SETTINGS_WINDOW_H - 1.0
            }
            _ => false,
        });
        assert!(!dimmer, "Settings is a window, not a mixer overlay");
    }

    #[test]
    fn channels_columns_split_document_panel() {
        let panel = document_panel(CHANNELS_WINDOW_W, CHANNELS_WINDOW_H);
        let (l, r) = channels_columns(CHANNELS_WINDOW_W, CHANNELS_WINDOW_H);
        assert!((l.x - panel.x).abs() < 0.5);
        assert!((l.y - panel.y).abs() < 0.5);
        assert!((l.h - panel.h).abs() < 0.5);
        assert!((r.h - panel.h).abs() < 0.5);
        assert!((l.w + r.w - panel.w).abs() < 1.0);
        assert!(l.w > 300.0 && r.w > 300.0);
        assert!((r.x + r.w - (panel.x + panel.w)).abs() < 0.5);
        assert!(l.y >= HEADER_H);
    }

    #[test]
    fn menu_hover_ignores_x_outside_shifted_rect() {
        let rect = Rect { x: 100.0, y: 50.0, w: 200.0, h: 80.0 };
        let items = vec![item("A"), item("B")];
        assert!(menu_at(rect, &items, 110.0, 60.0).is_some());
        assert!(menu_at(rect, &items, 20.0, 60.0).is_none());
    }

    fn test_engine() -> AnalogEngine {
        let config = analog::SessionConfig::new();
        let mixer = analog::MixerState::new();
        let surface = analog::SurfaceState::new();
        AnalogEngine::new(mixer, surface, config, osc::OscSession::new())
    }

    #[test]
    fn channels_mix_out_hit_is_settings_style_picker() {
        let engine = test_engine();
        let (_, hits) = paint_channels(
            &engine,
            CHANNELS_WINDOW_W,
            CHANNELS_WINDOW_H,
            &TextFocus::None,
            false,
            0.0,
        );
        let panel = document_panel(CHANNELS_WINDOW_W, CHANNELS_WINDOW_H);
        let picker = channels_mix_out_rect(CHANNELS_WINDOW_W);
        let settings = settings_hits(SETTINGS_WINDOW_W, SETTINGS_WINDOW_H);
        assert!((picker.x - (panel.x + DOC_FIELD_X)).abs() < 0.5);
        assert!((picker.w - settings.projects_folder.w).abs() < 0.5);
        assert!((picker.h - settings.projects_folder.h).abs() < 0.5);
        assert!(picker.y >= HEADER_H);
        assert!(picker.y + picker.h <= channels_list_top(CHANNELS_WINDOW_H));
        assert!(picker.x >= HEADER_TRAFFIC_INSET);
        assert_eq!(
            hit_channels(&hits, picker.x + picker.w * 0.5, picker.y + picker.h * 0.5),
            Some(ChannelsHit::MixOut)
        );
        let (left, _) = channels_columns(CHANNELS_WINDOW_W, CHANNELS_WINDOW_H);
        assert_ne!(
            hit_channels(
                &hits,
                left.x + CHANNELS_PAD_X + 8.0,
                channels_list_top(CHANNELS_WINDOW_H) + 8.0
            ),
            Some(ChannelsHit::MixOut)
        );
        let close = window_close_rect(CHANNELS_WINDOW_W, CHANNELS_WINDOW_H);
        assert_eq!(
            hit_channels(&hits, close.x + close.w * 0.5, close.y + close.h * 0.5),
            Some(ChannelsHit::Close)
        );
        assert!(document_chrome_drag(HEADER_TRAFFIC_INSET * 0.5, 12.0));
        assert_eq!(hit_channels(&hits, HEADER_TRAFFIC_INSET * 0.5, 12.0), None);
        assert!(!document_chrome_drag(close.x + 4.0, close.y + 4.0));
    }

    #[test]
    fn settings_content_clears_titlebar_and_empty_chrome_drags() {
        let hits = settings_hits(SETTINGS_WINDOW_W, SETTINGS_WINDOW_H);
        assert!(hits.panel.y >= HEADER_H);
        assert!(hits.post_fader.y >= HEADER_H);
        assert!(hits.close.y > HEADER_H);
        assert!(document_chrome_drag(12.0, 12.0));
        assert!(!document_chrome_drag(hits.osc_host.x + 4.0, hits.osc_host.y + 4.0));
        assert!(!document_chrome_drag(hits.close.x + 4.0, hits.close.y + 4.0));
    }

    #[test]
    fn channels_mix_out_paints_label_and_current_output() {
        let mut engine = test_engine();
        engine.config.main_output = 14;
        let name = engine.output_name(14);
        let (cmds, _) = paint_channels(
            &engine,
            CHANNELS_WINDOW_W,
            CHANNELS_WINDOW_H,
            &TextFocus::None,
            false,
            0.0,
        );
        let texts: Vec<&str> = cmds
            .iter()
            .filter_map(|c| match c {
                DrawCmd::Text(t) => Some(t.text.as_str()),
                _ => None,
            })
            .collect();
        assert!(texts.contains(&"Channels"), "{texts:?}");
        assert!(texts.contains(&"Mix (Main Out)"), "{texts:?}");
        assert!(texts.contains(&"Inputs"), "{texts:?}");
        assert!(texts.contains(&"Outputs"), "{texts:?}");
        assert!(texts.iter().any(|t| t.eq_ignore_ascii_case("close")), "{texts:?}");
        assert!(texts.iter().any(|t| *t == name), "missing {name:?} in {texts:?}");
        let title = cmds.iter().find_map(|c| match c {
            DrawCmd::Text(t) if t.text == "Channels" => Some(t),
            _ => None,
        });
        let settings_title = {
            let (scmds, _) = paint_settings(
                &engine,
                SETTINGS_WINDOW_W,
                SETTINGS_WINDOW_H,
                &TextFocus::None,
                false,
                "/tmp",
            );
            scmds.iter().find_map(|c| match c {
                DrawCmd::Text(t) if t.text == "Settings" => Some(t.size),
                _ => None,
            })
        };
        let Some(title) = title else { panic!("expected Channels title") };
        assert!((title.size - settings_title.unwrap_or(14.0)).abs() < 0.05);
        assert!(title.bold);
    }

    #[test]
    fn channels_panel_matches_settings_document() {
        let channels = document_panel(CHANNELS_WINDOW_W, CHANNELS_WINDOW_H);
        let settings = settings_panel(SETTINGS_WINDOW_W, SETTINGS_WINDOW_H);
        assert!((channels.x - settings.x).abs() < 0.5);
        assert!((channels.y - settings.y).abs() < 0.5);
        assert!(channels.y >= HEADER_H);
        let close = window_close_rect(CHANNELS_WINDOW_W, CHANNELS_WINDOW_H);
        assert!(channels.y + channels.h <= close.y + 0.5);
        assert!(close.x > CHANNELS_WINDOW_W * 0.5);
    }

    #[test]
    fn channels_content_clears_titlebar_and_empty_chrome_drags() {
        let panel = document_panel(CHANNELS_WINDOW_W, CHANNELS_WINDOW_H);
        let picker = channels_mix_out_rect(CHANNELS_WINDOW_W);
        let close = window_close_rect(CHANNELS_WINDOW_W, CHANNELS_WINDOW_H);
        assert!(panel.y >= HEADER_H);
        assert!(picker.y >= HEADER_H);
        assert!(channels_list_top(CHANNELS_WINDOW_H) >= picker.y + picker.h);
        assert!(document_chrome_drag(12.0, 12.0));
        assert!(!document_chrome_drag(picker.x + 4.0, picker.y + 4.0));
        assert!(!document_chrome_drag(close.x + 4.0, close.y + 4.0));
    }

    #[test]
    fn channels_popup_stays_in_window_without_footer() {
        let anchor = channels_mix_out_rect(CHANNELS_WINDOW_W);
        let items = vec![item("A"), item("B"), item("C")];
        let r = layout_popup_window(anchor, &items, CHANNELS_WINDOW_W, CHANNELS_WINDOW_H);
        assert!(r.x >= OVERLAY_MARGIN - 0.5);
        assert!(r.x + r.w <= CHANNELS_WINDOW_W - OVERLAY_MARGIN + 0.5);
        assert!(r.y >= OVERLAY_MARGIN - 0.5);
        assert!(r.y + r.h <= CHANNELS_WINDOW_H - OVERLAY_MARGIN + 0.5);
        assert!(r.y >= anchor.y + anchor.h);
    }
}
