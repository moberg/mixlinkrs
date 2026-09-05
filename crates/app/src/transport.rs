//! Record / play / locate / export.

use std::sync::atomic::Ordering;

use engine_api::{UiCommand, MIX_PLAY_MAX_LANES};
use project::{MixAutomationTarget, MixClip, MixLane, MixTime, ProjectStore};
use ui_mixlink::chrome::Page;

use crate::state::AppState;

impl AppState {
    pub(crate) fn toggle_record(&mut self) {
        if self.recording {
            self.recording = false;
            let _ = self.engine_handles.cmd_tx.try_push(UiCommand::SetRecording { on: false });
            if let Some(rec) = self.recorder.take() {
                let _ = rec.stop(self.engine);
            }
            if let Some(folder) = ProjectStore::current_url(&self.analog.config) {
                self.project.increment_take(&folder);
                self.take_number = self.project.next_take(&folder);
                self.reload_mix();
            }
            return;
        }
        if ProjectStore::resolve_root(&self.analog.config).is_none() {
            log::warn!("record: set a projects folder first");
            return;
        }
        if ProjectStore::current_url(&self.analog.config).is_none() {
            match self.project.create_project(&mut self.analog.config) {
                Ok(_) => {
                    self.analog.persist();
                    self.reload_mix();
                }
                Err(e) => {
                    log::warn!("record: could not create project: {e}");
                    return;
                }
            }
        }
        let Some(folder) = ProjectStore::current_url(&self.analog.config) else {
            return;
        };
        if self._stream.is_none() {
            log::warn!("record: audio is not running");
            return;
        }
        let sr = self
            ._stream
            .as_ref()
            .map(|s| s.sample_rate())
            .unwrap_or_else(|| unsafe { (*self.engine).sample_rate() });
        self.take_number = self.project.next_take(&folder);
        self.publish_schedule();
        let Some(rec) =
            crate::record::Recorder::start(self.engine, &self.analog, folder, self.take_number, sr)
        else {
            log::warn!("record: could not start take");
            return;
        };
        let _ = self.engine_handles.cmd_tx.try_push(UiCommand::ArmRings);
        self.recorder = Some(rec);
        self.recording = true;
        let _ = self.engine_handles.cmd_tx.try_push(UiCommand::SetRecording { on: true });
    }

    pub(crate) fn halt_mix_play(&mut self) {
        if let Some(player) = self.mix_player.take() {
            player.stop();
        }
        self.playing = false;
        let _ = self.engine_handles.cmd_tx.try_push(UiCommand::TransportStop);
        for peak in self.engine_handles.lane_peaks.iter() {
            peak.store(0.0, Ordering::Relaxed);
        }
        self.engine_handles.listen_peak.store(0.0, Ordering::Relaxed);
    }

    pub(crate) fn finish_mix_play_if_done(&mut self) {
        if self.playing && self.mix_player.as_ref().is_some_and(|p| !p.is_running()) {
            self.halt_mix_play();
            self.publish_schedule();
        }
    }

    pub(crate) fn toggle_play(&mut self) {
        if self.playing {
            self.halt_mix_play();
            let _ = self
                .engine_handles
                .cmd_tx
                .try_push(UiCommand::TransportSeek { sample: self.locate_frame });
            self.publish_schedule();
            return;
        }
        if self._stream.is_none() {
            log::warn!("play: audio is not running");
            return;
        }
        let Some(graph) = self.build_play_graph() else {
            log::warn!("play: set a projects folder first");
            return;
        };
        let end = graph.end_frame;
        if end <= 0 {
            log::warn!("play: nothing to play");
            return;
        }
        let playhead = self.engine_handles.sample_position.load(Ordering::Relaxed);
        if playhead >= end {
            self.locate_frame = self.arrangement_origin.max(0);
        }
        let _ = self
            .engine_handles
            .cmd_tx
            .try_push(UiCommand::TransportSeek { sample: self.locate_frame });
        self.engine_handles.sample_position.store(self.locate_frame, Ordering::Relaxed);
        let io_block = self._stream.as_ref().map(|s| s.buffer_frames() as usize).unwrap_or(128);
        self.playing = true;
        self.publish_schedule();
        self.mix_player = Some(crate::mix_play::MixPlayer::start(
            self.engine,
            self.engine_handles.controls.clone(),
            graph,
            self.locate_frame,
            io_block,
        ));
    }

    pub(crate) fn audition_from_origin(&mut self) {
        self.locate_to(self.arrangement_origin.max(0));
        if !self.playing {
            self.toggle_play();
        }
    }

