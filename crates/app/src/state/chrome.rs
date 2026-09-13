//! Window chrome: renderer, page, pointer, overlays, and mixer view toggles.

use std::sync::Arc;
use std::time::Instant;

use analog::ReturnLane;
use project::MixLane;
use render::Rect;
use ui_mixlink::chrome::Page;
use ui_mixlink::mixer::{MixerExtraHit, StripKind};
use ui_mixlink::overlay::{Overlay, TextFocus};
use ui_mixlink::sidebar::SidebarHit;
use winit::keyboard::ModifiersState;
use winit::window::Window;

use crate::menu_action::MenuAction;

pub(crate) struct Chrome {
    pub window: Arc<Window>,
    pub renderer: render::Renderer,
    pub page: Page,
    pub cursor: (f32, f32),
    pub drag: Option<Drag>,
    pub last_click: Option<(Instant, f32, f32)>,
    pub overlay: Option<Overlay>,
    pub menu_action: Option<MenuAction>,
    pub channels: Option<ChannelsWindow>,
    pub chains: Option<ChainsWindow>,
    pub settings: Option<SettingsWindow>,
    pub modifiers: ModifiersState,
    pub text_focus: TextFocus,
    pub edit_buf: String,
    pub edit_replace: bool,
    pub caret_on: bool,
    pub caret_at: Instant,
    pub sidebar_scroll: f32,
    pub sidebar_width: f32,
    pub sidebar_hits: Vec<(Rect, SidebarHit)>,
    pub mixer_extras: Vec<(Rect, MixerExtraHit)>,
    pub waveforms: asset::WaveformCache,
    pub trim_cursors: Option<crate::cursors::TrimCursors>,
    pub last_cursor: crate::cursors::ArrCursor,
    pub show_knobs: bool,
    pub show_mixer: bool,
    pub show_inserts: bool,
    pub mixer_scroll: f32,
    pub mixer_touch: Option<TouchPulse<StripKind>>,
    pub mix_touch: Option<TouchPulse<usize>>,
}

pub(crate) struct TouchPulse<T> {
    pub id: T,
    pub started: Instant,
    pub last: Instant,
}

impl Chrome {
    pub fn note_strip(&mut self, kind: StripKind) {
        note_pulse(&mut self.mixer_touch, kind);
    }

    pub fn note_mix_track(&mut self, track: usize) {
        note_pulse(&mut self.mix_touch, track);
    }

    pub fn active_strip_glow(&self) -> Option<(StripKind, f32)> {
        pulse_glow(&self.mixer_touch)
    }

    pub fn active_mix_glow(&self) -> Option<(usize, f32)> {
        pulse_glow(&self.mix_touch)
    }
}

fn note_pulse<T: PartialEq>(slot: &mut Option<TouchPulse<T>>, id: T) {
    let now = Instant::now();
    let live = ui_mixlink::mixer::STRIP_TOUCH_HOLD + ui_mixlink::mixer::STRIP_TOUCH_FADE;
    match slot {
        Some(pulse) if pulse.id == id && pulse.last.elapsed().as_secs_f32() < live => {
            pulse.last = now;
        }
        _ => *slot = Some(TouchPulse { id, started: now, last: now }),
    }
}

fn pulse_glow<T: Copy>(slot: &Option<TouchPulse<T>>) -> Option<(T, f32)> {
    let pulse = slot.as_ref()?;
    let amount = ui_mixlink::mixer::strip_touch_alpha(
        pulse.started.elapsed().as_secs_f32(),
        pulse.last.elapsed().as_secs_f32(),
    );
    (amount > 0.01).then_some((pulse.id, amount))
}

/// MixLink `Window("Channels")` — dedicated wgpu window, not a mixer overlay.
pub(crate) struct ChannelsWindow {
    pub renderer: render::Renderer,
    pub window: Arc<Window>,
    pub cursor: (f32, f32),
    pub scroll: f32,
    pub menu: Option<Overlay>,
}

/// Catalog editor for hardware presets and effect chains.
pub(crate) struct ChainsWindow {
    pub renderer: render::Renderer,
    pub window: Arc<Window>,
    pub cursor: (f32, f32),
    pub scroll: f32,
    pub tab: ui_mixlink::chains::ChainsTab,
    pub menu: Option<Overlay>,
}

/// MixLink `Settings` scene — dedicated wgpu window, not a mixer overlay.
pub(crate) struct SettingsWindow {
    pub renderer: render::Renderer,
    pub window: Arc<Window>,
    pub cursor: (f32, f32),
}

#[derive(Clone, Debug)]
pub(crate) enum Drag {
    Fader {
        kind: StripKind,
        rail_top: f32,
        rail_bot: f32,
    },
    Knob {
        kind: StripKind,
        lane: Option<ReturnLane>,
        start_y: f32,
        start: f32,
    },
    ClipMove {
        ids: Vec<uuid::Uuid>,
        anchor: uuid::Uuid,
        start_x: f32,
        start_y: f32,
        origins: Vec<(uuid::Uuid, MixLane, i64)>,
        copy: bool,
    },
    ClipEdge {
        id: uuid::Uuid,
        left: bool,
        start_x: f32,
        start_frame: i64,
        start_source: i64,
        start_count: i64,
    },
    ClipFade {
        id: uuid::Uuid,
        left: bool,
        start_x: f32,
        start_frames: i64,
    },
    ClipLoop {
        id: uuid::Uuid,
        start_x: f32,
        start_count: i64,
    },
    ClipSlip {
        id: uuid::Uuid,
        start_x: f32,
        start_source: i64,
    },
    Select {
        start_lane: MixLane,
        start: i64,
        all_lanes: bool,
        start_x: f32,
        start_y: f32,
        live: bool,
    },
    Start {
        origin: i64,
        start_x: f32,
        live: bool,
    },
    Zoom {
        start_ppb: f32,
        start_scroll: f32,
        anchor_bar: f64,
        start_x: f32,
        start_y: f32,
        live: bool,
    },
    MixFader {
        track: usize,
        rail_top: f32,
        rail_bot: f32,
    },
    ControlRoomFader {
        rail_top: f32,
        rail_bot: f32,
    },
    MixPan {
        track: usize,
        start_y: f32,
        start: f32,
    },
    MixKnob {
        track: usize,
        knob: usize,
        start_y: f32,
        start: f32,
    },
    Tempo {
        start_y: f32,
        start_bpm: f64,
        live: bool,
    },
    SidebarResize {
        start_x: f32,
        start_w: f32,
    },
}
