//! MixLink project documents, take WAV names, undo, and folder store.
//!
//! Session load/save lives in [`analog::SessionConfig`]. This crate owns mix
//! JSON, `project.json` (including the chain catalog), take numbering, and
//! security-scoped project bookmarks.

#![forbid(unsafe_op_in_unsafe_fn)]

pub mod document;
pub mod meta;
pub mod session;
pub mod store;
pub mod undo;

pub use analog::{ChainKind, ChainRef, ReturnLane};
pub use document::{
    copy_clip_ids, copy_time_range, last_clip_end, parse_take_wav, sanitize_take_name,
    strip_adat_channel, take_clip_id, take_wav_name, ArrSelection, MixArrangement,
    MixAutomationLane, MixAutomationPoint, MixAutomationTarget, MixClip, MixDocument, MixGrid,
    MixInsert, MixKnobMap, MixLane, MixPasteEntry, MixPasteboard, MixTime, MixTrack, TakeFile,
    TakeInfo, FADER_LIN_0DB, KNOB_COUNT, MIN_CLIP_FRAMES,
};
pub use meta::{MixListEntry, ProjectMeta};
pub use session::{
    app_support_dir, bookmark_from_path, insert_state_url, plugin_slot_state_url,
    plugin_stage_global_preview_url, plugin_stage_global_state_url, plugin_stage_preview_url,
    plugin_stage_state_url, resolve_bookmark, uuid_upper,
    BookmarkError,
};
pub use store::{delete_take_files, scan_take_infos, scan_takes, ProjectStore, StoreError};
pub use undo::{UndoEntry, UndoStack, UNDO_LEVELS};
