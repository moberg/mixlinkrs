//! Arrangement clip / time / zoom drags.

use project::{ArrSelection, MixClip, MixLane, MixTime, MixTrack};
use render::Rect;
use ui_mixlink::arrangement::ArrangementLayout;
use ui_mixlink::hit::{self, Hit};
use ui_mixlink::overlay::MenuAction;
use ui_mixlink::widgets::MenuItem;

use crate::state::{AppState, Chrome, Drag, Timeline};

impl AppState {
    pub(crate) fn clip_edit_drag(&self) -> bool {
        matches!(
            self.chrome.drag,
            Some(
                Drag::ClipMove { .. }
                    | Drag::ClipEdge { .. }
                    | Drag::ClipFade { .. }
                    | Drag::ClipLoop { .. }
                    | Drag::ClipSlip { .. }
            )
        )
    }

    pub(crate) fn hidden_clip_ids(&self) -> Vec<uuid::Uuid> {
        self.timeline.hidden_clip_ids(&self.chrome)
    }

    pub(crate) fn arrangement_cursor(&self) -> crate::cursors::ArrCursor {
        if let Some(Drag::ClipEdge { left, .. }) = &self.chrome.drag {
            return if *left {
                crate::cursors::ArrCursor::TrimLeft
            } else {
                crate::cursors::ArrCursor::TrimRight
            };
        }
        if self.chrome.overlay.is_some() || self.chrome.drag.is_some() || !self.is_editing_mix() {
            return crate::cursors::ArrCursor::Default;
        }
        let (x, y) = self.chrome.cursor;
        match self.hit_body(x, y) {
            Some(Hit::ClipEdge { left: true, .. }) => crate::cursors::ArrCursor::TrimLeft,
            Some(Hit::ClipEdge { left: false, .. }) => crate::cursors::ArrCursor::TrimRight,
            _ => crate::cursors::ArrCursor::Default,
        }
    }

    pub(crate) fn update_cursor(&mut self) {
        self.chrome.apply_cursor(self.arrangement_cursor());
    }

    pub(crate) fn arr_layout(&self) -> ArrangementLayout {
        self.timeline.arr_layout(&self.chrome)
    }

    pub(crate) fn select_arrange_lane(&mut self, track: usize) {
        let tracks = self.arrangement_tracks();
        self.timeline.select_arrange_lane(&tracks, track);
    }

    pub(crate) fn select_arrange_clip(&mut self, lane: MixLane, id: uuid::Uuid) {
        let tracks = self.arrangement_tracks();
        self.timeline.select_arrange_clip(&tracks, lane, id);
    }

    pub(crate) fn open_arrange_menu(&mut self, x: f32, y: f32) {
        let items = vec![
            MenuItem { id: "copy".into(), label: "Copy".into(), checked: false, section: None },
            MenuItem { id: "paste".into(), label: "Paste".into(), checked: false, section: None },
            MenuItem {
                id: "duplicate".into(),
                label: "Duplicate".into(),
                checked: false,
                section: None,
            },
            MenuItem { id: "split".into(), label: "Split".into(), checked: false, section: None },
            MenuItem {
                id: "delete".into(),
                label: "Delete".into(),
                checked: false,
                section: Some(" ".into()),
            },
        ];
        self.chrome.place_menu(Rect { x, y, w: 1.0, h: 1.0 }, items, MenuAction::Arrange);
    }

    pub(crate) fn on_browser_context(&mut self, hit: ui_mixlink::mix_browser::BrowserHit) {
        let Some(anchor) = ui_mixlink::mix_browser::row_rect(&self.mix_browser_view(), hit) else {
            return;
        };
        match hit {
            ui_mixlink::mix_browser::BrowserHit::Mix(id) => {
                let mut items: Vec<MenuItem> = self
                    .session
                    .takes
                    .iter()
                    .map(|n| MenuItem {
                        id: format!("take:{n}"),
                        label: format!("Start from take {n}"),
                        checked: false,
                        section: None,
                    })
                    .collect();
                items.push(MenuItem {
                    id: "delete".into(),
                    label: "Delete Mix…".into(),
                    checked: false,
                    section: Some(" ".into()),
                });
                self.chrome.place_menu(anchor, items, MenuAction::MixContext { id });
            }
            ui_mixlink::mix_browser::BrowserHit::Take(number) => {
                self.chrome.place_menu(
                    anchor,
                    vec![
                        MenuItem {
                            id: "start".into(),
                            label: "Start mix from this take".into(),
                            checked: false,
                            section: None,
                        },
                        MenuItem {
                            id: "copy".into(),
                            label: "Copy take to clipboard".into(),
                            checked: false,
                            section: None,
                        },
                    ],
                    MenuAction::TakeContext { number },
                );
            }
            _ => {}
        }
    }

