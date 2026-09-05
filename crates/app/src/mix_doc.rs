//! Mix document mutations, undo, and project persist.

use std::sync::atomic::Ordering;

use analog::SessionConfig;
use project::{
    copy_clip_ids, copy_time_range, ArrSelection, MixArrangement, MixDocument, MixLane,
    MixListEntry, MixTrack, ProjectStore, TakeInfo,
};

use crate::arr_drag::MixEdit;
use crate::state::{AppState, Session, Timeline};

impl AppState {
    pub(crate) fn after_mix_edit(&mut self) {
        self.session.persist_mix(&self.surface.analog.config);
        self.publish_play_graph();
        self.publish_schedule();
    }

    pub(crate) fn mutate_mix(
        &mut self,
        title: &str,
        coalesce: bool,
        f: impl FnOnce(&mut MixDocument),
    ) {
        if self.session.mutate(title, coalesce, f) {
            self.after_mix_edit();
        }
    }

    pub(crate) fn delete_clips(&mut self) {
        if self.session.delete_clips(&mut self.timeline) {
            self.after_mix_edit();
        }
    }

    pub(crate) fn delete_selected_tracks(&mut self) {
        if self.session.delete_selected_tracks(&mut self.timeline) {
            self.after_mix_edit();
        }
    }

    pub(crate) fn delete_time(&mut self) {
        if self.session.delete_time(&mut self.timeline) {
            self.after_mix_edit();
        }
    }

    pub(crate) fn copy_selection(&mut self) {
        self.session.copy_selection(&self.timeline);
    }

    pub(crate) fn cut_clips(&mut self) {
        self.session.copy_selection(&self.timeline);
        if self.session.delete_clips(&mut self.timeline) {
            self.after_mix_edit();
        }
    }

    pub(crate) fn duplicate_clips(&mut self) {
        if self.session.duplicate_clips(&mut self.timeline) {
            self.after_mix_edit();
        }
    }

    pub(crate) fn split_clips(&mut self) {
        if self.session.split_clips(&self.timeline) {
            self.after_mix_edit();
        }
    }

    pub(crate) fn mixer_tracks(&self) -> Vec<MixTrack> {
        self.session.mixer_tracks()
    }

    pub(crate) fn mix_index_for_lane(&self, lane: MixLane) -> Option<usize> {
        self.mixer_tracks().iter().position(|t| t.lane == lane)
    }

    pub(crate) fn mixer_track_count(&self) -> usize {
        self.mixer_tracks().len()
    }

    pub(crate) fn mixer_lane_at(&self, track: usize) -> Option<MixLane> {
        self.mixer_tracks().get(track).map(|t| t.lane)
    }

    pub(crate) fn mix_track(&self, track: usize) -> Option<MixTrack> {
        self.mixer_tracks().get(track).cloned()
    }

    pub(crate) fn mix_track_mut(&mut self, track: usize) -> Option<&mut MixTrack> {
        let lane = self.mixer_lane_at(track)?;
        if self.session.viewing_take.is_some() && lane != MixLane::Main {
            return self.session.take_view.as_mut()?.iter_mut().find(|t| t.lane == lane);
        }
        self.session.mix.as_mut()?.track_mut(lane)
    }

    pub(crate) fn set_mix_fader(&mut self, track: usize, v: f32) {
        if let Some(t) = self.mix_track_mut(track) {
            t.fader = v;
        }
    }

    pub(crate) fn set_mix_pan(&mut self, track: usize, v: f32) {
        if let Some(t) = self.mix_track_mut(track) {
            t.pan = v;
        }
    }

    pub(crate) fn set_mix_knob(&mut self, track: usize, knob: usize, v: f32) {
        if let Some(t) = self.mix_track_mut(track) {
            if let Some(slot) = t.knobs.get_mut(knob) {
                *slot = v;
            }
        }
    }

    pub(crate) fn rename_mix(&mut self, id: uuid::Uuid, name: String) {
        let name = name.trim().to_string();
        if name.is_empty() {
            return;
        }
        let Some(mix) = self.session.mixes.iter_mut().find(|m| m.id == id) else {
            return;
        };
        if mix.name == name {
            return;
        }
        mix.name = name.clone();
        if let Some(folder) = ProjectStore::current_url(&self.surface.analog.config) {
            if let Err(e) = self.session.project.save_mix(mix, &folder) {
                log::warn!("save mix: {e}");
            }
        }
        if let Some(active) = self.session.mix.as_mut() {
            if active.id == id {
                active.name = name;
            }
        }
        self.persist_project_meta();
    }

