//! MixLink-style take recorder: rings first, then a background writer that
//! opens WAVs immediately and drains 4096-frame blocks until stop.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use analog::{AnalogEngine, ReturnLane};
use engine::Engine;
use engine_api::{MASTER_TAP, TAP_COUNT};
use project::{take_wav_name, MixLane, ProjectStore, TakeInfo};

const BLOCK: usize = 4096;

pub struct Recorder {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<bool>>,
    take: i32,
}

impl Recorder {
    /// Allocate rings, spawn the writer, then the caller flips the IOProc gate.
    pub fn start(
        engine: *mut Engine,
        analog: &AnalogEngine,
        folder: PathBuf,
        take: i32,
        sample_rate: u32,
    ) -> Option<Self> {
        if sample_rate == 0 {
            return None;
        }
        let tracks = record_tracks(analog, take);
        if tracks.is_empty() {
            log::warn!("record: no enabled tracks");
            return None;
        }
        unsafe {
            let engine = &mut *engine;
            for tap in 0..TAP_COUNT {
                engine.enable_ring(tap, false);
            }
            for track in &tracks {
                engine.enable_ring(track.tap, true);
            }
        }
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let engine_addr = engine as usize;
        let thread = thread::Builder::new()
            .name("mixlink.record".into())
            .spawn(move || write_loop(engine_addr, folder, tracks, sample_rate, flag))
            .ok()?;
        Some(Self { stop, thread: Some(thread), take })
    }

