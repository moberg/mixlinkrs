//! Dated project folders, take counters, mix JSON, insert-state paths.

use std::fs;
use std::path::{Path, PathBuf};

use analog::SessionConfig;
use thiserror::Error;
use uuid::Uuid;

use crate::document::{parse_take_wav, MixDocument, MixLane, TakeFile, TakeInfo};
use crate::meta::ProjectMeta;
use crate::session::{bookmark_from_path, insert_state_url, resolve_bookmark, uuid_upper};

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("no projects folder")]
    NoRoot,
    #[error("no current project")]
    NoProject,
    #[error("a project with that name already exists")]
    NameTaken,
    #[error("bookmark: {0}")]
    Bookmark(#[from] crate::session::BookmarkError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Security-scoped projects root, dated session folders, and take counters.
#[derive(Clone, Debug, Default)]
pub struct ProjectStore;

impl ProjectStore {
    pub fn new() -> Self {
        Self
    }

    pub fn resolve_root(config: &SessionConfig) -> Option<PathBuf> {
        let data = config.projects_root_bookmark.as_deref()?;
        resolve_bookmark(data).ok()
    }

    pub fn bookmark_for(url: &Path) -> Option<Vec<u8>> {
        bookmark_from_path(url).ok()
    }

    pub fn current_url(config: &SessionConfig) -> Option<PathBuf> {
        let root = Self::resolve_root(config)?;
        let name = config.current_project_relative.as_deref()?;
        if name.is_empty() {
            return None;
        }
        Some(root.join(name))
    }

    pub fn display_name(config: &SessionConfig) -> String {
        config.current_project_relative.clone().unwrap_or_default()
    }

    pub fn date_label(config: &SessionConfig) -> String {
        config.current_project_relative.as_deref().and_then(date_prefix).unwrap_or_default()
    }

    /// Editable suffix after `YYYY-MM-DD - `.
    pub fn name_suffix(config: &SessionConfig) -> String {
        let Some(name) = config.current_project_relative.as_deref() else {
            return String::new();
        };
        if let Some(date) = date_prefix(name) {
            name[date.len()..].trim_start_matches([' ', '-']).into()
        } else {
            name.into()
        }
    }

    /// Immediate subfolders of the projects root, sorted by name.
    pub fn list_projects(config: &SessionConfig) -> Vec<String> {
        let Some(root) = Self::resolve_root(config) else {
            return Vec::new();
        };
        let Ok(entries) = fs::read_dir(&root) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .flatten()
            .filter_map(|entry| {
                let ft = entry.file_type().ok()?;
                if !ft.is_dir() {
                    return None;
                }
                let name = entry.file_name().into_string().ok()?;
                if name.starts_with('.') {
                    return None;
                }
                Some(name)
            })
            .collect();
        names.sort();
        names
    }

    /// Today’s local date (`YYYY-MM-DD`). Collisions become `YYYY-MM-DD-2`, then `-3`.
    pub fn create_project(&self, config: &mut SessionConfig) -> Result<PathBuf, StoreError> {
        let root = Self::resolve_root(config).ok_or(StoreError::NoRoot)?;
        fs::create_dir_all(&root)?;
        let name = unique_dated_name(&root, &today_prefix());
        let url = root.join(&name);
        fs::create_dir_all(&url)?;
        self.save_meta(&ProjectMeta::default(), &url)?;
        config.current_project_relative = Some(name);
        Ok(url)
    }

    /// Renames the project folder. Empty input is ignored (name unchanged).
    pub fn rename_current(
        &self,
        raw: &str,
        config: &mut SessionConfig,
    ) -> Result<PathBuf, StoreError> {
        let root = Self::resolve_root(config).ok_or(StoreError::NoRoot)?;
        let current = config.current_project_relative.clone().ok_or(StoreError::NoProject)?;
        let trimmed = sanitize_project_name(raw);
        if trimmed.is_empty() {
            return Ok(root.join(&current));
        }
        let from = root.join(&current);
        let to = root.join(&trimmed);
        if from.file_name() != to.file_name() {
            if to.exists() {
                return Err(StoreError::NameTaken);
            }
            fs::rename(&from, &to)?;
        }
        config.current_project_relative = Some(trimmed);
        Ok(to)
    }

    pub fn next_take(&self, project: &Path) -> i32 {
        let sidecar = self.load_meta(project).next_take;
        let scanned = scan_takes(project);
        sidecar.max(scanned + 1)
    }

    pub fn increment_take(&self, project: &Path) {
        let mut meta = self.load_meta(project);
        meta.next_take = self.next_take(project);
        let _ = self.save_meta(&meta, project);
    }

    pub fn load_meta(&self, project: &Path) -> ProjectMeta {
        let path = sidecar_url(project);
        let Ok(data) = fs::read(&path) else {
            return ProjectMeta::default();
        };
        let Ok(mut meta) = serde_json::from_slice::<ProjectMeta>(&data) else {
            return ProjectMeta::default();
        };
        meta.normalize();
        meta
    }

    pub fn save_meta(&self, meta: &ProjectMeta, project: &Path) -> Result<(), StoreError> {
        fs::create_dir_all(project)?;
        let data = serde_json::to_vec_pretty(meta)?;
        atomic_write(&sidecar_url(project), &data)
    }

    pub fn mix_url(&self, mix_id: Uuid, project: &Path) -> PathBuf {
        project.join(format!("mix-{}.json", uuid_upper(&mix_id)))
    }

    pub fn insert_state_url(
        &self,
        mix_id: Uuid,
        insert_id: Uuid,
        bundle_path: &str,
        project: &Path,
    ) -> PathBuf {
        insert_state_url(project, mix_id, insert_id, bundle_path)
    }

    pub fn load_mix(&self, mix_id: Uuid, project: &Path) -> Option<MixDocument> {
        let data = fs::read(self.mix_url(mix_id, project)).ok()?;
        serde_json::from_slice(&data).ok()
    }

    /// MixLink `MixStore.scanTakes`: parseable non-mix WAVs grouped by take number.
    pub fn scan_take_infos(&self, project: &Path, sample_rate: f64) -> Vec<TakeInfo> {
        scan_take_infos(project, sample_rate)
    }

    pub fn save_mix(&self, mix: &MixDocument, project: &Path) -> Result<(), StoreError> {
        fs::create_dir_all(project)?;
        let data = serde_json::to_vec_pretty(mix)?;
        atomic_write(&self.mix_url(mix.id, project), &data)
    }
}

/// Remove every parseable take WAV for `number`, including `{N}-mix.wav`.
pub fn delete_take_files(project: &Path, number: i32) -> Vec<String> {
    take_filenames(project, number)
        .into_iter()
        .filter(|name| match fs::remove_file(project.join(name)) {
            Ok(()) => true,
            Err(e) => {
                log::warn!("delete take {number} {name}: {e}");
                false
            }
        })
        .collect()
}

fn take_filenames(project: &Path, number: i32) -> Vec<String> {
    let Ok(entries) = fs::read_dir(project) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_str()?;
            if !name.to_ascii_lowercase().ends_with(".wav") {
                return None;
            }
            let (take, _, _) = parse_take_wav(name)?;
            (take == number).then(|| name.to_string())
        })
        .collect();
    names.sort();
    names
}

