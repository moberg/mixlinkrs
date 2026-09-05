//! Shared app types. Domain behavior lives in `impl AppState` on sibling modules.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use analog::{AnalogEngine, MixerState, ReturnLane, SessionConfig, SurfaceState, XlRuntime};
use engine::{Engine, EngineHandles, Schedule};
use midi_xl::{LedFrame, MidiSession, SessionEvent};
use project::{
    ArrSelection, MixClip, MixDocument, MixGrid, MixLane, MixPasteboard, ProjectStore, TakeInfo,
    UndoStack,
};
use render::Rect;
use ui_mixlink::chrome::Page;
use ui_mixlink::mixer::{MixerExtraHit, MixerLayout, StripKind};
use ui_mixlink::overlay::{Overlay, TextFocus};
use ui_mixlink::sidebar::SidebarHit;
use winit::dpi::LogicalSize;
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::ModifiersState;
use winit::window::Window;

pub(crate) struct AppState {
    pub window: Arc<Window>,
    pub renderer: render::Renderer,
    pub analog: AnalogEngine,
    pub engine: *mut Engine,
    pub engine_handles: EngineHandles,
    pub _stream: Option<audio_io::DeviceStream>,
    pub midi: MidiIo,
    pub page: Page,
    pub cursor: (f32, f32),
    pub drag: Option<Drag>,
    pub last_click: Option<(Instant, f32, f32)>,
    pub recording: bool,
    pub playing: bool,
    pub automation_armed: bool,
    pub show_knobs: bool,
    pub show_mixer: bool,
    pub show_inserts: bool,
    pub xl: XlRuntime,
    pub led_refresh_at: Option<Instant>,
    pub last_led: Option<LedFrame>,
    pub project: ProjectStore,
    pub mix: Option<MixDocument>,
    pub mixes: Vec<MixDocument>,
    pub takes: Vec<i32>,
    pub take_infos: Vec<TakeInfo>,
    pub take_view: Option<Vec<project::MixTrack>>,
    pub viewing_take: Option<i32>,
    pub recorder: Option<crate::record::Recorder>,
    pub mix_player: Option<crate::mix_play::MixPlayer>,
    pub control_room_fader: f32,
    pub pasteboard: Option<MixPasteboard>,
    pub undo: UndoStack<MixDocument>,
    pub selected_lane: Option<MixLane>,
    pub selection: ArrSelection,
    pub pixels_per_bar: f32,
    pub scroll_x: f32,
    pub scroll_y: f32,
    pub grid: MixGrid,
    pub grid_enabled: bool,
    pub tempo: f64,
    pub locate_frame: i64,
    pub arrangement_origin: i64,
    pub clip_preview: Option<Vec<(MixLane, MixClip)>>,
    pub clip_readout: Option<String>,
    pub mixer_scroll: f32,
    pub sidebar_scroll: f32,
    pub overlay: Option<Overlay>,
    pub channels: Option<ChannelsWindow>,
    pub modifiers: ModifiersState,
    pub text_focus: TextFocus,
    pub edit_buf: String,
    pub edit_replace: bool,
    pub caret_on: bool,
    pub caret_at: Instant,
    pub last_midi: String,
    pub sidebar_hits: Vec<(Rect, SidebarHit)>,
    pub mixer_extras: Vec<(Rect, MixerExtraHit)>,
    pub waveforms: asset::WaveformCache,
    pub plugin_refs: HashMap<i32, vst3_host::MixLinkVST3Ref>,
    pub insert_refs: HashMap<uuid::Uuid, vst3_host::MixLinkVST3Ref>,
    pub take_number: i32,
    pub trim_cursors: Option<crate::cursors::TrimCursors>,
    pub last_cursor: crate::cursors::ArrCursor,
}

/// MixLink `Window("Channels")` — dedicated wgpu window, not a mixer overlay.
pub(crate) struct ChannelsWindow {
    pub renderer: render::Renderer,
    pub window: Arc<Window>,
    pub cursor: (f32, f32),
    pub scroll: f32,
}

pub(crate) enum MidiIo {
    None(MidiSession<midi_xl::NullSink>),
    #[cfg(target_os = "macos")]
    Hw(MidiSession<midi_xl::MidiEndpoint>),
}

impl MidiIo {
    pub(crate) fn status(&self) -> String {
        match self {
            Self::None(s) => s.led_status.clone(),
            #[cfg(target_os = "macos")]
            Self::Hw(s) => s.led_status.clone(),
        }
    }

    pub(crate) fn drain(&mut self) -> Vec<SessionEvent> {
        match self {
            Self::None(_) => Vec::new(),
            #[cfg(target_os = "macos")]
            Self::Hw(s) => s.drain_input(),
        }
    }

    pub(crate) fn send_leds(&mut self, frame: &LedFrame) {
        match self {
            Self::None(s) => s.send_leds(frame),
            #[cfg(target_os = "macos")]
            Self::Hw(s) => s.send_leds(frame),
        }
    }

