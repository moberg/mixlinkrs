//! Project bookmarks and plugin-state paths.
//!
//! Session config load/save is in [`analog::SessionConfig`]. Live send-slot
//! state lives under Application Support (`plugin-slot-{n}-{bundle}.state`);
//! plugin-chain stage state is dual-scope:
//! - global: [`plugin_stage_global_state_url`] next to session config
//! - project: [`plugin_stage_state_url`] inside the project folder
//! Mix inserts use [`insert_state_url`] next to the mix JSON.

use std::path::{Path, PathBuf};

use analog::SessionConfig;
use thiserror::Error;
use uuid::Uuid;

const PATH_BOOKMARK_PREFIX: &[u8] = b"path://";

#[derive(Debug, Error)]
pub enum BookmarkError {
    #[error("bookmark is empty")]
    Empty,
    #[error("could not resolve bookmark")]
    Unresolved,
    #[error("path is not valid UTF-8")]
    InvalidPath,
}

/// MixLink writes uppercase `UUID.uuidString` into filenames.
pub fn uuid_upper(id: &Uuid) -> String {
    id.as_hyphenated().to_string().to_ascii_uppercase()
}

/// `~/Library/Application Support/MixLink` — same folder as analog session files.
pub fn app_support_dir() -> PathBuf {
    SessionConfig::storage_directory()
}

/// `{project}/mix-{mixUUID}-{insertUUID}-{bundleName}.state`
pub fn insert_state_url(
    project: &Path,
    mix_id: Uuid,
    insert_id: Uuid,
    bundle_path: &str,
) -> PathBuf {
    let bundle_name =
        Path::new(bundle_path).file_name().and_then(|s| s.to_str()).unwrap_or(bundle_path);
    project.join(format!(
        "mix-{}-{}-{}.state",
        uuid_upper(&mix_id),
        uuid_upper(&insert_id),
        bundle_name
    ))
}

/// `{project}/plugin-stage-{stageUUID}-preview.png`
pub fn plugin_stage_preview_url(project: &Path, stage_id: Uuid) -> PathBuf {
    project.join(format!("plugin-stage-{}-preview.png", uuid_upper(&stage_id)))
}

/// `~/Library/Application Support/MixLink/plugin-stage-{stageUUID}-preview.png`
pub fn plugin_stage_global_preview_url(stage_id: Uuid) -> PathBuf {
    app_support_dir().join(format!("plugin-stage-{}-preview.png", uuid_upper(&stage_id)))
}

/// `{project}/plugin-stage-{stageUUID}-{bundleName}.state`
pub fn plugin_stage_state_url(
    project: &Path,
    stage_id: Uuid,
    bundle_path: &str,
) -> PathBuf {
    let bundle_name =
        Path::new(bundle_path).file_name().and_then(|s| s.to_str()).unwrap_or(bundle_path);
    project.join(format!(
        "plugin-stage-{}-{}.state",
        uuid_upper(&stage_id),
        bundle_name
    ))
}