    pub fn stop(mut self, engine: *mut Engine) -> bool {
        self.stop.store(true, Ordering::Relaxed);
        let ok = self.thread.take().and_then(|t| t.join().ok()).unwrap_or(false);
        if ok {
            log::info!("recorded take {}", self.take);
        }
        unsafe {
            let engine = &mut *engine;
            for tap in 0..TAP_COUNT {
                engine.enable_ring(tap, false);
            }
        }
        ok
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct RecordTrack {
    tap: usize,
    filename: String,
}

fn record_tracks(analog: &AnalogEngine, take: i32) -> Vec<RecordTrack> {
    (0..TAP_COUNT)
        .filter(|&tap| tap_enabled(analog, tap))
        .map(|tap| RecordTrack {
            tap,
            filename: take_wav_name(take, tap_lane(tap), &tap_name(analog, tap_lane(tap))),
        })
        .collect()
}

fn write_loop(
    engine_addr: usize,
    folder: PathBuf,
    tracks: Vec<RecordTrack>,
    sample_rate: u32,
    stop: Arc<AtomicBool>,
) -> bool {
    let mut writers = Vec::with_capacity(tracks.len());
    for track in &tracks {
        let path = folder.join(&track.filename);
        match asset::StreamingWav::create(&path, sample_rate) {
            Ok(w) => {
                log::info!("record open {}", path.display());
                writers.push(w);
            }
            Err(e) => {
                log::error!("record {}: {e}", path.display());
                return false;
            }
        }
    }

    let mut left = vec![0.0f32; BLOCK];
    let mut right = vec![0.0f32; BLOCK];
    let engine = engine_addr as *mut Engine;

    while !stop.load(Ordering::Relaxed) {
        let mut idle = true;
        for (i, track) in tracks.iter().enumerate() {
            let n = unsafe { (*engine).copy_record_frames(track.tap, &mut left, &mut right) };
            if n < 0 {
                log::error!("record overrun on tap {}", track.tap);
                continue;
            }
            if n == 0 {
                continue;
            }
            idle = false;
            if let Err(e) = writers[i].write_block(&left[..n as usize], &right[..n as usize]) {
                log::error!("record write: {e}");
            }
        }
        if idle {
            thread::sleep(Duration::from_millis(2));
        }
    }

    loop {
        let mut drained = false;
        for (i, track) in tracks.iter().enumerate() {
            let n = unsafe { (*engine).copy_record_frames(track.tap, &mut left, &mut right) };
            if n <= 0 {
                continue;
            }
            drained = true;
            if let Err(e) = writers[i].write_block(&left[..n as usize], &right[..n as usize]) {
                log::error!("record flush: {e}");
            }
        }
        if !drained {
            break;
        }
    }

    for writer in writers {
        if let Err(e) = writer.finalize() {
            log::error!("record finalize: {e}");
        }
    }
    true
}

pub fn lane_on_record_list(analog: &AnalogEngine, lane: MixLane) -> bool {
    tap_on_record_list(analog, lane_tap(lane))
}

fn lane_tap(lane: MixLane) -> usize {
    match lane {
        MixLane::Strip(i) => (i as usize).min(7),
        MixLane::ReturnLane(lane) => 8 + lane as usize,
        MixLane::Main => MASTER_TAP,
    }
}

/// MixLink `AudioTap.recordTracks`: enabled strips, visible send returns,
/// both buses, master. Hidden extra sends stay off the take. Silence does
/// not skip a tap — TotalMix may fade the channel up mid-take.
fn tap_on_record_list(analog: &AnalogEngine, tap: usize) -> bool {
    if tap == MASTER_TAP {
        return true;
    }
    if tap < 8 {
        return analog.config.is_strip_enabled(tap);
    }
    let Some(lane) = ReturnLane::ALL.get(tap - 8).copied() else {
        return false;
    };
    if !analog.config.is_return_enabled(lane) {
        return false;
    }
    if lane.is_send() {
        analog.config.visible_send_lanes().contains(&lane)
    } else {
        true
    }
}

fn tap_enabled(analog: &AnalogEngine, tap: usize) -> bool {
    tap_on_record_list(analog, tap)
}

fn tap_lane(tap: usize) -> MixLane {
    if tap == MASTER_TAP {
        MixLane::Main
    } else if tap < 8 {
        MixLane::Strip(tap as i32)
    } else {
        MixLane::ReturnLane(ReturnLane::ALL[tap - 8])
    }
}

fn tap_name(analog: &AnalogEngine, lane: MixLane) -> String {
    match lane {
        MixLane::Strip(i) => analog
            .config
            .strips
            .get(i as usize)
            .map(|s| analog.display_name(s.channel_id()))
            .unwrap_or_else(|| lane.title()),
        MixLane::ReturnLane(r) => analog
            .config
            .effect_ref(r)
            .map(|e| match e {
                analog::EffectRef::Plugin(id) => {
                    analog.config.plugin(id).map(|p| p.title()).unwrap_or_default()
                }
                analog::EffectRef::Hardware(id) => {
                    analog.config.hardware_effect(id).map(|h| h.title()).unwrap_or_default()
                }
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| lane.title()),
        MixLane::Main => "Mix".into(),
    }
}

pub fn list_takes(folder: &Path) -> Vec<i32> {
    ProjectStore::new().scan_take_infos(folder, 48_000.0).into_iter().map(|t| t.number).collect()
}

pub fn list_take_infos(folder: &Path, sample_rate: f64) -> Vec<TakeInfo> {
    let mut takes = ProjectStore::new().scan_take_infos(folder, sample_rate);
    for take in &mut takes {
        for file in &mut take.files {
            let path = folder.join(&file.filename);
            file.frame_count = asset::wav_frame_count(&path);
            file.sample_rate = sample_rate;
        }
    }
    takes
}

pub fn planned_filenames(analog: &AnalogEngine, take: i32) -> Vec<String> {
    record_tracks(analog, take).into_iter().map(|t| t.filename).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use analog::{MixerState, OscSession, ReturnLane, SessionConfig, SurfaceState};

    fn test_analog() -> AnalogEngine {
        AnalogEngine::new(
            MixerState::new(),
            SurfaceState::new(),
            SessionConfig::new(),
            OscSession::new(),
        )
    }

    #[test]
    fn planned_names_include_strips_and_mix() {
        let analog = test_analog();
        let names = planned_filenames(&analog, 3);
        assert!(names.iter().any(|n| n.starts_with("3-ch-01-") && n.ends_with(".wav")));
        assert!(names.iter().any(|n| n == "3-mix.wav"));
    }

    #[test]
    fn planned_names_skip_hidden_send_returns() {
        let analog = test_analog();
        let names = planned_filenames(&analog, 3);
        assert!(names.iter().any(|n| n.contains("-ret-A-") || n.contains("-ret-B-")));
        assert!(names.iter().any(|n| n.starts_with("3-bus-")));
        assert!(names.iter().all(|n| {
            !n.contains("-ret-C-")
                && !n.contains("-ret-D-")
                && !n.contains("-ret-E-")
                && !n.contains("-ret-F-")
        }));
    }

    #[test]
    fn lane_on_record_list_matches_compact_taps() {
        let analog = test_analog();
        assert!(lane_on_record_list(&analog, MixLane::Strip(0)));
        assert!(lane_on_record_list(&analog, MixLane::ReturnLane(ReturnLane::SendA)));
        assert!(lane_on_record_list(&analog, MixLane::ReturnLane(ReturnLane::Bus2)));
        assert!(!lane_on_record_list(&analog, MixLane::ReturnLane(ReturnLane::SendC)));
        assert!(!lane_on_record_list(&analog, MixLane::ReturnLane(ReturnLane::SendF)));
    }

    #[test]
    fn visible_send_is_armed_even_without_effect() {
        let mut analog = test_analog();
        analog.config.effect_return_count = 3;
        analog.config.ensure_return_lane(ReturnLane::SendC);
        let names = planned_filenames(&analog, 3);
        assert!(names.iter().any(|n| n.contains("-ret-C-")));
        assert!(names.iter().all(|n| !n.contains("-ret-D-")));
    }

    #[test]
    fn list_takes_skips_mix_wav() {
        let dir = std::env::temp_dir().join(format!("mixlink-rec-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("7-ch-01-Rytm.wav"), []).unwrap();
        std::fs::write(dir.join("7-mix.wav"), []).unwrap();
        std::fs::write(dir.join("notes.txt"), []).unwrap();
        assert_eq!(list_takes(&dir), vec![7]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
