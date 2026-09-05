//! Mix document mutations, undo, and project persist.

use std::sync::atomic::Ordering;

use project::{
    copy_clip_ids, copy_time_range, ArrSelection, MixArrangement, MixDocument, MixLane,
    MixListEntry, MixTrack, ProjectStore,
};

use crate::state::AppState;

impl AppState {
    pub(crate) fn mutate_mix(
        &mut self,
        title: &str,
        coalesce: bool,
        f: impl FnOnce(&mut MixDocument),
    ) {
        if !self.is_editing_mix() {
            return;
        }
        let Some(mut mix) = self.mix.take() else {
            return;
        };
        self.undo.mutate(title, coalesce, &mut mix, f);
        self.mix = Some(mix);
        self.persist_mix();
        self.publish_play_graph();
        self.publish_schedule();
    }

    pub(crate) fn delete_clips(&mut self) {
        if !self.is_editing_mix() {
            return;
        }
        if self.selection.has_range() && self.selection.clips.is_empty() {
            let (start, end) = self.selection.range();
            let lanes = self.selection.lanes.clone();
            self.mutate_mix("Delete", false, |doc| {
                for lane in &lanes {
                    doc.clear_range(*lane, start, end);
                }
            });
            return;
        }
        let ids = self.selection.clips.clone();
        if !ids.is_empty() {
            self.mutate_mix("Delete", false, |doc| doc.remove_clips(&ids));
            self.selection.clips.clear();
            return;
        }
        self.delete_selected_tracks();
    }

