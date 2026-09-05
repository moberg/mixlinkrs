//! Pointer, wheel, and key handling for the main window.

use std::time::Instant;

use analog::MixAssign;
use project::{ArrSelection, MixTime};
use ui_mixlink::chrome::{self, ChromeHit, Page, HEADER_H};
use ui_mixlink::hit::{self, Hit, Pad};
use ui_mixlink::mixer::{MixerExtraHit, MixerLayout, StripKind};
use ui_mixlink::overlay::{self, TextFocus};
use ui_mixlink::sidebar;
use ui_mixlink::theme::Layout;
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, NamedKey};

use crate::state::{AppState, Chrome, Drag};

impl Chrome {
    pub(crate) fn sidebar_open(&self) -> bool {
        self.page == Page::Record || self.show_inserts
    }

    pub(crate) fn body_rect(&self) -> (f32, f32, f32, f32) {
        let (w, h) = self.renderer.logical_size();
        let side = if self.sidebar_open() { Layout::SIDEBAR_WIDTH } else { 0.0 };
        (0.0, HEADER_H, w - side, h - HEADER_H - chrome::footer_height(self.page))
    }

    pub(crate) fn apply_cursor(&mut self, next: crate::cursors::ArrCursor) {
        if next == self.last_cursor {
            return;
        }
        self.last_cursor = next;
        if let Some(cursors) = &self.trim_cursors {
            cursors.apply(&self.window, next);
        } else {
            crate::cursors::apply_fallback(&self.window, next);
        }
    }
}

impl AppState {
    pub(crate) fn hit_body(&self, x: f32, y: f32) -> Option<Hit> {
        let (bx, by, bw, bh) = self.chrome.body_rect();
        if x < bx || x > bx + bw || y < by || y > by + bh {
            return None;
        }
        match self.chrome.page {
            Page::Record => {
                let layout = MixerLayout::new(
                    bx,
                    by,
                    bw,
                    bh,
                    self.surface.analog.config.effect_return_count as usize,
                );
                hit::hit_mixer(
                    &layout,
                    self.surface.analog.config.effect_return_count as usize,
                    x,
                    y,
                )
            }
            Page::Mix => {
                let mix_h =
                    ui_mixlink::mix_mixer::height(self.chrome.show_knobs, self.chrome.show_mixer);
                if y >= by + bh - mix_h {
                    let n = self.mixer_track_count();
                    return hit::hit_mix_mixer(
                        bx + ui_mixlink::mix_browser::WIDTH,
                        by + bh - mix_h,
                        bw - ui_mixlink::mix_browser::WIDTH,
                        mix_h,
                        n,
                        self.chrome.show_knobs,
                        self.chrome.show_mixer,
                        x,
                        y,
                    );
                }
                let tracks = self.arrangement_tracks();
                let sr = self.audio.sample_rate();
                hit::hit_arrangement(
                    &self.arr_layout(),
                    &tracks,
                    self.session.viewing_take.is_some(),
                    self.timeline.arrangement_origin,
                    self.timeline.tempo,
                    sr,
                    x,
                    y,
                )
            }
        }
    }

    fn browser_hit_is_edit_target(&self, hit: ui_mixlink::mix_browser::BrowserHit) -> bool {
        match (&self.chrome.text_focus, hit) {
            (TextFocus::MixName(id), ui_mixlink::mix_browser::BrowserHit::Mix(hit)) => *id == hit,
            (TextFocus::TakeName(n), ui_mixlink::mix_browser::BrowserHit::Take(hit)) => *n == hit,
            _ => false,
        }
    }

