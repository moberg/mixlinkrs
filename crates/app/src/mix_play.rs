//! MixLink `MixPlayer`: background thread fills per-lane rings; IOProc mixes.

use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use arc_swap::ArcSwap;
use engine::{Engine, RtControls};
use engine_api::MIX_PLAY_MAX_LANES;
use hound::WavReader;
use project::MixClip;

const SEEK_NONE: i64 = i64::MIN;
const DECLICK_MS: f64 = 3.0;

pub struct MixPlayTrack {
    pub clips: Vec<MixClip>,
    pub is_main: bool,
}

pub struct MixPlayGraph {
    pub folder: PathBuf,
    pub tracks: Vec<MixPlayTrack>,
    pub end_frame: i64,
    pub sample_rate: f64,
}

impl MixPlayGraph {
    pub fn live_end(&self) -> i64 {
        self.tracks
            .iter()
            .filter(|t| !t.is_main)
            .flat_map(|t| t.clips.iter())
            .map(MixClip::mix_end_frame)
            .max()
            .unwrap_or(self.end_frame)
            .max(self.end_frame)
    }
}

pub struct MixPlayer {
    stop: Arc<AtomicBool>,
    seek: Arc<AtomicI64>,
    running: Arc<AtomicBool>,
    graph: Arc<ArcSwap<MixPlayGraph>>,
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
        let graph = Arc::new(ArcSwap::from_pointee(graph));
        let flag = stop.clone();
        let seek_flag = seek.clone();
        let live = running.clone();
        let graph_flag = graph.clone();
        let engine_addr = engine as usize;
        let block = io_block.max(64);
        let thread = thread::Builder::new()
            .name("mixlink.mixplay".into())
            .spawn(move || {
                render_loop(engine_addr, controls, graph_flag, start, block, flag, seek_flag);
                live.store(false, Ordering::Relaxed);
            })
            .ok();
        Self { stop, seek, running, graph, thread }
    }

    pub fn publish_graph(&self, graph: MixPlayGraph) {
        self.graph.store(Arc::new(graph));
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
    graph: Arc<ArcSwap<MixPlayGraph>>,
    start: i64,
    block: usize,
    stop: Arc<AtomicBool>,
    seek: Arc<AtomicI64>,
) {
    let engine = engine_addr as *mut Engine;
    let initial = graph.load();
    let mut end = initial.live_end();
    if end <= 0 {
        return;
    }
    let mut lane_count;
    let mut files: HashMap<String, WavReader<BufReader<File>>> = HashMap::new();
    let mut lane_l = vec![vec![0.0f32; block]; MIX_PLAY_MAX_LANES];
    let mut lane_r = vec![vec![0.0f32; block]; MIX_PLAY_MAX_LANES];
    let mut mix_l = vec![0.0f32; block];
    let mut mix_r = vec![0.0f32; block];
    let mut frame = start.clamp(0, end);
    let mut playback_live = false;

    while !stop.load(Ordering::Relaxed) {
        let live = graph.load();
        end = live.live_end();
        lane_count = live.tracks.len().min(MIX_PLAY_MAX_LANES);
        if frame >= end {
            break;
        }
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
        for (slot, track) in live.tracks.iter().take(lane_count).enumerate() {
            lane_l[slot][..n].fill(0.0);
            lane_r[slot][..n].fill(0.0);
            fill_clips(
                &track.clips,
                &mut lane_l[slot][..n],
                &mut lane_r[slot][..n],
                frame,
                &live.folder,
                live.sample_rate,
                &mut files,
            );
            if !track.is_main {
                for i in 0..n {
                    mix_l[i] += lane_l[slot][i];
                    mix_r[i] += lane_r[slot][i];
                }
            }
        }
        if let Some(main) = live.tracks.iter().take(lane_count).position(|t| t.is_main) {
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
    sample_rate: f64,
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
        let mut pos = overlap_start;
        while pos < overlap_end {
            let local = pos - clip.mix_start_frame;
            let remain = overlap_end - pos;
            let (source_frame, run) = source_run(clip, local, remain);
            let dest_offset = (pos - start) as usize;
            let _ = read_clip_overlap(
                folder,
                &clip.source_file,
                source_frame,
                run,
                dest_offset,
                dest_l,
                dest_r,
                sample_rate,
                files,
                |i| edge_gain(clip, local + i as i64, sample_rate),
            );
            pos += run as i64;
        }
    }
}

fn source_run(clip: &MixClip, local: i64, remain: i64) -> (i64, usize) {
    if clip.looped && clip.loop_frames > 0 {
        let loop_pos = local.rem_euclid(clip.loop_frames);
        let src = clip.source_start_frame + loop_pos;
        let run = (clip.loop_frames - loop_pos).min(remain).max(1) as usize;
        (src, run)
    } else {
        (clip.source_start_frame + local, remain.max(0) as usize)
    }
}

fn edge_gain(clip: &MixClip, local: i64, rate: f64) -> f32 {
    let n = clip.source_frame_count.max(1);
    let declick = ((rate * DECLICK_MS / 1000.0) as i64).max(8).min(n / 2);
    let fade_in = clip.fade_in_frames.max(declick).min(n / 2);
    let fade_out = clip.fade_out_frames.max(declick).min(n / 2);
    let mut g = 1.0f32;
    if local < fade_in {
        g *= local as f32 / fade_in.max(1) as f32;
    }
    let remaining = n - local;
    if remaining < fade_out {
        g *= remaining as f32 / fade_out.max(1) as f32;
    }
    g.clamp(0.0, 1.0)
}

fn read_clip_overlap(
    folder: &Path,
    name: &str,
    source_frame: i64,
    count: usize,
    dest_offset: usize,
    dest_l: &mut [f32],
    dest_r: &mut [f32],
    _sample_rate: f64,
    files: &mut HashMap<String, WavReader<BufReader<File>>>,
    mut gain: impl FnMut(usize) -> f32,
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
    let ch = spec.channels.max(1) as usize;
    let remaining = (reader.duration() as i64 - source_frame).max(0) as usize;
    let n = count.min(remaining);
    if n == 0 || source_frame < 0 {
        return false;
    }
    if reader.seek(source_frame as u32).is_err() {
        return false;
    }
    match spec.sample_format {
        hound::SampleFormat::Float => copy_overlap(
            &mut reader.samples::<f32>(),
            n,
            ch,
            dest_offset,
            dest_l,
            dest_r,
            &mut gain,
            |s| s,
        ),
        hound::SampleFormat::Int => {
            let max = match spec.bits_per_sample {
                8 => 128.0,
                16 => 32768.0,
                24 => 8_388_608.0,
                _ => 2_147_483_648.0,
            };
            copy_overlap(
                &mut reader.samples::<i32>(),
                n,
                ch,
                dest_offset,
                dest_l,
                dest_r,
                &mut gain,
                |s| s as f32 / max,
            )
        }
    }
}

fn copy_overlap<S>(
    samples: &mut impl Iterator<Item = Result<S, hound::Error>>,
    n: usize,
    ch: usize,
    dest_offset: usize,
    dest_l: &mut [f32],
    dest_r: &mut [f32],
    gain: &mut impl FnMut(usize) -> f32,
    to_f32: impl Fn(S) -> f32,
) -> bool {
    for i in 0..n {
        let sl = samples.next().and_then(|s| s.ok()).map(&to_f32).unwrap_or(0.0);
        let sr = if ch > 1 {
            samples.next().and_then(|s| s.ok()).map(&to_f32).unwrap_or(0.0)
        } else {
            sl
        };
        for _ in 2..ch {
            let _ = samples.next();
        }
        let g = gain(i);
        let dest = dest_offset + i;
        if dest < dest_l.len() {
            dest_l[dest] += sl * g;
        }
        if dest < dest_r.len() {
            dest_r[dest] += sr * g;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use project::{MixClip, MixLane};
    use uuid::Uuid;

    fn clip(file: &str, src: i64, count: i64, mix: i64) -> MixClip {
        let mut c = MixClip::new(1, MixLane::Strip(0), file, src, count, mix, src + count);
        c.id = Uuid::new_v4();
        c
    }

    #[test]
    fn fill_clips_reads_overlap() {
        let dir = std::env::temp_dir().join(format!("mixlink-mixplay-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("take.wav");
        let left: Vec<f32> = (0..2000).map(|i| i as f32).collect();
        let right: Vec<f32> = (0..2000).map(|i| i as f32 + 0.5).collect();
        asset::write_bounce(&path, 48_000, &left, &right).unwrap();

        let clip = clip("take.wav", 1, 1800, 2);
        let mut dest_l = vec![0.0f32; 8];
        let mut dest_r = vec![0.0f32; 8];
        let mut files = HashMap::new();
        fill_clips(&[clip], &mut dest_l, &mut dest_r, 500, &dir, 48_000.0, &mut files);
        assert_eq!(dest_l[0], 499.0);
        assert_eq!(dest_r[0], 499.5);
        assert_eq!(dest_l[3], 502.0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fill_clips_two_files_with_gap() {
        let dir = std::env::temp_dir().join(format!("mixlink-mixplay-gap-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let a: Vec<f32> = vec![1.0; 10_000];
        let b: Vec<f32> = vec![2.0; 10_000];
        asset::write_bounce(&dir.join("a.wav"), 48_000, &a, &a).unwrap();
        asset::write_bounce(&dir.join("b.wav"), 48_000, &b, &b).unwrap();
        let clips = [clip("a.wav", 0, 4_000, 0), clip("b.wav", 0, 4_000, 8_000)];
        let mut dest_l = vec![0.0f32; 16];
        let mut dest_r = vec![0.0f32; 16];
        let mut files = HashMap::new();
        fill_clips(&clips, &mut dest_l, &mut dest_r, 2_000, &dir, 48_000.0, &mut files);
        assert!((dest_l[8] - 1.0).abs() < 1e-5, "first file {}", dest_l[8]);
        dest_l.fill(0.0);
        dest_r.fill(0.0);
        fill_clips(&clips, &mut dest_l, &mut dest_r, 5_000, &dir, 48_000.0, &mut files);
        assert!(dest_l.iter().all(|s| *s == 0.0), "gap should be silence");
        dest_l.fill(0.0);
        dest_r.fill(0.0);
        fill_clips(&clips, &mut dest_l, &mut dest_r, 9_000, &dir, 48_000.0, &mut files);
        assert!((dest_l[8] - 2.0).abs() < 1e-5, "second file {}", dest_l[8]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fill_clips_plays_a_file_recorded_at_another_rate() {
        let dir = std::env::temp_dir().join(format!("mixlink-mixplay-rate-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let pcm: Vec<f32> = vec![0.25; 2_000];
        asset::write_bounce(&dir.join("7-ch-01-Kick.wav"), 44_100, &pcm, &pcm).unwrap();
        let clip = clip("7-ch-01-Kick.wav", 0, 1_000, 0);
        let mut dest_l = vec![0.0f32; 8];
        let mut dest_r = vec![0.0f32; 8];
        let mut files = HashMap::new();
        fill_clips(&[clip], &mut dest_l, &mut dest_r, 200, &dir, 48_000.0, &mut files);
        assert!(
            dest_l.iter().any(|s| *s > 0.2),
            "take from another session must still play, got {dest_l:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
