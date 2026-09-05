//! MixLink project documents, take WAV names, undo, and folder store.
//!
//! Session load/save lives in [`analog::SessionConfig`]. This crate owns mix
//! JSON, `project.json`, take numbering, and security-scoped project bookmarks.

#![forbid(unsafe_op_in_unsafe_fn)]

pub mod document;
pub mod meta;
pub mod session;
pub mod store;
pub mod undo;

pub use analog::ReturnLane;
pub use document::{
    parse_take_wav, sanitize_take_name, strip_adat_channel, take_clip_id, take_wav_name,
    MixArrangement,
    MixAutomationLane, MixAutomationPoint, MixAutomationTarget, MixClip, MixDocument, MixGrid,
    MixInsert, MixKnobMap, MixLane, MixPasteboard, MixTime, MixTrack, TakeFile, TakeInfo,
    FADER_LIN_0DB, KNOB_COUNT,
};
pub use meta::{MixListEntry, ProjectMeta};
pub use session::{
    app_support_dir, bookmark_from_path, insert_state_url, plugin_slot_state_url, resolve_bookmark,
    uuid_upper, BookmarkError,
};
pub use store::{scan_take_infos, scan_takes, ProjectStore, StoreError};
pub use undo::{UndoEntry, UndoStack, UNDO_LEVELS};
