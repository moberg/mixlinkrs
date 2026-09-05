//! Project document, takes, undo, and pasteboard.

use project::{MixDocument, MixPasteboard, ProjectStore, TakeInfo, UndoStack};

#[derive(Default)]
pub(crate) struct Session {
    pub project: ProjectStore,
    pub mix: Option<MixDocument>,
    pub mixes: Vec<MixDocument>,
    pub takes: Vec<i32>,
    pub take_infos: Vec<TakeInfo>,
    pub take_view: Option<Vec<project::MixTrack>>,
    pub viewing_take: Option<i32>,
    pub take_number: i32,
    pub undo: UndoStack<MixDocument>,
    pub pasteboard: Option<MixPasteboard>,
}
