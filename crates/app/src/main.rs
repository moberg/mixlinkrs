//! MixLinkRs — winit + wgpu shell.

use std::time::Instant;

use ui_mixlink::chrome::Page;
use ui_mixlink::overlay::TextFocus;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::WindowId;

mod alloc;
mod arr_drag;
mod arrange;
mod channels;
mod cursors;
mod display_sleep;
mod input;
mod menus;
mod midi;
mod mix_doc;
mod mix_play;
mod native;
mod paint;
mod plugins;
mod record;
mod schedule;
mod state;
mod transport;

use state::AppState;

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

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }
        let boot = AppState::boot(event_loop);
        boot.window.request_redraw();
        self.state = Some(boot);
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
                state.close_channels();
            }
            WindowEvent::CloseRequested => {
                if state.recording {
                    state.toggle_record();
                }
                if state.playing {
                    state.halt_mix_play();
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
                state.apply_drag(x, y);
                state.update_cursor();
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
                    state.on_channels_wheel(dy);
                } else {
                    state.on_wheel(dx, dy);
                }
            }
            WindowEvent::MouseInput { state: st, button: MouseButton::Left, .. } if is_channels => {
                if st == ElementState::Pressed {
                    state.on_channels_press();
                }
            }
            WindowEvent::MouseInput { state: st, button: MouseButton::Left, .. } => match st {
                ElementState::Pressed => {
                    state.on_press(event_loop);
                    state.update_cursor();
                }
                ElementState::Released => {
                    state.on_release();
                    state.update_cursor();
                }
            },
            WindowEvent::MouseInput { state: st, button: MouseButton::Right, .. } if is_main => {
                if st == ElementState::Pressed {
                    state.on_context_press();
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
                    state.on_channels_key(&event.logical_key);
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed {
                    return;
                }
                if state.on_key(&event.logical_key) {
                    return;
                }
                match event.logical_key {
                    Key::Named(NamedKey::Tab) => state.cycle_page(state.modifiers.shift_key()),
                    Key::Named(NamedKey::Space) if state.page == Page::Mix => state.toggle_play(),
                    Key::Named(NamedKey::Delete) | Key::Named(NamedKey::Backspace)
                        if state.page == Page::Mix =>
                    {
                        if state.modifiers.super_key() && state.modifiers.shift_key() {
                            state.delete_time();
                        } else {
                            state.delete_clips();
                        }
                    }
                    Key::Named(NamedKey::ArrowLeft) if state.page == Page::Mix => {
                        state.nudge_locate(-1);
                    }
                    Key::Named(NamedKey::ArrowRight) if state.page == Page::Mix => {
                        state.nudge_locate(1);
                    }
                    Key::Named(NamedKey::ArrowUp) if state.page == Page::Mix => {
                        state.select_lane(-1);
                    }
                    Key::Named(NamedKey::ArrowDown) if state.page == Page::Mix => {
                        state.select_lane(1);
                    }
                    Key::Character(c) if c.eq_ignore_ascii_case("z") => {
                        if state.modifiers.super_key() || state.modifiers.control_key() {
                            state.apply_undo(state.modifiers.shift_key());
                        } else if state.page == Page::Mix {
                            state.show_mixer = !state.show_mixer;
                        } else {
                            state.apply_undo(false);
                        }
                    }
                    Key::Character(c) if c.eq_ignore_ascii_case("c") && state.page == Page::Mix => {
                        state.copy_selection();
                    }
                    Key::Character(c) if c.eq_ignore_ascii_case("v") && state.page == Page::Mix => {
                        state.paste_clips();
                    }
                    Key::Character(c) if c.eq_ignore_ascii_case("x") && state.page == Page::Mix => {
                        state.cut_clips();
                    }
                    Key::Character(c) if c.eq_ignore_ascii_case("d") && state.page == Page::Mix => {
                        state.duplicate_clips();
                    }
                    Key::Character(c)
                        if c.eq_ignore_ascii_case("e")
                            && (state.modifiers.super_key() || state.modifiers.control_key()) =>
                    {
                        state.split_clips();
                    }
                    Key::Character(c) if c.eq_ignore_ascii_case("i") && state.page == Page::Mix => {
                        state.show_inserts = !state.show_inserts;
                    }
                    _ => {}
                }
            }
            WindowEvent::RedrawRequested if is_channels => {
                state.paint_channels_window();
            }
            WindowEvent::RedrawRequested => {
                tick(state);
                state.paint();
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
            state.poll_midi_and_leds();
        }
    }
}

fn tick(state: &mut AppState) {
    display_sleep::poll_external_wake();
    state.finish_mix_play_if_done();
    state.analog.poll_osc();
    state.poll_midi_and_leds();
    if state.caret_at.elapsed().as_millis() > 500 {
        state.caret_on = !state.caret_on;
        state.caret_at = Instant::now();
    }
    state.write_automation_if_armed();
    state.publish_schedule();
}
