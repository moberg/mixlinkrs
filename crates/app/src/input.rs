//! Pointer, wheel, and key handling for the main window.

use std::time::Instant;

use analog::{MixAssign, ReturnLane};
use project::{ArrSelection, MixLane, MixTime};
use ui_mixlink::chrome::{self, ChromeHit, Page, HEADER_H};
use ui_mixlink::hit::{self, Hit, Pad};
use ui_mixlink::mixer::{MixerExtraHit, MixerLayout, StripKind};
use ui_mixlink::overlay::{self, TextFocus};
use ui_mixlink::sidebar;
use ui_mixlink::theme::Layout;
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, NamedKey};

use crate::mix_doc::project_parts;
use crate::state::{AppState, Drag};

impl AppState {
    pub(crate) fn sidebar_open(&self) -> bool {
        self.page == Page::Record || self.show_inserts
    }

    pub(crate) fn body_rect(&self) -> (f32, f32, f32, f32) {
        let (w, h) = self.renderer.logical_size();
        let side = if self.sidebar_open() { Layout::SIDEBAR_WIDTH } else { 0.0 };
        (0.0, HEADER_H, w - side, h - HEADER_H - chrome::footer_height(self.page))
    }

    pub(crate) fn hit_body(&self, x: f32, y: f32) -> Option<Hit> {
        let (bx, by, bw, bh) = self.body_rect();
        if x < bx || x > bx + bw || y < by || y > by + bh {
            return None;
        }
        match self.page {
            Page::Record => {
                let layout = MixerLayout::new(
                    bx,
                    by,
                    bw,
                    bh,
                    self.analog.config.effect_return_count as usize,
                );
                hit::hit_mixer(&layout, self.analog.config.effect_return_count as usize, x, y)
            }
            Page::Mix => {
                let mix_h = ui_mixlink::mix_mixer::height(self.show_knobs, self.show_mixer);
                if y >= by + bh - mix_h {
                    let n = self.mixer_track_count();
                    return hit::hit_mix_mixer(
                        bx + ui_mixlink::mix_browser::WIDTH,
                        by + bh - mix_h,
                        bw - ui_mixlink::mix_browser::WIDTH,
                        mix_h,
                        n,
                        self.show_knobs,
                        self.show_mixer,
                        x,
                        y,
                    );
                }
                let tracks = self.arrangement_tracks();
                let sr = self.sample_rate();
                hit::hit_arrangement(
                    &self.arr_layout(),
                    &tracks,
                    self.viewing_take.is_some(),
                    self.arrangement_origin,
                    self.tempo,
                    sr,
                    x,
                    y,
                )
            }
        }
    }

