//! Immediate-mode frame build for the main window.

use std::sync::atomic::Ordering;

use engine_api::TAP_COUNT;
use project::MixTime;
use render::Rect;
use ui_mixlink::chrome::{self, ChromeState};
use ui_mixlink::mixer::MixerLayout;
use ui_mixlink::overlay::{self, Overlay, TextFocus};
use ui_mixlink::sidebar;

use crate::mix_doc::project_parts;
use crate::state::AppState;
use project::ProjectStore;
use ui_mixlink::chrome::Page;

impl AppState {
    pub(crate) fn paint(&mut self) {
        let (w, h) = self.chrome.renderer.logical_size();
        let sr = self.audio._stream.as_ref().map(|s| s.sample_rate() as f64).unwrap_or(48_000.0);
        let playhead = if self.audio.playing {
            self.audio.engine_handles.sample_position.load(Ordering::Relaxed)
        } else {
            self.timeline.locate_frame
        };
        let chrome = ChromeState {
            page: self.chrome.page,
            tempo_text: if self.chrome.text_focus == TextFocus::Tempo {
                self.chrome.edit_buf.clone()
            } else {
                crate::arrange::format_tempo(self.timeline.tempo)
            },
            playing: self.audio.playing,
            recording: self.audio.recording,
            position: MixTime::format_position(
                playhead - self.timeline.arrangement_origin,
                self.timeline.tempo,
                sr,
            ),
            grid_on: self.timeline.grid_enabled,
            grid_title: self.timeline.grid.title().into(),
            auto_on: self.timeline.automation_armed,
            knobs_on: self.chrome.show_knobs,
            inserts_on: self.chrome.show_inserts,
            osc_connected: self.surface.analog.osc.is_connected(),
            osc_status: if self.surface.analog.osc.is_connected() {
                format!(
                    "OSC {}:{}",
                    self.surface.analog.config.osc_host, self.surface.analog.config.osc_send_port
                )
            } else {
                "Waiting for TotalMix".into()
            },
            midi_status: self.surface.midi.status(),
            last_midi: self.surface.last_midi.clone(),
            mute_mode: matches!(
                self.surface.analog.surface.track_control_mode,
                analog::TrackControlMode::Mute
            ),
            solo_mode: matches!(
                self.surface.analog.surface.track_control_mode,
                analog::TrackControlMode::Solo
            ),
            project_name: self
                .surface
                .analog
                .config
                .current_project_relative
                .clone()
                .unwrap_or_default(),
            focus: &self.chrome.text_focus,
            caret: self.chrome.caret_on,
        };
        let mut scene = chrome::paint(&chrome, w, h);
        let peaks: [f32; TAP_COUNT] = std::array::from_fn(|i| {
            engine::display_level(self.audio.engine_handles.peaks[i].load(Ordering::Relaxed))
        });
        let (bx, by, bw, bh) = self.chrome.body_rect();
        let (proj_date, proj_suffix) = project_parts(
            self.surface.analog.config.current_project_relative.as_deref().unwrap_or(""),
        );
        let device = self
            .audio
            ._stream
            .as_ref()
            .map(|_| self.surface.analog.config.audio_device_contains.clone())
            .unwrap_or_else(|| self.surface.analog.config.audio_device_contains.clone());
        let sample_rate = self.audio._stream.as_ref().map(|s| s.sample_rate()).unwrap_or(48_000);
        let buffer_frames = self.audio._stream.as_ref().map(|s| s.buffer_frames()).unwrap_or(128);
        let latency_ms = buffer_frames as f32 / sample_rate as f32 * 1000.0;
        match self.chrome.page {
            Page::Record => {
                let send_count = self.surface.analog.config.effect_return_count as usize;
                let mut layout = MixerLayout::new(bx, by, bw, bh, send_count);
                let mixer_max = layout.max_scroll_x(send_count);
                self.chrome.mixer_scroll = self.chrome.mixer_scroll.clamp(0.0, mixer_max);
                layout.scroll_x = self.chrome.mixer_scroll;
                let (cmds, extras) = ui_mixlink::mixer::paint(&ui_mixlink::mixer::MixerView {
                    engine: &self.surface.analog,
                    peaks: &peaks,
                    layout,
                });
                scene.extend(cmds);
                self.chrome.mixer_extras = extras;
            }
            Page::Mix => {
                self.ensure_waveforms();
                scene.push(render::DrawCmd::Layer);
                scene.push(render::DrawCmd::Clip { rect: Rect { x: bx, y: by, w: bw, h: bh } });
                let mix_h =
                    ui_mixlink::mix_mixer::height(self.chrome.show_knobs, self.chrome.show_mixer);
                let mix_tracks = self.mixer_tracks();
                let lane_peaks: Vec<f32> = (0..mix_tracks.len())
                    .map(|i| {
                        self.audio
                            .engine_handles
                            .lane_peaks
                            .get(i)
                            .map(|p| engine::display_level(p.load(Ordering::Relaxed)))
                            .unwrap_or(0.0)
                    })
                    .collect();
                let tracks = self.arrangement_tracks();
                let arr = self.arr_layout();
                let hidden = self.hidden_clip_ids();
                scene.extend(ui_mixlink::arrangement::paint(
                    &ui_mixlink::arrangement::ArrangementView {
                        layout: arr,
                        mix: self.session.mix.as_ref(),
                        tracks: &tracks,
                        selected_lane: self.timeline.selected_lane,
                        selection: &self.timeline.selection,
                        playhead,
                        origin: self.timeline.arrangement_origin,
                        tempo: self.timeline.tempo,
                        sample_rate: sr,
                        grid: self.timeline.grid,
                        grid_enabled: self.timeline.grid_enabled,
                        viewing_take: self.session.viewing_take.is_some(),
                        home_take: self
                            .session
                            .viewing_take
                            .or_else(|| self.session.mix.as_ref().and_then(|m| m.home_take())),
                        drag_preview: self.timeline.clip_preview.as_deref(),
                        readout: self.timeline.clip_readout.as_deref(),
                        waveforms: Some(&self.chrome.waveforms),
                        show_selection: !self.clip_edit_drag(),
                        hide_clip_ids: &hidden,
                    },
                ));
                scene.extend(ui_mixlink::mix_mixer::paint(&ui_mixlink::mix_mixer::MixMixerView {
                    x: bx + ui_mixlink::mix_browser::WIDTH,
                    y: by + bh - mix_h,
                    w: bw - ui_mixlink::mix_browser::WIDTH,
                    h: mix_h,
                    tracks: &mix_tracks,
                    selected_lane: self.timeline.selected_lane,
                    show_knobs: self.chrome.show_knobs,
                    visible: self.chrome.show_mixer,
                    peaks: &lane_peaks,
                    control_room_fader: self.surface.control_room_fader,
                    control_room_peak: engine::display_level(
                        self.audio.engine_handles.listen_peak.load(Ordering::Relaxed),
                    ),
                    engine: &self.surface.analog,
                }));
                scene.extend(ui_mixlink::mix_browser::paint(
                    &ui_mixlink::mix_browser::MixBrowserView {
                        x: bx,
                        y: by,
                        h: bh,
                        mixes: &self.session.mixes,
                        selected_mix: self.session.mix.as_ref().map(|m| m.id),
                        takes: &self.session.takes,
                        selected_take: self.session.viewing_take,
                        mixer_collapsed: !self.chrome.show_mixer,
                    },
                ));
            }
        }

        if self.chrome.sidebar_open() {
            let sidebar_max = self.sidebar_scroll_max(h);
            self.chrome.sidebar_scroll = self.chrome.sidebar_scroll.clamp(0.0, sidebar_max);
            let (side_cmds, side_hits) = sidebar::paint(
                &sidebar::SidebarView {
                    page: self.chrome.page,
                    engine: &self.surface.analog,
                    project_name: self
                        .surface
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
                    mix: self.session.mix.as_ref(),
                    selected_lane: self.timeline.selected_lane,
                    scroll: self.chrome.sidebar_scroll,
                    focus: &self.chrome.text_focus,
                    caret: self.chrome.caret_on,
                },
                w,
                h,
            );
            scene.extend(side_cmds);
            self.chrome.sidebar_hits = side_hits;
        } else {
            self.chrome.sidebar_hits.clear();
        }

        if let Some(overlay) = &self.chrome.overlay {
            scene.push(render::DrawCmd::Layer);
            match overlay {
                Overlay::Menu { .. } => {
                    let hover = match overlay {
                        Overlay::Menu { rect, items, .. } => overlay::menu_at(
                            *rect,
                            items,
                            self.chrome.cursor.0,
                            self.chrome.cursor.1,
                        ),
                        _ => None,
                    };
                    scene.extend(overlay::paint_menu(overlay, hover));
                }
                Overlay::Settings => {
                    let (cmds, _) = overlay::paint_settings(
                        &self.surface.analog,
                        w,
                        h,
                        &self.chrome.text_focus,
                        self.chrome.caret_on,
                    );
                    scene.extend(cmds);
                }
            }
        }
        let _ = self.chrome.renderer.render_scene(&scene);
    }

    pub(crate) fn mix_browser_view(&self) -> ui_mixlink::mix_browser::MixBrowserView<'_> {
        let (_, by, _, bh) = self.chrome.body_rect();
        ui_mixlink::mix_browser::MixBrowserView {
            x: 0.0,
            y: by,
            h: bh,
            mixes: &self.session.mixes,
            selected_mix: self.session.mix.as_ref().map(|m| m.id),
            takes: &self.session.takes,
            selected_take: self.session.viewing_take,
            mixer_collapsed: !self.chrome.show_mixer,
        }
    }

    pub(crate) fn ensure_waveforms(&mut self) {
        let Some(folder) = ProjectStore::current_url(&self.surface.analog.config) else { return };
        let files: Vec<String> = self
            .arrangement_tracks()
            .iter()
            .flat_map(|t| t.clips.iter().map(|c| c.source_file.clone()))
            .collect();
        for file in files {
            let path = folder.join(&file);
            if path.exists() {
                self.chrome.waveforms.request(&file, path);
            }
        }
    }
}
