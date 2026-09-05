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

pub(crate) struct Chrome {
    pub window: Arc<Window>,
    pub renderer: render::Renderer,
    pub page: Page,
    pub cursor: (f32, f32),
    pub drag: Option<Drag>,
    pub last_click: Option<(Instant, f32, f32)>,
    pub overlay: Option<Overlay>,
    pub channels: Option<ChannelsWindow>,
    pub modifiers: ModifiersState,
    pub text_focus: TextFocus,
    pub edit_buf: String,
    pub edit_replace: bool,
    pub caret_on: bool,
    pub caret_at: Instant,
    pub sidebar_scroll: f32,
    pub sidebar_hits: Vec<(Rect, SidebarHit)>,
    pub mixer_extras: Vec<(Rect, MixerExtraHit)>,
    pub waveforms: asset::WaveformCache,
    pub trim_cursors: Option<crate::cursors::TrimCursors>,
    pub last_cursor: crate::cursors::ArrCursor,
    pub show_knobs: bool,
    pub show_mixer: bool,
    pub show_inserts: bool,
    pub mixer_scroll: f32,
}

/// MixLink `Window("Channels")` — dedicated wgpu window, not a mixer overlay.
pub(crate) struct ChannelsWindow {
    pub renderer: render::Renderer,
    pub window: Arc<Window>,
    pub cursor: (f32, f32),
    pub scroll: f32,
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
}