    pub(crate) fn on_press(&mut self, event_loop: &ActiveEventLoop) {
        let (x, y) = self.cursor;
        let (w, h) = self.renderer.logical_size();

        if let Some(overlay) = self.overlay.clone() {
            if self.handle_overlay_press(&overlay, x, y, w, h) {
                return;
            }
        }

        if self.text_focus == TextFocus::Tempo
            && !matches!(chrome::hit_chrome(self.page, w, h, x, y), Some(ChromeHit::Tempo))
        {
            self.commit_focus();
        }

        if let Some(hit) = chrome::hit_chrome(self.page, w, h, x, y) {
            match hit {
                ChromeHit::Play => self.toggle_play(),
                ChromeHit::Rec => {
                    self.toggle_record();
                    self.sync_xl_leds(true);
                }
                ChromeHit::MuteMode => {
                    self.analog.surface.toggle_mute_mode();
                    self.sync_xl_leds(true);
                }
                ChromeHit::SoloMode => {
                    self.analog.surface.toggle_solo_mode();
                    self.sync_xl_leds(true);
                }
                ChromeHit::Grid => {
                    self.grid_enabled = !self.grid_enabled;
                    self.persist_project_meta();
                }
                ChromeHit::GridStep => self.open_grid_menu(),
                ChromeHit::Auto => self.automation_armed = !self.automation_armed,
                ChromeHit::Knobs => self.show_knobs = !self.show_knobs,
                ChromeHit::Inserts => self.show_inserts = !self.show_inserts,
                ChromeHit::Export => self.export_mix(),
                ChromeHit::Tempo => {
                    if self.text_focus == TextFocus::Tempo {
                        self.commit_focus();
                    }
                    self.drag =
                        Some(Drag::Tempo { start_y: y, start_bpm: self.tempo, live: false });
                }
                ChromeHit::Page(p) => {
                    if self.page == Page::Mix {
                        self.persist_mix();
                    }
                    self.page = p;
                }
            }
            return;
        }

        if let Some((rect, hit)) =
            self.sidebar_hits.iter().rev().find(|(r, _)| overlay::contains(*r, x, y))
        {
            self.handle_sidebar(hit.clone(), *rect, event_loop);
            return;
        }

        if let Some((_, extra)) =
            self.mixer_extras.iter().rev().find(|(r, _)| overlay::contains(*r, x, y))
        {
            match extra {
                MixerExtraHit::AddReturn => self.analog.add_effect_return(),
                MixerExtraHit::RemoveReturn => self.analog.remove_last_effect_return(),
                MixerExtraHit::ControlWithPan => {
                    let on = !self.analog.config.pan_knobs_control_send_c;
                    self.analog.set_pan_knobs_control_send_c(on);
                    self.sync_xl_leds(true);
                }
            }
            return;
        }

        let (bx, by, bw, bh) = self.body_rect();
        let layout =
            MixerLayout::new(bx, by, bw, bh, self.analog.config.effect_return_count as usize);
        if let Some(rect) = ui_mixlink::mixer::control_with_pan_rect(
            &layout,
            self.analog.config.effect_return_count as usize,
        ) {
            if overlay::contains(rect, x, y) {
                let on = !self.analog.config.pan_knobs_control_send_c;
                self.analog.set_pan_knobs_control_send_c(on);
                return;
            }
        }

        let now = Instant::now();
        let double = self
            .last_click
            .map(|(t, lx, ly)| t.elapsed().as_millis() < 350 && (x - lx).hypot(y - ly) < 4.0)
            .unwrap_or(false);
        self.last_click = Some((now, x, y));

        if self.page == Page::Mix {
            if let Some(hit) = ui_mixlink::mix_browser::hit(&self.mix_browser_view(), x, y) {
                if self.modifiers.control_key() {
                    self.on_browser_context(hit);
                    return;
                }
                match hit {
                    ui_mixlink::mix_browser::BrowserHit::Mix(id) => {
                        self.select_mix(id);
                    }
                    ui_mixlink::mix_browser::BrowserHit::Take(n) => {
                        self.select_take(n);
                    }
                    ui_mixlink::mix_browser::BrowserHit::Mixer => {
                        self.show_mixer = true;
                    }
                }
                return;
            }
        }

        if let Some(hit) = self.hit_body(x, y) {
            match hit {
                Hit::Fader { kind, rail_top, rail_bot } => {
                    if double {
                        self.set_fader(kind, osc::FADER_LIN_0DB);
                    } else {
                        self.drag = Some(Drag::Fader { kind, rail_top, rail_bot });
                        self.apply_drag(x, y);
                    }
                }
                Hit::Knob { kind, lane, .. } => {
                    if double && lane.is_none() {
                        self.set_pan(kind, 0.5);
                    } else {
                        let start = self.current_knob(kind, lane);
                        self.drag = Some(Drag::Knob { kind, lane, start_y: y, start });
                    }
                }
                Hit::Pad { kind: StripKind::Input(i), which } => match which {
                    Pad::Solo => self.analog.toggle_solo(i),
                    Pad::Mute => self.analog.toggle_mute(i),
                    Pad::Bus1 => {
                        let prev = self.analog.surface.strips[i].assign;
                        let next =
                            if prev == MixAssign::Bus1 { MixAssign::Main } else { MixAssign::Bus1 };
                        self.analog.apply_assign(i, prev, next);
                    }
                    Pad::Bus2 => {
                        let prev = self.analog.surface.strips[i].assign;
                        let next =
                            if prev == MixAssign::Bus2 { MixAssign::Main } else { MixAssign::Bus2 };
                        self.analog.apply_assign(i, prev, next);
                    }
                },
                Hit::Pad { kind: StripKind::Return(lane), which } => match which {
                    Pad::Solo => self.analog.toggle_return_solo(lane),
                    Pad::Mute => self.analog.toggle_return_mute(lane),
                    _ => {}
                },
                Hit::Enable { kind: StripKind::Input(i) } => {
                    self.analog.config.strips[i].enabled = !self.analog.config.strips[i].enabled;
                    self.analog.apply_channel_enable(i);
                    self.analog.persist();
                }
                Hit::Enable { kind: StripKind::Return(lane) } => {
                    self.analog.toggle_return_enabled(lane);
                }
                Hit::Name { kind } => self.open_name_menu(kind, x, y),
                Hit::Ruler { .. } | Hit::TimeRuler => {
                    self.drag = Some(Drag::Zoom {
                        start_ppb: self.pixels_per_bar,
                        start_scroll: self.scroll_x,
                        anchor_bar: ((x - self.body_rect().0 - 148.0 - 86.0) + self.scroll_x)
                            as f64
                            / self.pixels_per_bar.max(1.0) as f64,
                        start_x: x,
                        start_y: y,
                        live: false,
                    });
                }
                Hit::Locate => {
                    if self.modifiers.control_key() && !self.modifiers.super_key() {
                        let tracks = self.arrangement_tracks();
                        if let Some((_, id)) = ui_mixlink::arrangement::clip_at(
                            &self.arr_layout(),
                            &tracks,
                            self.viewing_take.is_some(),
                            self.tempo,
                            self.sample_rate(),
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
                    self.drag = Some(Drag::Start {
                        origin: self.arrangement_origin,
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
                        self.drag = Some(Drag::MixFader { track, rail_top, rail_bot });
                        self.apply_drag(x, y);
                    }
                }
                Hit::MixControlRoomFader { rail_top, rail_bot } => {
                    if double {
                        self.control_room_fader = osc::FADER_LIN_0DB;
                    } else {
                        self.drag = Some(Drag::ControlRoomFader { rail_top, rail_bot });
                        self.apply_drag(x, y);
                    }
                }
                Hit::MixPan { track } => {
                    if double {
                        self.set_mix_pan(track, 0.5);
                        self.persist_mix();
                    } else {
                        let start = self.mix_track(track).map(|t| t.pan).unwrap_or(0.5);
                        self.drag = Some(Drag::MixPan { track, start_y: y, start });
                    }
                }
                Hit::MixMute { track } => {
                    if let Some(t) = self.mix_track_mut(track) {
                        t.mute = !t.mute;
                        self.persist_mix();
                    }
                }
                Hit::MixMixerHandle => {
                    self.show_mixer = true;
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
                    self.drag = Some(Drag::MixKnob { track, knob, start_y: y, start });
                }
                _ => {}
            }
        }
        self.sync_rt();
    }

    pub(crate) fn on_release(&mut self) {
        if let Some(Drag::Zoom { start_x, start_y, live, .. }) = self.drag {
            if !live {
                let frame =
                    hit::frame_at_x(&self.arr_layout(), start_x, self.tempo, self.sample_rate());
                self.locate_to(self.snap_playhead_frame(frame));
                self.selection.clear();
            }
            let _ = start_y;
        }
        if let Some(Drag::Start { live, .. }) = self.drag {
            if live {
                self.persist_start();
            } else {
                self.audition_from_origin();
            }
        }
        if matches!(
            self.drag,
            Some(Drag::MixFader { .. } | Drag::MixPan { .. } | Drag::MixKnob { .. })
        ) {
            self.persist_mix();
        }
        if let Some(Drag::Tempo { live, .. }) = self.drag {
            if live {
                self.persist_project_meta();
            } else {
                self.begin_tempo_edit();
            }
        }
        self.commit_arrangement_drag();
        self.drag = None;
        self.clip_preview = None;
        self.clip_readout = None;
    }

    pub(crate) fn apply_drag(&mut self, x: f32, y: f32) {
        match self.drag.clone() {
            Some(Drag::Fader { kind, rail_top, rail_bot }) => {
                let h = (rail_bot - rail_top).max(1.0);
                let t = ((rail_bot - y) / h).clamp(0.0, 1.0);
                self.set_fader(kind, t);
            }
            Some(Drag::Knob { kind, lane, start_y, start }) => {
                let next = (start - (y - start_y) / Layout::KNOB_DRAG_PX).clamp(0.0, 1.0);
                if let Some(lane) = lane {
                    if let StripKind::Input(i) = kind {
                        self.analog.apply_aux(i, lane, next);
                    }
                } else {
                    self.set_pan(kind, next);
                }
            }
            Some(Drag::Zoom { start_ppb, start_scroll, anchor_bar, start_x, start_y, live }) => {
                let dx = x - start_x;
                let dy = y - start_y;
                if dx.hypot(dy) < 3.0 {
                    return;
                }
                if !live && dx.abs() > dy.abs() * 1.4 {
                    let start = self.snap_playhead_frame(hit::frame_at_x(
                        &self.arr_layout(),
                        start_x,
                        self.tempo,
                        self.sample_rate(),
                    ));
                    self.drag = Some(Drag::Select {
                        start_lane: self.selected_lane.unwrap_or(MixLane::Strip(0)),
                        start,
                        all_lanes: true,
                        start_x,
                        start_y,
                        live: true,
                    });
                    self.apply_drag(x, y);
                    return;
                }
                let factor = 1.012f32.powf(dy);
                let next = (start_ppb * factor).clamp(10.0, 16_000.0);
                self.pixels_per_bar = next;
                self.scroll_x = (f64::from(start_scroll) + anchor_bar * f64::from(next - start_ppb)
                    - f64::from(dx))
                .max(0.0) as f32;
                self.drag = Some(Drag::Zoom {
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
                let sr = self.sample_rate();
                let delta = MixTime::frame_from_bar(
                    ((x - start_x) / self.pixels_per_bar.max(1.0)) as f64,
                    self.tempo,
                    sr,
                );
                let next = (origin + delta).max(0);
                // Snap relative to the drag-start origin — START *is* the grid zero.
                let bypass = self.modifiers.super_key();
                let snapped = if self.grid_enabled && !bypass {
                    MixTime::snap(next, self.grid.raw(), self.tempo, sr, origin).max(0)
                } else {
                    next
                };
                self.set_arrangement_start(snapped);
                self.clip_readout = Some(format!(
                    "START  {}",
                    MixTime::format_position(self.arrangement_origin, self.tempo, sr)
                ));
                self.drag = Some(Drag::Start { origin, start_x, live: true });
            }
            Some(Drag::Select { start_lane, start, all_lanes, start_x, start_y, live }) => {
                if !live {
                    if (x - start_x).hypot(y - start_y) < 3.0 {
                        return;
                    }
                    self.drag = Some(Drag::Select {
                        start_lane,
                        start,
                        all_lanes,
                        start_x,
                        start_y,
                        live: true,
                    });
                }
                let sr = self.sample_rate();
                let mut end = hit::frame_at_x(&self.arr_layout(), x, self.tempo, sr);
                if self.grid_enabled && !self.modifiers.super_key() {
                    end = MixTime::snap(
                        end,
                        self.grid.raw(),
                        self.tempo,
                        sr,
                        self.arrangement_origin,
                    );
                }
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
                    self.selected_lane = Some(*lane);
                }
                self.selection = ArrSelection { lanes, start, end, clips: Vec::new() };
                self.edge_auto_scroll(x, y);
            }
            Some(Drag::MixFader { track, rail_top, rail_bot }) => {
                let h = (rail_bot - rail_top).max(1.0);
                self.set_mix_fader(track, ((rail_bot - y) / h).clamp(0.0, 1.0));
            }
            Some(Drag::ControlRoomFader { rail_top, rail_bot }) => {
                let h = (rail_bot - rail_top).max(1.0);
                self.control_room_fader = ((rail_bot - y) / h).clamp(0.0, 1.0);
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
                    self.modifiers.alt_key(),
                );
                self.set_tempo(next);
                self.drag = Some(Drag::Tempo { start_y, start_bpm, live: true });
            }
            None => {}
        }
        self.sync_rt();
    }

    pub(crate) fn current_knob(&self, kind: StripKind, lane: Option<ReturnLane>) -> f32 {
        match (kind, lane) {
            (StripKind::Input(i), Some(lane)) => self.analog.surface.strips[i].aux(lane),
            (StripKind::Input(i), None) => self.analog.surface.strips[i].pan,
            (StripKind::Return(r), None) => self
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

    pub(crate) fn current_fader(&self, kind: StripKind) -> f32 {
        match kind {
            StripKind::Input(i) => self.analog.surface.strips[i].fader,
            StripKind::Return(lane) => self
                .analog
                .surface
                .returns
                .iter()
                .find(|x| x.id == lane as i32)
                .map(|x| x.fader)
                .unwrap_or(0.0),
            StripKind::Main => self.analog.mixer.main_fader,
        }
    }

    pub(crate) fn set_fader(&mut self, kind: StripKind, v: f32) {
        // Record analog only. Never writes MixDocument / take_view faders.
        match kind {
            StripKind::Input(i) => self.analog.apply_fader(i, v),
            StripKind::Return(lane) => self.analog.apply_return_fader(lane as i32, v),
            StripKind::Main => {
                self.analog.mixer.main_fader = v;
                self.analog
                    .osc
                    .send_float(osc::output_fader_lin(self.analog.config.main_output), v);
            }
        }
    }

    pub(crate) fn set_pan(&mut self, kind: StripKind, v: f32) {
        match kind {
            StripKind::Input(i) => self.analog.apply_pan(i, v),
            StripKind::Return(lane) => self.analog.apply_return_pan(lane as i32, v),
            StripKind::Main => {}
        }
    }

    pub(crate) fn sidebar_scroll_max(&self, h: f32) -> f32 {
        let (proj_date, proj_suffix) =
            project_parts(self.analog.config.current_project_relative.as_deref().unwrap_or(""));
        sidebar::max_scroll(
            &sidebar::SidebarView {
                page: self.page,
                engine: &self.analog,
                project_name: "",
                project_date: &proj_date,
                project_suffix: &proj_suffix,
                sample_rate: 0,
                buffer_frames: 0,
                latency_ms: 0.0,
                device_name: "",
                mix: self.mix.as_ref(),
                selected_lane: self.selected_lane,
                scroll: 0.0,
                focus: &self.text_focus,
                caret: false,
            },
            h,
        )
    }

    pub(crate) fn on_wheel(&mut self, dx: f32, dy: f32) {
        let (x, y) = self.cursor;
        let (w, h) = self.renderer.logical_size();
        if matches!(chrome::hit_chrome(self.page, w, h, x, y), Some(ChromeHit::Tempo)) {
            if self.text_focus == TextFocus::Tempo {
                self.commit_focus();
            }
            let step = if self.modifiers.alt_key() { 0.1 } else { 1.0 };
            let next = if dy > 0.0 { self.tempo + step } else { self.tempo - step };
            self.set_tempo(crate::arrange::tempo_from_drag(
                next,
                0.0,
                0.0,
                self.modifiers.alt_key(),
            ));
            self.persist_project_meta();
            return;
        }
        if self.sidebar_open() && x >= w - Layout::SIDEBAR_WIDTH {
            let max = self.sidebar_scroll_max(h);
            self.sidebar_scroll = (self.sidebar_scroll - dy).clamp(0.0, max);
            return;
        }
        if let Some(hit) = self.hit_body(x, y) {
            match hit {
                Hit::Fader { kind, .. } => {
                    self.set_fader(kind, (self.current_fader(kind) + dy * 0.002).clamp(0.0, 1.0));
                    return;
                }
                Hit::Knob { kind, lane, .. } => {
                    let next = (self.current_knob(kind, lane) + dy * 0.002).clamp(0.0, 1.0);
                    if let Some(lane) = lane {
                        if let StripKind::Input(i) = kind {
                            self.analog.apply_aux(i, lane, next);
                        }
                    } else {
                        self.set_pan(kind, next);
                    }
                    return;
                }
                Hit::MixFader { track, .. } => {
                    let cur = self.mix_track(track).map(|t| t.fader).unwrap_or(0.0);
                    self.set_mix_fader(track, (cur + dy * 0.002).clamp(0.0, 1.0));
                    return;
                }
                Hit::MixControlRoomFader { .. } => {
                    self.control_room_fader =
                        (self.control_room_fader + dy * 0.002).clamp(0.0, 1.0);
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
        if self.page == Page::Record {
            let (bx, by, bw, bh) = self.body_rect();
            let send_count = self.analog.config.effect_return_count as usize;
            let layout = MixerLayout::new(bx, by, bw, bh, send_count);
            let max = layout.max_scroll_x(send_count);
            self.mixer_scroll = (self.mixer_scroll - dx - dy).clamp(0.0, max);
        } else {
            self.scroll_x = (self.scroll_x - dx).max(0.0);
            self.scroll_y = (self.scroll_y - dy).max(0.0);
        }
    }

    pub(crate) fn on_key(&mut self, key: &Key) -> bool {
        if matches!(key, Key::Named(NamedKey::Escape)) {
            if self.overlay.is_some() || self.text_focus != TextFocus::None {
                self.overlay = None;
                self.text_focus = TextFocus::None;
                self.edit_replace = false;
                return true;
            }
            return false;
        }
        if self.text_focus == TextFocus::None {
            return false;
        }
        match key {
            Key::Named(NamedKey::Enter) => {
                self.commit_focus();
                true
            }
            Key::Named(NamedKey::Backspace) => {
                if self.text_focus == TextFocus::Tempo && self.edit_replace {
                    self.edit_buf.clear();
                    self.edit_replace = false;
                } else {
                    self.edit_focus(|s| {
                        s.pop();
                    });
                }
                true
            }
            Key::Character(c) => {
                if self.text_focus == TextFocus::Tempo {
                    if !c.chars().all(|ch| ch.is_ascii_digit() || ch == '.') {
                        return true;
                    }
                    if self.edit_replace {
                        self.edit_buf.clear();
                        self.edit_replace = false;
                    }
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
        let (x, y) = self.cursor;
        let (w, h) = self.renderer.logical_size();
        if let Some(overlay) = self.overlay.clone() {
            if self.handle_overlay_press(&overlay, x, y, w, h) {
                return;
            }
        }
        if self.page != Page::Mix {
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
