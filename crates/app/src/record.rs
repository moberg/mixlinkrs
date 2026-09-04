//! Take WAV writer. UI arms rings, then `recording = 1`. Stop flips the flag
//! and drains rings after the IOProc has left them alone.

use std::path::Path;
use std::thread;
use std::time::Duration;

use analog::{AnalogEngine, ReturnLane};
use engine::Engine;
use engine_api::{MASTER_TAP, TAP_COUNT};
use project::{take_wav_name, MixLane, ProjectStore};

pub fn arm_and_start(engine: *mut Engine, analog: &AnalogEngine) {
    unsafe {
        let engine = &mut *engine;
        for tap in 0..TAP_COUNT {
            let on = tap_enabled(analog, tap);
            engine.enable_ring(tap, on);
        }
    }
}

pub fn finish_take(engine: *mut Engine, analog: &AnalogEngine, take: i32) {
    thread::sleep(Duration::from_millis(40));
    let Some(folder) = ProjectStore::current_url(&analog.config) else {
        log::warn!("record: no current project");
        return;
    };
    unsafe {
        let engine = &mut *engine;
        for tap in 0..TAP_COUNT {
            if !tap_enabled(analog, tap) {
                continue;
            }
            let mut left = vec![0.0f32; 48_000 * 8];
            let mut right = vec![0.0f32; 48_000 * 8];
            let n = engine.copy_record_frames(tap, &mut left, &mut right);
            if n <= 0 {
                continue;
            }
            left.truncate(n as usize);
            right.truncate(n as usize);
            let lane = tap_lane(tap);
            let name = tap_name(analog, lane);
            let filename = take_wav_name(take, lane, &name);
            let path = folder.join(&filename);
            if let Err(e) = asset::write_bounce(&path, 48_000, &left, &right) {
                log::error!("record {filename}: {e}");
            } else {
                log::info!("wrote {}", path.display());
            }
        }
    }
}

fn tap_enabled(analog: &AnalogEngine, tap: usize) -> bool {
    if tap == MASTER_TAP {
        return true;
    }
    if tap < 8 {
        return analog.config.strips.get(tap).is_some_and(|s| s.enabled);
    }
    let lane = ReturnLane::ALL.get(tap - 8).copied();
    lane.is_some_and(|l| analog.config.is_return_enabled(l))
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
                analog::EffectRef::Plugin(id) => analog.config.plugin(id).map(|p| p.title()).unwrap_or_default(),
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
    let mut takes = Vec::new();
    if let Ok(rd) = std::fs::read_dir(folder) {
        for ent in rd.flatten() {
            if let Some(name) = ent.file_name().to_str() {
                if let Some((take, _, _)) = project::parse_take_wav(name) {
                    if !takes.contains(&take) {
                        takes.push(take);
                    }
                }
            }
        }
    }
    takes.sort_unstable();
    takes
}