    pub(crate) fn delete_selected_tracks(&mut self) {
        let mut lanes: Vec<MixLane> =
            self.selection.lanes.iter().copied().filter(|l| *l != MixLane::Main).collect();
        if lanes.is_empty() {
            if let Some(lane) = self.selected_lane.filter(|l| *l != MixLane::Main) {
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
            return;
        }
        self.mutate_mix("Delete track", false, |doc| doc.remove_lanes(&lanes));
        self.selection.clear();
        let remaining = self.arrangement_tracks();
        self.selected_lane =
            remaining.iter().map(|t| t.lane).find(|l| *l != MixLane::Main).or(Some(MixLane::Main));
    }

    pub(crate) fn delete_time(&mut self) {
        if !self.is_editing_mix() || !self.selection.has_range() {
            return;
        }
        let (start, end) = self.selection.range();
        let lanes = self.selection.lanes.clone();
        self.mutate_mix("Delete time", false, |doc| doc.delete_time(&lanes, start, end));
    }

    pub(crate) fn copy_selection(&mut self) {
        let tracks = self.arrangement_tracks();
        let mut lanes = self.selection.lanes.clone();
        if lanes.is_empty() {
            if let Some(lane) = self.selected_lane.filter(|l| *l != MixLane::Main) {
                lanes.push(lane);
            }
        }
        let board = if self.selection.has_range() {
            copy_time_range(&tracks, &lanes, self.selection.start, self.selection.end)
                .or_else(|| copy_clip_ids(&tracks, &self.selection.clips))
        } else {
            copy_clip_ids(&tracks, &self.selection.clips)
        };
        if board.is_some() {
            self.pasteboard = board;
        }
    }

    pub(crate) fn cut_clips(&mut self) {
        if !self.is_editing_mix() {
            return;
        }
        self.copy_selection();
        self.delete_clips();
    }

    pub(crate) fn duplicate_clips(&mut self) {
        if !self.is_editing_mix() {
            return;
        }
        if !self.selection.has_range() {
            self.copy_selection();
            let Some(board) = self.pasteboard.clone() else {
                return;
            };
            let at = board
                .entries
                .iter()
                .map(|e| e.clip.mix_end_frame())
                .max()
                .unwrap_or(self.locate_frame);
            self.mutate_mix("Duplicate", false, |doc| {
                let mut placements = Vec::new();
                for entry in &board.entries {
                    let mut clip = entry.clip.clone();
                    clip.mix_start_frame = at + (entry.clip.mix_start_frame - board.origin);
                    placements.push((entry.from_lane, clip));
                }
                doc.insert_clips(&placements);
            });
            return;
        }
        let (start, end) = self.selection.range();
        let mut lanes = self.selection.lanes.clone();
        if lanes.is_empty() {
            if let Some(lane) = self.selected_lane.filter(|l| *l != MixLane::Main) {
                lanes.push(lane);
            }
        }
        if lanes.is_empty() {
            return;
        }
        let mut placed = None;
        self.mutate_mix("Duplicate", false, |doc| {
            placed = doc.duplicate_range(&lanes, start, end);
        });
        if let Some((a, b, dest_lanes)) = placed {
            self.selection.start = a;
            self.selection.end = b;
            self.selection.lanes = dest_lanes;
            self.selection.clips.clear();
        }
    }

    pub(crate) fn split_clips(&mut self) {
        if !self.is_editing_mix() {
            return;
        }
        let lanes = if self.selection.lanes.is_empty() {
            self.selected_lane.into_iter().collect()
        } else {
            self.selection.lanes.clone()
        };
        let frame = self.locate_frame;
        self.mutate_mix("Split", false, |doc| doc.split_at(frame, &lanes));
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
        if self.viewing_take.is_some() && lane != MixLane::Main {
            return self.take_view.as_mut()?.iter_mut().find(|t| t.lane == lane);
        }
        self.mix.as_mut()?.track_mut(lane)
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

    pub(crate) fn new_mix(&mut self) {
        let n = self.mixes.len() + 1;
        let mut mix =
            MixDocument::empty(format!("Mix {n}"), self.analog.config.effect_return_count);
        if let Some(take) = self.viewing_take {
            mix.start_frame = self.take_start_from_store(take);
        }
        if let Some(folder) = ProjectStore::current_url(&self.analog.config) {
            if let Err(e) = self.project.save_mix(&mix, &folder) {
                log::warn!("save mix: {e}");
            }
        }
        let id = mix.id;
        self.mixes.push(mix);
        self.select_mix(id);
    }

    pub(crate) fn start_from_take(&mut self, number: i32) {
        let Some(info) = self.take_infos.iter().find(|t| t.number == number).cloned() else {
            return;
        };
        if self.mix.is_none() {
            if let Some(id) = self.mixes.first().map(|m| m.id) {
                self.select_mix(id);
            } else {
                self.new_mix();
            }
        }
        let origin = self.take_start_from_store(number);
        let Some(mut mix) = self.mix.take() else { return };
        self.undo.mutate("Start from take", false, &mut mix, |doc| {
            doc.load_from_take(&info, origin);
        });
        let id = mix.id;
        self.mix = Some(mix);
        self.persist_mix();
        self.select_mix(id);
    }

    pub(crate) fn copy_take_to_clipboard(&mut self, number: i32) {
        self.select_take(number);
        let Some(info) = self.take_infos.iter().find(|t| t.number == number) else {
            return;
        };
        let tracks: Vec<_> = info
            .arrangement_tracks()
            .into_iter()
            .filter(|track| crate::record::lane_on_record_list(&self.analog, track.lane))
            .collect();
        let ids: Vec<_> = tracks.iter().flat_map(|t| t.clips.iter().map(|c| c.id)).collect();
        self.pasteboard = copy_clip_ids(&tracks, &ids);
    }

    pub(crate) fn delete_mix_id(&mut self, id: uuid::Uuid) {
        if !crate::native::confirm_delete_mix() {
            return;
        }
        if let Some(folder) = ProjectStore::current_url(&self.analog.config) {
            let path = self.project.mix_url(id, &folder);
            let _ = std::fs::remove_file(path);
        }
        self.mixes.retain(|m| m.id != id);
        if self.mix.as_ref().is_some_and(|m| m.id == id) {
            self.mix = None;
        }
        if self.mix.is_none() && self.viewing_take.is_none() {
            if let Some(first) = self.mixes.first().map(|m| m.id) {
                self.select_mix(first);
            }
        }
        self.persist_project_meta();
    }

    pub(crate) fn persist_mix(&mut self) {
        let takes = self.take_infos.clone();
        if let Some(mix) = self.mix.as_mut() {
            mix.backfill_source_totals(&takes);
        }
        let Some(mix) = &self.mix else { return };
        if let Some(folder) = ProjectStore::current_url(&self.analog.config) {
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

    pub(crate) fn apply_undo(&mut self, redo: bool) {
        let Some(current) = self.mix.take() else { return };
        let restored = if redo { self.undo.redo(current) } else { self.undo.undo(current) };
        if let Some((_, doc)) = restored {
            self.mix = Some(doc);
            self.persist_mix();
            self.publish_play_graph();
        }
        self.publish_schedule();
    }

    pub(crate) fn paste_clips(&mut self) {
        if self.pasteboard.as_ref().is_none_or(|b| b.entries.is_empty()) {
            return;
        }
        let leaving_take = self.viewing_take.is_some();
        if leaving_take {
            if self.mix.is_none() {
                return;
            }
            self.viewing_take = None;
            self.take_view = None;
            self.sync_origin();
            self.locate_to(self.arrangement_origin.max(0));
            self.persist_project_meta();
        }
        if !self.is_editing_mix() {
            return;
        }
        let Some(board) = self.pasteboard.clone() else {
            return;
        };
        let dest = self.selected_lane.filter(|l| *l != MixLane::Main).unwrap_or(MixLane::Strip(0));
        if dest == MixLane::Main {
            return;
        }
        let at = crate::arrange::mix_paste_frame(
            leaving_take,
            self.mix.as_ref().map(|m| m.start_frame).unwrap_or(0),
            self.playing,
            self.engine_handles.sample_position.load(Ordering::Relaxed),
            self.locate_frame,
        );
        self.mutate_mix("Paste", false, |doc| doc.paste_clips(&board, dest, at));
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
        self.selection = ArrSelection { lanes, start, end, clips: Vec::new() };
    }

    pub(crate) fn arrangement_tracks(&self) -> Vec<MixTrack> {
        if self.viewing_take.is_some() {
            self.take_view.clone().unwrap_or_default()
        } else {
            self.mix.as_ref().map(MixDocument::channel_tracks).unwrap_or_default()
        }
    }

    pub(crate) fn select_mix(&mut self, id: uuid::Uuid) {
        if self.playing {
            self.halt_mix_play();
        }
        let leaving_take = self.viewing_take.is_some();
        self.mix = self.mixes.iter().find(|m| m.id == id).cloned();
        let takes = self.take_infos.clone();
        if let Some(mix) = &mut self.mix {
            mix.backfill_source_totals(&takes);
        }
        self.viewing_take = None;
        self.take_view = None;
        self.selection.clear();
        self.clip_preview = None;
        self.sync_origin();
        if leaving_take {
            self.locate_to(self.arrangement_origin.max(0));
        }
        self.persist_project_meta();
        self.publish_schedule();
    }

    pub(crate) fn select_take(&mut self, number: i32) {
        if self.playing {
            self.halt_mix_play();
        }
        self.viewing_take = Some(number);
        self.selection.clear();
        self.clip_preview = None;
        self.take_view = self.take_infos.iter().find(|t| t.number == number).map(|info| {
            info.arrangement_tracks()
                .into_iter()
                .filter(|track| crate::record::lane_on_record_list(&self.analog, track.lane))
                .collect()
        });
        self.sync_origin();
        self.locate_to(self.arrangement_origin.max(0));
        self.persist_project_meta();
    }

    pub(crate) fn take_start_from_store(&self, number: i32) -> i64 {
        ProjectStore::current_url(&self.analog.config)
            .map(|folder| self.project.load_meta(&folder).take_start_frame(number))
            .unwrap_or(0)
    }

    pub(crate) fn sync_origin(&mut self) {
        self.arrangement_origin = if let Some(n) = self.viewing_take {
            self.take_start_from_store(n)
        } else {
            self.mix.as_ref().map(|m| m.start_frame.max(0)).unwrap_or(0)
        };
    }

    pub(crate) fn arrangement_length(&self) -> i64 {
        if let Some(n) = self.viewing_take {
            self.take_infos.iter().find(|t| t.number == n).map(|t| t.frame_count()).unwrap_or(0)
        } else {
            self.mix.as_ref().map(|m| m.last_clip_end()).unwrap_or(0)
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
        self.arrangement_origin = origin;
        if self.viewing_take.is_none() {
            if let Some(mix) = self.mix.as_mut() {
                mix.start_frame = origin;
            }
        }
    }

    pub(crate) fn persist_start(&mut self) {
        self.set_arrangement_start(self.arrangement_origin);
        if self.viewing_take.is_some() {
            self.persist_project_meta();
        } else {
            self.persist_mix();
        }
        self.locate_to(self.arrangement_origin);
    }

    pub(crate) fn persist_project_meta(&mut self) {
        let Some(folder) = ProjectStore::current_url(&self.analog.config) else {
            return;
        };
        let mut meta = self.project.load_meta(&folder);
        meta.tempo = self.tempo;
        meta.grid = self.grid;
        meta.grid_enabled = self.grid_enabled;
        meta.pixels_per_bar = self.pixels_per_bar as f64;
        meta.selected_lane = self.selected_lane;
        meta.active_mix_id = self.mix.as_ref().map(|m| m.id);
        meta.mixes =
            self.mixes.iter().map(|m| MixListEntry { id: m.id, name: m.name.clone() }).collect();
        meta.arrangement = if let Some(n) = self.viewing_take {
            Some(MixArrangement::Take(n))
        } else {
            self.mix.as_ref().map(|m| MixArrangement::Mix(m.id))
        };
        if let Some(n) = self.viewing_take {
            meta.set_take_start_frame(n, self.arrangement_origin);
        }
        let _ = self.project.save_meta(&meta, &folder);
    }

    /// MixLink `MixStore.load`: sidecar + filesystem takes. No default mix.

    pub(crate) fn reload_mix(&mut self) {
        if self.playing {
            self.halt_mix_play();
        }
        let Some(folder) = ProjectStore::current_url(&self.analog.config) else {
            if self.analog.config.projects_root_bookmark.is_some() {
                log::warn!(
                    "project: bookmark did not resolve (relative {:?})",
                    self.analog.config.current_project_relative
                );
            }
            self.takes.clear();
            self.take_infos.clear();
            self.take_view = None;
            self.mixes.clear();
            self.mix = None;
            return;
        };
        if !folder.exists() {
            log::warn!("project: folder missing {}", folder.display());
        }
        let sr = self._stream.as_ref().map(|s| s.sample_rate() as f64).unwrap_or(48_000.0);
        let meta = self.project.load_meta(&folder);
        self.set_tempo(meta.tempo);
        self.grid = meta.grid;
        self.grid_enabled = meta.grid_enabled;
        self.pixels_per_bar = meta.pixels_per_bar as f32;
        self.selected_lane = meta.selected_lane;
        self.take_number = self.project.next_take(&folder);
        self.take_infos = crate::record::list_take_infos(&folder, sr);
        self.takes = self.take_infos.iter().map(|t| t.number).collect();

        self.mixes.clear();
        for entry in &meta.mixes {
            if let Some(doc) = self.project.load_mix(entry.id, &folder) {
                self.mixes.push(doc);
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
                            if !self.mixes.iter().any(|m| m.id == doc.id) {
                                self.mixes.push(doc);
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
            .and_then(|id| self.mixes.iter().find(|m| m.id == id).cloned())
            .or_else(|| self.mixes.first().cloned());
        self.mix = active;
        match meta.arrangement {
            Some(MixArrangement::Take(n)) if self.takes.contains(&n) => {
                self.select_take(n);
            }
            Some(MixArrangement::Mix(id)) => {
                if self.mixes.iter().any(|m| m.id == id) {
                    self.mix = self.mixes.iter().find(|m| m.id == id).cloned();
                }
                self.viewing_take = None;
                self.take_view = None;
            }
            _ => {
                self.viewing_take = None;
                self.take_view = None;
            }
        }
        self.sync_origin();
        log::info!(
            "project {} — {} mix(es), take(s) {:?}",
            folder.display(),
            self.mixes.len(),
            self.takes
        );
    }
}

pub(crate) fn project_parts(rel: &str) -> (String, String) {
    if let Some((date, rest)) = rel.split_once(" - ") {
        (date.to_string(), rest.to_string())
    } else {
        (rel.to_string(), String::new())
    }
}
