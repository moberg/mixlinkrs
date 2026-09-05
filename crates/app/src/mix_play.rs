//! MixLink `MixPlayer`: background thread fills per-lane rings; IOProc mixes.

use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use engine::{Engine, RtControls};
use engine_api::MIX_PLAY_MAX_LANES;
use hound::WavReader;
use project::MixClip;

const SEEK_NONE: i64 = i64::MIN;

pub struct MixPlayTrack {
    pub clips: Vec<MixClip>,
    pub is_main: bool,
}

pub struct MixPlayGraph {
    pub folder: PathBuf,
    pub tracks: Vec<MixPlayTrack>,
    pub end_frame: i64,
}

pub struct MixPlayer {
    stop: Arc<AtomicBool>,
    seek: Arc<AtomicI64>,
    running: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl MixPlayer {
    pub fn start(
        engine: *mut Engine,
        controls: Arc<RtControls>,
        graph: MixPlayGraph,
        start: i64,
        io_block: usize,
    ) -> Self {
        unsafe {
            (*engine).reset_mix_lanes();
        }
        controls.flush.store(false, Ordering::Relaxed);
        let stop = Arc::new(AtomicBool::new(false));
        let seek = Arc::new(AtomicI64::new(SEEK_NONE));
        let running = Arc::new(AtomicBool::new(true));
        let flag = stop.clone();
        let seek_flag = seek.clone();
        let live = running.clone();
        let engine_addr = engine as usize;
        let block = io_block.max(64);
        let thread = thread::Builder::new()
            .name("mixlink.mixplay".into())
            .spawn(move || {
                render_loop(engine_addr, controls, graph, start, block, flag, seek_flag);
                live.store(false, Ordering::Relaxed);
            })
            .ok();
        Self { stop, seek, running, thread }
    }

    pub fn request_seek(&self, frame: i64) {
        self.seek.store(frame.max(0), Ordering::Relaxed);
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        self.running.store(false, Ordering::Relaxed);
    }
}

impl Drop for MixPlayer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn render_loop(
    engine_addr: usize,
    controls: Arc<RtControls>,
    graph: MixPlayGraph,
    start: i64,
    block: usize,
    stop: Arc<AtomicBool>,
    seek: Arc<AtomicI64>,
) {
    let engine = engine_addr as *mut Engine;
    let end = graph.end_frame;
    if end <= 0 {
        return;
    }
    let lane_count = graph.tracks.len().min(MIX_PLAY_MAX_LANES);
    let mut files: HashMap<String, WavReader<BufReader<File>>> = HashMap::new();
    let mut lane_l = vec![vec![0.0f32; block]; lane_count];
    let mut lane_r = vec![vec![0.0f32; block]; lane_count];
    let mut mix_l = vec![0.0f32; block];
    let mut mix_r = vec![0.0f32; block];
    let mut frame = start.clamp(0, end);
    let mut playback_live = false;

    while !stop.load(Ordering::Relaxed) && frame < end {
        let pending = seek.swap(SEEK_NONE, Ordering::Relaxed);
        if pending != SEEK_NONE {
            frame = pending.clamp(0, end);
            controls.flush.store(true, Ordering::Relaxed);
        }
        let n = (end - frame).min(block as i64).max(0) as usize;
        if n == 0 {
            break;
        }
        mix_l[..n].fill(0.0);
        mix_r[..n].fill(0.0);
        for (slot, track) in graph.tracks.iter().take(lane_count).enumerate() {
            lane_l[slot][..n].fill(0.0);
            lane_r[slot][..n].fill(0.0);
            fill_clips(
                &track.clips,
                &mut lane_l[slot][..n],
                &mut lane_r[slot][..n],
                frame,
                &graph.folder,
                &mut files,
            );
            if !track.is_main {
                for i in 0..n {
                    mix_l[i] += lane_l[slot][i];
                    mix_r[i] += lane_r[slot][i];
                }
            }
        }
        if let Some(main) = graph.tracks.iter().take(lane_count).position(|t| t.is_main) {
            lane_l[main][..n].copy_from_slice(&mix_l[..n]);
            lane_r[main][..n].copy_from_slice(&mix_r[..n]);
        }

        let mut written = 0;
        while written < n && !stop.load(Ordering::Relaxed) {
            let mut room = n - written;
            for slot in 0..lane_count {
                room = room.min(unsafe { (*engine).lane_room(slot) });
            }
            if room == 0 {
                thread::sleep(Duration::from_millis(2));
                continue;
            }
            for slot in 0..lane_count {
                let _ = unsafe {
                    (*engine).push_lane_frames(
                        slot,
                        &lane_l[slot][written..written + room],
                        &lane_r[slot][written..written + room],
                    )
                };
            }
            written += room;
        }
        if !playback_live {
            controls.mix_playing.store(true, Ordering::Relaxed);
            playback_live = true;
        }
        frame += n as i64;
    }
}

pub fn fill_clips(
    clips: &[MixClip],
    dest_l: &mut [f32],
    dest_r: &mut [f32],
    start: i64,
    folder: &Path,
    files: &mut HashMap<String, WavReader<BufReader<File>>>,
) {
    let frames = dest_l.len().min(dest_r.len());
    let end = start + frames as i64;
    for clip in clips {
        let overlap_start = start.max(clip.mix_start_frame);
        let overlap_end = end.min(clip.mix_end_frame());
        if overlap_end <= overlap_start {
            continue;
        }
        let dest_offset = (overlap_start - start) as usize;
        let source_frame = clip.source_start_frame + (overlap_start - clip.mix_start_frame);
        let count = (overlap_end - overlap_start) as usize;
        if !read_clip_overlap(
            folder,
            &clip.source_file,
            source_frame,
            count,
            dest_offset,
            dest_l,
            dest_r,
            files,
        ) {
            continue;
        }
    }
}

fn read_clip_overlap(
    folder: &Path,
    name: &str,
    source_frame: i64,
    count: usize,
    dest_offset: usize,
    dest_l: &mut [f32],
    dest_r: &mut [f32],
    files: &mut HashMap<String, WavReader<BufReader<File>>>,
) -> bool {
    if !files.contains_key(name) {
        let path = folder.join(name);
        let Ok(reader) = WavReader::open(&path) else {
            return false;
        };
        files.insert(name.to_string(), reader);
    }
    let Some(reader) = files.get_mut(name) else {
        return false;
    };
    let spec = reader.spec();
    if spec.sample_format != hound::SampleFormat::Float {
        return false;
    }
    let ch = spec.channels.max(1) as usize;
    let remaining = (reader.duration() as i64 - source_frame).max(0) as usize;
    let n = count.min(remaining);
    if n == 0 || source_frame < 0 {
        return false;
    }
    if reader.seek(source_frame as u32).is_err() {
        return false;
    }
    let mut samples = reader.samples::<f32>();
    for i in 0..n {
        let sl = samples.next().and_then(|s| s.ok()).unwrap_or(0.0);
        let sr = if ch > 1 { samples.next().and_then(|s| s.ok()).unwrap_or(0.0) } else { sl };
        for _ in 2..ch {
            let _ = samples.next();
        }
        let dest = dest_offset + i;
        if dest < dest_l.len() {
            dest_l[dest] += sl;
        }
        if dest < dest_r.len() {
            dest_r[dest] += sr;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use project::{MixClip, MixLane};
    use uuid::Uuid;

    #[test]
    fn fill_clips_reads_overlap() {
        let dir = std::env::temp_dir().join(format!("mixlink-mixplay-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("take.wav");
        let left: Vec<f32> = (0..8).map(|i| i as f32).collect();
        let right: Vec<f32> = (0..8).map(|i| i as f32 + 0.5).collect();
        asset::write_bounce(&path, 48_000, &left, &right).unwrap();

        let clip = MixClip {
            id: Uuid::new_v4(),
            source_take: 1,
            source_lane: MixLane::Strip(0),
            source_file: "take.wav".into(),
            source_start_frame: 1,
            source_frame_count: 4,
            mix_start_frame: 2,
        };
        let mut dest_l = vec![0.0f32; 8];
        let mut dest_r = vec![0.0f32; 8];
        let mut files = HashMap::new();
        fill_clips(&[clip], &mut dest_l, &mut dest_r, 0, &dir, &mut files);
        assert_eq!(dest_l[2], 1.0);
        assert_eq!(dest_r[2], 1.5);
        assert_eq!(dest_l[5], 4.0);
        assert_eq!(dest_l[1], 0.0);
        assert_eq!(dest_l[6], 0.0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