    pub(crate) fn rename_take(&mut self, number: i32, name: String) {
        let name = name.trim().to_string();
        if name.is_empty() {
            self.session.take_names.remove(&number);
        } else {
            self.session.take_names.insert(number, name);
        }
        self.persist_project_meta();
    }

    pub(crate) fn new_mix(&mut self) {
        let n = self.session.mixes.len() + 1;
        let mut mix =
            MixDocument::empty(format!("Mix {n}"), self.surface.analog.config.effect_return_count);
        if let Some(take) = self.session.viewing_take {
            mix.start_frame = self.take_start_from_store(take);
        }
        if let Some(folder) = ProjectStore::current_url(&self.surface.analog.config) {
            if let Err(e) = self.session.project.save_mix(&mix, &folder) {
                log::warn!("save mix: {e}");
            }
        }
        let id = mix.id;
        self.session.mixes.push(mix);
        self.select_mix(id);
    }

    pub(crate) fn start_from_take(&mut self, number: i32) {
        let Some(info) = self.session.take_infos.iter().find(|t| t.number == number).cloned()
        else {
            return;
        };
        let origin = self.take_start_from_store(number);
        let send_count = self.surface.analog.config.effect_return_count;
        let id = self.session.start_from_take(&info, origin, send_count);
        if let Some(folder) = ProjectStore::current_url(&self.surface.analog.config) {
            if let Some(mix) = self.session.mixes.iter().find(|m| m.id == id) {
                if let Err(e) = self.session.project.save_mix(mix, &folder) {
                    log::warn!("save mix: {e}");
                }
            }
        }
        self.select_mix(id);
    }

    pub(crate) fn copy_take_to_clipboard(&mut self, number: i32) {
        self.select_take(number);
        let Some(info) = self.session.take_infos.iter().find(|t| t.number == number) else {
            return;
        };
        let tracks: Vec<_> = info
            .arrangement_tracks()
            .into_iter()
            .filter(|track| crate::record::lane_on_record_list(&self.surface.analog, track.lane))
            .collect();
        let ids: Vec<_> = tracks.iter().flat_map(|t| t.clips.iter().map(|c| c.id)).collect();
        self.session.pasteboard = copy_clip_ids(&tracks, &ids);
    }

    pub(crate) fn delete_take(&mut self, number: i32) {
        if !crate::native::confirm_delete_take() {
            return;
        }
        let Some(folder) = ProjectStore::current_url(&self.surface.analog.config) else {
            return;
        };
        let deleted = project::delete_take_files(&folder, number);
        for name in &deleted {
            self.chrome.waveforms.note_file_finished(name);
        }
        self.session.take_infos = crate::record::list_take_infos(&folder, self.audio.sample_rate());
        self.session.takes = self.session.take_infos.iter().map(|t| t.number).collect();
        self.session.take_names.remove(&number);
        if self.session.viewing_take == Some(number) && !self.session.takes.contains(&number) {
            if let Some(id) = self
                .session
                .mix
                .as_ref()
                .map(|m| m.id)
                .or_else(|| self.session.mixes.first().map(|m| m.id))
            {
                self.select_mix(id);
            } else {
                if self.audio.playing {
                    self.halt_mix_play();
                }
                self.session.viewing_take = None;
                self.session.take_view = None;
                self.sync_origin();
                self.persist_project_meta();
                self.publish_schedule();
            }
        }
    }

    pub(crate) fn delete_mix_id(&mut self, id: uuid::Uuid) {
        if !crate::native::confirm_delete_mix() {
            return;
        }
        if let Some(folder) = ProjectStore::current_url(&self.surface.analog.config) {
            let path = self.session.project.mix_url(id, &folder);
            let _ = std::fs::remove_file(path);
        }
        self.session.mixes.retain(|m| m.id != id);
        if self.session.mix.as_ref().is_some_and(|m| m.id == id) {
            self.session.mix = None;
        }
        if self.session.mix.is_none() && self.session.viewing_take.is_none() {
            if let Some(first) = self.session.mixes.first().map(|m| m.id) {
                self.select_mix(first);
            }
        }
        self.persist_project_meta();
    }

    pub(crate) fn persist_mix(&mut self) {
        self.session.persist_mix(&self.surface.analog.config);
    }

