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
        let (w, h) = self.renderer.logical_size();
        let sr = self._stream.as_ref().map(|s| s.sample_rate() as f64).unwrap_or(48_000.0);
        let playhead = if self.playing {
            self.engine_handles.sample_position.load(Ordering::Relaxed)
        } else {
            self.locate_frame
        };
        let chrome = ChromeState {
            page: self.page,
            tempo_text: if self.text_focus == TextFocus::Tempo {
                self.edit_buf.clone()
            } else {
                crate::arrange::format_tempo(self.tempo)
            },
            playing: self.playing,
            recording: self.recording,
            position: MixTime::format_position(playhead - self.arrangement_origin, self.tempo, sr),
            grid_on: self.grid_enabled,
            grid_title: self.grid.title().into(),
            auto_on: self.automation_armed,
            knobs_on: self.show_knobs,
            inserts_on: self.show_inserts,
            osc_connected: self.analog.osc.is_connected(),
            osc_status: if self.analog.osc.is_connected() {
                format!("OSC {}:{}", self.analog.config.osc_host, self.analog.config.osc_send_port)
            } else {
                "Waiting for TotalMix".into()
            },
            midi_status: self.midi.status(),
            last_midi: self.last_midi.clone(),
            mute_mode: matches!(
                self.analog.surface.track_control_mode,
                analog::TrackControlMode::Mute
            ),
            solo_mode: matches!(
                self.analog.surface.track_control_mode,
                analog::TrackControlMode::Solo
            ),
            project_name: self.analog.config.current_project_relative.clone().unwrap_or_default(),
            focus: &self.text_focus,
            caret: self.caret_on,
        };
        let mut scene = chrome::paint(&chrome, w, h);
        let peaks: [f32; TAP_COUNT] = std::array::from_fn(|i| {
            engine::display_level(self.engine_handles.peaks[i].load(Ordering::Relaxed))
        });
        let (bx, by, bw, bh) = self.body_rect();
        let (proj_date, proj_suffix) =
            project_parts(self.analog.config.current_project_relative.as_deref().unwrap_or(""));
        let device = self
            ._stream
            .as_ref()
            .map(|_| self.analog.config.audio_device_contains.clone())
            .unwrap_or_else(|| self.analog.config.audio_device_contains.clone());
        let sample_rate = self._stream.as_ref().map(|s| s.sample_rate()).unwrap_or(48_000);
        let buffer_frames = self._stream.as_ref().map(|s| s.buffer_frames()).unwrap_or(128);
        let latency_ms = buffer_frames as f32 / sample_rate as f32 * 1000.0;
        match self.page {
            Page::Record => {
                let send_count = self.analog.config.effect_return_count as usize;
                let mut layout = MixerLayout::new(bx, by, bw, bh, send_count);
                let mixer_max = layout.max_scroll_x(send_count);
                self.mixer_scroll = self.mixer_scroll.clamp(0.0, mixer_max);
                layout.scroll_x = self.mixer_scroll;
                let (cmds, extras) = ui_mixlink::mixer::paint(&ui_mixlink::mixer::MixerView {
                    engine: &self.analog,
                    peaks: &peaks,
                    layout,
                });
                scene.extend(cmds);
                self.mixer_extras = extras;
            }
            Page::Mix => {
                self.ensure_waveforms();
                scene.push(render::DrawCmd::Layer);
                scene.push(render::DrawCmd::Clip { rect: Rect { x: bx, y: by, w: bw, h: bh } });
                let mix_h = ui_mixlink::mix_mixer::height(self.show_knobs, self.show_mixer);
                let mix_tracks = self.mixer_tracks();
                let lane_peaks: Vec<f32> = (0..mix_tracks.len())
                    .map(|i| {
                        self.engine_handles
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
                        mix: self.mix.as_ref(),
                        tracks: &tracks,
                        selected_lane: self.selected_lane,
                        selection: &self.selection,
                        playhead,
                        origin: self.arrangement_origin,
                        tempo: self.tempo,
                        sample_rate: sr,
                        grid: self.grid,
                        grid_enabled: self.grid_enabled,
                        viewing_take: self.viewing_take.is_some(),
                        home_take: self
                            .viewing_take
                            .or_else(|| self.mix.as_ref().and_then(|m| m.home_take())),
                        drag_preview: self.clip_preview.as_deref(),
                        readout: self.clip_readout.as_deref(),
                        waveforms: Some(&self.waveforms),
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
                    selected_lane: self.selected_lane,
                    show_knobs: self.show_knobs,
                    visible: self.show_mixer,
                    peaks: &lane_peaks,
                    control_room_fader: self.control_room_fader,
                    control_room_peak: engine::display_level(
                        self.engine_handles.listen_peak.load(Ordering::Relaxed),
                    ),
                    engine: &self.analog,
                }));
                scene.extend(ui_mixlink::mix_browser::paint(
                    &ui_mixlink::mix_browser::MixBrowserView {
                        x: bx,
                        y: by,
                        h: bh,
                        mixes: &self.mixes,
                        selected_mix: self.mix.as_ref().map(|m| m.id),
                        takes: &self.takes,
                        selected_take: self.viewing_take,
                        mixer_collapsed: !self.show_mixer,
                    },
                ));
            }
        }

        if self.sidebar_open() {
            let sidebar_max = self.sidebar_scroll_max(h);
            self.sidebar_scroll = self.sidebar_scroll.clamp(0.0, sidebar_max);
            let (side_cmds, side_hits) = sidebar::paint(
                &sidebar::SidebarView {
                    page: self.page,
                    engine: &self.analog,
                    project_name: self
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
                    mix: self.mix.as_ref(),
                    selected_lane: self.selected_lane,
                    scroll: self.sidebar_scroll,
                    focus: &self.text_focus,
                    caret: self.caret_on,
                },
                w,
                h,
            );
            scene.extend(side_cmds);
            self.sidebar_hits = side_hits;
        } else {
            self.sidebar_hits.clear();
        }

        if let Some(overlay) = &self.overlay {
            scene.push(render::DrawCmd::Layer);
            match overlay {
                Overlay::Menu { .. } => {
                    let hover = match overlay {
                        Overlay::Menu { rect, items, .. } => {
                            overlay::menu_at(*rect, items, self.cursor.0, self.cursor.1)
                        }
                        _ => None,
                    };
                    scene.extend(overlay::paint_menu(overlay, hover));
                }
                Overlay::Settings => {
                    let (cmds, _) = overlay::paint_settings(
                        &self.analog,
                        w,
                        h,
                        &self.text_focus,
                        self.caret_on,
                    );
                    scene.extend(cmds);
                }
            }
        }
        let _ = self.renderer.render_scene(&scene);
    }

    pub(crate) fn mix_browser_view(&self) -> ui_mixlink::mix_browser::MixBrowserView<'_> {
        let (_, by, _, bh) = self.body_rect();
        ui_mixlink::mix_browser::MixBrowserView {
            x: 0.0,
            y: by,
            h: bh,
            mixes: &self.mixes,
            selected_mix: self.mix.as_ref().map(|m| m.id),
            takes: &self.takes,
            selected_take: self.viewing_take,
            mixer_collapsed: !self.show_mixer,
        }
    }

    pub(crate) fn ensure_waveforms(&mut self) {
        let Some(folder) = ProjectStore::current_url(&self.analog.config) else { return };
        let files: Vec<String> = self
            .arrangement_tracks()
            .iter()
            .flat_map(|t| t.clips.iter().map(|c| c.source_file.clone()))
            .collect();
        for file in files {
            let path = folder.join(&file);
            if path.exists() {
                self.waveforms.request(&file, path);
            }
        }
    }
}