    pub(crate) fn begin_time_select(&mut self, x: f32, y: f32, all_lanes: bool) {
        let frame = self.snap_playhead_frame(hit::frame_at_x(
            &self.arr_layout(),
            x,
            self.timeline.tempo,
            self.audio.sample_rate(),
        ));
        let tracks = self.arrangement_tracks();
        let lane = ui_mixlink::arrangement::track_index_at(&self.arr_layout(), y, tracks.len())
            .and_then(|i| tracks.get(i).map(|t| t.lane))
            .or(self.timeline.selected_lane)
            .unwrap_or(MixLane::Strip(0));
        self.timeline.selected_lane = Some(lane);
        self.timeline.selection.clear();
        self.chrome.drag = Some(Drag::Select {
            start_lane: lane,
            start: frame,
            all_lanes,
            start_x: x,
            start_y: y,
            live: false,
        });
    }

    pub(crate) fn begin_clip_move(&mut self, lane: MixLane, id: uuid::Uuid, x: f32, y: f32) {
        let tracks = self.arrangement_tracks();
        let Some((_, clip)) = crate::arrange::find_clip(&tracks, id) else {
            return;
        };
        let add = self.chrome.modifiers.shift_key() || self.chrome.modifiers.super_key();
        self.timeline.selection =
            crate::arrange::selection_for_clip(lane, clip, add, &self.timeline.selection);
        self.timeline.selection.start = 0;
        self.timeline.selection.end = 0;
        self.timeline.selected_lane = Some(lane);
        let ids = if self.timeline.selection.clips.contains(&id)
            && !self.timeline.selection.clips.is_empty()
        {
            self.timeline.selection.clips.clone()
        } else {
            vec![id]
        };
        let origins: Vec<_> = ids
            .iter()
            .filter_map(|cid| {
                crate::arrange::find_clip(&tracks, *cid).map(|(l, c)| (*cid, l, c.mix_start_frame))
            })
            .collect();
        let copy = self.chrome.modifiers.alt_key();
        let slip = self.chrome.modifiers.control_key() && !self.chrome.modifiers.super_key();
        if !self.is_editing_mix() {
            return;
        }
        if slip {
            self.begin_clip_slip(id, x);
            return;
        }
        self.chrome.drag =
            Some(Drag::ClipMove { ids, anchor: id, start_x: x, start_y: y, origins, copy });
    }

    pub(crate) fn begin_clip_edge(&mut self, id: uuid::Uuid, left: bool, x: f32) {
        let tracks = self.arrangement_tracks();
        let Some((lane, clip)) = crate::arrange::find_clip(&tracks, id) else {
            return;
        };
        self.timeline.selected_lane = Some(lane);
        if !self.is_editing_mix() {
            return;
        }
        self.chrome.drag = Some(Drag::ClipEdge {
            id,
            left,
            start_x: x,
            start_frame: clip.mix_start_frame,
            start_source: clip.source_start_frame,
            start_count: clip.source_frame_count,
        });
    }

    pub(crate) fn begin_clip_fade(&mut self, id: uuid::Uuid, left: bool, x: f32) {
        let tracks = self.arrangement_tracks();
        let Some((lane, clip)) = crate::arrange::find_clip(&tracks, id) else {
            return;
        };
        self.timeline.selected_lane = Some(lane);
        if !self.is_editing_mix() {
            return;
        }
        self.chrome.drag = Some(Drag::ClipFade {
            id,
            left,
            start_x: x,
            start_frames: if left { clip.fade_in_frames } else { clip.fade_out_frames },
        });
    }

    pub(crate) fn begin_clip_loop(&mut self, id: uuid::Uuid, x: f32) {
        let tracks = self.arrangement_tracks();
        let Some((lane, clip)) = crate::arrange::find_clip(&tracks, id) else {
            return;
        };
        self.timeline.selected_lane = Some(lane);
        if !self.is_editing_mix() {
            return;
        }
        self.chrome.drag =
            Some(Drag::ClipLoop { id, start_x: x, start_count: clip.source_frame_count });
    }

    pub(crate) fn begin_clip_slip(&mut self, id: uuid::Uuid, x: f32) {
        let tracks = self.arrangement_tracks();
        let Some((lane, clip)) = crate::arrange::find_clip(&tracks, id) else {
            return;
        };
        self.timeline.selected_lane = Some(lane);
        if !self.is_editing_mix() {
            return;
        }
        self.chrome.drag =
            Some(Drag::ClipSlip { id, start_x: x, start_source: clip.source_start_frame });
    }