    pub(crate) fn apply_undo(&mut self, redo: bool) {
        if self.session.mix.is_none() {
            return;
        }
        if self.session.apply_undo(redo) {
            self.after_mix_edit();
        } else {
            self.publish_schedule();
        }
    }

    pub(crate) fn paste_clips(&mut self) {
        let leaving_take = self.session.viewing_take.is_some();
        if leaving_take {
            if self.session.mix.is_none() {
                return;
            }
            self.session.viewing_take = None;
            self.session.take_view = None;
            self.sync_origin();
            self.locate_to(self.timeline.arrangement_origin.max(0));
            self.persist_project_meta();
        }
        let playhead = self.audio.engine_handles.sample_position.load(Ordering::Relaxed);
        if self.session.paste_clips(&mut self.timeline, leaving_take, self.audio.playing, playhead)
        {
            self.after_mix_edit();
        }
    }

    pub(crate) fn arrangement_tracks(&self) -> Vec<MixTrack> {
        self.session.arrangement_tracks()
    }

    pub(crate) fn select_mix(&mut self, id: uuid::Uuid) {
        if self.audio.playing {
            self.halt_mix_play();
        }
        let leaving_take = self.session.viewing_take.is_some();
        self.session.mix = self.session.mixes.iter().find(|m| m.id == id).cloned();
        let takes = self.session.take_infos.clone();
        if let Some(mix) = &mut self.session.mix {
            mix.backfill_source_totals(&takes);
        }
        self.session.viewing_take = None;
        self.session.take_view = None;
        self.timeline.selection.clear();
        self.timeline.clip_preview = None;
        self.sync_origin();
        if leaving_take {
            self.locate_to(self.timeline.arrangement_origin.max(0));
        }
        self.persist_project_meta();
        self.publish_schedule();
    }

    pub(crate) fn select_take(&mut self, number: i32) {
        if self.audio.playing {
            self.halt_mix_play();
        }
        self.session.viewing_take = Some(number);
        self.timeline.selection.clear();
        self.timeline.clip_preview = None;
        self.session.take_view =
            self.session.take_infos.iter().find(|t| t.number == number).map(|info| {
                info.arrangement_tracks()
                    .into_iter()
                    .filter(|track| {
                        crate::record::lane_on_record_list(&self.surface.analog, track.lane)
                    })
                    .collect()
            });
        self.sync_origin();
        self.locate_to(self.timeline.arrangement_origin.max(0));
        self.persist_project_meta();
    }

    pub(crate) fn take_start_from_store(&self, number: i32) -> i64 {
        ProjectStore::current_url(&self.surface.analog.config)
            .map(|folder| self.session.project.load_meta(&folder).take_start_frame(number))
            .unwrap_or(0)
    }

    pub(crate) fn sync_origin(&mut self) {
        self.timeline.arrangement_origin = if let Some(n) = self.session.viewing_take {
            self.take_start_from_store(n)
        } else {
            self.session.mix.as_ref().map(|m| m.start_frame.max(0)).unwrap_or(0)
        };
    }

    pub(crate) fn arrangement_length(&self) -> i64 {
        if let Some(n) = self.session.viewing_take {
            self.session
                .take_infos
                .iter()
                .find(|t| t.number == n)
                .map(|t| t.frame_count())
                .unwrap_or(0)
        } else {
            self.session.mix.as_ref().map(|m| m.last_clip_end()).unwrap_or(0)
        }
    }

    pub(crate) fn clamp_arrangement_start(&self, frame: i64) -> i64 {
        let length = self.arrangement_length();
        if length <= 0 {
            return frame.max(0);
        }
        frame.max(0).min(length - 1)
    }

    pub(crate) fn set_arrangement_start(&mut self, frame: i64) {
        let origin = self.clamp_arrangement_start(frame);
        self.timeline.arrangement_origin = origin;
        if self.session.viewing_take.is_none() {
            if let Some(mix) = self.session.mix.as_mut() {
                mix.start_frame = origin;
            }
        }
    }

    pub(crate) fn persist_start(&mut self) {
        self.set_arrangement_start(self.timeline.arrangement_origin);
        if self.session.viewing_take.is_some() {
            self.persist_project_meta();
        } else {
            self.persist_mix();
        }
        self.locate_to(self.timeline.arrangement_origin);
    }

