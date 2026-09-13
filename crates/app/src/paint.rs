//! Immediate-mode frame build for the main window.

use std::sync::atomic::Ordering;

use engine_api::TAP_COUNT;
use project::MixTime;
use render::Rect;
use ui_mixlink::chrome::{self, ChromeState};
use ui_mixlink::mixer::MixerLayout;
use ui_mixlink::overlay::{self, Overlay, TextFocus};
use ui_mixlink::sidebar;

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
            project_name: if self.chrome.text_focus == TextFocus::ProjectName {
                self.chrome.edit_buf.clone()
            } else {
                self.surface.analog.config.current_project_relative.clone().unwrap_or_default()
            },
            project_active: matches!(
                self.chrome.menu_action,
                Some(crate::menu_action::MenuAction::SwitchProject)
            ),
            focus: &self.chrome.text_focus,
            caret: self.chrome.caret_on,
        };
        let mut scene = chrome::paint(&chrome, w, h);
        let peaks: [f32; TAP_COUNT] = std::array::from_fn(|i| {
            engine::display_level(self.audio.engine_handles.peaks[i].load(Ordering::Relaxed))
        });
        let (bx, by, bw, bh) = self.chrome.body_rect();
        match self.chrome.page {
            Page::Record => {
                let send_count = self.surface.analog.config.effect_return_count as usize;
                let mut layout = MixerLayout::new(bx, by, bw, bh, send_count);
                let mixer_max = layout.max_scroll_x(send_count);
                self.chrome.mixer_scroll = self.chrome.mixer_scroll.clamp(0.0, mixer_max);
                layout.scroll_x = self.chrome.mixer_scroll;
                let take = self.session.take_number;
                let take_title = project::ProjectMeta::take_title(
                    take,
                    self.session.take_names.get(&take).map(String::as_str),
                );
                let (cmds, extras) = ui_mixlink::mixer::paint(&ui_mixlink::mixer::MixerView {
                    engine: &self.surface.analog,
                    peaks: &peaks,
                    layout,
                    touch: self.chrome.active_strip_glow(),
                    deck: ui_mixlink::deck::DeckView {
                        recording: self.audio.recording,
                        take_number: take,
                        take_title,
                        elapsed: self
                            .audio
                            .record_started
                            .map(|t| t.elapsed().as_secs_f32())
                            .unwrap_or(0.0),
                        tracks: crate::record::armed_track_count(&self.surface.analog),
                        sample_rate: sr as u32,
                        ready: ProjectStore::resolve_root(&self.surface.analog.config).is_some()
                            && self.audio._stream.is_some(),
                    },
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
                self.timeline.clamp_scroll_y(&self.chrome, tracks.len());
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
                    touch: self.chrome.active_mix_glow(),
                }));
                scene.extend(ui_mixlink::mix_browser::paint(&self.mix_browser_view()));
            }
        }

        if self.chrome.sidebar_open() {
            let sidebar_max = self.sidebar_scroll_max(h);
            self.chrome.sidebar_scroll = self.chrome.sidebar_scroll.clamp(0.0, sidebar_max);
            let (side_cmds, side_hits) = self.with_sidebar_view(|view| sidebar::paint(view, w, h));
            scene.extend(side_cmds);
            self.chrome.sidebar_hits = side_hits;
        } else {
            self.chrome.sidebar_hits.clear();
        }

        if let Some(overlay) = &self.chrome.overlay {
            scene.push(render::DrawCmd::Layer);
            let Overlay::Menu { rect, items } = overlay;
            let hover = overlay::menu_at(*rect, items, self.chrome.cursor.0, self.chrome.cursor.1);
            scene.extend(overlay::paint_menu(overlay, hover));
        }
        let pngs: Vec<(u128, &[u8])> = self
            .audio
            .plugin_previews
            .iter()
            .map(|(id, png)| (id.as_u128(), png.as_slice()))
            .collect();
        self.chrome.renderer.sync_thumbs(&pngs);
        let _ = self.chrome.renderer.render_scene(&scene);
    }

    pub(crate) fn with_sidebar_view<R>(&self, f: impl FnOnce(&sidebar::SidebarView<'_>) -> R) -> R {
        let sample_rate = self.audio._stream.as_ref().map(|s| s.sample_rate()).unwrap_or(48_000);
        let buffer_frames = self.audio._stream.as_ref().map(|s| s.buffer_frames()).unwrap_or(128);
        let latency_ms = buffer_frames as f32 / sample_rate as f32 * 1000.0;
        let thumbs: std::collections::HashSet<uuid::Uuid> =
            self.audio.plugin_previews.keys().copied().collect();
        f(&sidebar::SidebarView {
            page: self.chrome.page,
            engine: &self.surface.analog,
            sample_rate,
            buffer_frames,
            latency_ms,
            device_name: &self.surface.analog.config.audio_device_contains,
            mix: self.session.mix.as_ref(),
            selected_lane: self.timeline.selected_lane,
            scroll: self.chrome.sidebar_scroll,
            focus: &self.chrome.text_focus,
            caret: self.chrome.caret_on,
            thumbs: &thumbs,
            width: self.chrome.sidebar_width,
        })
    }

    pub(crate) fn mix_browser_view(&self) -> ui_mixlink::mix_browser::MixBrowserView<'_> {
        let (_, by, _, bh) = self.chrome.body_rect();
        let (edit, edit_text) = match self.chrome.text_focus {
            ui_mixlink::overlay::TextFocus::MixName(id) => {
                (Some(ui_mixlink::mix_browser::BrowserEdit::Mix(id)), self.chrome.edit_buf.as_str())
            }
            ui_mixlink::overlay::TextFocus::TakeName(n) => {
                (Some(ui_mixlink::mix_browser::BrowserEdit::Take(n)), self.chrome.edit_buf.as_str())
            }
            _ => (None, ""),
        };
        ui_mixlink::mix_browser::MixBrowserView {
            x: 0.0,
            y: by,
            h: bh,
            mixes: &self.session.mixes,
            selected_mix: self.session.mix.as_ref().map(|m| m.id),
            takes: &self.session.takes,
            take_names: &self.session.take_names,
            selected_take: self.session.viewing_take,
            mixer_collapsed: !self.chrome.show_mixer,
            edit,
            edit_text,
            caret: self.chrome.caret_on,
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
