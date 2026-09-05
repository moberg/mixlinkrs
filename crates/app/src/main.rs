//! MixLinkRs — winit + wgpu shell.

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;

use analog::{
    apply_xl, AnalogEngine, ChannelID, EffectRef, MixAssign, MixerBus, MixerState, ReturnLane,
    SessionConfig, SurfaceState, XlEffect, XlRuntime,
};
use engine::{AudioTapBinding, Engine, EngineHandles, LanePlayer, MixGain, Schedule, StripFeed};
use engine_api::{UiCommand, MASTER_TAP, MIX_PLAY_MAX_LANES, TAP_COUNT};
use midi_xl::{
    describe, schedule_refresh, LedFrame, MidiSession, SessionEvent, TrackControlMode as XlMode,
};
use project::{
    MixArrangement, MixAutomationTarget, MixDocument, MixGrid, MixInsert, MixLane, MixListEntry,
    MixPasteboard, MixTime, MixTrack, ProjectStore, TakeInfo, UndoStack,
};
use render::Rect;
use ui_mixlink::arrangement::ArrangementLayout;
use ui_mixlink::chrome::{self, ChromeHit, ChromeState, Page, HEADER_H};
use ui_mixlink::hit::{self, Hit, Pad};
use ui_mixlink::mixer::{self, MixerExtraHit, MixerLayout, StripKind};
use ui_mixlink::overlay::{self, MenuAction, Overlay, TextFocus};
use ui_mixlink::sidebar::{self, SidebarHit};
use ui_mixlink::theme::Layout;
use ui_mixlink::widgets::MenuItem;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowId};

mod alloc;
mod display_sleep;
mod mix_play;
mod native;
mod record;

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    alloc::install_global_hook();
    let event_loop = EventLoop::new()?;
    let mut app = App::default();
    event_loop.run_app(&mut app)?;
    Ok(())
}

#[derive(Default)]
struct App {
    state: Option<AppState>,
}

struct AppState {
    window: Arc<Window>,
    renderer: render::Renderer,
    analog: AnalogEngine,
    engine: *mut Engine,
    engine_handles: EngineHandles,
    _stream: Option<audio_io::DeviceStream>,
    midi: MidiIo,
    page: Page,
    cursor: (f32, f32),
    drag: Option<Drag>,
    last_click: Option<(Instant, f32, f32)>,
    recording: bool,
    playing: bool,
    automation_armed: bool,
    show_knobs: bool,
    show_mixer: bool,
    show_inserts: bool,
    xl: XlRuntime,
    led_refresh_at: Option<Instant>,
    last_led: Option<LedFrame>,
    project: ProjectStore,
    mix: Option<MixDocument>,
    mixes: Vec<MixDocument>,
    takes: Vec<i32>,
    take_infos: Vec<TakeInfo>,
    take_view: Option<Vec<MixTrack>>,
    viewing_take: Option<i32>,
    recorder: Option<record::Recorder>,
    mix_player: Option<mix_play::MixPlayer>,
    control_room_fader: f32,
    pasteboard: Option<MixPasteboard>,
    undo: UndoStack<MixDocument>,
    selected_lane: Option<MixLane>,
    selected_clips: Vec<uuid::Uuid>,
    pixels_per_bar: f32,
    scroll_x: f32,
    scroll_y: f32,
    grid: MixGrid,
    grid_enabled: bool,
    tempo: f64,
    locate_frame: i64,
    arrangement_origin: i64,
    bar_selection: Option<(MixLane, i64, i64)>,
    mixer_scroll: f32,
    sidebar_scroll: f32,
    overlay: Option<Overlay>,
    channels: Option<ChannelsWindow>,
    modifiers: ModifiersState,
    text_focus: TextFocus,
    caret_on: bool,
    caret_at: Instant,
    last_midi: String,
    sidebar_hits: Vec<(Rect, SidebarHit)>,
    mixer_extras: Vec<(Rect, MixerExtraHit)>,
    waveforms: asset::WaveformCache,
    plugin_refs: HashMap<i32, vst3_host::MixLinkVST3Ref>,
    insert_refs: HashMap<uuid::Uuid, vst3_host::MixLinkVST3Ref>,
    take_number: i32,
}

/// MixLink `Window("Channels")` — dedicated wgpu window, not a mixer overlay.
struct ChannelsWindow {
    renderer: render::Renderer,
    window: Arc<Window>,
    cursor: (f32, f32),
    scroll: f32,
}

enum MidiIo {
    None(MidiSession<midi_xl::NullSink>),
    #[cfg(target_os = "macos")]
    Hw(MidiSession<midi_xl::MidiEndpoint>),
}

impl MidiIo {
    fn status(&self) -> String {
        match self {
            Self::None(s) => s.led_status.clone(),
            #[cfg(target_os = "macos")]
            Self::Hw(s) => s.led_status.clone(),
        }
    }

    fn drain(&mut self) -> Vec<SessionEvent> {
        match self {
            Self::None(_) => Vec::new(),
            #[cfg(target_os = "macos")]
            Self::Hw(s) => s.drain_input(),
        }
    }

    fn send_leds(&mut self, frame: &LedFrame) {
        match self {
            Self::None(s) => s.send_leds(frame),
            #[cfg(target_os = "macos")]
            Self::Hw(s) => s.send_leds(frame),
        }
    }