    pub(crate) fn persist_project_meta(&mut self) {
        let Some(folder) = ProjectStore::current_url(&self.surface.analog.config) else {
            return;
        };
        let mut meta = self.session.project.load_meta(&folder);
        meta.tempo = self.timeline.tempo;
        meta.grid = self.timeline.grid;
        meta.grid_enabled = self.timeline.grid_enabled;
        meta.pixels_per_bar = self.timeline.pixels_per_bar as f64;
        meta.selected_lane = self.timeline.selected_lane;
        meta.active_mix_id = self.session.mix.as_ref().map(|m| m.id);
        meta.mixes = self
            .session
            .mixes
            .iter()
            .map(|m| MixListEntry { id: m.id, name: m.name.clone() })
            .collect();
        meta.take_names = self
            .session
            .take_names
            .iter()
            .filter(|(_, name)| !name.is_empty())
            .map(|(n, name)| (n.to_string(), name.clone()))
            .collect();
        meta.arrangement = if let Some(n) = self.session.viewing_take {
            Some(MixArrangement::Take(n))
        } else {
            self.session.mix.as_ref().map(|m| MixArrangement::Mix(m.id))
        };
        if let Some(n) = self.session.viewing_take {
            meta.set_take_start_frame(n, self.timeline.arrangement_origin);
        }
        let _ = self.session.project.save_meta(&meta, &folder);
    }

    /// MixLink `MixStore.load`: sidecar + filesystem takes. No default mix.