/// MixLink `MixStore.scanTakes` — `{N}-mix.wav` is recorded but hidden from the take list.
pub fn scan_take_infos(project: &Path, sample_rate: f64) -> Vec<TakeInfo> {
    let Ok(entries) = fs::read_dir(project) else {
        return Vec::new();
    };
    let mut grouped: std::collections::BTreeMap<i32, Vec<TakeFile>> =
        std::collections::BTreeMap::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !name.to_ascii_lowercase().ends_with(".wav") {
            continue;
        }
        let Some((take, lane, display)) = parse_take_wav(name) else {
            continue;
        };
        if lane == MixLane::Main {
            continue;
        }
        grouped.entry(take).or_default().push(TakeFile {
            lane,
            name: display,
            filename: name.to_string(),
            frame_count: 0,
            sample_rate,
        });
    }
    grouped
        .into_iter()
        .map(|(number, mut files)| {
            files.sort_by(|a, b| a.lane.id().cmp(&b.lane.id()));
            TakeInfo { number, files }
        })
        .collect()
}

/// Largest integer before the first `-` of any `.wav` in `project`.
///
/// `next_take = max(meta.next_take, scanned + 1)`.
pub fn scan_takes(project: &Path) -> i32 {
    let Ok(entries) = fs::read_dir(project) else {
        return 0;
    };
    let mut max_take = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.to_ascii_lowercase().ends_with(".wav") {
            continue;
        }
        let stem = name
            .strip_suffix(".wav")
            .or_else(|| name.strip_suffix(".WAV"))
            .or_else(|| name.strip_suffix(".Wav"))
            .unwrap_or(&name);
        if let Some(n) = stem.split('-').next().and_then(|s| s.parse::<i32>().ok()) {
            max_take = max_take.max(n);
        }
    }
    max_take
}