/// `~/Library/Application Support/MixLink/plugin-stage-{stageUUID}-{bundle}.state`
///
/// Global chain preset state (survives across projects).
pub fn plugin_stage_global_state_url(stage_id: Uuid, bundle_path: &str) -> PathBuf {
    let name = Path::new(bundle_path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(bundle_path);
    app_support_dir().join(format!(
        "plugin-stage-{}-{}.state",
        uuid_upper(&stage_id),
        name
    ))
}

/// `~/Library/Application Support/MixLink/plugin-slot-{slot}-{bundle}.state`
///
/// Falls back to `plugin-slot-{slot}.state` when the bundle is unknown so a
/// swap cannot restore another plugin's blob.
pub fn plugin_slot_state_url(slot: i32, bundle_path: Option<&str>) -> PathBuf {
    let dir = app_support_dir();
    match bundle_path.filter(|p| !p.is_empty()) {
        None => dir.join(format!("plugin-slot-{slot}.state")),
        Some(path) => {
            let name = Path::new(path).file_name().and_then(|s| s.to_str()).unwrap_or(path);
            dir.join(format!("plugin-slot-{slot}-{name}.state"))
        }
    }
}

/// Security-scoped bookmark for `path`. Uses objc2 `NSURL` on macOS; otherwise
/// stores a `path://` stub that [`resolve_bookmark`] can read back.
pub fn bookmark_from_path(path: &Path) -> Result<Vec<u8>, BookmarkError> {
    #[cfg(target_os = "macos")]
    {
        if let Some(data) = macos::bookmark_from_path(path) {
            if !data.is_empty() {
                return Ok(data);
            }
        }
    }
    let s = path.to_str().ok_or(BookmarkError::InvalidPath)?;
    let mut data = PATH_BOOKMARK_PREFIX.to_vec();
    data.extend_from_slice(s.as_bytes());
    Ok(data)
}

/// Resolve a MixLink / MixLinkRs bookmark. Starts security-scoped access when
/// the bookmark is a real `NSURL` blob.
pub fn resolve_bookmark(data: &[u8]) -> Result<PathBuf, BookmarkError> {
    if data.is_empty() {
        return Err(BookmarkError::Empty);
    }
    if let Some(path) = path_stub(data) {
        return Ok(path);
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(path) = macos::resolve_bookmark(data) {
            return Ok(path);
        }
    }
    if let Ok(s) = std::str::from_utf8(data) {
        if !s.is_empty() && (s.starts_with('/') || s.starts_with("file:")) {
            return Ok(PathBuf::from(s));
        }
    }
    Err(BookmarkError::Unresolved)
}

fn path_stub(data: &[u8]) -> Option<PathBuf> {
    let rest = data.strip_prefix(PATH_BOOKMARK_PREFIX)?;
    let s = std::str::from_utf8(rest).ok()?;
    if s.is_empty() {
        None
    } else {
        Some(PathBuf::from(s))
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use std::path::{Path, PathBuf};

    use objc2::rc::Id;
    use objc2::runtime::{AnyObject, Bool};
    use objc2::{class, msg_send, msg_send_id};
    use objc2_foundation::{NSData, NSString, NSURL};

    /// `NSURLBookmarkCreationWithSecurityScope`
    const CREATE_SECURITY_SCOPE: usize = 1 << 11;
    /// `NSURLBookmarkResolutionWithSecurityScope`
    const RESOLVE_SECURITY_SCOPE: usize = 1 << 10;

    pub fn bookmark_from_path(path: &Path) -> Option<Vec<u8>> {
        let path = path.to_str()?;
        let ns_path = NSString::from_str(path);
        let url: Id<NSURL> = unsafe { msg_send_id![class!(NSURL), fileURLWithPath: &*ns_path] };
        let mut error: *mut AnyObject = std::ptr::null_mut();
        let data: Option<Id<NSData>> = unsafe {
            msg_send_id![
                &*url,
                bookmarkDataWithOptions: CREATE_SECURITY_SCOPE,
                includingResourceValuesForKeys: std::ptr::null::<AnyObject>(),
                relativeToURL: std::ptr::null::<AnyObject>(),
                error: &mut error,
            ]
        };
        data.filter(|d| !d.is_empty()).map(|d| d.bytes().to_vec())
    }

    pub fn resolve_bookmark(data: &[u8]) -> Option<PathBuf> {
        resolve_with_options(data, RESOLVE_SECURITY_SCOPE)
            .or_else(|| resolve_with_options(data, 0))
            .or_else(|| path_from_bookmark_data(data))
    }

    fn resolve_with_options(data: &[u8], options: usize) -> Option<PathBuf> {
        let ns_data = NSData::with_bytes(data);
        let mut stale = Bool::NO;
        let mut error: *mut AnyObject = std::ptr::null_mut();
        let url: Option<Id<NSURL>> = unsafe {
            msg_send_id![
                class!(NSURL),
                URLByResolvingBookmarkData: &*ns_data,
                options: options,
                relativeToURL: std::ptr::null::<AnyObject>(),
                bookmarkDataIsStale: &mut stale,
                error: &mut error,
            ]
        };
        let url = url?;
        if options & RESOLVE_SECURITY_SCOPE != 0 {
            let _: bool = unsafe { msg_send![&*url, startAccessingSecurityScopedResource] };
        }
        let ns_path: Option<Id<NSString>> = unsafe { msg_send_id![&*url, path] };
        ns_path.map(|p| PathBuf::from(p.to_string()))
    }

    /// `+[NSURL resourceValuesForKeys:fromBookmarkData:]` — path without resolving
    /// as the creating app. MixLink bookmarks often fail in an unsigned MixLinkRs.
    fn path_from_bookmark_data(data: &[u8]) -> Option<PathBuf> {
        use objc2_foundation::NSArray;
        let ns_data = NSData::with_bytes(data);
        let key = NSString::from_str("NSURLPathKey");
        let keys: Id<NSArray<NSString>> = NSArray::from_vec(vec![key]);
        let values: Option<Id<objc2_foundation::NSDictionary<NSString, AnyObject>>> = unsafe {
            msg_send_id![
                class!(NSURL),
                resourceValuesForKeys: &*keys,
                fromBookmarkData: &*ns_data
            ]
        };
        let values = values?;
        let path_key = NSString::from_str("NSURLPathKey");
        let value: Option<Id<NSString>> =
            unsafe { msg_send_id![&*values, objectForKey: &*path_key] };
        value.map(|p| PathBuf::from(p.to_string())).filter(|p| !p.as_os_str().is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_stub_roundtrip() {
        let path = Path::new("/tmp/MixLink Projects/2026-09-03 - 1");
        let data = bookmark_from_path(path).unwrap();
        let resolved = resolve_bookmark(&data).unwrap();
        assert_eq!(resolved, path);
    }

    #[test]
    fn mixlink_session_bookmark_resolves_current_project() {
        let session = analog::SessionConfig::load();
        let Some(data) = session.projects_root_bookmark.as_deref() else {
            return;
        };
        let root = resolve_bookmark(data).expect("MixLink projects bookmark did not resolve");
        let Some(rel) = session.current_project_relative.as_deref() else {
            return;
        };
        let folder = root.join(rel);
        assert!(folder.is_dir(), "expected current project at {}", folder.display());
        let takes = crate::scan_take_infos(&folder, 48_000.0);
        assert!(!takes.is_empty(), "expected at least one take in {}", folder.display());
    }

    #[test]
    fn insert_and_slot_state_names() {
        let mix = Uuid::from_u128(0xAABBCCDD);
        let insert = Uuid::from_u128(0x11223344);
        let url = insert_state_url(
            Path::new("/proj"),
            mix,
            insert,
            "/Library/Audio/Plug-Ins/VST3/BigSky.vst3",
        );
        let name = url.file_name().unwrap().to_str().unwrap();
        assert!(name.starts_with("mix-"));
        assert!(name.ends_with("-BigSky.vst3.state"));
        assert!(name.contains(&uuid_upper(&mix)));
        assert!(name.contains(&uuid_upper(&insert)));

        let slot = plugin_slot_state_url(0, Some("/plugins/Kilohearts.vst3"));
        assert_eq!(
            slot.file_name().unwrap().to_str().unwrap(),
            "plugin-slot-0-Kilohearts.vst3.state"
        );
        let fallback = plugin_slot_state_url(2, None);
        assert_eq!(fallback.file_name().unwrap().to_str().unwrap(), "plugin-slot-2.state");

        let stage = Uuid::from_u128(0x99);
        let project = plugin_stage_state_url(
            Path::new("/proj"),
            stage,
            "/Library/Audio/Plug-Ins/VST3/Presswerk.vst3",
        );
        assert_eq!(
            project.file_name().unwrap().to_str().unwrap(),
            &format!("plugin-stage-{}-Presswerk.vst3.state", uuid_upper(&stage))
        );
        let global = plugin_stage_global_state_url(stage, "/plugins/Presswerk.vst3");
        assert!(global
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .ends_with("-Presswerk.vst3.state"));
    }
}