    pub(crate) fn reload_mix(&mut self) {
        if self.audio.playing {
            self.halt_mix_play();
        }
        let Some(folder) = ProjectStore::current_url(&self.surface.analog.config) else {
            if self.surface.analog.config.projects_root_bookmark.is_some() {
                log::warn!(
                    "project: bookmark did not resolve (relative {:?})",
                    self.surface.analog.config.current_project_relative
                );
            }
            self.session.takes.clear();
            self.session.take_names.clear();
            self.session.take_infos.clear();
            self.session.take_view = None;
            self.session.mixes.clear();
            self.session.mix = None;
            return;
        };
        if !folder.exists() {
            log::warn!("project: folder missing {}", folder.display());
        }
        let sr = self.audio._stream.as_ref().map(|s| s.sample_rate() as f64).unwrap_or(48_000.0);
        let meta = self.session.project.load_meta(&folder);
        self.set_tempo(meta.tempo);
        self.timeline.grid = meta.grid;
        self.timeline.grid_enabled = meta.grid_enabled;
        self.timeline.pixels_per_bar = meta.pixels_per_bar as f32;
        self.timeline.selected_lane = meta.selected_lane;
        self.session.take_number = self.session.project.next_take(&folder);
        self.session.take_infos = crate::record::list_take_infos(&folder, sr);
        self.session.takes = self.session.take_infos.iter().map(|t| t.number).collect();
        self.session.take_names = meta
            .take_names
            .iter()
            .filter_map(|(k, v)| k.parse().ok().filter(|_| !v.is_empty()).map(|n| (n, v.clone())))
            .collect();

        self.session.mixes.clear();
        for entry in &meta.mixes {
            if let Some(doc) = self.session.project.load_mix(entry.id, &folder) {
                self.session.mixes.push(doc);
            }
        }
        if let Ok(rd) = std::fs::read_dir(&folder) {
            for ent in rd.flatten() {
                let path = ent.path();
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name.starts_with("mix-")
                    && path.extension().and_then(|e| e.to_str()) == Some("json")
                {
                    if let Ok(data) = std::fs::read(&path) {
                        if let Ok(doc) = serde_json::from_slice::<MixDocument>(&data) {
                            if !self.session.mixes.iter().any(|m| m.id == doc.id) {
                                self.session.mixes.push(doc);
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
            .and_then(|id| self.session.mixes.iter().find(|m| m.id == id).cloned())
            .or_else(|| self.session.mixes.first().cloned());
        self.session.mix = active;
        match meta.arrangement {
            Some(MixArrangement::Take(n)) if self.session.takes.contains(&n) => {
                self.select_take(n);
            }
            Some(MixArrangement::Mix(id)) => {
                if self.session.mixes.iter().any(|m| m.id == id) {
                    self.session.mix = self.session.mixes.iter().find(|m| m.id == id).cloned();
                }
                self.session.viewing_take = None;
                self.session.take_view = None;
            }
            _ => {
                self.session.viewing_take = None;
                self.session.take_view = None;
            }
        }
        self.sync_origin();
        log::info!(
            "project {} — {} mix(es), take(s) {:?}",
            folder.display(),
            self.session.mixes.len(),
            self.session.takes
        );
    }
}

impl Session {
    /// Always allocate a new mix document from the take. Never reuse or overwrite
    /// a mix that was already started from a take.
    pub(crate) fn start_from_take(
        &mut self,
        info: &TakeInfo,
        origin: i64,
        send_count: i32,
    ) -> uuid::Uuid {
        let n = self.mixes.len() + 1;
        let mut mix = MixDocument::empty(format!("Mix {n}"), send_count);
        mix.load_from_take(info, origin);
        let id = mix.id;
        self.mixes.push(mix);
        id
    }

    #[must_use]
    pub(crate) fn mutate(
        &mut self,
        title: &str,
        coalesce: bool,
        f: impl FnOnce(&mut MixDocument),
    ) -> bool {
        if !self.is_editing_mix() {
            return false;
        }
        let Some(mut mix) = self.mix.take() else {
            return false;
        };
        let changed = self.undo.mutate(title, coalesce, &mut mix, f);
        self.mix = Some(mix);
        changed
    }

    pub(crate) fn persist_mix(&mut self, config: &SessionConfig) {
        let takes = self.take_infos.clone();
        if let Some(mix) = self.mix.as_mut() {
            mix.backfill_source_totals(&takes);
        }
        let Some(mix) = &self.mix else { return };
        if let Some(folder) = ProjectStore::current_url(config) {
            if let Err(e) = self.project.save_mix(mix, &folder) {
                log::warn!("save mix: {e}");
            }
        }
        if let Some(cur) = &self.mix {
            if let Some(slot) = self.mixes.iter_mut().find(|m| m.id == cur.id) {
                *slot = cur.clone();
            }
        }
    }

    pub(crate) fn apply_undo(&mut self, redo: bool) -> bool {
        let Some(current) = self.mix.take() else {
            return false;
        };
        let empty = if redo { !self.undo.can_redo() } else { !self.undo.can_undo() };
        if empty {
            self.mix = Some(current);
            return false;
        }
        let restored = if redo { self.undo.redo(current) } else { self.undo.undo(current) };
        if let Some((_, doc)) = restored {
            self.mix = Some(doc);
            true
        } else {
            false
        }
    }

    pub(crate) fn delete_clips(&mut self, timeline: &mut Timeline) -> bool {
        if !self.is_editing_mix() {
            return false;
        }
        if timeline.selection.has_range() && timeline.selection.clips.is_empty() {
            let (start, end) = timeline.selection.range();
            let lanes = timeline.selection.lanes.clone();
            return self.mutate("Delete", false, |doc| {
                for lane in &lanes {
                    doc.clear_range(*lane, start, end);
                }
            });
        }
        let ids = timeline.selection.clips.clone();
        if !ids.is_empty() {
            let changed = self.mutate("Delete", false, |doc| doc.remove_clips(&ids));
            timeline.selection.clips.clear();
            return changed;
        }
        self.delete_selected_tracks(timeline)
    }

    pub(crate) fn delete_selected_tracks(&mut self, timeline: &mut Timeline) -> bool {
        let mut lanes: Vec<MixLane> =
            timeline.selection.lanes.iter().copied().filter(|l| *l != MixLane::Main).collect();
        if lanes.is_empty() {
            if let Some(lane) = timeline.selected_lane.filter(|l| *l != MixLane::Main) {
                lanes.push(lane);
            }
        }
        let mut unique = Vec::new();
        for lane in lanes {
            if !unique.contains(&lane) {
                unique.push(lane);
            }
        }
        let lanes = unique;
        if lanes.is_empty() {
            return false;
        }
        let changed = self.mutate("Delete track", false, |doc| doc.remove_lanes(&lanes));
        timeline.selection.clear();
        let remaining = self.arrangement_tracks();
        timeline.selected_lane =
            remaining.iter().map(|t| t.lane).find(|l| *l != MixLane::Main).or(Some(MixLane::Main));
        changed
    }

    pub(crate) fn delete_time(&mut self, timeline: &Timeline) -> bool {
        if !self.is_editing_mix() || !timeline.selection.has_range() {
            return false;
        }
        let (start, end) = timeline.selection.range();
        let lanes = timeline.selection.lanes.clone();
        self.mutate("Delete time", false, |doc| doc.delete_time(&lanes, start, end))
    }

    pub(crate) fn copy_selection(&mut self, timeline: &Timeline) {
        let tracks = self.arrangement_tracks();
        let mut lanes = timeline.selection.lanes.clone();
        if lanes.is_empty() {
            if let Some(lane) = timeline.selected_lane.filter(|l| *l != MixLane::Main) {
                lanes.push(lane);
            }
        }
        let board = if timeline.selection.has_range() {
            copy_time_range(&tracks, &lanes, timeline.selection.start, timeline.selection.end)
                .or_else(|| copy_clip_ids(&tracks, &timeline.selection.clips))
        } else {
            copy_clip_ids(&tracks, &timeline.selection.clips)
        };
        if board.is_some() {
            self.pasteboard = board;
        }
    }

    pub(crate) fn duplicate_clips(&mut self, timeline: &mut Timeline) -> bool {
        if !self.is_editing_mix() {
            return false;
        }
        if !timeline.selection.has_range() {
            self.copy_selection(timeline);
            let Some(board) = self.pasteboard.clone() else {
                return false;
            };
            let at = board
                .entries
                .iter()
                .map(|e| e.clip.mix_end_frame())
                .max()
                .unwrap_or(timeline.locate_frame);
            return self.mutate("Duplicate", false, |doc| {
                let mut placements = Vec::new();
                for entry in &board.entries {
                    let mut clip = entry.clip.clone();
                    clip.mix_start_frame = at + (entry.clip.mix_start_frame - board.origin);
                    placements.push((entry.from_lane, clip));
                }
                doc.insert_clips(&placements);
            });
        }
        let (start, end) = timeline.selection.range();
        let mut lanes = timeline.selection.lanes.clone();
        if lanes.is_empty() {
            if let Some(lane) = timeline.selected_lane.filter(|l| *l != MixLane::Main) {
                lanes.push(lane);
            }
        }
        if lanes.is_empty() {
            return false;
        }
        let mut placed = None;
        let changed = self.mutate("Duplicate", false, |doc| {
            placed = doc.duplicate_range(&lanes, start, end);
        });
        if let Some((a, b, dest_lanes)) = placed {
            timeline.selection.start = a;
            timeline.selection.end = b;
            timeline.selection.lanes = dest_lanes;
            timeline.selection.clips.clear();
        }
        changed
    }

    pub(crate) fn split_clips(&mut self, timeline: &Timeline) -> bool {
        if !self.is_editing_mix() {
            return false;
        }
        let lanes = if timeline.selection.lanes.is_empty() {
            timeline.selected_lane.into_iter().collect()
        } else {
            timeline.selection.lanes.clone()
        };
        let frame = timeline.locate_frame;
        self.mutate("Split", false, |doc| doc.split_at(frame, &lanes))
    }

    pub(crate) fn apply_mix_edit(&mut self, edit: MixEdit, timeline: &mut Timeline) -> bool {
        match edit {
            MixEdit::Move { copy, ids, preview } => {
                let changed =
                    self.mutate(if copy { "Copy clip" } else { "Move clip" }, false, |doc| {
                        if !copy {
                            doc.remove_clips(&ids);
                        }
                        doc.insert_clips(&preview);
                    });
                timeline.selection.clips = preview
                    .iter()
                    .filter_map(|(lane, clip)| {
                        self.mix.as_ref().and_then(|m| {
                            m.track(*lane)
                                .and_then(|t| {
                                    t.clips.iter().find(|c| {
                                        c.mix_start_frame == clip.mix_start_frame
                                            && c.source_file == clip.source_file
                                            && c.source_start_frame == clip.source_start_frame
                                    })
                                })
                                .map(|c| c.id)
                        })
                    })
                    .collect();
                timeline.selection.start = 0;
                timeline.selection.end = 0;
                changed
            }
            MixEdit::Trim { id, lane, next } => self.mutate("Trim clip", false, |doc| {
                doc.remove_clips(&[id]);
                doc.insert_clips(&[(lane, next)]);
            }),
            MixEdit::Fade { id, fade_in, fade_out } => self.mutate("Fade", false, |doc| {
                if let Some(clip) =
                    doc.tracks.iter_mut().flat_map(|t| t.clips.iter_mut()).find(|c| c.id == id)
                {
                    clip.fade_in_frames = fade_in;
                    clip.fade_out_frames = fade_out;
                }
            }),
            MixEdit::Loop { id, lane, next } => self.mutate("Loop clip", false, |doc| {
                doc.remove_clips(&[id]);
                doc.insert_clips(&[(lane, next)]);
            }),
            MixEdit::Slip { id, source_start } => self.mutate("Slip clip", false, |doc| {
                if let Some(clip) =
                    doc.tracks.iter_mut().flat_map(|t| t.clips.iter_mut()).find(|c| c.id == id)
                {
                    clip.source_start_frame = source_start;
                }
            }),
        }
    }

    pub(crate) fn paste_clips(
        &mut self,
        timeline: &mut Timeline,
        leaving_take: bool,
        playing: bool,
        playhead: i64,
    ) -> bool {
        if self.pasteboard.as_ref().is_none_or(|b| b.entries.is_empty()) {
            return false;
        }
        if !self.is_editing_mix() {
            return false;
        }
        let Some(board) = self.pasteboard.clone() else {
            return false;
        };
        let dest =
            timeline.selected_lane.filter(|l| *l != MixLane::Main).unwrap_or(MixLane::Strip(0));
        if dest == MixLane::Main {
            return false;
        }
        let at = crate::arrange::mix_paste_frame(
            leaving_take,
            self.mix.as_ref().map(|m| m.start_frame).unwrap_or(0),
            playing,
            playhead,
            timeline.locate_frame,
        );
        let changed = self.mutate("Paste", false, |doc| doc.paste_clips(&board, dest, at));
        let origin = board.origin;
        let start = board
            .entries
            .iter()
            .map(|e| at + (e.clip.mix_start_frame - origin))
            .min()
            .unwrap_or(at);
        let end = board
            .entries
            .iter()
            .map(|e| at + (e.clip.mix_end_frame() - origin))
            .max()
            .unwrap_or(at);
        let lanes = if board.single_lane() { vec![dest] } else { board.lanes() };
        timeline.selection = ArrSelection { lanes, start, end, clips: Vec::new() };
        changed
    }

    pub(crate) fn is_editing_mix(&self) -> bool {
        self.viewing_take.is_none() && self.mix.is_some()
    }

    pub(crate) fn mixer_tracks(&self) -> Vec<MixTrack> {
        let mut tracks = if self.viewing_take.is_some() {
            self.take_view.clone().unwrap_or_default()
        } else {
            self.mix.as_ref().map(MixDocument::channel_tracks).unwrap_or_default()
        };
        let mut main = self
            .mix
            .as_ref()
            .and_then(|m| m.tracks.iter().find(|t| t.lane == MixLane::Main).cloned())
            .unwrap_or_else(|| MixTrack::empty(MixLane::Main, Some("Main".into())));
        main.clips.clear();
        main.name = "Main".into();
        tracks.push(main);
        tracks
    }

    pub(crate) fn arrangement_tracks(&self) -> Vec<MixTrack> {
        if self.viewing_take.is_some() {
            self.take_view.clone().unwrap_or_default()
        } else {
            self.mix.as_ref().map(MixDocument::channel_tracks).unwrap_or_default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use project::{MixClip, TakeFile, TakeInfo};

    fn take_info(number: i32, frames: i64) -> TakeInfo {
        TakeInfo {
            number,
            files: vec![TakeFile {
                lane: MixLane::Strip(0),
                name: "Rytm".into(),
                filename: format!("{number}-ch-01-Rytm.wav"),
                frame_count: frames,
                sample_rate: 48_000.0,
            }],
        }
    }

    fn clip(start: i64, count: i64) -> MixClip {
        MixClip::new(1, MixLane::Strip(0), "take-1.wav", 0, count, start, count)
    }

    fn session_with_clip() -> (Session, Timeline, uuid::Uuid) {
        let mut doc = MixDocument::empty("Test", 2);
        doc.insert_clips(&[(MixLane::Strip(0), clip(0, 48_000))]);
        let id = doc.track(MixLane::Strip(0)).unwrap().clips[0].id;
        let session = Session { mix: Some(doc), ..Session::default() };
        let mut timeline = Timeline::default();
        timeline.selected_lane = Some(MixLane::Strip(0));
        (session, timeline, id)
    }

    #[test]
    fn delete_clips_removes_selected_ids() {
        let (mut session, mut timeline, id) = session_with_clip();
        timeline.selection.clips = vec![id];
        assert!(session.delete_clips(&mut timeline));
        assert!(session.mix.as_ref().unwrap().track(MixLane::Strip(0)).unwrap().clips.is_empty());
        assert!(timeline.selection.clips.is_empty());
    }

    #[test]
    fn delete_clips_clears_time_range() {
        let (mut session, mut timeline, _) = session_with_clip();
        timeline.selection.start = 0;
        timeline.selection.end = 48_000;
        timeline.selection.lanes = vec![MixLane::Strip(0)];
        assert!(session.delete_clips(&mut timeline));
        assert!(session.mix.as_ref().unwrap().track(MixLane::Strip(0)).unwrap().clips.is_empty());
    }

    #[test]
    fn delete_clips_falls_through_to_track() {
        let (mut session, mut timeline, _) = session_with_clip();
        timeline.selected_lane = Some(MixLane::Strip(0));
        assert!(session.delete_clips(&mut timeline));
        assert!(session.mix.as_ref().unwrap().track(MixLane::Strip(0)).is_none());
        assert_ne!(timeline.selected_lane, Some(MixLane::Strip(0)));
        assert!(timeline.selected_lane.is_some());
    }

    #[test]
    fn paste_clips_uses_selected_destination() {
        let (mut session, mut timeline, id) = session_with_clip();
        timeline.selection.clips = vec![id];
        session.copy_selection(&timeline);
        timeline.selected_lane = Some(MixLane::Strip(1));
        timeline.locate_frame = 96_000;
        assert!(session.paste_clips(&mut timeline, false, false, 0));
        assert_eq!(timeline.selection.lanes, vec![MixLane::Strip(1)]);
        let dest = session.mix.as_ref().unwrap().track(MixLane::Strip(1)).unwrap();
        assert_eq!(dest.clips.len(), 1);
        assert_eq!(dest.clips[0].mix_start_frame, 96_000);
    }

    #[test]
    fn start_from_take_creates_a_new_mix_each_time() {
        let take = take_info(7, 1000);
        let mut existing = MixDocument::empty("Mix 1", 2);
        existing.load_from_take(&take, 100);
        let existing_id = existing.id;
        let existing_clip = existing.track(MixLane::Strip(0)).unwrap().clips[0].clone();

        let mut session = Session {
            mix: Some(existing.clone()),
            mixes: vec![existing],
            viewing_take: Some(7),
            ..Session::default()
        };

        let first = session.start_from_take(&take, 100, 2);
        assert_ne!(first, existing_id);
        assert_eq!(session.mixes.len(), 2);
        assert_eq!(session.mix.as_ref().map(|m| m.id), Some(existing_id));
        let created = session.mixes.iter().find(|m| m.id == first).unwrap();
        assert_eq!(created.name, "Mix 2");
        assert_eq!(created.origin_take, Some(7));
        assert_eq!(created.start_frame, 0);
        let clip = &created.track(MixLane::Strip(0)).unwrap().clips[0];
        assert_eq!(clip.source_take, 7);
        assert_eq!(clip.source_start_frame, 100);
        assert_eq!(clip.source_frame_count, 900);

        let original = session.mixes.iter().find(|m| m.id == existing_id).unwrap();
        assert_eq!(original.origin_take, Some(7));
        assert_eq!(original.track(MixLane::Strip(0)).unwrap().clips[0].id, existing_clip.id);
        assert_eq!(
            original.track(MixLane::Strip(0)).unwrap().clips[0].source_start_frame,
            existing_clip.source_start_frame
        );

        let second = session.start_from_take(&take, 200, 2);
        assert_ne!(second, first);
        assert_ne!(second, existing_id);
        assert_eq!(session.mixes.len(), 3);
        let created = session.mixes.iter().find(|m| m.id == second).unwrap();
        assert_eq!(created.name, "Mix 3");
        assert_eq!(created.origin_take, Some(7));
        let clip = &created.track(MixLane::Strip(0)).unwrap().clips[0];
        assert_eq!(clip.source_start_frame, 200);
        assert_eq!(clip.source_frame_count, 800);
        assert_eq!(session.mixes.iter().filter(|m| m.origin_take == Some(7)).count(), 3);
    }

    #[test]
    fn apply_undo_restores_document() {
        let (mut session, mut timeline, id) = session_with_clip();
        timeline.selection.clips = vec![id];
        assert!(session.delete_clips(&mut timeline));
        assert!(session.mix.as_ref().unwrap().track(MixLane::Strip(0)).unwrap().clips.is_empty());
        assert!(session.apply_undo(false));
        assert_eq!(session.mix.as_ref().unwrap().track(MixLane::Strip(0)).unwrap().clips.len(), 1);
        assert!(session.apply_undo(true));
        assert!(session.mix.as_ref().unwrap().track(MixLane::Strip(0)).unwrap().clips.is_empty());
    }
}
