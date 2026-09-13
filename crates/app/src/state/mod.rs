//! Shared app types. Domain behavior lives in `impl AppState` on sibling modules.

mod audio;
mod chrome;
mod session;
mod surface;
mod timeline;

pub(crate) use audio::{Audio, PluginStateScope};
pub(crate) use chrome::{ChannelsWindow, ChainsWindow, Chrome, Drag, SettingsWindow};
pub(crate) use session::Session;
pub(crate) use surface::{MidiIo, Surface};
pub(crate) use timeline::Timeline;

use analog::{AnalogEngine, MixerState, SessionConfig, SurfaceState};
use engine::{Engine, Schedule};
use midi_xl::MidiSession;
use project::{ArrSelection, MixGrid, ProjectStore, UndoStack};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use ui_mixlink::chrome::Page;
use ui_mixlink::mixer::MixerLayout;
use ui_mixlink::overlay::TextFocus;
use winit::dpi::LogicalSize;
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::ModifiersState;
use winit::window::Window;

pub(crate) struct AppState {
    pub surface: Surface,
    pub audio: Audio,
    pub session: Session,
    pub timeline: Timeline,
    pub chrome: Chrome,
}

impl AppState {
    pub(crate) fn boot(event_loop: &ActiveEventLoop) -> Self {
        let window = Arc::new(
            event_loop
                .create_window(crate::native::merge_titlebar(
                    Window::default_attributes()
                        .with_title("MixLinkRs")
                        .with_inner_size(LogicalSize::new(1672.0, 941.0))
                        .with_min_inner_size(LogicalSize::new(
                            MixerLayout::min_window_width(3) as f64,
                            640.0,
                        )),
                ))
                .expect("window"),
        );
        crate::native::apply_app_icon();
        let renderer = pollster::block_on(render::Renderer::new(window.clone()));

        let config = SessionConfig::load();
        let mut mixer = MixerState::new();
        mixer.monitored_output = config.main_output;
        let mut surface_state = SurfaceState::new();
        surface_state.load_returns(&config);
        let osc = analog::OscSession::new();
        if let Err(e) =
            osc.start(&config.osc_host, config.osc_send_port as u16, config.osc_listen_port as u16)
        {
            log::warn!("OSC: {e}");
        }
        osc.send_dump_requests();
        let mut analog = AnalogEngine::new(mixer, surface_state, config.clone(), osc);
        // Isolate only. apply_all_returns would write the default-0 surface
        // into TotalMix (−∞ on Galaxy / Heat / hardware returns).
        analog.isolate_plugin_playback_from_hardware();

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
            surface: Surface {
                analog,
                xl: analog::XlRuntime::default(),
                midi,
                last_midi: String::new(),
                led_refresh_at: None,
                last_led: None,
                control_room_fader: osc::FADER_LIN_0DB,
            },
            audio: Audio {
                engine: engine_ptr,
                engine_handles: handles,
                _stream: stream,
                recorder: None,
                mix_player: None,
                recording: false,
                record_started: None,
                playing: false,
                plugin_refs: HashMap::new(),
                insert_refs: HashMap::new(),
                plugin_state_scope: HashMap::new(),
                plugin_previews: HashMap::new(),
                preview_due: HashMap::new(),
            },
            session: Session {
                project: ProjectStore::new(),
                mix: None,
                mixes: Vec::new(),
                takes: Vec::new(),
                take_names: HashMap::new(),
                take_infos: Vec::new(),
                take_view: None,
                viewing_take: None,
                take_number: 1,
                undo: UndoStack::new(),
                pasteboard: None,
            },
            timeline: Timeline {
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
                automation_armed: false,
            },
            chrome: Chrome {
                window,
                renderer,
                page: Page::Record,
                cursor: (0.0, 0.0),
                drag: None,
                last_click: None,
                overlay: None,
                menu_action: None,
                channels: None,
                chains: None,
                settings: None,
                modifiers: ModifiersState::empty(),
                text_focus: TextFocus::None,
                edit_buf: String::new(),
                edit_replace: false,
                caret_on: true,
                caret_at: Instant::now(),
                sidebar_scroll: 0.0,
                sidebar_width: config.sidebar_width,
                sidebar_hits: Vec::new(),
                mixer_extras: Vec::new(),
                waveforms: asset::WaveformCache::new(),
                trim_cursors: crate::cursors::TrimCursors::create(event_loop),
                last_cursor: crate::cursors::ArrCursor::Default,
                show_knobs: false,
                show_mixer: true,
                show_inserts: false,
                mixer_scroll: 0.0,
                mixer_touch: None,
                mix_touch: None,
            },
        };
        boot.surface.xl.clear_on_connect();
        vst3_host::install_state_dirty_handler();
        boot.publish_schedule();
        boot.reload_mix();
        boot.load_configured_plugins();
        boot
    }
}
