//! Dedicated Channels wgpu window.

use std::sync::Arc;

use ui_mixlink::overlay::{self, ChannelsHit, Overlay, TextFocus};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, NamedKey};

use analog::AnalogEngine;

use crate::state::{AppState, ChannelsWindow, Chrome};

impl Chrome {
    pub(crate) fn open_or_focus_channels(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(ch) = &self.channels {
            ch.window.set_minimized(false);
            ch.window.focus_window();
            ch.window.request_redraw();
            return;
        }
        let window = match event_loop.create_window(crate::native::extra_window_attrs(
            "Channels",
            overlay::CHANNELS_WINDOW_W as f64,
            overlay::CHANNELS_WINDOW_H as f64,
        )) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                log::error!("channels window: {e}");
                return;
            }
        };
        let zoom = self.renderer.ui_zoom();
        let mut renderer = pollster::block_on(render::Renderer::new(window.clone()));
        renderer.set_ui_zoom(zoom);
        window.focus_window();
        window.request_redraw();
        self.channels =
            Some(ChannelsWindow { renderer, window, cursor: (0.0, 0.0), scroll: 0.0, menu: None });
        self.text_focus = TextFocus::None;
    }

    pub(crate) fn close_channels(&mut self) {
        if matches!(self.text_focus, TextFocus::GearAlias(_)) {
            self.text_focus = TextFocus::None;
        }
        if self.channels.as_ref().is_some_and(|c| c.menu.is_some()) {
            self.menu_action = None;
        }
        self.channels = None;
    }

    pub(crate) fn paint_channels_window(&mut self, analog: &AnalogEngine) {
        let (cmds, scroll) = {
            let Some(ch) = self.channels.as_ref() else { return };
            let (w, h) = ch.renderer.logical_size();
            let max = overlay::channels_max_scroll(analog, h);
            let scroll = ch.scroll.min(max);
            let (mut cmds, _) =
                overlay::paint_channels(analog, w, h, &self.text_focus, self.caret_on, scroll);
            if let Some(menu) = &ch.menu {
                let Overlay::Menu { rect, items } = menu;
                let hover = overlay::menu_at(*rect, items, ch.cursor.0, ch.cursor.1);
                cmds.extend(overlay::paint_menu(menu, hover));
            }
            (cmds, scroll)
        };
        let Some(ch) = self.channels.as_mut() else { return };
        ch.scroll = scroll;
        if let Err(e) = ch.renderer.render_scene(&cmds) {
            log::error!("channels render: {e}");
        }
    }

    pub(crate) fn on_channels_wheel(&mut self, analog: &AnalogEngine, dy: f32) {
        if self.channels.as_ref().is_some_and(|ch| ch.menu.is_some()) {
            return;
        }
        let h = match self.channels.as_ref() {
            Some(ch) => ch.renderer.logical_size().1,
            None => return,
        };
        let max = overlay::channels_max_scroll(analog, h);
        if let Some(ch) = self.channels.as_mut() {
            ch.scroll = (ch.scroll - dy).clamp(0.0, max);
        }
    }
}

impl AppState {
    pub(crate) fn on_channels_press(&mut self) {
        let (x, y, w, h, scroll) = {
            let Some(ch) = self.chrome.channels.as_ref() else { return };
            let (w, h) = ch.renderer.logical_size();
            (ch.cursor.0, ch.cursor.1, w, h, ch.scroll)
        };
        if let Some(Overlay::Menu { rect, items }) =
            self.chrome.channels.as_ref().and_then(|c| c.menu.clone())
        {
            if overlay::contains(rect, x, y) {
                if let Some(i) = overlay::menu_at(rect, &items, x, y) {
                    if let Some(item) = items.get(i).cloned() {
                        if let Some(action) = self.chrome.menu_action.take() {
                            self.apply_menu(action, &item);
                        }
                    }
                }
            }
            if let Some(ch) = self.chrome.channels.as_mut() {
                ch.menu = None;
            }
            self.chrome.menu_action = None;
            return;
        }
        let (_, hits) = overlay::paint_channels(
            &self.surface.analog,
            w,
            h,
            &self.chrome.text_focus,
            false,
            scroll,
        );
        if let Some((rect, hit)) = hits.into_iter().rev().find(|(r, _)| overlay::contains(*r, x, y))
        {
            match hit {
                ChannelsHit::MixOut => self.open_channels_mix_out_menu(rect),
                ChannelsHit::GearAlias(id) => self.chrome.text_focus = TextFocus::GearAlias(id),
                ChannelsHit::Close => {
                    self.chrome.close_channels();
                    return;
                }
            }
        } else if overlay::document_chrome_drag(x, y) {
            if let Some(ch) = self.chrome.channels.as_ref() {
                let _ = ch.window.drag_window();
            }
        } else if matches!(self.chrome.text_focus, TextFocus::GearAlias(_)) {
            self.chrome.text_focus = TextFocus::None;
        }
    }

    pub(crate) fn on_channels_key(&mut self, key: &Key) {
        if matches!(key, Key::Named(NamedKey::Escape)) {
            if self.chrome.channels.as_ref().is_some_and(|c| c.menu.is_some()) {
                if let Some(ch) = self.chrome.channels.as_mut() {
                    ch.menu = None;
                }
                self.chrome.menu_action = None;
                return;
            }
            if matches!(self.chrome.text_focus, TextFocus::GearAlias(_)) {
                self.chrome.text_focus = TextFocus::None;
            }
            return;
        }
        if !matches!(self.chrome.text_focus, TextFocus::GearAlias(_)) {
            return;
        }
        match key {
            Key::Named(NamedKey::Enter) => self.commit_focus(),
            Key::Named(NamedKey::Backspace) => self.edit_focus(|s| {
                s.pop();
            }),
            Key::Character(c) if c.chars().all(|ch| !ch.is_control()) => {
                let add = c.to_string();
                self.edit_focus(|s| s.push_str(&add));
            }
            _ => {}
        }
    }
}