    fn last_message(&self) -> String {
        match self {
            Self::None(s) => s.last_message.clone(),
            #[cfg(target_os = "macos")]
            Self::Hw(s) => s.last_message.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Drag {
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
    Clip {
        lane: MixLane,
        clip: usize,
        start_x: f32,
        start_frame: i64,
    },
    Select {
        lane: MixLane,
        start: i64,
    },
    Start {
        origin: i64,
        start_x: f32,
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
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }
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
        native::apply_app_icon();
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
            selected_clips: Vec::new(),
            pixels_per_bar: 48.0,
            scroll_x: 0.0,
            scroll_y: 0.0,
            grid: MixGrid::Bar1,
            grid_enabled: true,
            tempo: 120.0,
            locate_frame: 0,
            arrangement_origin: 0,
            bar_selection: None,
            mixer_scroll: 0.0,
            sidebar_scroll: 0.0,
            overlay: None,
            channels: None,
            modifiers: ModifiersState::empty(),
            text_focus: TextFocus::None,
            caret_on: true,
            caret_at: Instant::now(),
            last_midi: String::new(),
            sidebar_hits: Vec::new(),
            mixer_extras: Vec::new(),
            waveforms: asset::WaveformCache::new(),
            plugin_refs: HashMap::new(),
            insert_refs: HashMap::new(),
            take_number: 1,
        };
        boot.xl.clear_on_connect();
        publish_schedule(&boot);
        reload_mix(&mut boot);
        load_configured_plugins(&mut boot);
        self.state = Some(boot);
        self.state.as_ref().unwrap().window.request_redraw();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(state) = self.state.as_mut() else { return };
        let is_channels = state.channels.as_ref().is_some_and(|c| c.window.id() == id);
        let is_main = id == state.window.id();
        if !is_main && !is_channels {
            return;
        }
        match event {
            WindowEvent::CloseRequested if is_channels => {
                close_channels(state);
            }
            WindowEvent::CloseRequested => {
                if state.recording {
                    toggle_record(state);
                }
                if state.playing {
                    halt_mix_play(state);
                }
                state.analog.config.save();
                for slot in 0..vst3_host::SLOT_COUNT {
                    vst3_host::exchange_and_retire(slot, std::ptr::null_mut());
                }
                event_loop.exit();
            }
            WindowEvent::Resized(size) if is_channels => {
                if let Some(ch) = &mut state.channels {
                    ch.renderer.resize(size.width, size.height);
                    ch.window.request_redraw();
                }
            }
            WindowEvent::Resized(size) => {
                state.renderer.resize(size.width, size.height);
                state.window.request_redraw();
            }
            WindowEvent::CursorMoved { position, .. } if is_channels => {
                if let Some(ch) = &mut state.channels {
                    let s = ch.renderer.effective_scale();
                    ch.cursor = (position.x as f32 / s, position.y as f32 / s);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let s = state.renderer.effective_scale();
                let x = position.x as f32 / s;
                let y = position.y as f32 / s;
                state.cursor = (x, y);
                apply_drag(state, x, y);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let scale = if is_channels {
                    state.channels.as_ref().map(|c| c.renderer.effective_scale()).unwrap_or(1.0)
                } else {
                    state.renderer.effective_scale()
                };
                let (dx, dy) = match delta {
                    MouseScrollDelta::PixelDelta(p) => (p.x as f32 / scale, p.y as f32 / scale),
                    MouseScrollDelta::LineDelta(x, y) => (x * 40.0, y * 40.0),
                };
                if is_channels {
                    on_channels_wheel(state, dy);
                } else {
                    on_wheel(state, dx, dy);
                }
            }
            WindowEvent::MouseInput { state: st, button: MouseButton::Left, .. } if is_channels => {
                if st == ElementState::Pressed {
                    on_channels_press(state);
                }
            }
            WindowEvent::MouseInput { state: st, button: MouseButton::Left, .. } => match st {
                ElementState::Pressed => on_press(state, event_loop),
                ElementState::Released => on_release(state),
            },
            WindowEvent::MouseInput { state: st, button: MouseButton::Right, .. } if is_main => {
                if st == ElementState::Pressed {
                    on_context_press(state);
                }
            }
            WindowEvent::Focused(true) if is_main => {
                if matches!(state.text_focus, TextFocus::GearAlias(_)) {
                    state.text_focus = TextFocus::None;
                }
            }
            WindowEvent::ModifiersChanged(mods) => {
                state.modifiers = mods.state();
            }
            WindowEvent::KeyboardInput { event, .. } if is_channels => {
                if event.state == ElementState::Pressed {
                    on_channels_key(state, &event.logical_key);
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed {
                    return;
                }
                if on_key(state, &event.logical_key) {
                    return;
                }
                match event.logical_key {
                    Key::Named(NamedKey::Tab) => cycle_page(state, state.modifiers.shift_key()),
                    Key::Named(NamedKey::Space) if state.page == Page::Mix => toggle_play(state),
                    Key::Named(NamedKey::Delete) | Key::Named(NamedKey::Backspace)
                        if state.page == Page::Mix =>
                    {
                        delete_clips(state);
                    }
                    Key::Named(NamedKey::ArrowLeft) if state.page == Page::Mix => {
                        select_lane(state, -1)
                    }
                    Key::Named(NamedKey::ArrowRight) if state.page == Page::Mix => {
                        select_lane(state, 1)
                    }
                    Key::Character(c) if c.eq_ignore_ascii_case("z") => {
                        if state.modifiers.super_key() || state.modifiers.control_key() {
                            apply_undo(state, state.modifiers.shift_key());
                        } else if state.page == Page::Mix {
                            state.show_mixer = !state.show_mixer;
                        } else {
                            apply_undo(state, false);
                        }
                    }
                    Key::Character(c) if c.eq_ignore_ascii_case("c") => {
                        if let Some(mix) = &state.mix {
                            state.pasteboard = mix.copy_clips(&state.selected_clips);
                        }
                    }
                    Key::Character(c) if c.eq_ignore_ascii_case("v") => paste_clips(state),
                    Key::Character(c) if c.eq_ignore_ascii_case("i") && state.page == Page::Mix => {
                        state.show_inserts = !state.show_inserts;
                    }
                    _ => {}
                }
            }
            WindowEvent::RedrawRequested if is_channels => {
                paint_channels_window(state);
            }
            WindowEvent::RedrawRequested => {
                tick(state);
                paint(state);
                if let Some(ch) = &state.channels {
                    ch.window.request_redraw();
                }
                state.window.request_redraw();
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _: &ActiveEventLoop) {
        if let Some(state) = self.state.as_mut() {
            poll_midi_and_leds(state);
        }
    }
}

fn tick(state: &mut AppState) {
    display_sleep::poll_external_wake();
    finish_mix_play_if_done(state);
    state.analog.poll_osc();
    poll_midi_and_leds(state);
    if state.caret_at.elapsed().as_millis() > 500 {
        state.caret_on = !state.caret_on;
        state.caret_at = Instant::now();
    }
    write_automation_if_armed(state);
    publish_schedule(state);
}

fn poll_midi_and_leds(state: &mut AppState) {
    let events = state.midi.drain();
    let mut pad_off = false;
    for ev in events {
        pad_off |= matches!(ev, SessionEvent::PadOff);
        handle_midi(state, ev);
    }
    let msg = state.midi.last_message();
    if !msg.is_empty() {
        state.last_midi = msg;
    }
    sync_xl_leds(state, pad_off);
}

/// MixLink `pushPadLEDs` — write every LED, then refresh after the pad flash.
fn push_pad_leds(state: &mut AppState) {
    let frame = led_frame(state);
    state.midi.send_leds(&frame);
    state.last_led = Some(frame);
    state.led_refresh_at = Some(Instant::now() + schedule_refresh());
}

/// MixLink `pushPadLEDs`: full write now, then one more after 80 ms so the XL’s
/// momentary flash does not leave Mute/Solo/Focus dark. Idle frames are silent.
fn sync_xl_leds(state: &mut AppState, reassert: bool) {
    let frame = led_frame(state);
    let refresh_due = state.led_refresh_at.is_some_and(|at| Instant::now() >= at);
    if reassert || state.last_led != Some(frame) {
        state.midi.send_leds(&frame);
        state.last_led = Some(frame);
        state.led_refresh_at = Some(Instant::now() + schedule_refresh());
        return;
    }
    if refresh_due {
        state.led_refresh_at = None;
        state.midi.send_leds(&frame);
        state.last_led = Some(frame);
    }
}

fn led_frame(state: &AppState) -> LedFrame {
    let mut focus = [false; 8];
    let mut control = [midi_xl::LED_OFF; 8];
    let mode = match state.analog.surface.track_control_mode {
        analog::TrackControlMode::BusAssign => XlMode::BusAssign,
        analog::TrackControlMode::Mute => XlMode::Mute,
        analog::TrackControlMode::Solo => XlMode::Solo,
    };
    if state.page == Page::Mix {
        let tracks = mixer_tracks(state);
        for i in 0..8 {
            let lane = MixLane::Strip(i as i32);
            let track = tracks.iter().find(|t| t.lane == lane);
            focus[i] = state.selected_lane == Some(lane);
            control[i] = midi_xl::control_led(
                mode,
                false,
                track.map(|t| t.mute).unwrap_or(false),
                track.map(|t| t.solo).unwrap_or(false),
            );
        }
    } else {
        for i in 0..8 {
            let strip = &state.analog.surface.strips[i];
            focus[i] = strip.assign == MixAssign::Bus1;
            let id = state.analog.config.strips[i].channel_id();
            let muted = state.analog.mixer.channel(id).map(|c| c.mute).unwrap_or(false);
            let soloed = state.analog.mixer.channel(id).map(|c| c.solo).unwrap_or(false);
            control[i] = midi_xl::control_led(mode, strip.assign == MixAssign::Bus2, muted, soloed);
        }
    }
    LedFrame {
        focus_on_bus1: focus,
        control_vel: control,
        device: display_sleep::is_asleep(),
        mute: matches!(state.analog.surface.track_control_mode, analog::TrackControlMode::Mute),
        solo: matches!(state.analog.surface.track_control_mode, analog::TrackControlMode::Solo),
        arm: state.recording,
        send_up: state.xl.send_select_up,
        send_down: state.xl.send_select_down,
        track_left: state.analog.config.pan_knobs_control_send_c
            && state.analog.config.effect_return_count >= 3,
    }
}

/// Mix-page XL writes the mix document only. Record analog faders stay put.
fn handle_mix_xl(state: &mut AppState, control: midi_xl::Control, value: f32) -> bool {
    match control {
        midi_xl::Control::Fader(i) => {
            if let Some(track) = mix_index_for_lane(state, MixLane::Strip(i as i32)) {
                set_mix_fader(state, track, value);
            }
            true
        }
        midi_xl::Control::Pan(i) => {
            if state.analog.config.pan_knobs_control_send_c {
                return false;
            }
            if let Some(track) = mix_index_for_lane(state, MixLane::Strip(i as i32)) {
                set_mix_pan(state, track, value);
            }
            true
        }
        midi_xl::Control::Focus(i) => {
            state.selected_lane = Some(MixLane::Strip(i as i32));
            true
        }
        midi_xl::Control::Control(i) => {
            let Some(track) = mix_index_for_lane(state, MixLane::Strip(i as i32)) else {
                return true;
            };
            match state.analog.surface.track_control_mode {
                analog::TrackControlMode::Mute => {
                    if let Some(t) = mix_track_mut(state, track) {
                        t.mute = !t.mute;
                    }
                    persist_mix(state);
                }
                analog::TrackControlMode::Solo => {
                    if let Some(t) = mix_track_mut(state, track) {
                        t.solo = !t.solo;
                    }
                    persist_mix(state);
                }
                analog::TrackControlMode::BusAssign => {}
            }
            true
        }
        _ => false,
    }
}

fn handle_midi(state: &mut AppState, ev: SessionEvent) {
    match ev {
        SessionEvent::Control { control, value } => {
            state.last_midi = describe(control);
            let mode_before = state.analog.surface.track_control_mode;
            if state.page == Page::Mix && handle_mix_xl(state, control, value) {
                if matches!(control, midi_xl::Control::Focus(_) | midi_xl::Control::Control(_)) {
                    push_pad_leds(state);
                }
                sync_rt(state);
                return;
            }
            match apply_xl(&mut state.analog, &mut state.xl, control, value) {
                XlEffect::ToggleRecord => toggle_record(state),
                XlEffect::ToggleSleep => display_sleep::toggle(),
                XlEffect::NudgeMain { next } => {
                    state
                        .analog
                        .osc
                        .send_float(osc::output_fader_lin(state.analog.config.main_output), next);
                }
                XlEffect::None => {}
            }
            // MixLink `toggleMuteMode` / `toggleSoloMode` / `toggleControl` call
            // `pushPadLEDs` on the same MainActor turn as the button.
            if matches!(
                control,
                midi_xl::Control::Mute
                    | midi_xl::Control::Solo
                    | midi_xl::Control::Focus(_)
                    | midi_xl::Control::Control(_)
                    | midi_xl::Control::Arm
                    | midi_xl::Control::Device
                    | midi_xl::Control::SendSelectUp
                    | midi_xl::Control::SendSelectDown
                    | midi_xl::Control::TrackSelectLeft
            ) || state.analog.surface.track_control_mode != mode_before
            {
                push_pad_leds(state);
            }
        }
        SessionEvent::Unmapped { status, data1, data2 } => {
            state.last_midi = format!("{status:02X} {data1:02X} {data2:02X} (unmapped)");
        }
        SessionEvent::PadOff => {}
        SessionEvent::TemplateChanged(_) => {}
    }
    sync_rt(state);
}

fn toggle_record(state: &mut AppState) {
    if state.recording {
        state.recording = false;
        let _ = state.engine_handles.cmd_tx.try_push(UiCommand::SetRecording { on: false });
        if let Some(rec) = state.recorder.take() {
            let _ = rec.stop(state.engine);
        }
        if let Some(folder) = ProjectStore::current_url(&state.analog.config) {
            state.project.increment_take(&folder);
            state.take_number = state.project.next_take(&folder);
            reload_mix(state);
        }
        return;
    }
    if ProjectStore::resolve_root(&state.analog.config).is_none() {
        log::warn!("record: set a projects folder first");
        return;
    }
    if ProjectStore::current_url(&state.analog.config).is_none() {
        match state.project.create_project(&mut state.analog.config) {
            Ok(_) => {
                state.analog.persist();
                reload_mix(state);
            }
            Err(e) => {
                log::warn!("record: could not create project: {e}");
                return;
            }
        }
    }
    let Some(folder) = ProjectStore::current_url(&state.analog.config) else {
        return;
    };
    if state._stream.is_none() {
        log::warn!("record: audio is not running");
        return;
    }
    let sr = state
        ._stream
        .as_ref()
        .map(|s| s.sample_rate())
        .unwrap_or_else(|| unsafe { (*state.engine).sample_rate() });
    state.take_number = state.project.next_take(&folder);
    publish_schedule(state);
    let Some(rec) =
        record::Recorder::start(state.engine, &state.analog, folder, state.take_number, sr)
    else {
        log::warn!("record: could not start take");
        return;
    };
    let _ = state.engine_handles.cmd_tx.try_push(UiCommand::ArmRings);
    state.recorder = Some(rec);
    state.recording = true;
    let _ = state.engine_handles.cmd_tx.try_push(UiCommand::SetRecording { on: true });
}

fn halt_mix_play(state: &mut AppState) {
    if let Some(player) = state.mix_player.take() {
        player.stop();
    }
    state.playing = false;
    let _ = state.engine_handles.cmd_tx.try_push(UiCommand::TransportStop);
    for peak in state.engine_handles.lane_peaks.iter() {
        peak.store(0.0, Ordering::Relaxed);
    }
    state.engine_handles.listen_peak.store(0.0, Ordering::Relaxed);
}

fn finish_mix_play_if_done(state: &mut AppState) {
    if state.playing && state.mix_player.as_ref().is_some_and(|p| !p.is_running()) {
        halt_mix_play(state);
        publish_schedule(state);
    }
}

fn toggle_play(state: &mut AppState) {
    if state.playing {
        halt_mix_play(state);
        let _ = state
            .engine_handles
            .cmd_tx
            .try_push(UiCommand::TransportSeek { sample: state.locate_frame });
        publish_schedule(state);
        return;
    }
    if state._stream.is_none() {
        log::warn!("play: audio is not running");
        return;
    }
    let Some(folder) = ProjectStore::current_url(&state.analog.config) else {
        log::warn!("play: set a projects folder first");
        return;
    };
    let tracks = mixer_tracks(state);
    let end = tracks
        .iter()
        .filter(|t| t.lane != MixLane::Main)
        .flat_map(|t| t.clips.iter())
        .map(project::MixClip::mix_end_frame)
        .max()
        .unwrap_or(0);
    if end <= 0 {
        log::warn!("play: nothing to play");
        return;
    }
    let playhead = state.engine_handles.sample_position.load(Ordering::Relaxed);
    if playhead >= end {
        state.locate_frame = state.arrangement_origin.max(0);
    }
    let _ = state
        .engine_handles
        .cmd_tx
        .try_push(UiCommand::TransportSeek { sample: state.locate_frame });
    let graph = mix_play::MixPlayGraph {
        folder,
        tracks: tracks
            .into_iter()
            .take(MIX_PLAY_MAX_LANES)
            .map(|t| mix_play::MixPlayTrack {
                clips: if t.lane == MixLane::Main { Vec::new() } else { t.clips },
                is_main: t.lane == MixLane::Main,
            })
            .collect(),
        end_frame: end,
    };
    let io_block = state._stream.as_ref().map(|s| s.buffer_frames() as usize).unwrap_or(128);
    state.playing = true;
    publish_schedule(state);
    state.mix_player = Some(mix_play::MixPlayer::start(
        state.engine,
        state.engine_handles.controls.clone(),
        graph,
        state.locate_frame,
        io_block,
    ));
}

/// MixLinkRs: Tab swaps `Page::Record` ↔ `Page::Mix` (header RECORD/MIX). MixLink has no Tab binding.
fn cycle_page(state: &mut AppState, _reverse: bool) {
    if state.page == Page::Mix {
        persist_mix(state);
    }
    state.page = match state.page {
        Page::Record => Page::Mix,
        Page::Mix => Page::Record,
    };
}

fn delete_clips(state: &mut AppState) {
    let ids = state.selected_clips.clone();
    if let Some(mut mix) = state.mix.take() {
        state.undo.mutate("Delete", false, &mut mix, |doc| {
            for track in &mut doc.tracks {
                track.clips.retain(|c| !ids.contains(&c.id));
            }
        });
        state.mix = Some(mix);
        persist_mix(state);
    }
    state.selected_clips.clear();
}

fn select_lane(state: &mut AppState, delta: i32) {
    let lanes: Vec<MixLane> = mixer_tracks(state).into_iter().map(|t| t.lane).collect();
    if lanes.is_empty() {
        return;
    }
    let cur =
        state.selected_lane.and_then(|l| lanes.iter().position(|&x| x == l)).unwrap_or(0) as i32;
    let next = (cur + delta).clamp(0, lanes.len() as i32 - 1) as usize;
    state.selected_lane = Some(lanes[next]);
}

fn sync_rt(state: &mut AppState) {
    for i in 0..8 {
        state.engine_handles.controls.set_fader(i, state.analog.surface.strips[i].fader);
    }
    publish_schedule(state);
}

fn sidebar_open(state: &AppState) -> bool {
    state.page == Page::Record || state.show_inserts
}

fn body_rect(state: &AppState) -> (f32, f32, f32, f32) {
    let (w, h) = state.renderer.logical_size();
    let side = if sidebar_open(state) { Layout::SIDEBAR_WIDTH } else { 0.0 };
    (0.0, HEADER_H, w - side, h - HEADER_H - Layout::FOOTER_H)
}

fn hit_body(state: &AppState, x: f32, y: f32) -> Option<Hit> {
    let (bx, by, bw, bh) = body_rect(state);
    if x < bx || x > bx + bw || y < by || y > by + bh {
        return None;
    }
    match state.page {
        Page::Record => {
            let layout =
                MixerLayout::new(bx, by, bw, bh, state.analog.config.effect_return_count as usize);
            hit::hit_mixer(&layout, state.analog.config.effect_return_count as usize, x, y)
        }
        Page::Mix => {
            let mix_h = ui_mixlink::mix_mixer::height(state.show_knobs, state.show_mixer);
            if y >= by + bh - mix_h {
                let n = mixer_track_count(state);
                return hit::hit_mix_mixer(
                    bx + ui_mixlink::mix_browser::WIDTH,
                    by + bh - mix_h,
                    bw - ui_mixlink::mix_browser::WIDTH,
                    mix_h,
                    n,
                    state.show_knobs,
                    state.show_mixer,
                    x,
                    y,
                );
            }
            let n = arrangement_track_count(state);
            let sr = state._stream.as_ref().map(|s| s.sample_rate() as f64).unwrap_or(48_000.0);
            hit::hit_arrangement(
                &arr_layout(state),
                n,
                state.arrangement_origin,
                state.tempo,
                sr,
                x,
                y,
            )
        }
    }
}

fn on_press(state: &mut AppState, event_loop: &ActiveEventLoop) {
    let (x, y) = state.cursor;
    let (w, h) = state.renderer.logical_size();

    if let Some(overlay) = state.overlay.clone() {
        if handle_overlay_press(state, &overlay, x, y, w, h) {
            return;
        }
    }

    if let Some(hit) = chrome::hit_chrome(state.page, w, h, x, y) {
        match hit {
            ChromeHit::Play => toggle_play(state),
            ChromeHit::Rec => {
                toggle_record(state);
                sync_xl_leds(state, true);
            }
            ChromeHit::MuteMode => {
                state.analog.surface.toggle_mute_mode();
                sync_xl_leds(state, true);
            }
            ChromeHit::SoloMode => {
                state.analog.surface.toggle_solo_mode();
                sync_xl_leds(state, true);
            }
            ChromeHit::Grid => state.grid_enabled = !state.grid_enabled,
            ChromeHit::Auto => state.automation_armed = !state.automation_armed,
            ChromeHit::Knobs => state.show_knobs = !state.show_knobs,
            ChromeHit::Inserts => state.show_inserts = !state.show_inserts,
            ChromeHit::Export => export_mix(state),
            ChromeHit::Tempo => state.text_focus = TextFocus::Tempo,
            ChromeHit::Page(p) => {
                if state.page == Page::Mix {
                    persist_mix(state);
                }
                state.page = p;
            }
        }
        return;
    }

    if let Some((rect, hit)) =
        state.sidebar_hits.iter().rev().find(|(r, _)| overlay::contains(*r, x, y))
    {
        handle_sidebar(state, hit.clone(), *rect, event_loop);
        return;
    }

    if let Some((_, extra)) =
        state.mixer_extras.iter().rev().find(|(r, _)| overlay::contains(*r, x, y))
    {
        match extra {
            MixerExtraHit::AddReturn => state.analog.add_effect_return(),
            MixerExtraHit::RemoveReturn => state.analog.remove_last_effect_return(),
            MixerExtraHit::ControlWithPan => {
                let on = !state.analog.config.pan_knobs_control_send_c;
                state.analog.set_pan_knobs_control_send_c(on);
                sync_xl_leds(state, true);
            }
        }
        return;
    }

    let (bx, by, bw, bh) = body_rect(state);
    let layout = MixerLayout::new(bx, by, bw, bh, state.analog.config.effect_return_count as usize);
    if let Some(rect) = ui_mixlink::mixer::control_with_pan_rect(
        &layout,
        state.analog.config.effect_return_count as usize,
    ) {
        if overlay::contains(rect, x, y) {
            let on = !state.analog.config.pan_knobs_control_send_c;
            state.analog.set_pan_knobs_control_send_c(on);
            return;
        }
    }

    let now = Instant::now();
    let double = state
        .last_click
        .map(|(t, lx, ly)| t.elapsed().as_millis() < 350 && (x - lx).hypot(y - ly) < 4.0)
        .unwrap_or(false);
    state.last_click = Some((now, x, y));

    if state.page == Page::Mix {
        if let Some(hit) = ui_mixlink::mix_browser::hit(&mix_browser_view(state), x, y) {
            if state.modifiers.control_key() {
                on_browser_context(state, hit);
                return;
            }
            match hit {
                ui_mixlink::mix_browser::BrowserHit::Mix(id) => {
                    select_mix(state, id);
                }
                ui_mixlink::mix_browser::BrowserHit::Take(n) => {
                    select_take(state, n);
                }
                ui_mixlink::mix_browser::BrowserHit::Mixer => {
                    state.show_mixer = true;
                }
            }
            return;
        }
    }

    if let Some(hit) = hit_body(state, x, y) {
        match hit {
            Hit::Fader { kind, rail_top, rail_bot } => {
                if double {
                    set_fader(state, kind, osc::FADER_LIN_0DB);
                } else {
                    state.drag = Some(Drag::Fader { kind, rail_top, rail_bot });
                    apply_drag(state, x, y);
                }
            }
            Hit::Knob { kind, lane, .. } => {
                if double && lane.is_none() {
                    set_pan(state, kind, 0.5);
                } else {
                    let start = current_knob(state, kind, lane);
                    state.drag = Some(Drag::Knob { kind, lane, start_y: y, start });
                }
            }
            Hit::Pad { kind: StripKind::Input(i), which } => match which {
                Pad::Solo => state.analog.toggle_solo(i),
                Pad::Mute => state.analog.toggle_mute(i),
                Pad::Bus1 => {
                    let prev = state.analog.surface.strips[i].assign;
                    let next =
                        if prev == MixAssign::Bus1 { MixAssign::Main } else { MixAssign::Bus1 };
                    state.analog.apply_assign(i, prev, next);
                }
                Pad::Bus2 => {
                    let prev = state.analog.surface.strips[i].assign;
                    let next =
                        if prev == MixAssign::Bus2 { MixAssign::Main } else { MixAssign::Bus2 };
                    state.analog.apply_assign(i, prev, next);
                }
            },
            Hit::Pad { kind: StripKind::Return(lane), which } => match which {
                Pad::Solo => state.analog.toggle_return_solo(lane),
                Pad::Mute => state.analog.toggle_return_mute(lane),
                _ => {}
            },
            Hit::Enable { kind: StripKind::Input(i) } => {
                state.analog.config.strips[i].enabled = !state.analog.config.strips[i].enabled;
                state.analog.apply_channel_enable(i);
                state.analog.persist();
            }
            Hit::Enable { kind: StripKind::Return(lane) } => {
                state.analog.toggle_return_enabled(lane);
            }
            Hit::Name { kind } => open_name_menu(state, kind, x, y),
            Hit::Ruler { .. } | Hit::TimeRuler => {
                state.drag = Some(Drag::Zoom {
                    start_ppb: state.pixels_per_bar,
                    start_scroll: state.scroll_x,
                    anchor_bar: ((x - body_rect(state).0 - 148.0 - 86.0) + state.scroll_x) as f64
                        / state.pixels_per_bar.max(1.0) as f64,
                    start_x: x,
                    start_y: y,
                    live: false,
                });
            }
            Hit::Locate => {
                let sr = state._stream.as_ref().map(|s| s.sample_rate() as f64).unwrap_or(48_000.0);
                let frame = hit::frame_at_x(&arr_layout(state), x, state.tempo, sr);
                let snapped = if state.grid_enabled {
                    MixTime::snap(
                        frame,
                        state.grid.raw(),
                        state.tempo,
                        sr,
                        state.arrangement_origin,
                    )
                } else {
                    frame
                };
                locate_to(state, snapped.max(0));
                state.bar_selection = None;
            }
            Hit::Lane { track } => {
                if let Some(t) = arrangement_tracks(state).get(track) {
                    state.selected_lane = Some(t.lane);
                }
            }
            Hit::StartMarker => {
                state.drag = Some(Drag::Start { origin: state.arrangement_origin, start_x: x });
            }
            Hit::Clip { track, clip } => {
                if let Some(t) = arrangement_tracks(state).get(track) {
                    if let Some(c) = t.clips.get(clip) {
                        state.selected_clips = vec![c.id];
                        state.selected_lane = Some(t.lane);
                        state.drag = Some(Drag::Clip {
                            lane: t.lane,
                            clip,
                            start_x: x,
                            start_frame: c.mix_start_frame,
                        });
                    }
                }
            }
            Hit::MixFader { track, rail_top, rail_bot } => {
                if double {
                    set_mix_fader(state, track, osc::FADER_LIN_0DB);
                } else {
                    state.drag = Some(Drag::MixFader { track, rail_top, rail_bot });
                    apply_drag(state, x, y);
                }
            }
            Hit::MixControlRoomFader { rail_top, rail_bot } => {
                if double {
                    state.control_room_fader = osc::FADER_LIN_0DB;
                } else {
                    state.drag = Some(Drag::ControlRoomFader { rail_top, rail_bot });
                    apply_drag(state, x, y);
                }
            }
            Hit::MixPan { track } => {
                if double {
                    set_mix_pan(state, track, 0.5);
                    persist_mix(state);
                } else {
                    let start = mix_track(state, track).map(|t| t.pan).unwrap_or(0.5);
                    state.drag = Some(Drag::MixPan { track, start_y: y, start });
                }
            }
            Hit::MixMute { track } => {
                if let Some(t) = mix_track_mut(state, track) {
                    t.mute = !t.mute;
                    persist_mix(state);
                }
            }
            Hit::MixMixerHandle => {
                state.show_mixer = true;
            }
            Hit::MixSolo { track } => {
                if let Some(t) = mix_track_mut(state, track) {
                    t.solo = !t.solo;
                    persist_mix(state);
                }
            }
            Hit::MixKnob { track, knob } => {
                let start =
                    mix_track(state, track).and_then(|t| t.knobs.get(knob).copied()).unwrap_or(0.0);
                state.drag = Some(Drag::MixKnob { track, knob, start_y: y, start });
            }
            _ => {}
        }
    }
    sync_rt(state);
}

fn arr_layout(state: &AppState) -> ArrangementLayout {
    let (bx, by, bw, bh) = body_rect(state);
    let mix_h = ui_mixlink::mix_mixer::height(state.show_knobs, state.show_mixer);
    ArrangementLayout {
        x: bx + 148.0,
        y: by,
        w: bw - 148.0,
        h: bh - mix_h,
        scroll_x: state.scroll_x,
        scroll_y: state.scroll_y,
        pixels_per_bar: state.pixels_per_bar,
    }
}

fn on_release(state: &mut AppState) {
    if let Some(Drag::Zoom { start_x, start_y, live, .. }) = state.drag {
        if !live {
            let sr = state._stream.as_ref().map(|s| s.sample_rate() as f64).unwrap_or(48_000.0);
            let frame = hit::frame_at_x(&arr_layout(state), start_x, state.tempo, sr);
            locate_to(state, frame.max(0));
        }
        let _ = start_y;
    }
    if matches!(state.drag, Some(Drag::Start { .. })) {
        persist_start(state);
    }
    if matches!(
        state.drag,
        Some(
            Drag::MixFader { .. } | Drag::MixPan { .. } | Drag::MixKnob { .. } | Drag::Clip { .. }
        )
    ) {
        persist_mix(state);
    }
    state.drag = None;
}

fn apply_drag(state: &mut AppState, x: f32, y: f32) {
    match state.drag {
        Some(Drag::Fader { kind, rail_top, rail_bot }) => {
            let h = (rail_bot - rail_top).max(1.0);
            let t = ((rail_bot - y) / h).clamp(0.0, 1.0);
            set_fader(state, kind, t);
        }
        Some(Drag::Knob { kind, lane, start_y, start }) => {
            let next = (start - (y - start_y) / Layout::KNOB_DRAG_PX).clamp(0.0, 1.0);
            if let Some(lane) = lane {
                if let StripKind::Input(i) = kind {
                    state.analog.apply_aux(i, lane, next);
                }
            } else {
                set_pan(state, kind, next);
            }
        }
        Some(Drag::Zoom { start_ppb, start_scroll, anchor_bar, start_x, start_y, .. }) => {
            let dx = x - start_x;
            let dy = y - start_y;
            if dx.hypot(dy) < 3.0 {
                return;
            }
            let factor = 1.012f32.powf(dy);
            let next = (start_ppb * factor).clamp(10.0, 16_000.0);
            state.pixels_per_bar = next;
            state.scroll_x =
                (start_scroll + (anchor_bar as f32) * (next - start_ppb) - dx).max(0.0);
            state.drag = Some(Drag::Zoom {
                start_ppb,
                start_scroll,
                anchor_bar,
                start_x,
                start_y,
                live: true,
            });
        }
        Some(Drag::Clip { lane, clip, start_x, start_frame }) => {
            let sr = state._stream.as_ref().map(|s| s.sample_rate() as f64).unwrap_or(48_000.0);
            let delta_bars = ((x - start_x) / state.pixels_per_bar.max(1.0)) as f64;
            let next = start_frame + MixTime::frame_from_bar(delta_bars, state.tempo, sr);
            let snapped = if state.grid_enabled {
                MixTime::snap(next, state.grid.raw(), state.tempo, sr, state.arrangement_origin)
            } else {
                next
            };
            if let Some(mut mix) = state.mix.take() {
                state.undo.mutate("Move clip", true, &mut mix, |doc| {
                    if let Some(c) = doc.track_mut(lane).and_then(|t| t.clips.get_mut(clip)) {
                        c.mix_start_frame = snapped.max(0);
                    }
                });
                state.mix = Some(mix);
            }
        }
        Some(Drag::Start { origin, start_x }) => {
            let sr = state._stream.as_ref().map(|s| s.sample_rate() as f64).unwrap_or(48_000.0);
            let delta = MixTime::frame_from_bar(
                ((x - start_x) / state.pixels_per_bar.max(1.0)) as f64,
                state.tempo,
                sr,
            );
            let next = (origin + delta).max(0);
            state.arrangement_origin = if state.grid_enabled {
                MixTime::snap(next, state.grid.raw(), state.tempo, sr, 0).max(0)
            } else {
                next
            };
        }
        Some(Drag::Select { lane, start }) => {
            let sr = state._stream.as_ref().map(|s| s.sample_rate() as f64).unwrap_or(48_000.0);
            let end = hit::frame_at_x(&arr_layout(state), x, state.tempo, sr);
            state.bar_selection = Some((lane, start, end));
        }
        Some(Drag::MixFader { track, rail_top, rail_bot }) => {
            let h = (rail_bot - rail_top).max(1.0);
            set_mix_fader(state, track, ((rail_bot - y) / h).clamp(0.0, 1.0));
        }
        Some(Drag::ControlRoomFader { rail_top, rail_bot }) => {
            let h = (rail_bot - rail_top).max(1.0);
            state.control_room_fader = ((rail_bot - y) / h).clamp(0.0, 1.0);
        }
        Some(Drag::MixPan { track, start_y, start }) => {
            set_mix_pan(
                state,
                track,
                (start - (y - start_y) / Layout::KNOB_DRAG_PX).clamp(0.0, 1.0),
            );
        }
        Some(Drag::MixKnob { track, knob, start_y, start }) => {
            set_mix_knob(
                state,
                track,
                knob,
                (start - (y - start_y) / Layout::KNOB_DRAG_PX).clamp(0.0, 1.0),
            );
        }
        None => {}
    }
    sync_rt(state);
}

fn current_knob(state: &AppState, kind: StripKind, lane: Option<ReturnLane>) -> f32 {
    match (kind, lane) {
        (StripKind::Input(i), Some(lane)) => state.analog.surface.strips[i].aux(lane),
        (StripKind::Input(i), None) => state.analog.surface.strips[i].pan,
        (StripKind::Return(r), None) => state
            .analog
            .surface
            .returns
            .iter()
            .find(|x| x.id == r as i32)
            .map(|x| x.pan)
            .unwrap_or(0.5),
        _ => 0.5,
    }
}

fn current_fader(state: &AppState, kind: StripKind) -> f32 {
    match kind {
        StripKind::Input(i) => state.analog.surface.strips[i].fader,
        StripKind::Return(lane) => state
            .analog
            .surface
            .returns
            .iter()
            .find(|x| x.id == lane as i32)
            .map(|x| x.fader)
            .unwrap_or(0.0),
        StripKind::Main => state.analog.mixer.main_fader,
    }
}

fn set_fader(state: &mut AppState, kind: StripKind, v: f32) {
    // Record analog only. Never writes MixDocument / take_view faders.
    match kind {
        StripKind::Input(i) => state.analog.apply_fader(i, v),
        StripKind::Return(lane) => state.analog.apply_return_fader(lane as i32, v),
        StripKind::Main => {
            state.analog.mixer.main_fader = v;
            state.analog.osc.send_float(osc::output_fader_lin(state.analog.config.main_output), v);
        }
    }
}

fn set_pan(state: &mut AppState, kind: StripKind, v: f32) {
    match kind {
        StripKind::Input(i) => state.analog.apply_pan(i, v),
        StripKind::Return(lane) => state.analog.apply_return_pan(lane as i32, v),
        StripKind::Main => {}
    }
}

fn sidebar_scroll_max(state: &AppState, h: f32) -> f32 {
    let (proj_date, proj_suffix) =
        project_parts(state.analog.config.current_project_relative.as_deref().unwrap_or(""));
    sidebar::max_scroll(
        &sidebar::SidebarView {
            page: state.page,
            engine: &state.analog,
            project_name: "",
            project_date: &proj_date,
            project_suffix: &proj_suffix,
            sample_rate: 0,
            buffer_frames: 0,
            latency_ms: 0.0,
            device_name: "",
            mix: state.mix.as_ref(),
            selected_lane: state.selected_lane,
            scroll: 0.0,
            focus: &state.text_focus,
            caret: false,
        },
        h,
    )
}

fn on_wheel(state: &mut AppState, dx: f32, dy: f32) {
    let (x, y) = state.cursor;
    let (w, h) = state.renderer.logical_size();
    if sidebar_open(state) && x >= w - Layout::SIDEBAR_WIDTH {
        let max = sidebar_scroll_max(state, h);
        state.sidebar_scroll = (state.sidebar_scroll - dy).clamp(0.0, max);
        return;
    }
    if let Some(hit) = hit_body(state, x, y) {
        match hit {
            Hit::Fader { kind, .. } => {
                set_fader(state, kind, (current_fader(state, kind) + dy * 0.002).clamp(0.0, 1.0));
                return;
            }
            Hit::Knob { kind, lane, .. } => {
                let next = (current_knob(state, kind, lane) + dy * 0.002).clamp(0.0, 1.0);
                if let Some(lane) = lane {
                    if let StripKind::Input(i) = kind {
                        state.analog.apply_aux(i, lane, next);
                    }
                } else {
                    set_pan(state, kind, next);
                }
                return;
            }
            Hit::MixFader { track, .. } => {
                let cur = mix_track(state, track).map(|t| t.fader).unwrap_or(0.0);
                set_mix_fader(state, track, (cur + dy * 0.002).clamp(0.0, 1.0));
                return;
            }
            Hit::MixControlRoomFader { .. } => {
                state.control_room_fader = (state.control_room_fader + dy * 0.002).clamp(0.0, 1.0);
                return;
            }
            Hit::MixPan { track } => {
                let cur = mix_track(state, track).map(|t| t.pan).unwrap_or(0.5);
                set_mix_pan(state, track, (cur + dy * 0.002).clamp(0.0, 1.0));
                return;
            }
            Hit::MixKnob { track, knob } => {
                let cur =
                    mix_track(state, track).and_then(|t| t.knobs.get(knob).copied()).unwrap_or(0.0);
                set_mix_knob(state, track, knob, (cur + dy * 0.002).clamp(0.0, 1.0));
                return;
            }
            _ => {}
        }
    }
    if state.page == Page::Record {
        let (bx, by, bw, bh) = body_rect(state);
        let send_count = state.analog.config.effect_return_count as usize;
        let layout = MixerLayout::new(bx, by, bw, bh, send_count);
        let max = layout.max_scroll_x(send_count);
        state.mixer_scroll = (state.mixer_scroll - dx - dy).clamp(0.0, max);
    } else {
        state.scroll_x = (state.scroll_x - dx).max(0.0);
        state.scroll_y = (state.scroll_y - dy).max(0.0);
    }
}

fn mixer_tracks(state: &AppState) -> Vec<MixTrack> {
    let mut tracks = if state.viewing_take.is_some() {
        state.take_view.clone().unwrap_or_default()
    } else {
        state.mix.as_ref().map(MixDocument::channel_tracks).unwrap_or_default()
    };
    let mut main = state
        .mix
        .as_ref()
        .and_then(|m| m.tracks.iter().find(|t| t.lane == MixLane::Main).cloned())
        .unwrap_or_else(|| MixTrack::empty(MixLane::Main, Some("Main".into())));
    main.clips.clear();
    main.name = "Main".into();
    tracks.push(main);
    tracks
}

fn mix_index_for_lane(state: &AppState, lane: MixLane) -> Option<usize> {
    mixer_tracks(state).iter().position(|t| t.lane == lane)
}

fn mixer_track_count(state: &AppState) -> usize {
    mixer_tracks(state).len()
}

fn mixer_lane_at(state: &AppState, track: usize) -> Option<MixLane> {
    mixer_tracks(state).get(track).map(|t| t.lane)
}

fn mix_track(state: &AppState, track: usize) -> Option<MixTrack> {
    mixer_tracks(state).get(track).cloned()
}

fn mix_track_mut(state: &mut AppState, track: usize) -> Option<&mut MixTrack> {
    let lane = mixer_lane_at(state, track)?;
    if state.viewing_take.is_some() && lane != MixLane::Main {
        return state.take_view.as_mut()?.iter_mut().find(|t| t.lane == lane);
    }
    state.mix.as_mut()?.track_mut(lane)
}

fn set_mix_fader(state: &mut AppState, track: usize, v: f32) {
    if let Some(t) = mix_track_mut(state, track) {
        t.fader = v;
    }
}

fn set_mix_pan(state: &mut AppState, track: usize, v: f32) {
    if let Some(t) = mix_track_mut(state, track) {
        t.pan = v;
    }
}

fn set_mix_knob(state: &mut AppState, track: usize, knob: usize, v: f32) {
    if let Some(t) = mix_track_mut(state, track) {
        if let Some(slot) = t.knobs.get_mut(knob) {
            *slot = v;
        }
    }
}

fn locate_to(state: &mut AppState, frame: i64) {
    state.locate_frame = frame.max(0);
    let _ = state
        .engine_handles
        .cmd_tx
        .try_push(UiCommand::TransportSeek { sample: state.locate_frame });
    if let Some(player) = &state.mix_player {
        player.request_seek(state.locate_frame);
    }
}

fn write_automation_if_armed(state: &mut AppState) {
    if !state.playing || !state.automation_armed {
        return;
    }
    let frame = state.engine_handles.sample_position.load(Ordering::Relaxed);
    let lane = state.selected_lane;
    if let Some(mut mix) = state.mix.take() {
        if let Some(track) = lane.and_then(|l| mix.tracks.iter_mut().find(|t| t.lane == l)) {
            track.write_automation(MixAutomationTarget::Volume, frame, track.fader);
            track.write_automation(MixAutomationTarget::Pan, frame, track.pan);
            for (i, v) in track.knobs.clone().into_iter().enumerate() {
                track.write_automation(MixAutomationTarget::Knob(i as i32), frame, v);
            }
        }
        state.mix = Some(mix);
    }
}

fn export_mix(state: &mut AppState) {
    let Some(mix) = state.mix.clone() else { return };
    let Some(folder) = ProjectStore::current_url(&state.analog.config) else { return };
    let sr = state._stream.as_ref().map(|s| s.sample_rate()).unwrap_or(48_000);
    let mut end = 0i64;
    for t in &mix.tracks {
        for c in &t.clips {
            end = end.max(c.mix_end_frame());
        }
    }
    let n = end.max(1) as usize;
    let mut left = vec![0.0f32; n];
    let mut right = vec![0.0f32; n];
    let any_solo = mix.tracks.iter().any(|t| t.solo);
    for track in &mix.tracks {
        if track.lane == MixLane::Main || track.mute || (any_solo && !track.solo) {
            continue;
        }
        let amp = osc::fader_lin_to_amp(track.fader);
        let (gl, gr) = osc::stereo_pan_amps(amp, track.pan);
        for clip in &track.clips {
            let path = folder.join(&clip.source_file);
            let (cl, cr) =
                read_clip_stereo(&path, clip.source_start_frame, clip.source_frame_count);
            let dest = clip.mix_start_frame.max(0) as usize;
            for i in 0..cl.len() {
                let d = dest + i;
                if d >= n {
                    break;
                }
                left[d] += cl[i] * gl;
                right[d] += cr.get(i).copied().unwrap_or(cl[i]) * gr;
            }
        }
    }
    let main_amp = mix
        .tracks
        .iter()
        .find(|t| t.lane == MixLane::Main)
        .map(|t| if t.mute { 0.0 } else { osc::fader_lin_to_amp(t.fader) })
        .unwrap_or(1.0);
    if main_amp != 1.0 {
        for i in 0..n {
            left[i] *= main_amp;
            right[i] *= main_amp;
        }
    }
    let path = folder.join(format!("{}.wav", mix.name));
    match asset::write_bounce(&path, sr, &left, &right) {
        Ok(()) => log::info!("exported {}", path.display()),
        Err(e) => log::error!("export: {e}"),
    }
}

fn read_clip_stereo(path: &std::path::Path, start: i64, count: i64) -> (Vec<f32>, Vec<f32>) {
    let Ok(mut reader) = hound::WavReader::open(path) else {
        return (Vec::new(), Vec::new());
    };
    let spec = reader.spec();
    if spec.sample_format != hound::SampleFormat::Float {
        return (Vec::new(), Vec::new());
    }
    let ch = spec.channels.max(1) as usize;
    let start = start.max(0) as usize;
    let count = count.max(0) as usize;
    let mut samples = reader.samples::<f32>();
    for _ in 0..start.saturating_mul(ch) {
        let _ = samples.next();
    }
    let mut l = Vec::with_capacity(count);
    let mut r = Vec::with_capacity(count);
    for _ in 0..count {
        let sl = samples.next().and_then(|s| s.ok()).unwrap_or(0.0);
        let sr = if ch > 1 { samples.next().and_then(|s| s.ok()).unwrap_or(0.0) } else { sl };
        for _ in 2..ch {
            let _ = samples.next();
        }
        l.push(sl);
        r.push(sr);
    }
    (l, r)
}

fn paint(state: &mut AppState) {
    let (w, h) = state.renderer.logical_size();
    let sr = state._stream.as_ref().map(|s| s.sample_rate() as f64).unwrap_or(48_000.0);
    let playhead = if state.playing {
        state.engine_handles.sample_position.load(Ordering::Relaxed)
    } else {
        state.locate_frame
    };
    let chrome = ChromeState {
        page: state.page,
        tempo: state.tempo,
        playing: state.playing,
        recording: state.recording,
        position: MixTime::format_position(playhead - state.arrangement_origin, state.tempo, sr),
        grid_on: state.grid_enabled,
        grid_title: state.grid.title().into(),
        auto_on: state.automation_armed,
        knobs_on: state.show_knobs,
        inserts_on: state.show_inserts,
        osc_connected: state.analog.osc.is_connected(),
        osc_status: if state.analog.osc.is_connected() {
            format!("OSC {}:{}", state.analog.config.osc_host, state.analog.config.osc_send_port)
        } else {
            "Waiting for TotalMix".into()
        },
        midi_status: state.midi.status(),
        last_midi: state.last_midi.clone(),
        mute_mode: matches!(
            state.analog.surface.track_control_mode,
            analog::TrackControlMode::Mute
        ),
        solo_mode: matches!(
            state.analog.surface.track_control_mode,
            analog::TrackControlMode::Solo
        ),
        project_name: state.analog.config.current_project_relative.clone().unwrap_or_default(),
        focus: &state.text_focus,
        caret: state.caret_on,
    };
    let mut scene = chrome::paint(&chrome, w, h);
    let peaks: [f32; TAP_COUNT] = std::array::from_fn(|i| {
        engine::display_level(state.engine_handles.peaks[i].load(Ordering::Relaxed))
    });
    let (bx, by, bw, bh) = body_rect(state);
    let (proj_date, proj_suffix) =
        project_parts(state.analog.config.current_project_relative.as_deref().unwrap_or(""));
    let device = state
        ._stream
        .as_ref()
        .map(|_| state.analog.config.audio_device_contains.clone())
        .unwrap_or_else(|| state.analog.config.audio_device_contains.clone());
    let sample_rate = state._stream.as_ref().map(|s| s.sample_rate()).unwrap_or(48_000);
    let buffer_frames = state._stream.as_ref().map(|s| s.buffer_frames()).unwrap_or(128);
    let latency_ms = buffer_frames as f32 / sample_rate as f32 * 1000.0;
    match state.page {
        Page::Record => {
            let send_count = state.analog.config.effect_return_count as usize;
            let mut layout = MixerLayout::new(bx, by, bw, bh, send_count);
            let mixer_max = layout.max_scroll_x(send_count);
            state.mixer_scroll = state.mixer_scroll.clamp(0.0, mixer_max);
            layout.scroll_x = state.mixer_scroll;
            let (cmds, extras) = ui_mixlink::mixer::paint(&ui_mixlink::mixer::MixerView {
                engine: &state.analog,
                peaks: &peaks,
                layout,
            });
            scene.extend(cmds);
            state.mixer_extras = extras;
        }
        Page::Mix => {
            ensure_waveforms(state);
            scene.push(render::DrawCmd::Layer);
            scene.push(render::DrawCmd::Clip { rect: Rect { x: bx, y: by, w: bw, h: bh } });
            let mix_h = ui_mixlink::mix_mixer::height(state.show_knobs, state.show_mixer);
            let mix_tracks = mixer_tracks(state);
            let lane_peaks: Vec<f32> = (0..mix_tracks.len())
                .map(|i| {
                    state
                        .engine_handles
                        .lane_peaks
                        .get(i)
                        .map(|p| engine::display_level(p.load(Ordering::Relaxed)))
                        .unwrap_or(0.0)
                })
                .collect();
            let tracks = arrangement_tracks(state);
            let arr = arr_layout(state);
            scene.extend(ui_mixlink::arrangement::paint(
                &ui_mixlink::arrangement::ArrangementView {
                    layout: arr,
                    mix: state.mix.as_ref(),
                    tracks: &tracks,
                    selected_lane: state.selected_lane,
                    selected_clips: &state.selected_clips,
                    playhead,
                    origin: state.arrangement_origin,
                    tempo: state.tempo,
                    sample_rate: sr,
                    grid: state.grid,
                    grid_enabled: state.grid_enabled,
                    viewing_take: state.viewing_take.is_some(),
                    bar_selection: state.bar_selection,
                    waveforms: Some(&state.waveforms),
                },
            ));
            scene.extend(ui_mixlink::mix_mixer::paint(&ui_mixlink::mix_mixer::MixMixerView {
                x: bx + ui_mixlink::mix_browser::WIDTH,
                y: by + bh - mix_h,
                w: bw - ui_mixlink::mix_browser::WIDTH,
                h: mix_h,
                tracks: &mix_tracks,
                selected_lane: state.selected_lane,
                show_knobs: state.show_knobs,
                visible: state.show_mixer,
                peaks: &lane_peaks,
                control_room_fader: state.control_room_fader,
                control_room_peak: engine::display_level(
                    state.engine_handles.listen_peak.load(Ordering::Relaxed),
                ),
                engine: &state.analog,
            }));
            scene.extend(ui_mixlink::mix_browser::paint(
                &ui_mixlink::mix_browser::MixBrowserView {
                    x: bx,
                    y: by,
                    h: bh,
                    mixes: &state.mixes,
                    selected_mix: state.mix.as_ref().map(|m| m.id),
                    takes: &state.takes,
                    selected_take: state.viewing_take,
                    mixer_collapsed: !state.show_mixer,
                },
            ));
        }
    }

    if sidebar_open(state) {
        let sidebar_max = sidebar_scroll_max(state, h);
        state.sidebar_scroll = state.sidebar_scroll.clamp(0.0, sidebar_max);
        let (side_cmds, side_hits) = sidebar::paint(
            &sidebar::SidebarView {
                page: state.page,
                engine: &state.analog,
                project_name: state
                    .analog
                    .config
                    .current_project_relative
                    .as_deref()
                    .unwrap_or("Projects folder"),
                project_date: &proj_date,
                project_suffix: &proj_suffix,
                sample_rate,
                buffer_frames,
                latency_ms,
                device_name: &device,
                mix: state.mix.as_ref(),
                selected_lane: state.selected_lane,
                scroll: state.sidebar_scroll,
                focus: &state.text_focus,
                caret: state.caret_on,
            },
            w,
            h,
        );
        scene.extend(side_cmds);
        state.sidebar_hits = side_hits;
    } else {
        state.sidebar_hits.clear();
    }

    if let Some(overlay) = &state.overlay {
        scene.push(render::DrawCmd::Layer);
        match overlay {
            Overlay::Menu { .. } => {
                let hover = match overlay {
                    Overlay::Menu { rect, items, .. } => {
                        overlay::menu_at(*rect, items, state.cursor.0, state.cursor.1)
                    }
                    _ => None,
                };
                scene.extend(overlay::paint_menu(overlay, hover));
            }
            Overlay::Settings => {
                let (cmds, _) =
                    overlay::paint_settings(&state.analog, w, h, &state.text_focus, state.caret_on);
                scene.extend(cmds);
            }
        }
    }
    let _ = state.renderer.render_scene(&scene);
}

fn handle_overlay_press(
    state: &mut AppState,
    overlay: &Overlay,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) -> bool {
    match overlay {
        Overlay::Menu { rect, items, action } => {
            if overlay::contains(*rect, x, y) {
                if let Some(i) = overlay::menu_at(*rect, items, x, y) {
                    if let Some(item) = items.get(i).cloned() {
                        apply_menu(state, action.clone(), &item);
                    }
                }
                state.overlay = None;
                return true;
            }
            state.overlay = None;
            state.text_focus = TextFocus::None;
            true
        }
        Overlay::Settings => {
            let hits = overlay::settings_hits(w, h);
            if !overlay::contains(hits.panel, x, y) {
                state.overlay = None;
                state.text_focus = TextFocus::None;
                return true;
            }
            if overlay::contains(hits.post_fader, x, y) {
                let on = !state.analog.config.sends_post_fader;
                state.analog.set_sends_post_fader(on);
            } else if overlay::contains(hits.hardware_strips, x, y) {
                let on = !state.analog.config.hardware_strips;
                state.analog.set_hardware_strips(on);
            } else if overlay::contains(hits.osc_host, x, y) {
                state.text_focus = TextFocus::OscHost;
            } else if overlay::contains(hits.osc_send, x, y) {
                state.text_focus = TextFocus::OscSend;
            } else if overlay::contains(hits.osc_listen, x, y) {
                state.text_focus = TextFocus::OscListen;
            } else if overlay::contains(hits.midi, x, y) {
                state.text_focus = TextFocus::MidiNeedle;
            } else if overlay::contains(hits.apply, x, y) {
                apply_settings(state);
            }
            true
        }
    }
}

fn open_or_focus_channels(state: &mut AppState, event_loop: &ActiveEventLoop) {
    if let Some(ch) = &state.channels {
        ch.window.set_minimized(false);
        ch.window.focus_window();
        ch.window.request_redraw();
        return;
    }
    let window = match event_loop.create_window(
        Window::default_attributes()
            .with_title("Channels")
            .with_inner_size(LogicalSize::new(
                overlay::CHANNELS_WINDOW_W,
                overlay::CHANNELS_WINDOW_H,
            ))
            .with_min_inner_size(LogicalSize::new(
                overlay::CHANNELS_WINDOW_W,
                overlay::CHANNELS_WINDOW_H,
            )),
    ) {
        Ok(w) => Arc::new(w),
        Err(e) => {
            log::error!("channels window: {e}");
            return;
        }
    };
    let zoom = state.renderer.ui_zoom();
    let mut renderer = pollster::block_on(render::Renderer::new(window.clone()));
    renderer.set_ui_zoom(zoom);
    window.focus_window();
    window.request_redraw();
    state.channels = Some(ChannelsWindow { renderer, window, cursor: (0.0, 0.0), scroll: 0.0 });
    state.text_focus = TextFocus::None;
}

fn close_channels(state: &mut AppState) {
    if matches!(state.text_focus, TextFocus::GearAlias(_)) {
        state.text_focus = TextFocus::None;
    }
    state.channels = None;
}

fn paint_channels_window(state: &mut AppState) {
    let (cmds, scroll) = {
        let Some(ch) = state.channels.as_ref() else { return };
        let (w, h) = ch.renderer.logical_size();
        let max = overlay::channels_max_scroll(&state.analog, h);
        let scroll = ch.scroll.min(max);
        let (cmds, _) =
            overlay::paint_channels(&state.analog, w, h, &state.text_focus, state.caret_on, scroll);
        (cmds, scroll)
    };
    let Some(ch) = state.channels.as_mut() else { return };
    ch.scroll = scroll;
    if let Err(e) = ch.renderer.render_scene(&cmds) {
        log::error!("channels render: {e}");
    }
}

fn on_channels_press(state: &mut AppState) {
    let (x, y, w, h, scroll) = {
        let Some(ch) = state.channels.as_ref() else { return };
        let (w, h) = ch.renderer.logical_size();
        (ch.cursor.0, ch.cursor.1, w, h, ch.scroll)
    };
    let (_, fields) =
        overlay::paint_channels(&state.analog, w, h, &state.text_focus, false, scroll);
    if let Some((_, id)) = fields.into_iter().find(|(r, _)| overlay::contains(*r, x, y)) {
        state.text_focus = TextFocus::GearAlias(id);
    } else if matches!(state.text_focus, TextFocus::GearAlias(_)) {
        state.text_focus = TextFocus::None;
    }
}

fn on_channels_wheel(state: &mut AppState, dy: f32) {
    let h = match state.channels.as_ref() {
        Some(ch) => ch.renderer.logical_size().1,
        None => return,
    };
    let max = overlay::channels_max_scroll(&state.analog, h);
    if let Some(ch) = state.channels.as_mut() {
        ch.scroll = (ch.scroll - dy).clamp(0.0, max);
    }
}

fn on_channels_key(state: &mut AppState, key: &Key) {
    if matches!(key, Key::Named(NamedKey::Escape)) {
        if matches!(state.text_focus, TextFocus::GearAlias(_)) {
            state.text_focus = TextFocus::None;
        }
        return;
    }
    if !matches!(state.text_focus, TextFocus::GearAlias(_)) {
        return;
    }
    match key {
        Key::Named(NamedKey::Enter) => commit_focus(state),
        Key::Named(NamedKey::Backspace) => edit_focus(state, |s| {
            s.pop();
        }),
        Key::Character(c) if c.chars().all(|ch| !ch.is_control()) => {
            let add = c.to_string();
            edit_focus(state, |s| s.push_str(&add));
        }
        _ => {}
    }
}

fn handle_sidebar(
    state: &mut AppState,
    hit: SidebarHit,
    anchor: Rect,
    event_loop: &ActiveEventLoop,
) {
    match hit {
        SidebarHit::AddHardware => state.analog.add_hardware_effect(),
        SidebarHit::RemoveHardware(id) => state.analog.remove_hardware_effect(id),
        SidebarHit::EditHardwareName(id) => state.text_focus = TextFocus::HardwareName(id),
        SidebarHit::HardwareOutput(id) => {
            open_output_menu(state, MenuAction::HardwareOutput { id }, anchor)
        }
        SidebarHit::HardwareInput(id) => {
            open_input_menu(state, MenuAction::HardwareInput { id }, anchor)
        }
        SidebarHit::AddPlugin => state.analog.add_plugin(),
        SidebarHit::RemovePlugin(id) => {
            vst3_host::exchange_and_retire(id as u32, std::ptr::null_mut());
            state.plugin_refs.remove(&id);
            state.analog.remove_plugin(id);
        }
        SidebarHit::EditPluginName(id) => state.text_focus = TextFocus::PluginName(id),
        SidebarHit::PluginBundle(id) => {
            open_plugin_menu(state, MenuAction::PluginBundle { id }, anchor)
        }
        SidebarHit::PluginEdit(id) => {
            if let Some(&inst) = state.plugin_refs.get(&id) {
                let title = state
                    .analog
                    .config
                    .plugin(id)
                    .map(|p| p.title())
                    .unwrap_or_else(|| "Plugin".into());
                vst3_host::show_editor(inst, &title);
            } else {
                load_plugin_slot(state, id);
                if let Some(&inst) = state.plugin_refs.get(&id) {
                    let title = state
                        .analog
                        .config
                        .plugin(id)
                        .map(|p| p.title())
                        .unwrap_or_else(|| "Plugin".into());
                    vst3_host::show_editor(inst, &title);
                }
            }
        }
        SidebarHit::PluginBypass(id) => {
            let on = state.analog.config.plugin(id).map(|p| !p.bypassed).unwrap_or(true);
            state.analog.set_plugin_bypass(id, on);
            vst3_host::slot_set_bypass(id as u32, on);
        }
        SidebarHit::PluginPlayback(id) => open_playback_menu(state, id, anchor),
        SidebarHit::ProjectsFolder => {
            if let Some(path) = native::pick_projects_folder() {
                if let Some(data) = ProjectStore::bookmark_for(&path) {
                    state.analog.config.projects_root_bookmark = Some(data);
                    state.analog.persist();
                    reload_mix(state);
                }
            }
        }
        SidebarHit::ProjectName => state.text_focus = TextFocus::ProjectName,
        SidebarHit::NewProject => {
            if let Err(e) = state.project.create_project(&mut state.analog.config) {
                log::warn!("new project: {e}");
            } else {
                state.analog.persist();
                reload_mix(state);
            }
        }
        SidebarHit::MixOut => open_output_menu(state, MenuAction::MixOut, anchor),
        SidebarHit::AudioDevice => open_device_menu(state, anchor),
        SidebarHit::AudioBuffer => open_buffer_menu(state, anchor),
        SidebarHit::Channels => open_or_focus_channels(state, event_loop),
        SidebarHit::Settings => {
            state.overlay = Some(Overlay::Settings);
            state.text_focus = TextFocus::None;
        }
        SidebarHit::AddInsert => add_insert(state),
        SidebarHit::RemoveInsert(id) => {
            if let Some(inst) = state.insert_refs.remove(&id) {
                vst3_host::retire_instance(inst);
            }
            if let Some(mut mix) = state.mix.take() {
                for t in &mut mix.tracks {
                    t.inserts.retain(|i| i.id != id);
                }
                state.mix = Some(mix);
                persist_mix(state);
            }
        }
        SidebarHit::InsertBundle(id) => {
            open_plugin_menu(state, MenuAction::InsertBundle { insert: id }, anchor)
        }
        SidebarHit::InsertEdit(id) => {
            if let Some(&inst) = state.insert_refs.get(&id) {
                vst3_host::show_editor(inst, "Insert");
            } else {
                load_insert(state, id);
                if let Some(&inst) = state.insert_refs.get(&id) {
                    vst3_host::show_editor(inst, "Insert");
                }
            }
        }
        SidebarHit::InsertBypass(id) => {
            if let Some(mut mix) = state.mix.take() {
                for t in &mut mix.tracks {
                    if let Some(ins) = t.inserts.iter_mut().find(|i| i.id == id) {
                        ins.bypassed = !ins.bypassed;
                    }
                }
                state.mix = Some(mix);
                persist_mix(state);
            }
        }
    }
}

fn place_menu(state: &mut AppState, anchor: Rect, items: Vec<MenuItem>, action: MenuAction) {
    let (w, h) = state.renderer.logical_size();
    let rect = overlay::layout_popup(anchor, &items, w, h);
    state.overlay = Some(Overlay::Menu { rect, items, action });
}

fn name_row_anchor(state: &AppState, kind: StripKind) -> Rect {
    let (bx, by, bw, bh) = body_rect(state);
    let send_count = state.analog.config.effect_return_count as usize;
    let layout = MixerLayout::new(bx, by, bw, bh, send_count);
    let (sx, sw) = mixer::strip_frame(&layout, send_count, kind);
    let (bay_y, bay_h) = mixer::fader_bay_frame(&layout, send_count);
    Rect { x: sx, y: bay_y + bay_h, w: sw, h: Layout::NAME_ROW }
}

fn open_name_menu(state: &mut AppState, kind: StripKind, _x: f32, _y: f32) {
    let anchor = name_row_anchor(state, kind);
    match kind {
        StripKind::Input(i) => {
            let current = state.analog.config.strips.get(i).map(|s| s.channel_id().index);
            let items: Vec<MenuItem> = state
                .analog
                .mixer
                .strips(MixerBus::Input)
                .into_iter()
                .map(|ch| MenuItem {
                    id: ch.id.index.to_string(),
                    label: state.analog.display_name(ch.id),
                    checked: current == Some(ch.id.index),
                    section: None,
                })
                .collect();
            place_menu(state, anchor, items, MenuAction::StripSource { strip: i });
        }
        // MixLink `ReturnEffectMenu` applies to effect AND bus returns
        // (`case .effectReturn, .busReturn`). Main is plain Text — no menu.
        StripKind::Return(lane) => {
            let current = state.analog.config.effect_ref(lane);
            let mut items = vec![MenuItem {
                id: "none".into(),
                label: "No effect".into(),
                checked: current.is_none(),
                section: None,
            }];
            for hw in &state.analog.config.hardware_effects {
                items.push(MenuItem {
                    id: format!("hw:{}", hw.id),
                    label: hw.title(),
                    checked: current == Some(EffectRef::Hardware(hw.id)),
                    section: Some("Hardware".into()),
                });
            }
            for p in &state.analog.config.plugins {
                items.push(MenuItem {
                    id: format!("pl:{}", p.id),
                    label: p.title(),
                    checked: current == Some(EffectRef::Plugin(p.id)),
                    section: Some("Plugins".into()),
                });
            }
            place_menu(state, anchor, items, MenuAction::ReturnEffect { lane });
        }
        StripKind::Main => {}
    }
}

fn open_output_menu(state: &mut AppState, action: MenuAction, anchor: Rect) {
    let current = match action {
        MenuAction::MixOut => Some(state.analog.config.main_output),
        MenuAction::HardwareOutput { id } => {
            state.analog.config.hardware_effects.iter().find(|e| e.id == id).map(|e| e.output)
        }
        _ => None,
    };
    let items: Vec<MenuItem> = state
        .analog
        .mixer
        .strips(MixerBus::Output)
        .into_iter()
        .map(|ch| MenuItem {
            id: ch.id.index.to_string(),
            label: state.analog.output_name(ch.id.index),
            checked: current == Some(ch.id.index),
            section: None,
        })
        .collect();
    place_menu(state, anchor, items, action);
}

fn open_input_menu(state: &mut AppState, action: MenuAction, anchor: Rect) {
    let current = match action {
        MenuAction::HardwareInput { id } => {
            state.analog.config.hardware_effects.iter().find(|e| e.id == id).map(|e| e.input)
        }
        _ => None,
    };
    let items: Vec<MenuItem> = state
        .analog
        .mixer
        .strips(MixerBus::Input)
        .into_iter()
        .map(|ch| MenuItem {
            id: ch.id.index.to_string(),
            label: state.analog.display_name(ch.id),
            checked: current == Some(ch.id.index),
            section: None,
        })
        .collect();
    place_menu(state, anchor, items, action);
}

fn open_plugin_menu(state: &mut AppState, action: MenuAction, anchor: Rect) {
    let mut items =
        vec![MenuItem { id: String::new(), label: "None".into(), checked: false, section: None }];
    for p in vst3_host::scan_plugins() {
        items.push(MenuItem {
            id: p.bundle_path,
            label: p.name,
            checked: false,
            section: Some("VST3".into()),
        });
    }
    place_menu(state, anchor, items, action);
}

fn open_playback_menu(state: &mut AppState, id: i32, anchor: Rect) {
    let current = state.analog.config.plugin(id).map(|p| p.return_channel);
    let items: Vec<MenuItem> = state
        .analog
        .mixer
        .strips(MixerBus::Playback)
        .into_iter()
        .map(|ch| MenuItem {
            id: ch.id.index.to_string(),
            label: format!("{}/{}", ch.id.index + 1, ch.id.index + 2),
            checked: current == Some(ch.id.index),
            section: None,
        })
        .collect();
    place_menu(state, anchor, items, MenuAction::PluginPlayback { id });
}

fn open_device_menu(state: &mut AppState, anchor: Rect) {
    let current = state.analog.config.audio_device_contains.clone();
    let items: Vec<MenuItem> = audio_io::enumerate_devices()
        .into_iter()
        .filter(|d| d.usable())
        .map(|d| MenuItem {
            id: d.name.clone(),
            label: format!("{}  {}/{}", d.name, d.inputs, d.outputs),
            checked: !current.is_empty() && d.name.contains(&current),
            section: None,
        })
        .collect();
    place_menu(state, anchor, items, MenuAction::AudioDevice);
}

fn open_buffer_menu(state: &mut AppState, anchor: Rect) {
    let current = state.analog.config.audio_buffer_frames.unwrap_or(64);
    let items: Vec<MenuItem> = [32, 64, 128, 256, 512]
        .into_iter()
        .map(|n| MenuItem {
            id: n.to_string(),
            label: format!("{n} frames"),
            checked: current == n,
            section: None,
        })
        .collect();
    place_menu(state, anchor, items, MenuAction::AudioBuffer);
}

fn apply_menu(state: &mut AppState, action: MenuAction, item: &MenuItem) {
    match action {
        MenuAction::StripSource { strip } => {
            if let Ok(idx) = item.id.parse::<i32>() {
                state.analog.set_strip_source(strip, ChannelID::new(MixerBus::Input, idx));
            }
        }
        MenuAction::ReturnEffect { lane } => {
            let ref_ = if item.id == "none" || item.id.is_empty() {
                None
            } else if let Some(id) = item.id.strip_prefix("hw:").and_then(|s| s.parse().ok()) {
                Some(EffectRef::Hardware(id))
            } else if let Some(id) = item.id.strip_prefix("pl:").and_then(|s| s.parse().ok()) {
                Some(EffectRef::Plugin(id))
            } else {
                None
            };
            state.analog.set_return_effect(lane, ref_);
        }
        MenuAction::HardwareOutput { id } => {
            if let Ok(idx) = item.id.parse::<i32>() {
                state.analog.set_hardware_effect_io(id, Some(idx), None);
            }
        }
        MenuAction::HardwareInput { id } => {
            if let Ok(idx) = item.id.parse::<i32>() {
                state.analog.set_hardware_effect_io(id, None, Some(idx));
            }
        }
        MenuAction::MixOut => {
            if let Ok(idx) = item.id.parse::<i32>() {
                state.analog.set_main_output(idx);
            }
        }
        MenuAction::AudioDevice => {
            state.analog.set_audio_device(&item.id);
            restart_audio(state);
        }
        MenuAction::AudioBuffer => {
            if let Ok(n) = item.id.parse::<i32>() {
                state.analog.set_audio_buffer_frames(n);
                restart_audio(state);
            }
        }
        MenuAction::PluginBundle { id } => {
            let path = if item.id.is_empty() { None } else { Some(item.id.clone()) };
            let name = if item.label == "None" { None } else { Some(item.label.clone()) };
            state.analog.set_plugin_bundle(id, path, name);
            load_plugin_slot(state, id);
        }
        MenuAction::PluginPlayback { id } => {
            if let Ok(pair) = item.id.parse::<i32>() {
                state.analog.set_plugin_playback(id, pair);
            }
        }
        MenuAction::InsertBundle { insert } => {
            set_insert_bundle(state, insert, &item.id, &item.label);
        }
        MenuAction::MixContext { id } => {
            if item.id == "delete" {
                delete_mix_id(state, id);
            } else if let Some(n) = item.id.strip_prefix("take:").and_then(|s| s.parse().ok()) {
                select_mix(state, id);
                start_from_take(state, n);
            }
        }
        MenuAction::TakeContext { number } => match item.id.as_str() {
            "start" => start_from_take(state, number),
            "copy" => copy_take_to_clipboard(state, number),
            _ => {}
        },
    }
}

fn on_key(state: &mut AppState, key: &Key) -> bool {
    if matches!(key, Key::Named(NamedKey::Escape)) {
        if state.overlay.is_some() || state.text_focus != TextFocus::None {
            state.overlay = None;
            state.text_focus = TextFocus::None;
            return true;
        }
        return false;
    }
    if state.text_focus == TextFocus::None {
        return false;
    }
    match key {
        Key::Named(NamedKey::Enter) => {
            commit_focus(state);
            true
        }
        Key::Named(NamedKey::Backspace) => {
            edit_focus(state, |s| {
                s.pop();
            });
            true
        }
        Key::Character(c) => {
            if c.chars().all(|ch| !ch.is_control()) {
                let add = c.to_string();
                edit_focus(state, |s| s.push_str(&add));
            }
            true
        }
        _ => true,
    }
}

fn edit_focus(state: &mut AppState, f: impl FnOnce(&mut String)) {
    match state.text_focus {
        TextFocus::Tempo => {
            let mut s = format!("{:.1}", state.tempo);
            f(&mut s);
            if let Ok(v) = s.parse::<f64>() {
                state.tempo = v.clamp(20.0, 400.0);
            }
        }
        TextFocus::ProjectName => {
            let (_, mut suffix) = project_parts(
                state.analog.config.current_project_relative.as_deref().unwrap_or(""),
            );
            f(&mut suffix);
            if let Err(e) = state.project.rename_current(&suffix, &mut state.analog.config) {
                log::warn!("rename: {e}");
            } else {
                state.analog.persist();
            }
        }
        TextFocus::HardwareName(id) => {
            let mut name = state
                .analog
                .config
                .hardware_effects
                .iter()
                .find(|h| h.id == id)
                .map(|h| h.name.clone())
                .unwrap_or_default();
            f(&mut name);
            state.analog.set_hardware_effect_name(id, &name);
        }
        TextFocus::PluginName(id) => {
            let mut name =
                state.analog.config.plugin(id).map(|p| p.name.clone()).unwrap_or_default();
            f(&mut name);
            state.analog.set_plugin_name(id, &name);
        }
        TextFocus::GearAlias(id) => {
            let mut name = state.analog.config.gear_name(id);
            f(&mut name);
            state.analog.set_gear_name(id, &name);
        }
        TextFocus::OscHost => {
            f(&mut state.analog.config.osc_host);
            state.analog.persist();
        }
        TextFocus::OscSend => {
            let mut s = state.analog.config.osc_send_port.to_string();
            f(&mut s);
            if let Ok(v) = s.parse::<u16>() {
                state.analog.config.osc_send_port = v;
                state.analog.persist();
            }
        }
        TextFocus::OscListen => {
            let mut s = state.analog.config.osc_listen_port.to_string();
            f(&mut s);
            if let Ok(v) = s.parse::<u16>() {
                state.analog.config.osc_listen_port = v;
                state.analog.persist();
            }
        }
        TextFocus::MidiNeedle => {
            f(&mut state.analog.config.midi_device_contains);
            state.analog.persist();
        }
        TextFocus::None => {}
    }
}

fn commit_focus(state: &mut AppState) {
    if state.text_focus == TextFocus::ProjectName {
        reload_mix(state);
    }
    state.text_focus = TextFocus::None;
}

fn apply_settings(state: &mut AppState) {
    let host = state.analog.config.osc_host.clone();
    let send = state.analog.config.osc_send_port;
    let listen = state.analog.config.osc_listen_port;
    if let Err(e) = state.analog.osc.start(&host, send, listen) {
        log::warn!("OSC reconnect: {e}");
    }
    state.analog.osc.send_dump_requests();
    state.analog.persist();
    restart_audio(state);
    #[cfg(target_os = "macos")]
    {
        match MidiSession::connect(&state.analog.config.midi_device_contains) {
            Ok(s) => {
                state.midi = MidiIo::Hw(s);
                state.xl.clear_on_connect();
                state.last_led = None;
            }
            Err(e) => log::warn!("MIDI: {e}"),
        }
    }
}

fn restart_audio(state: &mut AppState) {
    state._stream = None;
    let engine_addr = state.engine as usize;
    let needle = state.analog.config.audio_device_contains.clone();
    let frames = state.analog.config.audio_buffer_frames.map(|n| n as u32);
    match audio_io::DeviceStream::open_named(
        &needle,
        frames,
        Box::new(move |input, output, timing| unsafe {
            let engine = engine_addr as *mut Engine;
            (*engine).process(input, output, timing.frames as usize, timing.host_time);
        }),
    ) {
        Ok(s) => {
            log::info!("audio {} Hz / {} frames", s.sample_rate(), s.buffer_frames());
            state._stream = Some(s);
        }
        Err(e) => log::warn!("audio: {e}"),
    }
}

fn load_plugin_slot(state: &mut AppState, id: i32) {
    let Some(plugin) = state.analog.config.plugin(id).cloned() else { return };
    let sr = state._stream.as_ref().map(|s| s.sample_rate() as f64).unwrap_or(48_000.0);
    let block = state._stream.as_ref().map(|s| s.buffer_frames()).unwrap_or(128);
    let inst = match plugin.bundle_path.as_deref() {
        Some(path) if !path.is_empty() => {
            vst3_host::load(path, plugin.class_uid.as_deref(), sr, block)
        }
        _ => std::ptr::null_mut(),
    };
    vst3_host::exchange_and_retire(id as u32, inst);
    vst3_host::slot_set_bypass(id as u32, plugin.bypassed);
    if inst.is_null() {
        state.plugin_refs.remove(&id);
    } else {
        state.plugin_refs.insert(id, inst);
    }
}

fn load_configured_plugins(state: &mut AppState) {
    let ids: Vec<i32> =
        state.analog.config.plugins.iter().filter(|p| p.is_loaded()).map(|p| p.id).collect();
    for id in ids {
        load_plugin_slot(state, id);
    }
}

fn load_insert(state: &mut AppState, id: uuid::Uuid) {
    let Some(mix) = &state.mix else { return };
    let Some(insert) = mix.tracks.iter().flat_map(|t| t.inserts.iter()).find(|i| i.id == id) else {
        return;
    };
    let Some(path) = insert.bundle_path.clone() else { return };
    let sr = state._stream.as_ref().map(|s| s.sample_rate() as f64).unwrap_or(48_000.0);
    let block = state._stream.as_ref().map(|s| s.buffer_frames()).unwrap_or(128);
    if let Some(prev) = state.insert_refs.remove(&id) {
        vst3_host::retire_instance(prev);
    }
    let inst = vst3_host::load(&path, insert.class_uid.as_deref(), sr, block);
    if !inst.is_null() {
        state.insert_refs.insert(id, inst);
    }
}

fn set_insert_bundle(state: &mut AppState, id: uuid::Uuid, path: &str, name: &str) {
    if let Some(mut mix) = state.mix.take() {
        for t in &mut mix.tracks {
            if let Some(ins) = t.inserts.iter_mut().find(|i| i.id == id) {
                if path.is_empty() {
                    ins.bundle_path = None;
                    ins.name.clear();
                } else {
                    ins.bundle_path = Some(path.to_string());
                    if name != "None" {
                        ins.name = name.to_string();
                    }
                }
            }
        }
        state.mix = Some(mix);
        persist_mix(state);
    }
    if path.is_empty() {
        if let Some(prev) = state.insert_refs.remove(&id) {
            vst3_host::retire_instance(prev);
        }
    } else {
        load_insert(state, id);
    }
}

fn add_insert(state: &mut AppState) {
    let lane = state.selected_lane.unwrap_or(MixLane::Main);
    if let Some(mut mix) = state.mix.take() {
        if let Some(track) = mix.tracks.iter_mut().find(|t| t.lane == lane) {
            track.inserts.push(MixInsert::new("Plugin"));
        }
        state.mix = Some(mix);
        persist_mix(state);
    }
}

fn new_mix(state: &mut AppState) {
    let n = state.mixes.len() + 1;
    let mut mix = MixDocument::empty(format!("Mix {n}"), state.analog.config.effect_return_count);
    if let Some(take) = state.viewing_take {
        mix.start_frame = take_start_from_store(state, take);
    }
    if let Some(folder) = ProjectStore::current_url(&state.analog.config) {
        if let Err(e) = state.project.save_mix(&mix, &folder) {
            log::warn!("save mix: {e}");
        }
    }
    let id = mix.id;
    state.mixes.push(mix);
    select_mix(state, id);
}

fn mix_browser_view(state: &AppState) -> ui_mixlink::mix_browser::MixBrowserView<'_> {
    let (_, by, _, bh) = body_rect(state);
    ui_mixlink::mix_browser::MixBrowserView {
        x: 0.0,
        y: by,
        h: bh,
        mixes: &state.mixes,
        selected_mix: state.mix.as_ref().map(|m| m.id),
        takes: &state.takes,
        selected_take: state.viewing_take,
        mixer_collapsed: !state.show_mixer,
    }
}

fn on_context_press(state: &mut AppState) {
    let (x, y) = state.cursor;
    let (w, h) = state.renderer.logical_size();
    if let Some(overlay) = state.overlay.clone() {
        if handle_overlay_press(state, &overlay, x, y, w, h) {
            return;
        }
    }
    if state.page != Page::Mix {
        return;
    }
    if let Some(hit) = ui_mixlink::mix_browser::hit(&mix_browser_view(state), x, y) {
        on_browser_context(state, hit);
    }
}

fn on_browser_context(state: &mut AppState, hit: ui_mixlink::mix_browser::BrowserHit) {
    let Some(anchor) = ui_mixlink::mix_browser::row_rect(&mix_browser_view(state), hit) else {
        return;
    };
    match hit {
        ui_mixlink::mix_browser::BrowserHit::Mix(id) => {
            let mut items: Vec<MenuItem> = state
                .takes
                .iter()
                .map(|n| MenuItem {
                    id: format!("take:{n}"),
                    label: format!("Start from take {n}"),
                    checked: false,
                    section: None,
                })
                .collect();
            items.push(MenuItem {
                id: "delete".into(),
                label: "Delete Mix…".into(),
                checked: false,
                section: Some(" ".into()),
            });
            place_menu(state, anchor, items, MenuAction::MixContext { id });
        }
        ui_mixlink::mix_browser::BrowserHit::Take(number) => {
            place_menu(
                state,
                anchor,
                vec![
                    MenuItem {
                        id: "start".into(),
                        label: "Start mix from this take".into(),
                        checked: false,
                        section: None,
                    },
                    MenuItem {
                        id: "copy".into(),
                        label: "Copy take to clipboard".into(),
                        checked: false,
                        section: None,
                    },
                ],
                MenuAction::TakeContext { number },
            );
        }
        _ => {}
    }
}

fn start_from_take(state: &mut AppState, number: i32) {
    let Some(info) = state.take_infos.iter().find(|t| t.number == number).cloned() else {
        return;
    };
    if state.mix.is_none() {
        if let Some(id) = state.mixes.first().map(|m| m.id) {
            select_mix(state, id);
        } else {
            new_mix(state);
        }
    }
    let origin = take_start_from_store(state, number);
    let Some(mut mix) = state.mix.take() else { return };
    state.undo.mutate("Start from take", false, &mut mix, |doc| {
        doc.load_from_take(&info, origin);
    });
    let id = mix.id;
    state.mix = Some(mix);
    persist_mix(state);
    select_mix(state, id);
}

fn copy_take_to_clipboard(state: &mut AppState, number: i32) {
    select_take(state, number);
    let Some(info) = state.take_infos.iter().find(|t| t.number == number) else {
        return;
    };
    let clips: Vec<_> = info
        .arrangement_tracks()
        .into_iter()
        .filter(|track| record::lane_on_record_list(&state.analog, track.lane))
        .flat_map(|track| track.clips)
        .collect();
    let Some(source_lane) = clips.first().map(|c| c.source_lane) else {
        return;
    };
    state.pasteboard = Some(MixPasteboard { clips, source_lane });
}

fn delete_mix_id(state: &mut AppState, id: uuid::Uuid) {
    if !native::confirm_delete_mix() {
        return;
    }
    if let Some(folder) = ProjectStore::current_url(&state.analog.config) {
        let path = state.project.mix_url(id, &folder);
        let _ = std::fs::remove_file(path);
    }
    state.mixes.retain(|m| m.id != id);
    if state.mix.as_ref().is_some_and(|m| m.id == id) {
        state.mix = None;
    }
    if state.mix.is_none() && state.viewing_take.is_none() {
        if let Some(first) = state.mixes.first().map(|m| m.id) {
            select_mix(state, first);
        }
    }
    persist_project_meta(state);
}

fn persist_mix(state: &mut AppState) {
    let Some(mix) = &state.mix else { return };
    if let Some(folder) = ProjectStore::current_url(&state.analog.config) {
        if let Err(e) = state.project.save_mix(mix, &folder) {
            log::warn!("save mix: {e}");
        }
    }
    if let Some(cur) = &state.mix {
        if let Some(slot) = state.mixes.iter_mut().find(|m| m.id == cur.id) {
            *slot = cur.clone();
        }
    }
}

fn project_parts(rel: &str) -> (String, String) {
    if let Some((date, rest)) = rel.split_once(" - ") {
        (date.to_string(), rest.to_string())
    } else {
        (rel.to_string(), String::new())
    }
}

fn ensure_waveforms(state: &mut AppState) {
    let Some(folder) = ProjectStore::current_url(&state.analog.config) else { return };
    let files: Vec<String> = arrangement_tracks(state)
        .iter()
        .flat_map(|t| t.clips.iter().map(|c| c.source_file.clone()))
        .collect();
    for file in files {
        let path = folder.join(&file);
        if path.exists() {
            state.waveforms.request(&file, path);
        }
    }
}

fn apply_undo(state: &mut AppState, redo: bool) {
    let Some(current) = state.mix.take() else { return };
    let restored = if redo { state.undo.redo(current) } else { state.undo.undo(current) };
    if let Some((_, doc)) = restored {
        state.mix = Some(doc);
        persist_mix(state);
    }
    publish_schedule(state);
}

fn paste_clips(state: &mut AppState) {
    let Some(board) = state.pasteboard.clone() else { return };
    let dest = state.selected_lane.unwrap_or(board.source_lane);
    let at = state.locate_frame;
    if let Some(mut mix) = state.mix.take() {
        state.undo.mutate("Paste", false, &mut mix, |doc| doc.paste_clips(&board, dest, at));
        state.mix = Some(mix);
        persist_mix(state);
    }
}

fn arrangement_tracks(state: &AppState) -> Vec<MixTrack> {
    if state.viewing_take.is_some() {
        state.take_view.clone().unwrap_or_default()
    } else {
        state.mix.as_ref().map(MixDocument::channel_tracks).unwrap_or_default()
    }
}

fn arrangement_track_count(state: &AppState) -> usize {
    arrangement_tracks(state).len()
}

fn select_mix(state: &mut AppState, id: uuid::Uuid) {
    if state.playing {
        halt_mix_play(state);
    }
    state.mix = state.mixes.iter().find(|m| m.id == id).cloned();
    state.viewing_take = None;
    state.take_view = None;
    sync_origin(state);
    persist_project_meta(state);
    publish_schedule(state);
}

fn select_take(state: &mut AppState, number: i32) {
    if state.playing {
        halt_mix_play(state);
    }
    state.viewing_take = Some(number);
    state.take_view = state.take_infos.iter().find(|t| t.number == number).map(|info| {
        info.arrangement_tracks()
            .into_iter()
            .filter(|track| record::lane_on_record_list(&state.analog, track.lane))
            .collect()
    });
    sync_origin(state);
    persist_project_meta(state);
}

fn take_start_from_store(state: &AppState, number: i32) -> i64 {
    ProjectStore::current_url(&state.analog.config)
        .map(|folder| state.project.load_meta(&folder).take_start_frame(number))
        .unwrap_or(0)
}

fn sync_origin(state: &mut AppState) {
    state.arrangement_origin = if let Some(n) = state.viewing_take {
        take_start_from_store(state, n)
    } else {
        state.mix.as_ref().map(|m| m.start_frame.max(0)).unwrap_or(0)
    };
}

fn persist_start(state: &mut AppState) {
    let origin = state.arrangement_origin.max(0);
    if state.viewing_take.is_some() {
        persist_project_meta(state);
        return;
    }
    if let Some(mix) = state.mix.as_mut() {
        mix.start_frame = origin;
    }
    persist_mix(state);
}

fn persist_project_meta(state: &mut AppState) {
    let Some(folder) = ProjectStore::current_url(&state.analog.config) else {
        return;
    };
    let mut meta = state.project.load_meta(&folder);
    meta.tempo = state.tempo;
    meta.grid = state.grid;
    meta.grid_enabled = state.grid_enabled;
    meta.pixels_per_bar = state.pixels_per_bar as f64;
    meta.selected_lane = state.selected_lane;
    meta.active_mix_id = state.mix.as_ref().map(|m| m.id);
    meta.mixes =
        state.mixes.iter().map(|m| MixListEntry { id: m.id, name: m.name.clone() }).collect();
    meta.arrangement = if let Some(n) = state.viewing_take {
        Some(MixArrangement::Take(n))
    } else {
        state.mix.as_ref().map(|m| MixArrangement::Mix(m.id))
    };
    if let Some(n) = state.viewing_take {
        meta.set_take_start_frame(n, state.arrangement_origin);
    }
    let _ = state.project.save_meta(&meta, &folder);
}

/// MixLink `MixStore.load`: sidecar + filesystem takes. No default mix.
fn reload_mix(state: &mut AppState) {
    if state.playing {
        halt_mix_play(state);
    }
    let Some(folder) = ProjectStore::current_url(&state.analog.config) else {
        if state.analog.config.projects_root_bookmark.is_some() {
            log::warn!(
                "project: bookmark did not resolve (relative {:?})",
                state.analog.config.current_project_relative
            );
        }
        state.takes.clear();
        state.take_infos.clear();
        state.take_view = None;
        state.mixes.clear();
        state.mix = None;
        return;
    };
    if !folder.exists() {
        log::warn!("project: folder missing {}", folder.display());
    }
    let sr = state._stream.as_ref().map(|s| s.sample_rate() as f64).unwrap_or(48_000.0);
    let meta = state.project.load_meta(&folder);
    state.tempo = meta.tempo;
    state.grid = meta.grid;
    state.grid_enabled = meta.grid_enabled;
    state.pixels_per_bar = meta.pixels_per_bar as f32;
    state.selected_lane = meta.selected_lane;
    state.take_number = state.project.next_take(&folder);
    state.take_infos = record::list_take_infos(&folder, sr);
    state.takes = state.take_infos.iter().map(|t| t.number).collect();

    state.mixes.clear();
    for entry in &meta.mixes {
        if let Some(doc) = state.project.load_mix(entry.id, &folder) {
            state.mixes.push(doc);
        }
    }
    if let Ok(rd) = std::fs::read_dir(&folder) {
        for ent in rd.flatten() {
            let path = ent.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with("mix-") && path.extension().and_then(|e| e.to_str()) == Some("json")
            {
                if let Ok(data) = std::fs::read(&path) {
                    if let Ok(doc) = serde_json::from_slice::<MixDocument>(&data) {
                        if !state.mixes.iter().any(|m| m.id == doc.id) {
                            state.mixes.push(doc);
                        }
                    } else {
                        log::warn!("project: could not parse {}", path.display());
                    }
                }
            }
        }
    }
    let active = meta
        .active_mix_id
        .and_then(|id| state.mixes.iter().find(|m| m.id == id).cloned())
        .or_else(|| state.mixes.first().cloned());
    state.mix = active;
    match meta.arrangement {
        Some(MixArrangement::Take(n)) if state.takes.contains(&n) => {
            select_take(state, n);
        }
        Some(MixArrangement::Mix(id)) => {
            if state.mixes.iter().any(|m| m.id == id) {
                state.mix = state.mixes.iter().find(|m| m.id == id).cloned();
            }
            state.viewing_take = None;
            state.take_view = None;
        }
        _ => {
            state.viewing_take = None;
            state.take_view = None;
        }
    }
    sync_origin(state);
    log::info!(
        "project {} — {} mix(es), take(s) {:?}",
        folder.display(),
        state.mixes.len(),
        state.takes
    );
}

fn publish_schedule(state: &AppState) {
    let mut schedule = Schedule::empty();
    for (i, strip) in state.analog.config.strips.iter().enumerate().take(8) {
        // Mono strips tap one input twice; record DSP pans that into a stereo file.
        schedule.taps[i] = AudioTapBinding::hardware(
            strip.index,
            if strip.linked_stereo { strip.index + 1 } else { strip.index },
        );
    }
    for lane in analog::ReturnLane::ALL {
        let tap = 8 + lane as usize;
        if tap >= TAP_COUNT {
            continue;
        }
        if let Some(id) = state.analog.config.return_source_id(lane) {
            schedule.taps[tap] = AudioTapBinding::hardware(id.index, id.index + 1);
        }
    }
    schedule.taps[MASTER_TAP] = AudioTapBinding::master_mix();
    publish_record_main_mix(state, &mut schedule);

    for slot in 0..8 {
        let id = slot as i32;
        let Some(plugin) = state.analog.config.plugin(id) else {
            continue;
        };
        if !plugin.is_loaded() {
            continue;
        }
        let used = analog::ALL_SEND_LANES.iter().any(|lane| {
            matches!(
                state.analog.config.send_destination(*lane),
                Some(analog::SendDestination::Plugin(pid)) if pid == id
            )
        }) || matches!(
            state.analog.config.send_destination(ReturnLane::Bus1),
            Some(analog::SendDestination::Plugin(pid)) if pid == id
        ) || matches!(
            state.analog.config.send_destination(ReturnLane::Bus2),
            Some(analog::SendDestination::Plugin(pid)) if pid == id
        );
        if !used {
            continue;
        }
        schedule.routes[slot].enabled = true;
        schedule.routes[slot].return_channel = plugin.return_channel;
        for (i, strip) in state.analog.config.strips.iter().enumerate().take(8) {
            schedule.routes[slot].feeds[i] = StripFeed {
                channel: strip.index,
                gain: state.analog.plugin_send_gain(id, i),
                linked: strip.linked_stereo,
            };
        }
    }

    let play_tracks = mixer_tracks(state);
    schedule.any_solo = play_tracks.iter().any(|t| t.solo);
    let main_dest = state.analog.config.mix_playback_channel(analog::MixLane::Main);
    for (i, track) in play_tracks.iter().take(MIX_PLAY_MAX_LANES).enumerate() {
        // Channel strips feed Main → Control Room only. They never write analog dests.
        let dest = if track.lane == MixLane::Main { main_dest } else { -1 };
        let amp = osc::fader_lin_to_amp(track.fader);
        let pan = if track.lane == MixLane::Main { 0.5 } else { track.pan };
        let (gain_l, gain_r) = osc::stereo_pan_amps(amp, pan);
        schedule.lanes[i] = LanePlayer {
            active: state.playing,
            is_main: track.lane == MixLane::Main,
            dest,
            muted: track.mute,
            soloed: track.solo,
            gain_l,
            gain_r,
            insert: usize::MAX,
        };
    }
    schedule.listen_amp = osc::fader_lin_to_amp(state.control_room_fader);
    state.engine_handles.schedule.store(Arc::new(schedule));
}

/// Print analog fader/mute and the live pan knob onto each stem.
/// Mix-page faders stay playback-only.
fn publish_record_main_mix(state: &AppState, schedule: &mut Schedule) {
    let any_solo = state.analog.any_solo_active();
    for i in 0..8 {
        let muted = analog_performance_muted(
            state.analog.strip_muted(i),
            state.analog.strip_soloed(i),
            any_solo,
        );
        let enabled = state.analog.config.is_strip_enabled(i);
        schedule.record_muted[i] = muted;
        schedule.mix_gains[i] = MixGain {
            gain: osc::fader_lin_to_amp(state.analog.strip_main_mix_lin(i)),
            pan: state.analog.strip_record_pan(i),
            muted,
            into_master: enabled && !muted,
        };
    }
    for lane in ReturnLane::ALL {
        let tap = 8 + lane as usize;
        if tap >= MASTER_TAP {
            continue;
        }
        let muted = analog_performance_muted(
            state.analog.return_muted(lane),
            state.analog.return_soloed(lane),
            any_solo,
        );
        let enabled = state.analog.config.is_return_enabled(lane);
        schedule.record_muted[tap] = muted;
        schedule.mix_gains[tap] = MixGain {
            gain: osc::fader_lin_to_amp(state.analog.return_main_mix_lin(lane)),
            pan: state.analog.return_record_pan(lane),
            muted,
            into_master: enabled && !muted,
        };
    }
}

fn analog_performance_muted(muted: bool, soloed: bool, any_solo: bool) -> bool {
    muted || (any_solo && !soloed)
}