    /// MixLinkRs: Tab swaps `Page::Record` ↔ `Page::Mix` (header RECORD/MIX). MixLink has no Tab binding.

    pub(crate) fn cycle_page(&mut self, _reverse: bool) {
        if self.page == Page::Mix {
            self.persist_mix();
        }
        self.page = match self.page {
            Page::Record => Page::Mix,
            Page::Mix => Page::Record,
        };
    }

    pub(crate) fn is_editing_mix(&self) -> bool {
        self.viewing_take.is_none() && self.mix.is_some()
    }

    pub(crate) fn sample_rate(&self) -> f64 {
        self._stream.as_ref().map(|s| s.sample_rate() as f64).unwrap_or(48_000.0)
    }

    pub(crate) fn nudge_locate(&mut self, dir: i32) {
        let sr = self.sample_rate();
        let step = MixTime::frame_from_bar(self.grid.raw(), self.tempo, sr).max(1);
        self.locate_to((self.locate_frame + dir as i64 * step).max(0));
    }

    pub(crate) fn select_lane(&mut self, delta: i32) {
        let lanes: Vec<MixLane> = self.mixer_tracks().into_iter().map(|t| t.lane).collect();
        if lanes.is_empty() {
            return;
        }
        let cur =
            self.selected_lane.and_then(|l| lanes.iter().position(|&x| x == l)).unwrap_or(0) as i32;
        let next = (cur + delta).clamp(0, lanes.len() as i32 - 1) as usize;
        self.selected_lane = Some(lanes[next]);
    }

    pub(crate) fn snap_playhead_frame(&self, frame: i64) -> i64 {
        crate::arrange::snap_locate(
            frame,
            self.grid_enabled,
            self.modifiers.super_key(),
            self.grid.raw(),
            self.tempo,
            self.sample_rate(),
            self.arrangement_origin,
        )
    }

    pub(crate) fn locate_to(&mut self, frame: i64) {
        self.locate_frame = frame.max(0);
        // Paint reads this atomically; don't wait for the audio thread to apply the seek.
        self.engine_handles.sample_position.store(self.locate_frame, Ordering::Relaxed);
        let _ = self
            .engine_handles
            .cmd_tx
            .try_push(UiCommand::TransportSeek { sample: self.locate_frame });
        if let Some(player) = &self.mix_player {
            player.request_seek(self.locate_frame);
        }
    }

    pub(crate) fn write_automation_if_armed(&mut self) {
        if !self.playing || !self.automation_armed {
            return;
        }
        let frame = self.engine_handles.sample_position.load(Ordering::Relaxed);
        let lane = self.selected_lane;
        if let Some(mut mix) = self.mix.take() {
            if let Some(track) = lane.and_then(|l| mix.tracks.iter_mut().find(|t| t.lane == l)) {
                track.write_automation(MixAutomationTarget::Volume, frame, track.fader);
                track.write_automation(MixAutomationTarget::Pan, frame, track.pan);
                for (i, v) in track.knobs.clone().into_iter().enumerate() {
                    track.write_automation(MixAutomationTarget::Knob(i as i32), frame, v);
                }
            }
            self.mix = Some(mix);
        }
    }

    pub(crate) fn export_mix(&mut self) {
        let Some(mix) = self.mix.clone() else { return };
        let Some(folder) = ProjectStore::current_url(&self.analog.config) else { return };
        let sr = self._stream.as_ref().map(|s| s.sample_rate()).unwrap_or(48_000);
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

    pub(crate) fn publish_play_graph(&self) {
        let Some(player) = &self.mix_player else {
            return;
        };
        if let Some(graph) = self.build_play_graph() {
            player.publish_graph(graph);
        }
    }

    pub(crate) fn build_play_graph(&self) -> Option<crate::mix_play::MixPlayGraph> {
        let folder = ProjectStore::current_url(&self.analog.config)?;
        let tracks = self.mixer_tracks();
        let end = tracks
            .iter()
            .filter(|t| t.lane != MixLane::Main)
            .flat_map(|t| t.clips.iter())
            .map(MixClip::mix_end_frame)
            .max()
            .unwrap_or(0);
        Some(crate::mix_play::MixPlayGraph {
            folder,
            tracks: tracks
                .into_iter()
                .take(MIX_PLAY_MAX_LANES)
                .map(|t| crate::mix_play::MixPlayTrack {
                    clips: if t.lane == MixLane::Main { Vec::new() } else { t.clips },
                    is_main: t.lane == MixLane::Main,
                })
                .collect(),
            end_frame: end,
            sample_rate: self.sample_rate(),
        })
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