    pub(crate) fn last_message(&self) -> String {
        match self {
            Self::None(s) => s.last_message.clone(),
            #[cfg(target_os = "macos")]
            Self::Hw(s) => s.last_message.clone(),
        }
    }
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

impl AppState {
    pub(crate) fn boot(event_loop: &ActiveEventLoop) -> Self {
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("MixLinkRs")
                        .with_inner_size(LogicalSize::new(1672.0, 941.0))
                        .with_min_inner_size(LogicalSize::new(
                            MixerLayout::min_window_width(3) as f64,
                            640.0,
                        )),
                )
                .expect("window"),
        );
        crate::native::apply_app_icon();
        let renderer = pollster::block_on(render::Renderer::new(window.clone()));

        let config = SessionConfig::load();
        let mut mixer = MixerState::new();
        mixer.monitored_output = config.main_output;
        let mut surface = SurfaceState::new();
        surface.load_returns(&config);
        let osc = analog::OscSession::new();
        if let Err(e) =
            osc.start(&config.osc_host, config.osc_send_port as u16, config.osc_listen_port as u16)
        {
            log::warn!("OSC: {e}");
        }
        osc.send_dump_requests();
        let analog = AnalogEngine::new(mixer, surface, config.clone(), osc);

        let (engine, handles) = Engine::new(48_000);
        let engine_leaked: &'static mut Engine = Box::leak(Box::new(engine));
        let engine_ptr = engine_leaked as *mut Engine;
        handles.schedule.store(Arc::new(Schedule::empty()));

        let needle = config.audio_device_contains.clone();
        let frames = config.audio_buffer_frames.map(|n| n as u32);
        let stream = audio_io::DeviceStream::open_named(
            &needle,
            frames,
            Box::new(move |input, output, timing| {
                engine_leaked.process(input, output, timing.frames as usize, timing.host_time);
            }),
        );
        let stream = match stream {
            Ok(s) => {
                log::info!("audio {} Hz / {} frames", s.sample_rate(), s.buffer_frames());
                Some(s)
            }
            Err(e) => {
                log::warn!("audio: {e}");
                None
            }
        };

        #[cfg(target_os = "macos")]
        let midi = match MidiSession::connect(&config.midi_device_contains) {
            Ok(s) => MidiIo::Hw(s),
            Err(e) => {
                log::warn!("MIDI: {e}");
                MidiIo::None(MidiSession::no_output())
            }
        };
        #[cfg(not(target_os = "macos"))]
        let midi = MidiIo::None(MidiSession::no_output());

        #[cfg(target_os = "macos")]
        platform_mac::permission::request_mic_permission();

        let mut boot = AppState {
            window,
            renderer,
            analog,
            engine: engine_ptr,
            engine_handles: handles,
            _stream: stream,
            midi,
            page: Page::Record,
            cursor: (0.0, 0.0),
            drag: None,
            last_click: None,
            recording: false,
            playing: false,
            automation_armed: false,
            show_knobs: false,
            show_mixer: true,
            show_inserts: false,
            xl: XlRuntime::default(),
            led_refresh_at: None,
            last_led: None,
            project: ProjectStore::new(),
            mix: None,
            mixes: Vec::new(),
            takes: Vec::new(),
            take_infos: Vec::new(),
            take_view: None,
            viewing_take: None,
            recorder: None,
            mix_player: None,
            control_room_fader: osc::FADER_LIN_0DB,
            pasteboard: None,
            undo: UndoStack::new(),
            selected_lane: None,
            selection: ArrSelection::default(),
            pixels_per_bar: 48.0,
            scroll_x: 0.0,
            scroll_y: 0.0,
            grid: MixGrid::Bar1,
            grid_enabled: true,
            tempo: 120.0,
            locate_frame: 0,
            arrangement_origin: 0,
            clip_preview: None,
            clip_readout: None,
            mixer_scroll: 0.0,
            sidebar_scroll: 0.0,
            overlay: None,
            channels: None,
            modifiers: ModifiersState::empty(),
            text_focus: TextFocus::None,
            edit_buf: String::new(),
            edit_replace: false,
            caret_on: true,
            caret_at: Instant::now(),
            last_midi: String::new(),
            sidebar_hits: Vec::new(),
            mixer_extras: Vec::new(),
            waveforms: asset::WaveformCache::new(),
            plugin_refs: HashMap::new(),
            insert_refs: HashMap::new(),
            take_number: 1,
            trim_cursors: crate::cursors::TrimCursors::create(event_loop),
            last_cursor: crate::cursors::ArrCursor::Default,
        };
        boot.xl.clear_on_connect();
        boot.publish_schedule();
        boot.reload_mix();
        boot.load_configured_plugins();
        boot
    }
}