fn sidecar_url(project: &Path) -> PathBuf {
    project.join("project.json")
}

fn today_prefix() -> String {
    #[cfg(unix)]
    {
        // MixLink DateFormatter uses the local calendar date.
        unsafe {
            let t = libc::time(std::ptr::null_mut());
            let mut tm = std::mem::zeroed::<libc::tm>();
            if libc::localtime_r(&t, &mut tm).is_null() {
                return civil_ymd(t.max(0) as u64);
            }
            format!("{:04}-{:02}-{:02}", tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday)
        }
    }
    #[cfg(not(unix))]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        civil_ymd(secs)
    }
}

fn civil_ymd(unix_secs: u64) -> String {
    // Howard Hinnant civil-from-days (UTC).
    let z = (unix_secs / 86_400) as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = y + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}")
}

fn date_prefix(name: &str) -> Option<String> {
    let first = name.split(' ').next()?;
    if first.len() != 10 {
        return None;
    }
    let parts: Vec<_> = first.split('-').collect();
    if parts.len() == 3 {
        Some(first.into())
    } else {
        None
    }
}

fn sanitize_project_name(raw: &str) -> String {
    let mapped: String = raw
        .trim()
        .chars()
        .map(|ch| match ch {
            '/' | ':' | '\\' => '-',
            c => c,
        })
        .collect();
    mapped.trim_matches(|c: char| c == '-' || c.is_whitespace()).into()
}

fn unique_dated_name(root: &Path, date: &str) -> String {
    if !root.join(date).exists() {
        return date.to_string();
    }
    let mut n = 2i32;
    loop {
        let name = format!("{date}-{n}");
        if !root.join(&name).exists() {
            return name;
        }
        n = n.saturating_add(1);
        if n > 10_000 {
            return name;
        }
    }
}