    pub(crate) fn on_press(&mut self, event_loop: &ActiveEventLoop) {
        let (x, y) = self.chrome.cursor;
        let (w, h) = self.chrome.renderer.logical_size();

        if let Some(overlay) = self.chrome.overlay.clone() {
            if self.handle_overlay_press(&overlay, x, y, w, h) {
                return;
            }
        }

        if self.chrome.text_focus == TextFocus::Tempo
            && !matches!(chrome::hit_chrome(self.chrome.page, w, h, x, y), Some(ChromeHit::Tempo))
        {
            self.commit_focus();
        }
        if self.chrome.text_focus == TextFocus::ProjectName
            && !matches!(
                chrome::hit_chrome(self.chrome.page, w, h, x, y),
                Some(ChromeHit::ProjectSelector)
            )
        {
            self.commit_focus();
        }
        if matches!(self.chrome.text_focus, TextFocus::MixName(_) | TextFocus::TakeName(_)) {
            let keep = ui_mixlink::mix_browser::hit(&self.mix_browser_view(), x, y)
                .is_some_and(|hit| self.browser_hit_is_edit_target(hit));
            if !keep {
                self.commit_focus();
            }
        }

        if let Some(hit) = chrome::hit_chrome(self.chrome.page, w, h, x, y) {
            match hit {
                ChromeHit::Play => self.toggle_play(),
                ChromeHit::Rec => {
                    self.toggle_record();
                    self.sync_xl_leds(true);
                }
                ChromeHit::Grid => {
                    self.timeline.grid_enabled = !self.timeline.grid_enabled;
                    self.persist_project_meta();
                }
                ChromeHit::GridStep => self.open_grid_menu(),
                ChromeHit::Auto => self.timeline.automation_armed = !self.timeline.automation_armed,
                ChromeHit::Knobs => self.chrome.show_knobs = !self.chrome.show_knobs,
                ChromeHit::Inserts => self.chrome.show_inserts = !self.chrome.show_inserts,
                ChromeHit::Export => self.export_mix(),
                ChromeHit::Tempo => {
                    if self.chrome.text_focus == TextFocus::Tempo {
                        self.commit_focus();
                    }
                    self.chrome.drag = Some(Drag::Tempo {
                        start_y: y,
                        start_bpm: self.timeline.tempo,
                        live: false,
                    });
                }
                ChromeHit::Page(p) => {
                    if self.chrome.page == Page::Mix {
                        self.persist_mix();
                    }
                    self.chrome.page = p;
                }
                ChromeHit::ProjectSelector | ChromeHit::ProjectMenu => {
                    if self.chrome.text_focus == TextFocus::ProjectName {
                        return;
                    }
                    let now = Instant::now();
                    let double = self
                        .chrome
                        .last_click
                        .map(|(t, lx, ly)| {
                            t.elapsed().as_millis() < 350 && (x - lx).hypot(y - ly) < 4.0
                        })
                        .unwrap_or(false);
                    self.chrome.last_click = Some((now, x, y));
                    if double {
                        self.begin_rename_project();
                    } else {
                        self.open_project_menu();
                    }
                }
                ChromeHit::RevealProject => self.reveal_current_project(),
                ChromeHit::NewProject => self.create_new_project(),
                ChromeHit::TitleDrag => {
                    let _ = self.chrome.window.drag_window();
                }
            }
            return;
        }

        if let Some((rect, hit)) =
            self.chrome.sidebar_hits.iter().rev().find(|(r, _)| overlay::contains(*r, x, y))
        {
            self.handle_sidebar(hit.clone(), *rect, event_loop);
            return;
        }

        if let Some((_, extra)) =
            self.chrome.mixer_extras.iter().rev().find(|(r, _)| overlay::contains(*r, x, y))
        {
            match extra {
                MixerExtraHit::AddReturn => self.surface.analog.add_effect_return(),
                MixerExtraHit::RemoveReturn => self.surface.analog.remove_last_effect_return(),
                MixerExtraHit::ControlWithPan => {
                    let on = !self.surface.analog.config.pan_knobs_control_send_c;
                    self.surface.analog.set_pan_knobs_control_send_c(on);
                    self.sync_xl_leds(true);
                }
                MixerExtraHit::Record => self.toggle_record(),
            }
            return;
        }

        let (bx, by, bw, bh) = self.chrome.body_rect();
        let send_count = self.surface.analog.config.effect_return_count as usize;
        let mut layout = MixerLayout::new(bx, by, bw, bh, send_count);
        layout.scroll_x = self.chrome.mixer_scroll;
        if let Some(rect) = ui_mixlink::mixer::control_with_pan_rect(&layout, send_count) {
            if overlay::contains(rect, x, y) {
                let on = !self.surface.analog.config.pan_knobs_control_send_c;
                self.surface.analog.set_pan_knobs_control_send_c(on);
                self.sync_xl_leds(true);
                return;
            }
        }

        let now = Instant::now();
        let double = self
            .chrome
            .last_click
            .map(|(t, lx, ly)| t.elapsed().as_millis() < 350 && (x - lx).hypot(y - ly) < 4.0)
            .unwrap_or(false);
        self.chrome.last_click = Some((now, x, y));

        if self.chrome.page == Page::Mix {
            if let Some(hit) = ui_mixlink::mix_browser::hit(&self.mix_browser_view(), x, y) {
                if self.chrome.modifiers.control_key() {
                    self.on_browser_context(hit);
                    return;
                }
                match hit {
                    ui_mixlink::mix_browser::BrowserHit::Mix(id) => {
                        if double {
                            self.begin_rename_mix(id);
                        } else {
                            self.select_mix(id);
                        }
                    }
                    ui_mixlink::mix_browser::BrowserHit::Take(n) => {
                        if double {
                            self.begin_rename_take(n);
                        } else {
                            self.select_take(n);
                        }
                    }
                    ui_mixlink::mix_browser::BrowserHit::Mixer => {
                        self.chrome.show_mixer = true;
                    }
                }
                return;
            }
        }

        if let Some(hit) = self.hit_body(x, y) {
            match hit {
                Hit::Fader { kind, rail_top, rail_bot } => {
                    if double {
                        self.surface.set_fader(kind, osc::FADER_LIN_0DB);
                    } else {
                        self.chrome.drag = Some(Drag::Fader { kind, rail_top, rail_bot });
                        self.apply_drag(x, y);
                    }
                }
                Hit::Knob { kind, lane, .. } => {
                    if double && lane.is_none() {
                        self.surface.set_pan(kind, 0.5);
                    } else {
                        let start = self.surface.current_knob(kind, lane);
                        self.chrome.drag = Some(Drag::Knob { kind, lane, start_y: y, start });
                    }
                }
                Hit::Pad { kind: StripKind::Input(i), which } => match which {
                    Pad::Solo => self.surface.analog.toggle_solo(i),
                    Pad::Mute => self.surface.analog.toggle_mute(i),
                    Pad::Bus1 => {
                        let prev = self.surface.analog.surface.strips[i].assign;
                        let next =
                            if prev == MixAssign::Bus1 { MixAssign::Main } else { MixAssign::Bus1 };
                        self.surface.analog.apply_assign(i, prev, next);
                    }
                    Pad::Bus2 => {
                        let prev = self.surface.analog.surface.strips[i].assign;
                        let next =
                            if prev == MixAssign::Bus2 { MixAssign::Main } else { MixAssign::Bus2 };
                        self.surface.analog.apply_assign(i, prev, next);
                    }
                },
                Hit::Pad { kind: StripKind::Return(lane), which } => match which {
                    Pad::Solo => self.surface.analog.toggle_return_solo(lane),
                    Pad::Mute => self.surface.analog.toggle_return_mute(lane),
                    _ => {}
                },
                Hit::Enable { kind: StripKind::Input(i) } => {
                    self.surface.analog.config.strips[i].enabled =
                        !self.surface.analog.config.strips[i].enabled;
                    self.surface.analog.apply_channel_enable(i);
                    self.surface.analog.persist();
                }
                Hit::Enable { kind: StripKind::Return(lane) } => {
                    self.surface.analog.toggle_return_enabled(lane);
                }
                Hit::Name { kind } => self.open_name_menu(kind, x, y),
                Hit::Ruler { .. } | Hit::TimeRuler => {
                    self.chrome.drag = Some(Drag::Zoom {
                        start_ppb: self.timeline.pixels_per_bar,
                        start_scroll: self.timeline.scroll_x,
                        anchor_bar: ((x - self.chrome.body_rect().0 - 148.0 - 86.0)
                            + self.timeline.scroll_x) as f64
                            / self.timeline.pixels_per_bar.max(1.0) as f64,
                        start_x: x,
                        start_y: y,
                        live: false,
                    });
                }
                Hit::Locate => {
                    if self.chrome.modifiers.control_key() && !self.chrome.modifiers.super_key() {
                        let tracks = self.arrangement_tracks();
                        if let Some((_, id)) = ui_mixlink::arrangement::clip_at(
                            &self.arr_layout(),
                            &tracks,
                            self.session.viewing_take.is_some(),
                            self.timeline.tempo,
                            self.audio.sample_rate(),
                            x,
                            y,
                        ) {
                            self.begin_clip_slip(id, x);
                        } else {
                            self.begin_time_select(x, y, false);
                        }
                    } else {
                        self.begin_time_select(x, y, false);
                    }
                }
                Hit::Lane { track } => self.select_arrange_lane(track),
                Hit::StartMarker => {
                    self.chrome.drag = Some(Drag::Start {
                        origin: self.timeline.arrangement_origin,
                        start_x: x,
                        live: false,
                    });
                }
                Hit::Clip { lane, id } => self.begin_clip_move(lane, id, x, y),
                Hit::ClipEdge { id, left } => self.begin_clip_edge(id, left, x),
                Hit::ClipFade { id, left } => self.begin_clip_fade(id, left, x),
                Hit::ClipLoop { id } => self.begin_clip_loop(id, x),
                Hit::ClipSlip { id } => self.begin_clip_slip(id, x),
                Hit::MixFader { track, rail_top, rail_bot } => {
                    if double {
                        self.set_mix_fader(track, osc::FADER_LIN_0DB);
                    } else {
                        self.chrome.drag = Some(Drag::MixFader { track, rail_top, rail_bot });
                        self.apply_drag(x, y);
                    }
                }
                Hit::MixControlRoomFader { rail_top, rail_bot } => {
                    if double {
                        self.surface.control_room_fader = osc::FADER_LIN_0DB;
                    } else {
                        self.chrome.drag = Some(Drag::ControlRoomFader { rail_top, rail_bot });
                        self.apply_drag(x, y);
                    }
                }
                Hit::MixPan { track } => {
                    if double {
                        self.set_mix_pan(track, 0.5);
                        self.persist_mix();
                    } else {
                        let start = self.mix_track(track).map(|t| t.pan).unwrap_or(0.5);
                        self.chrome.drag = Some(Drag::MixPan { track, start_y: y, start });
                    }
                }
                Hit::MixMute { track } => {
                    if let Some(t) = self.mix_track_mut(track) {
                        t.mute = !t.mute;
                        self.persist_mix();
                    }
                }
                Hit::MixMixerHandle => {
                    self.chrome.show_mixer = true;
                }
                Hit::MixSolo { track } => {
                    if let Some(t) = self.mix_track_mut(track) {
                        t.solo = !t.solo;
                        self.persist_mix();
                    }
                }
                Hit::MixKnob { track, knob } => {
                    let start = self
                        .mix_track(track)
                        .and_then(|t| t.knobs.get(knob).copied())
                        .unwrap_or(0.0);
                    self.chrome.drag = Some(Drag::MixKnob { track, knob, start_y: y, start });
                }
                _ => {}
            }
        }
        self.sync_rt();
    }