    pub(crate) fn preview_arrangement_drag(&mut self, x: f32, y: f32) {
        let sr = self.audio.sample_rate();
        let bypass = self.chrome.modifiers.super_key();
        let raw_delta = MixTime::frame_from_bar(
            ((x - match &self.chrome.drag {
                Some(Drag::ClipMove { start_x, .. })
                | Some(Drag::ClipEdge { start_x, .. })
                | Some(Drag::ClipFade { start_x, .. })
                | Some(Drag::ClipLoop { start_x, .. })
                | Some(Drag::ClipSlip { start_x, .. }) => *start_x,
                _ => x,
            }) / self.timeline.pixels_per_bar.max(1.0)) as f64,
            self.timeline.tempo,
            sr,
        );
        let delta = crate::arrange::snap_frame_delta(
            raw_delta,
            self.timeline.grid_enabled,
            bypass,
            self.timeline.grid.raw(),
            self.timeline.tempo,
            sr,
        );
        let tracks = self.arrangement_tracks();
        match self.chrome.drag.clone() {
            Some(Drag::ClipMove { origins, start_y, .. }) => {
                if !self.is_editing_mix() {
                    return;
                }
                let row_delta = ((y - start_y) / ui_mixlink::arrangement::TRACK_H).round() as i32;
                let mut preview = Vec::new();
                for (id, lane, start) in &origins {
                    let Some((_, clip)) = crate::arrange::find_clip(&tracks, *id) else {
                        continue;
                    };
                    let dest_lane =
                        crate::arrange::shift_lane(&tracks, *lane, row_delta).unwrap_or(*lane);
                    if dest_lane == MixLane::Main {
                        continue;
                    }
                    let mut next = clip.clone();
                    next.mix_start_frame = (*start + delta).max(0);
                    preview.push((dest_lane, next));
                }
                if let Some((_, clip)) = preview.first() {
                    self.timeline.clip_readout =
                        Some(crate::arrange::clip_readout(clip, self.timeline.tempo, sr));
                }
                self.timeline.clip_preview = Some(preview);
            }
            Some(Drag::ClipEdge { id, left, start_frame, start_source, start_count, .. }) => {
                let Some((lane, clip)) = crate::arrange::find_clip(&tracks, id) else {
                    return;
                };
                let mut next = clip.clone();
                next.mix_start_frame = start_frame;
                next.source_start_frame = start_source;
                next.source_frame_count = start_count;
                if left {
                    next.trim_left((start_frame + delta).max(0));
                } else {
                    next.trim_right(start_frame + start_count + delta);
                }
                self.timeline.clip_readout =
                    Some(crate::arrange::clip_readout(&next, self.timeline.tempo, sr));
                self.timeline.clip_preview = Some(vec![(lane, next)]);
            }
            Some(Drag::ClipFade { id, left, start_frames, .. }) => {
                let Some((lane, clip)) = crate::arrange::find_clip(&tracks, id) else {
                    return;
                };
                let mut next = clip.clone();
                let frames = (start_frames + delta).max(0);
                if left {
                    next.set_fade_in(frames);
                } else {
                    next.set_fade_out(frames);
                }
                self.timeline.clip_preview = Some(vec![(lane, next)]);
            }
            Some(Drag::ClipLoop { id, start_count, .. }) => {
                let Some((lane, clip)) = crate::arrange::find_clip(&tracks, id) else {
                    return;
                };
                let mut next = clip.clone();
                next.enable_loop();
                next.source_frame_count = (start_count + delta).max(project::MIN_CLIP_FRAMES);
                self.timeline.clip_readout =
                    Some(crate::arrange::clip_readout(&next, self.timeline.tempo, sr));
                self.timeline.clip_preview = Some(vec![(lane, next)]);
            }
            Some(Drag::ClipSlip { id, start_source, .. }) => {
                let Some((lane, clip)) = crate::arrange::find_clip(&tracks, id) else {
                    return;
                };
                let mut next = clip.clone();
                next.source_start_frame = start_source;
                next.slip(delta);
                self.timeline.clip_preview = Some(vec![(lane, next)]);
            }
            _ => {}
        }
        self.edge_auto_scroll(x, y);
    }

    pub(crate) fn commit_arrangement_drag(&mut self) {
        let Some(preview) = self.timeline.clip_preview.take() else {
            if let Some(Drag::Select { start, live, .. }) = self.chrome.drag {
                if !live {
                    self.locate_to(start);
                    self.timeline.selection.clear();
                }
            }
            return;
        };
        if !self.is_editing_mix() {
            return;
        }
        let Some(edit) = MixEdit::from_drag(&self.chrome.drag, preview) else {
            return;
        };
        if self.session.apply_mix_edit(edit, &mut self.timeline) {
            self.after_mix_edit();
        }
    }

