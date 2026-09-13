//! Dedicated Settings wgpu window.

use std::sync::Arc;

use project::ProjectStore;
use ui_mixlink::overlay::{self, TextFocus};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, NamedKey};

use analog::AnalogEngine;

use crate::state::{AppState, Chrome, SettingsWindow};

impl Chrome {
    pub(crate) fn open_or_focus_settings(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(win) = &self.settings {
            win.window.set_minimized(false);
            win.window.focus_window();
            win.window.request_redraw();
            return;
        }
        let window = match event_loop.create_window(crate::native::extra_window_attrs(
            "Settings",
            overlay::SETTINGS_WINDOW_W as f64,
            overlay::SETTINGS_WINDOW_H as f64,
        )) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                log::error!("settings window: {e}");
                return;
            }
        };
        let zoom = self.renderer.ui_zoom();
        let mut renderer = pollster::block_on(render::Renderer::new(window.clone()));
        renderer.set_ui_zoom(zoom);
        window.focus_window();
        window.request_redraw();
        self.settings = Some(SettingsWindow { renderer, window, cursor: (0.0, 0.0) });
        if overlay::settings_text_focus(&self.text_focus) {
            self.text_focus = TextFocus::None;
        }
    }

    pub(crate) fn close_settings(&mut self) {
        if overlay::settings_text_focus(&self.text_focus) {
            self.text_focus = TextFocus::None;
        }
        self.settings = None;
    }

    pub(crate) fn paint_settings_window(&mut self, analog: &AnalogEngine) {
        let Some(win) = self.settings.as_ref() else { return };
        let (w, h) = win.renderer.logical_size();
        let folder = ProjectStore::resolve_root(&analog.config)
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "Projects folder".into());
        let (cmds, _) =
            overlay::paint_settings(analog, w, h, &self.text_focus, self.caret_on, &folder);
        let Some(win) = self.settings.as_mut() else { return };
        if let Err(e) = win.renderer.render_scene(&cmds) {
            log::error!("settings render: {e}");
        }
    }
}

impl AppState {
    pub(crate) fn on_settings_press(&mut self) {
        let (x, y, w, h) = {
            let Some(win) = self.chrome.settings.as_ref() else { return };
            let (w, h) = win.renderer.logical_size();
            (win.cursor.0, win.cursor.1, w, h)
        };
        let hits = overlay::settings_hits(w, h);
        if overlay::contains(hits.close, x, y) {
            self.chrome.close_settings();
            return;
        }
        if overlay::contains(hits.post_fader, x, y) {
            let on = !self.surface.analog.config.sends_post_fader;
            self.surface.analog.set_sends_post_fader(on);
        } else if overlay::contains(hits.hardware_strips, x, y) {
            let on = !self.surface.analog.config.hardware_strips;
            self.surface.analog.set_hardware_strips(on);
        } else if overlay::contains(hits.osc_host, x, y) {
            self.chrome.text_focus = TextFocus::OscHost;
        } else if overlay::contains(hits.osc_send, x, y) {
            self.chrome.text_focus = TextFocus::OscSend;
        } else if overlay::contains(hits.osc_listen, x, y) {
            self.chrome.text_focus = TextFocus::OscListen;
        } else if overlay::contains(hits.midi, x, y) {
            self.chrome.text_focus = TextFocus::MidiNeedle;
        } else if overlay::contains(hits.projects_folder, x, y) {
            self.choose_projects_root();
        } else if overlay::contains(hits.apply, x, y) {
            self.apply_settings();
        } else if overlay::document_chrome_drag(x, y) {
            if let Some(win) = self.chrome.settings.as_ref() {
                let _ = win.window.drag_window();
            }
        } else if overlay::settings_text_focus(&self.chrome.text_focus) {
            self.chrome.text_focus = TextFocus::None;
        }
    }

    pub(crate) fn on_settings_key(&mut self, key: &Key) {
        if matches!(key, Key::Named(NamedKey::Escape)) {
            if overlay::settings_text_focus(&self.chrome.text_focus) {
                self.chrome.text_focus = TextFocus::None;
            }
            return;
        }
        if !overlay::settings_text_focus(&self.chrome.text_focus) {
            return;
        }
        match key {
            Key::Named(NamedKey::Enter) => self.commit_focus(),
            Key::Named(NamedKey::Backspace) => self.edit_focus(|s| {
                s.pop();
            }),
            other if self.type_into_focus(other) => {}
            _ => {}
        }
    }
}