    pub(crate) fn on_release(&mut self) {
        if let Some(Drag::Zoom { start_x, start_y, live, .. }) = self.chrome.drag {
            if !live {
                let frame = hit::frame_at_x(
                    &self.arr_layout(),
                    start_x,
                    self.timeline.tempo,
                    self.audio.sample_rate(),
                );
                self.locate_to(self.snap_playhead_frame(frame));
                self.timeline.selection.clear();
            }
            let _ = start_y;
        }
        if let Some(Drag::Start { live, .. }) = self.chrome.drag {
            if live {
                self.persist_start();
            } else {
                self.audition_from_origin();
            }
        }
        if matches!(
            self.chrome.drag,
            Some(Drag::MixFader { .. } | Drag::MixPan { .. } | Drag::MixKnob { .. })
        ) {
            self.persist_mix();
        }
        if let Some(Drag::Tempo { live, .. }) = self.chrome.drag {
            if live {
                self.persist_project_meta();
            } else {
                self.begin_tempo_edit();
            }
        }
        self.commit_arrangement_drag();
        self.chrome.drag = None;
        self.timeline.clip_preview = None;
        self.timeline.clip_readout = None;
    }

    pub(crate) fn apply_drag(&mut self, x: f32, y: f32) {
        match self.chrome.drag.clone() {
            Some(Drag::Fader { kind, rail_top, rail_bot }) => {
                let h = (rail_bot - rail_top).max(1.0);
                let t = ((rail_bot - y) / h).clamp(0.0, 1.0);
                self.surface.set_fader(kind, t);
            }
            Some(Drag::Knob { kind, lane, start_y, start }) => {
                let next = (start - (y - start_y) / Layout::KNOB_DRAG_PX).clamp(0.0, 1.0);
                if let Some(lane) = lane {
                    if let StripKind::Input(i) = kind {
                        self.surface.analog.apply_aux(i, lane, next);
                    }
                } else {
                    self.surface.set_pan(kind, next);
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
                self.timeline.pixels_per_bar = next;
                self.timeline.scroll_x = (f64::from(start_scroll)
                    + anchor_bar * f64::from(next - start_ppb)
                    - f64::from(dx))
                .max(0.0) as f32;
                self.chrome.drag = Some(Drag::Zoom {
                    start_ppb,
                    start_scroll,
                    anchor_bar,
                    start_x,
                    start_y,
                    live: true,
                });
            }
            Some(Drag::ClipMove { .. })
            | Some(Drag::ClipEdge { .. })
            | Some(Drag::ClipFade { .. })
            | Some(Drag::ClipLoop { .. })
            | Some(Drag::ClipSlip { .. }) => {
                self.preview_arrangement_drag(x, y);
            }
            Some(Drag::Start { origin, start_x, live }) => {
                if !live && (x - start_x).abs() < 3.0 {
                    return;
                }
                let sr = self.audio.sample_rate();
                let delta = MixTime::frame_from_bar(
                    ((x - start_x) / self.timeline.pixels_per_bar.max(1.0)) as f64,
                    self.timeline.tempo,
                    sr,
                );
                let next = (origin + delta).max(0);
                // Snap relative to the drag-start origin — START *is* the grid zero.
                let bypass = self.chrome.modifiers.super_key();
                let snapped = if self.timeline.grid_enabled && !bypass {
                    MixTime::snap(next, self.timeline.grid.raw(), self.timeline.tempo, sr, origin)
                        .max(0)
                } else {
                    next
                };
                self.set_arrangement_start(snapped);
                self.timeline.clip_readout = Some(format!(
                    "START  {}",
                    MixTime::format_position(
                        self.timeline.arrangement_origin,
                        self.timeline.tempo,
                        sr
                    )
                ));
                self.chrome.drag = Some(Drag::Start { origin, start_x, live: true });
            }
            Some(Drag::Select { start_lane, start, all_lanes, start_x, start_y, live }) => {
                if !live {
                    if (x - start_x).hypot(y - start_y) < 3.0 {
                        return;
                    }
                    self.chrome.drag = Some(Drag::Select {
                        start_lane,
                        start,
                        all_lanes,
                        start_x,
                        start_y,
                        live: true,
                    });
                }
                let sr = self.audio.sample_rate();
                let end = self.snap_select_frame(hit::frame_at_x(
                    &self.arr_layout(),
                    x,
                    self.timeline.tempo,
                    sr,
                ));
                let tracks = self.arrangement_tracks();
                let lanes = if all_lanes {
                    crate::arrange::all_lanes(&tracks)
                } else if let Some(idx) = ui_mixlink::arrangement::track_index_clamped(
                    &self.arr_layout(),
                    y,
                    tracks.len(),
                ) {
                    crate::arrange::lanes_between(&tracks, start_lane, tracks[idx].lane)
                } else {
                    vec![start_lane]
                };
                if let Some(lane) = lanes.last() {
                    self.timeline.selected_lane = Some(*lane);
                }
                self.timeline.selection = ArrSelection { lanes, start, end, clips: Vec::new() };
                self.edge_auto_scroll(x, y);
            }
            Some(Drag::MixFader { track, rail_top, rail_bot }) => {
                let h = (rail_bot - rail_top).max(1.0);
                self.set_mix_fader(track, ((rail_bot - y) / h).clamp(0.0, 1.0));
            }
            Some(Drag::ControlRoomFader { rail_top, rail_bot }) => {
                let h = (rail_bot - rail_top).max(1.0);
                self.surface.control_room_fader = ((rail_bot - y) / h).clamp(0.0, 1.0);
            }
            Some(Drag::MixPan { track, start_y, start }) => {
                self.set_mix_pan(
                    track,
                    (start - (y - start_y) / Layout::KNOB_DRAG_PX).clamp(0.0, 1.0),
                );
            }
            Some(Drag::MixKnob { track, knob, start_y, start }) => {
                self.set_mix_knob(
                    track,
                    knob,
                    (start - (y - start_y) / Layout::KNOB_DRAG_PX).clamp(0.0, 1.0),
                );
            }
            Some(Drag::Tempo { start_y, start_bpm, live }) => {
                if !live && (y - start_y).abs() < 3.0 {
                    return;
                }
                let next = crate::arrange::tempo_from_drag(
                    start_bpm,
                    start_y,
                    y,
                    self.chrome.modifiers.alt_key(),
                );
                self.set_tempo(next);
                self.chrome.drag = Some(Drag::Tempo { start_y, start_bpm, live: true });
            }
            None => {}
        }
        self.sync_rt();
    }

    pub(crate) fn sidebar_scroll_max(&self, h: f32) -> f32 {
        self.with_sidebar_view(|view| sidebar::max_scroll(view, h))
    }

    pub(crate) fn on_wheel(&mut self, dx: f32, dy: f32) {
        let (x, y) = self.chrome.cursor;
        let (w, h) = self.chrome.renderer.logical_size();
        if matches!(chrome::hit_chrome(self.chrome.page, w, h, x, y), Some(ChromeHit::Tempo)) {
            if self.chrome.text_focus == TextFocus::Tempo {
                self.commit_focus();
            }
            let step = if self.chrome.modifiers.alt_key() { 0.1 } else { 1.0 };
            let next =
                if dy > 0.0 { self.timeline.tempo + step } else { self.timeline.tempo - step };
            self.set_tempo(crate::arrange::tempo_from_drag(
                next,
                0.0,
                0.0,
                self.chrome.modifiers.alt_key(),
            ));
            self.persist_project_meta();
            return;
        }
        if self.chrome.sidebar_open() && x >= w - Layout::SIDEBAR_WIDTH {
            let max = self.sidebar_scroll_max(h);
            self.chrome.sidebar_scroll = (self.chrome.sidebar_scroll - dy).clamp(0.0, max);
            return;
        }
        if let Some(hit) = self.hit_body(x, y) {
            match hit {
                Hit::Fader { kind, .. } => {
                    self.surface.set_fader(
                        kind,
                        (self.surface.current_fader(kind) + dy * 0.002).clamp(0.0, 1.0),
                    );
                    return;
                }
                Hit::Knob { kind, lane, .. } => {
                    let next = (self.surface.current_knob(kind, lane) + dy * 0.002).clamp(0.0, 1.0);
                    if let Some(lane) = lane {
                        if let StripKind::Input(i) = kind {
                            self.surface.analog.apply_aux(i, lane, next);
                        }
                    } else {
                        self.surface.set_pan(kind, next);
                    }
                    return;
                }
                Hit::MixFader { track, .. } => {
                    let cur = self.mix_track(track).map(|t| t.fader).unwrap_or(0.0);
                    self.set_mix_fader(track, (cur + dy * 0.002).clamp(0.0, 1.0));
                    return;
                }
                Hit::MixControlRoomFader { .. } => {
                    self.surface.control_room_fader =
                        (self.surface.control_room_fader + dy * 0.002).clamp(0.0, 1.0);
                    return;
                }
                Hit::MixPan { track } => {
                    let cur = self.mix_track(track).map(|t| t.pan).unwrap_or(0.5);
                    self.set_mix_pan(track, (cur + dy * 0.002).clamp(0.0, 1.0));
                    return;
                }
                Hit::MixKnob { track, knob } => {
                    let cur = self
                        .mix_track(track)
                        .and_then(|t| t.knobs.get(knob).copied())
                        .unwrap_or(0.0);
                    self.set_mix_knob(track, knob, (cur + dy * 0.002).clamp(0.0, 1.0));
                    return;
                }
                _ => {}
            }
        }
        if self.chrome.page == Page::Record {
            let (bx, by, bw, bh) = self.chrome.body_rect();
            let send_count = self.surface.analog.config.effect_return_count as usize;
            let layout = MixerLayout::new(bx, by, bw, bh, send_count);
            let max = layout.max_scroll_x(send_count);
            self.chrome.mixer_scroll = (self.chrome.mixer_scroll - dx - dy).clamp(0.0, max);
        } else {
            self.timeline.scroll_x = (self.timeline.scroll_x - dx).max(0.0);
            self.timeline.scroll_y = (self.timeline.scroll_y - dy).max(0.0);
            self.timeline.clamp_scroll_y(&self.chrome, self.arrangement_tracks().len());
        }
    }

    pub(crate) fn on_key(&mut self, key: &Key) -> bool {
        if matches!(key, Key::Named(NamedKey::Escape)) {
            if self.chrome.overlay.is_some() || self.chrome.text_focus != TextFocus::None {
                self.chrome.overlay = None;
                self.chrome.menu_action = None;
                self.chrome.text_focus = TextFocus::None;
                self.chrome.edit_replace = false;
                return true;
            }
            return false;
        }
        if self.chrome.text_focus == TextFocus::None {
            return false;
        }
        match key {
            Key::Named(NamedKey::Enter) => {
                self.commit_focus();
                true
            }
            Key::Named(NamedKey::Backspace) => {
                if self.chrome.edit_replace
                    && matches!(
                        self.chrome.text_focus,
                        TextFocus::Tempo
                            | TextFocus::ProjectName
                            | TextFocus::MixName(_)
                            | TextFocus::TakeName(_)
                    )
                {
                    self.chrome.edit_buf.clear();
                    self.chrome.edit_replace = false;
                } else {
                    self.edit_focus(|s| {
                        s.pop();
                    });
                }
                true
            }
            Key::Character(c) => {
                if self.chrome.text_focus == TextFocus::Tempo {
                    if !c.chars().all(|ch| ch.is_ascii_digit() || ch == '.') {
                        return true;
                    }
                }
                if self.chrome.edit_replace
                    && matches!(
                        self.chrome.text_focus,
                        TextFocus::Tempo
                            | TextFocus::ProjectName
                            | TextFocus::MixName(_)
                            | TextFocus::TakeName(_)
                    )
                {
                    self.chrome.edit_buf.clear();
                    self.chrome.edit_replace = false;
                }
                if c.chars().all(|ch| !ch.is_control()) {
                    let add = c.to_string();
                    self.edit_focus(|s| s.push_str(&add));
                }
                true
            }
            _ => true,
        }
    }

    pub(crate) fn on_context_press(&mut self) {
        let (x, y) = self.chrome.cursor;
        let (w, h) = self.chrome.renderer.logical_size();
        if let Some(overlay) = self.chrome.overlay.clone() {
            if self.handle_overlay_press(&overlay, x, y, w, h) {
                return;
            }
        }
        if self.chrome.page != Page::Mix {
            return;
        }
        if let Some(hit) = ui_mixlink::mix_browser::hit(&self.mix_browser_view(), x, y) {
            self.on_browser_context(hit);
            return;
        }
        match self.hit_body(x, y) {
            Some(Hit::Lane { track }) => {
                self.select_arrange_lane(track);
                self.open_arrange_menu(x, y);
            }
            Some(Hit::Clip { id, .. })
            | Some(Hit::ClipEdge { id, .. })
            | Some(Hit::ClipFade { id, .. })
            | Some(Hit::ClipLoop { id })
            | Some(Hit::ClipSlip { id }) => {
                if let Some(lane) =
                    crate::arrange::find_clip(&self.arrangement_tracks(), id).map(|(l, _)| l)
                {
                    self.select_arrange_clip(lane, id);
                }
                self.open_arrange_menu(x, y);
            }
            Some(Hit::Locate) => self.open_arrange_menu(x, y),
            _ => {}
        }
    }
}
