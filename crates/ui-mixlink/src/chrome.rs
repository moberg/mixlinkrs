use analog::AnalogEngine;
use render::{DrawCmd, Rect};

use crate::overlay::TextFocus;
use crate::theme::{self, Layout};
use crate::widgets;

pub const HEADER_H: f32 = 36.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Record,
    Mix,
}

pub struct ChromeState<'a> {
    pub page: Page,
    pub tempo: f64,
    pub playing: bool,
    pub recording: bool,
    pub position: String,
    pub grid_on: bool,
    pub grid_title: String,
    pub auto_on: bool,
    pub knobs_on: bool,
    pub osc_connected: bool,
    pub osc_status: String,
    pub midi_status: String,
    pub last_midi: String,
    pub mute_mode: bool,
    pub solo_mode: bool,
    pub engine: &'a AnalogEngine,
    pub project_name: String,
    pub focus: &'a TextFocus,
    pub caret: bool,
}

pub fn paint(state: &ChromeState<'_>, w: f32, h: f32) -> Vec<DrawCmd> {
    let mut cmds = Vec::with_capacity(256);
    theme::fill(&mut cmds, Rect { x: 0.0, y: 0.0, w, h }, theme::WINDOW);
    paint_header(&mut cmds, state, w);
    paint_footer(&mut cmds, state, w, h);
    cmds
}

fn paint_header(cmds: &mut Vec<DrawCmd>, state: &ChromeState<'_>, w: f32) {
    theme::hardware_surface(cmds, Rect { x: 0.0, y: 0.0, w, h: HEADER_H }, theme::SurfaceStyle::Sidebar);
    theme::seam_h(cmds, 0.0, HEADER_H - 2.0, w, true);
    theme::text(cmds, Rect { x: 10.0, y: 8.0, w: 48.0, h: 20.0 }, "TEMPO", 9.0, theme::SECONDARY_TEXT, true);
    widgets::text_field_tempo(
        cmds,
        Rect { x: 58.0, y: 6.0, w: 52.0, h: 24.0 },
        &format!("{:.1}", state.tempo),
        *state.focus == TextFocus::Tempo,
        state.caret,
    );
    theme::text(cmds, Rect { x: 114.0, y: 8.0, w: 28.0, h: 20.0 }, "BPM", 10.0, theme::TEXT_DIM, false);
    if state.page == Page::Mix {
        let mut x = 150.0;
        widgets::hardware_pad(
            cmds,
            Rect { x, y: 6.0, w: 72.0, h: 24.0 },
            if state.playing { "Stop" } else { "Play" },
            state.playing,
            theme::METER_GREEN,
        );
        x += 78.0;
        theme::text_mono(cmds, Rect { x, y: 8.0, w: 80.0, h: 20.0 }, &state.position, 12.0, theme::TEXT, false);
        x += 86.0;
        widgets::hardware_pad(cmds, Rect { x, y: 6.0, w: 56.0, h: 24.0 }, "Grid", state.grid_on, theme::BLUE);
        x += 60.0;
        theme::text_center_mono(cmds, Rect { x, y: 6.0, w: 56.0, h: 24.0 }, &state.grid_title, 11.0, theme::TEXT, true);
        x += 62.0;
        widgets::hardware_pad(cmds, Rect { x, y: 6.0, w: 64.0, h: 24.0 }, "Auto", state.auto_on, theme::METER_RED);
        x += 70.0;
        widgets::hardware_pad(cmds, Rect { x, y: 6.0, w: 72.0, h: 24.0 }, "Knobs", state.knobs_on, theme::AMBER);
        x += 78.0;
        widgets::hardware_pad(cmds, Rect { x, y: 6.0, w: 80.0, h: 24.0 }, "Export", false, theme::BLUE);
    }
}