fn atomic_write(path: &Path, data: &[u8]) -> Result<(), StoreError> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, data)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::take_wav_name;
    use crate::{MixLane, ReturnLane};

    fn temp_dir() -> PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!("mixlink-project-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn scan_takes_from_fixtures() {
        let dir = temp_dir();
        for name in [
            take_wav_name(3, MixLane::Strip(0), "Rytm"),
            take_wav_name(3, MixLane::ReturnLane(ReturnLane::SendA), "BigSky"),
            take_wav_name(3, MixLane::ReturnLane(ReturnLane::Bus1), "x"),
            take_wav_name(3, MixLane::Main, ""),
        ] {
            assert_eq!(
                &name,
                match name.as_str() {
                    "3-ch-01-Rytm.wav" | "3-ret-A-BigSky.wav" | "3-bus-01-x.wav" | "3-mix.wav" =>
                        name.as_str(),
                    other => panic!("unexpected name {other}"),
                }
            );
            fs::write(dir.join(&name), []).unwrap();
        }
        fs::write(dir.join("notes.txt"), []).unwrap();
        let scanned = scan_takes(&dir);
        assert_eq!(scanned, 3);
        let store = ProjectStore::new();
        let mut meta = ProjectMeta::default();
        assert_eq!(store.next_take(&dir), 4);
        meta.next_take = 10;
        store.save_meta(&meta, &dir).unwrap();
        assert_eq!(store.next_take(&dir), 10);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn next_take_uses_sidecar_when_higher() {
        let dir = temp_dir();
        fs::write(dir.join("1-ch-01-Rytm.wav"), []).unwrap();
        let store = ProjectStore::new();
        let mut meta = ProjectMeta::default();
        meta.next_take = 5;
        store.save_meta(&meta, &dir).unwrap();
        assert_eq!(scan_takes(&dir), 1);
        assert_eq!(store.next_take(&dir), 5);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn delete_take_files_removes_wavs_including_mix() {
        let dir = temp_dir();
        fs::write(dir.join("7-ch-01-Rytm.wav"), []).unwrap();
        fs::write(dir.join("7-ret-A-BigSky.wav"), []).unwrap();
        fs::write(dir.join("7-mix.wav"), []).unwrap();
        fs::write(dir.join("8-ch-01-Rytm.wav"), []).unwrap();
        fs::write(dir.join("notes.txt"), []).unwrap();
        let deleted = delete_take_files(&dir, 7);
        assert_eq!(deleted, vec!["7-ch-01-Rytm.wav", "7-mix.wav", "7-ret-A-BigSky.wav"]);
        assert!(!dir.join("7-ch-01-Rytm.wav").exists());
        assert!(!dir.join("7-ret-A-BigSky.wav").exists());
        assert!(!dir.join("7-mix.wav").exists());
        assert!(dir.join("8-ch-01-Rytm.wav").exists());
        assert!(dir.join("notes.txt").exists());
        assert!(delete_take_files(&dir, 7).is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_take_infos_hides_mix_wav() {
        let dir = temp_dir();
        fs::write(dir.join("7-ch-01-Rytm.wav"), []).unwrap();
        fs::write(dir.join("7-ret-A-BigSky.wav"), []).unwrap();
        fs::write(dir.join("7-mix.wav"), []).unwrap();
        let takes = scan_take_infos(&dir, 48_000.0);
        assert_eq!(takes.len(), 1);
        assert_eq!(takes[0].number, 7);
        assert_eq!(takes[0].files.len(), 2);
        assert!(takes[0].files.iter().all(|f| f.lane != MixLane::Main));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn project_meta_reads_mixlink_active_mix_id() {
        let dir = temp_dir();
        let id = Uuid::from_u128(0xE2F1556A_BF54_41FF_9C00_5007D6A285FD);
        fs::write(
            dir.join("project.json"),
            format!(
                r#"{{"nextTake":8,"tempo":123,"mixes":[{{"id":"{id}","name":"Mix 1"}}],"activeMixID":"{id}","arrangement":{{"kind":"take","number":7}},"gridEnabled":false,"grid":1,"pixelsPerBar":14.2}}"#
            ),
        )
        .unwrap();
        let store = ProjectStore::new();
        let meta = store.load_meta(&dir);
        assert_eq!(meta.next_take, 8);
        assert_eq!(meta.tempo, 123.0);
        assert_eq!(meta.active_mix_id, Some(id));
        assert_eq!(meta.arrangement, Some(crate::MixArrangement::Take(7)));
        let _ = fs::remove_dir_all(&dir);
    }

    fn test_config(root: &Path) -> SessionConfig {
        let mut config = analog::SessionConfig::new();
        config.projects_root_bookmark = ProjectStore::bookmark_for(root);
        config
    }

    #[test]
    fn create_project_uses_date_then_dash_2() {
        let root = temp_dir();
        let mut config = test_config(&root);
        let store = ProjectStore::new();
        let date = today_prefix();
        let first = store.create_project(&mut config).unwrap();
        assert_eq!(first.file_name().unwrap().to_string_lossy(), date);
        assert!(first.join("project.json").exists());
        assert_eq!(config.current_project_relative.as_deref(), Some(date.as_str()));
        let second = store.create_project(&mut config).unwrap();
        assert_eq!(second.file_name().unwrap().to_string_lossy(), format!("{date}-2"));
        let third = store.create_project(&mut config).unwrap();
        assert_eq!(third.file_name().unwrap().to_string_lossy(), format!("{date}-3"));
        let listed = ProjectStore::list_projects(&config);
        assert_eq!(listed, vec![date.clone(), format!("{date}-2"), format!("{date}-3")]);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn rename_current_ignores_empty_and_renames_folder() {
        let root = temp_dir();
        let mut config = test_config(&root);
        let store = ProjectStore::new();
        let created = store.create_project(&mut config).unwrap();
        let original = created.file_name().unwrap().to_string_lossy().into_owned();
        let same = store.rename_current("   ", &mut config).unwrap();
        assert_eq!(same.file_name().unwrap().to_string_lossy(), original);
        assert!(created.exists());
        let renamed = store.rename_current("  Night session  ", &mut config).unwrap();
        assert_eq!(renamed.file_name().unwrap().to_string_lossy(), "Night session");
        assert!(!created.exists());
        assert!(renamed.join("project.json").exists());
        assert_eq!(config.current_project_relative.as_deref(), Some("Night session"));
        let _ = fs::remove_dir_all(&root);
    }
}