    pub(crate) fn edge_auto_scroll(&mut self, x: f32, y: f32) {
        self.timeline.edge_auto_scroll(&self.chrome, x, y);
    }
}

pub(crate) enum MixEdit {
    Move { copy: bool, ids: Vec<uuid::Uuid>, preview: Vec<(MixLane, MixClip)> },
    Trim { id: uuid::Uuid, lane: MixLane, next: MixClip },
    Fade { id: uuid::Uuid, fade_in: i64, fade_out: i64 },
    Loop { id: uuid::Uuid, lane: MixLane, next: MixClip },
    Slip { id: uuid::Uuid, source_start: i64 },
}

impl MixEdit {
    fn from_drag(drag: &Option<Drag>, preview: Vec<(MixLane, MixClip)>) -> Option<Self> {
        match drag {
            Some(Drag::ClipMove { origins, copy, .. }) => Some(Self::Move {
                copy: *copy,
                ids: origins.iter().map(|(id, _, _)| *id).collect(),
                preview,
            }),
            Some(Drag::ClipEdge { id, .. }) => {
                let (lane, next) = preview.into_iter().next()?;
                Some(Self::Trim { id: *id, lane, next })
            }
            Some(Drag::ClipFade { id, .. }) => {
                let (_, next) = preview.into_iter().next()?;
                Some(Self::Fade {
                    id: *id,
                    fade_in: next.fade_in_frames,
                    fade_out: next.fade_out_frames,
                })
            }
            Some(Drag::ClipLoop { id, .. }) => {
                let (lane, next) = preview.into_iter().next()?;
                Some(Self::Loop { id: *id, lane, next })
            }
            Some(Drag::ClipSlip { id, .. }) => {
                let (_, next) = preview.into_iter().next()?;
                Some(Self::Slip { id: *id, source_start: next.source_start_frame })
            }
            _ => None,
        }
    }
}

impl Timeline {
    pub(crate) fn hidden_clip_ids(&self, chrome: &Chrome) -> Vec<uuid::Uuid> {
        if self.clip_preview.is_none() {
            return Vec::new();
        }
        match &chrome.drag {
            Some(Drag::ClipMove { copy: true, .. }) => Vec::new(),
            Some(Drag::ClipMove { ids, .. }) => ids.clone(),
            Some(
                Drag::ClipEdge { id, .. }
                | Drag::ClipFade { id, .. }
                | Drag::ClipLoop { id, .. }
                | Drag::ClipSlip { id, .. },
            ) => vec![*id],
            _ => Vec::new(),
        }
    }

    pub(crate) fn arr_layout(&self, chrome: &Chrome) -> ArrangementLayout {
        let (bx, by, bw, bh) = chrome.body_rect();
        let mix_h = ui_mixlink::mix_mixer::height(chrome.show_knobs, chrome.show_mixer);
        ArrangementLayout {
            x: bx + 148.0,
            y: by,
            w: bw - 148.0,
            h: bh - mix_h,
            scroll_x: self.scroll_x,
            scroll_y: self.scroll_y,
            pixels_per_bar: self.pixels_per_bar,
        }
    }

    pub(crate) fn edge_auto_scroll(&mut self, chrome: &Chrome, x: f32, y: f32) {
        let layout = self.arr_layout(chrome);
        let margin = 28.0;
        if x > layout.x + layout.w - margin {
            self.scroll_x += 24.0;
        } else if x < layout.x + ui_mixlink::arrangement::HEADER_W + margin {
            self.scroll_x = (self.scroll_x - 24.0).max(0.0);
        }
        if y > layout.y + layout.h - ui_mixlink::arrangement::TIME_RULER_H - margin {
            self.scroll_y += 16.0;
        } else if y < layout.y + ui_mixlink::arrangement::RULER_H + margin {
            self.scroll_y = (self.scroll_y - 16.0).max(0.0);
        }
    }

    pub(crate) fn select_arrange_lane(&mut self, tracks: &[MixTrack], track: usize) {
        let Some(lane) = tracks.get(track).map(|t| t.lane) else {
            return;
        };
        self.selected_lane = Some(lane);
        self.selection.clear();
        self.selection.lanes = vec![lane];
    }

    pub(crate) fn select_arrange_clip(
        &mut self,
        tracks: &[MixTrack],
        lane: MixLane,
        id: uuid::Uuid,
    ) {
        if self.selection.clips.contains(&id) {
            self.selected_lane = Some(lane);
            return;
        }
        let Some((_, clip)) = crate::arrange::find_clip(tracks, id) else {
            return;
        };
        self.selection =
            crate::arrange::selection_for_clip(lane, clip, false, &ArrSelection::default());
        self.selected_lane = Some(lane);
    }
}