fn paint_footer(cmds: &mut Vec<DrawCmd>, state: &ChromeState<'_>, w: f32, h: f32) {
    let y = h - Layout::FOOTER_H;
    theme::hardware_surface(cmds, Rect { x: 0.0, y, w, h: Layout::FOOTER_H }, theme::SurfaceStyle::Sidebar);
    theme::seam_h(cmds, 0.0, y, w, true);
    let led = if state.osc_connected { theme::METER_GREEN } else { theme::TEXT_DIM };
    cmds.push(DrawCmd::RoundedRect {
        rect: Rect { x: 12.0, y: y + 18.0, w: 7.0, h: 7.0 },
        color: led,
        radius: 3.5,
    });
    let osc = if state.osc_connected { "OSC in" } else { "Waiting for TotalMix" };
    let osc_w = (osc.chars().count() as f32 * 6.4).max(40.0);
    theme::text(cmds, Rect { x: 24.0, y: y + 12.0, w: osc_w + 10.0, h: 20.0 }, osc, 11.0, theme::SECONDARY_TEXT, false);
    let midi = format!("{}  {}", state.midi_status, state.last_midi);
    if !midi.trim().is_empty() {
        theme::text_mono(
            cmds,
            Rect { x: 24.0 + osc_w + 16.0, y: y + 12.0, w: 360.0, h: 20.0 },
            midi,
            11.0,
            theme::SECONDARY_TEXT,
            false,
        );
    }
    let cx = w * 0.5 - 140.0;
    widgets::hardware_pad(cmds, Rect { x: cx, y: y + 8.0, w: 90.0, h: 26.0 }, "Mute", state.mute_mode, theme::AMBER);
    widgets::hardware_pad(cmds, Rect { x: cx + 96.0, y: y + 8.0, w: 90.0, h: 26.0 }, "Solo", state.solo_mode, theme::AMBER);
    widgets::hardware_pad(cmds, Rect { x: cx + 192.0, y: y + 8.0, w: 90.0, h: 26.0 }, "Rec", state.recording, theme::METER_RED);

    let rx = w - Layout::SIDEBAR_WIDTH - 348.0;
    route_pill(cmds, rx, y + 8.0, "MAIN", &state.engine.output_name(state.engine.config.main_output));
    route_pill(cmds, rx + 116.0, y + 8.0, "BUS 1", &state.engine.return_display_name(analog::ReturnLane::Bus1));
    route_pill(cmds, rx + 232.0, y + 8.0, "BUS 2", &state.engine.return_display_name(analog::ReturnLane::Bus2));
}

fn route_pill(cmds: &mut Vec<DrawCmd>, x: f32, y: f32, title: &str, value: &str) {
    widgets::hardware_module(cmds, Rect { x, y, w: 112.0, h: 26.0 });
    theme::text(cmds, Rect { x: x + 6.0, y, w: 36.0, h: 26.0 }, title, 9.0, theme::PRIMARY_TEXT, true);
    theme::text(cmds, Rect { x: x + 42.0, y, w: 66.0, h: 26.0 }, value.to_uppercase(), 9.0, theme::SECONDARY_TEXT, false);
}

pub fn hit_chrome(page: Page, w: f32, h: f32, x: f32, y: f32) -> Option<ChromeHit> {
    if y < HEADER_H {
        if x >= 58.0 && x < 122.0 {
            return Some(ChromeHit::Tempo);
        }
        if page == Page::Mix {
            let mut bx = 150.0;
            if x >= bx && x < bx + 72.0 {
                return Some(ChromeHit::Play);
            }
            bx += 78.0 + 86.0;
            if x >= bx && x < bx + 56.0 {
                return Some(ChromeHit::Grid);
            }
            bx += 60.0 + 62.0;
            if x >= bx && x < bx + 64.0 {
                return Some(ChromeHit::Auto);
            }
            bx += 70.0;
            if x >= bx && x < bx + 72.0 {
                return Some(ChromeHit::Knobs);
            }
            bx += 78.0;
            if x >= bx && x < bx + 80.0 {
                return Some(ChromeHit::Export);
            }
        }
        return None;
    }
    if y >= h - Layout::FOOTER_H {
        let cx = w * 0.5 - 140.0;
        if x >= cx && x < cx + 90.0 {
            return Some(ChromeHit::MuteMode);
        }
        if x >= cx + 96.0 && x < cx + 186.0 {
            return Some(ChromeHit::SoloMode);
        }
        if x >= cx + 192.0 && x < cx + 282.0 {
            return Some(ChromeHit::Rec);
        }
    }
    None
}

#[derive(Clone, Copy, Debug)]
pub enum ChromeHit {
    Tempo,
    Play,
    Grid,
    Auto,
    Knobs,
    Export,
    MuteMode,
    SoloMode,
    Rec,
}
