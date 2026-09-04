use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;

use arc_swap::ArcSwap;
use atomic_float::AtomicF32;
use dsp_core::MAX_INTERNAL_BLOCK;
use engine_api::{EngineEvent, UiCommand, MASTER_PLUGIN_SLOT, MASTER_TAP, MIX_PLAY_MAX_LANES, TAP_COUNT};
use rt_utils::spsc::{spsc_bounded, Consumer, Producer};

use crate::schedule::{RtControls, Schedule};
use crate::taps::display_level;

const MAX_FRAMES: usize = 4096;
const RING_SECONDS: f32 = 8.0;

pub struct Engine {
    schedule: Arc<ArcSwap<Schedule>>,
    controls: Arc<RtControls>,
    cmd_rx: Consumer<UiCommand>,
    ev_tx: Producer<EngineEvent>,
    sample_rate: u32,
    pos: Arc<AtomicI64>,
    xruns: Arc<AtomicU64>,
    peaks: Arc<[AtomicF32; TAP_COUNT]>,
    scratch_in_l: Vec<f32>,
    scratch_in_r: Vec<f32>,
    scratch_out_l: Vec<f32>,
    scratch_out_r: Vec<f32>,
    master_l: Vec<f32>,
    master_r: Vec<f32>,
    rings_l: [Vec<f32>; TAP_COUNT],
    rings_r: [Vec<f32>; TAP_COUNT],
    ring_cap: usize,
    ring_write: [usize; TAP_COUNT],
    ring_read: [usize; TAP_COUNT],
    ring_enabled: [bool; TAP_COUNT],
    lane_rings_l: [Vec<f32>; MIX_PLAY_MAX_LANES],
    lane_rings_r: [Vec<f32>; MIX_PLAY_MAX_LANES],
    lane_write: [usize; MIX_PLAY_MAX_LANES],
    lane_read: [usize; MIX_PLAY_MAX_LANES],
    lane_cap: usize,
    /// First IOProc on a thread may touch TLS (ArcSwap, no-alloc depth).
    rt_tls_warmed: bool,
}

pub struct EngineHandles {
    pub cmd_tx: Producer<UiCommand>,
    pub ev_rx: Consumer<EngineEvent>,
    pub schedule: Arc<ArcSwap<Schedule>>,
    pub controls: Arc<RtControls>,
    pub xruns: Arc<AtomicU64>,
    pub sample_position: Arc<AtomicI64>,
    pub peaks: Arc<[AtomicF32; TAP_COUNT]>,
}

impl Engine {
    pub fn new(sample_rate: u32) -> (Self, EngineHandles) {
        let (cmd_tx, cmd_rx) = spsc_bounded(1024);
        let (ev_tx, ev_rx) = spsc_bounded(4096);
        let schedule = Arc::new(ArcSwap::new(Arc::new(Schedule::empty())));
        let controls = Arc::new(RtControls::default());
        let xruns = Arc::new(AtomicU64::new(0));
        let pos = Arc::new(AtomicI64::new(0));
        let peaks = Arc::new(std::array::from_fn(|_| AtomicF32::new(0.0)));
        let ring_cap = ((sample_rate as f32 * RING_SECONDS) as usize).max(MAX_FRAMES * 2);
        let lane_cap = (sample_rate as usize * 2).max(MAX_FRAMES * 8);

        let engine = Engine {
            schedule: schedule.clone(),
            controls: controls.clone(),
            cmd_rx,
            ev_tx,
            sample_rate,
            pos: pos.clone(),
            xruns: xruns.clone(),
            peaks: peaks.clone(),
            scratch_in_l: vec![0.0; MAX_FRAMES],
            scratch_in_r: vec![0.0; MAX_FRAMES],
            scratch_out_l: vec![0.0; MAX_FRAMES],
            scratch_out_r: vec![0.0; MAX_FRAMES],
            master_l: vec![0.0; MAX_FRAMES],
            master_r: vec![0.0; MAX_FRAMES],
            rings_l: std::array::from_fn(|_| vec![0.0; ring_cap]),
            rings_r: std::array::from_fn(|_| vec![0.0; ring_cap]),
            ring_cap,
            ring_write: [0; TAP_COUNT],
            ring_read: [0; TAP_COUNT],
            ring_enabled: [false; TAP_COUNT],
            lane_rings_l: std::array::from_fn(|_| vec![0.0; lane_cap]),
            lane_rings_r: std::array::from_fn(|_| vec![0.0; lane_cap]),
            lane_write: [0; MIX_PLAY_MAX_LANES],
            lane_read: [0; MIX_PLAY_MAX_LANES],
            lane_cap,
            rt_tls_warmed: false,
        };
        (
            engine,
            EngineHandles { cmd_tx, ev_rx, schedule, controls, xruns, sample_position: pos, peaks },
        )
    }

    pub fn enable_ring(&mut self, tap: usize, on: bool) {
        if tap < TAP_COUNT {
            self.ring_enabled[tap] = on;
            self.ring_write[tap] = 0;
            self.ring_read[tap] = 0;
        }
    }

    pub fn copy_record_frames(&mut self, tap: usize, dst_l: &mut [f32], dst_r: &mut [f32]) -> i32 {
        if tap >= TAP_COUNT {
            return 0;
        }
        let cap = self.ring_cap;
        let mut read = self.ring_read[tap];
        let write = self.ring_write[tap];
        let available = (write + cap - read) % cap;
        if available >= cap.saturating_sub(1) {
            self.ring_read[tap] = write;
            let _ = self.ev_tx.try_push(EngineEvent::RecordOverrun { tap: tap as u8 });
            return -1;
        }
        let n = available.min(dst_l.len()).min(dst_r.len());
        for i in 0..n {
            dst_l[i] = self.rings_l[tap][read];
            dst_r[i] = self.rings_r[tap][read];
            read = (read + 1) % cap;
        }
        self.ring_read[tap] = read;
        n as i32
    }

    pub fn push_lane_frames(&mut self, lane: usize, l: &[f32], r: &[f32]) {
        if lane >= MIX_PLAY_MAX_LANES {
            return;
        }
        let n = l.len().min(r.len());
        let cap = self.lane_cap;
        for i in 0..n {
            let w = self.lane_write[lane];
            self.lane_rings_l[lane][w] = l[i];
            self.lane_rings_r[lane][w] = r[i];
            self.lane_write[lane] = (w + 1) % cap;
        }
    }

    /// CoreAudio IOProc entry. No alloc / lock / file I/O.
    pub fn process(
        &mut self,
        input: BufferList<'_>,
        output: BufferList<'_>,
        frames: usize,
        _host_time: u64,
    ) {
        // ArcSwap hazard pointers and the no-alloc TLS slot allocate on first
        // use on a thread. Load the schedule and skip the debug guard on the
        // first callback so that warmup cannot abort the process.
        let schedule = self.schedule.load();
        let warmed = self.rt_tls_warmed;
        if !warmed {
            let _ = rt_utils::alloc_guard::is_alloc_allowed();
            self.rt_tls_warmed = true;
        }

        #[cfg(debug_assertions)]
        let _guard = warmed.then(rt_utils::NoAllocGuard::new);

        let frames = frames.min(MAX_FRAMES);
        if frames == 0 {
            return;
        }

        self.cmd_rx.drain(32, |cmd| match cmd {
            UiCommand::TransportPlay => self.controls.mix_playing.store(true, Ordering::Relaxed),
            UiCommand::TransportStop => {
                self.controls.mix_playing.store(false, Ordering::Relaxed);
                self.pos.store(0, Ordering::Relaxed);
            }
            UiCommand::TransportSeek { sample } => {
                self.pos.store(sample.max(0), Ordering::Relaxed);
                self.controls.flush.store(true, Ordering::Relaxed);
            }
            UiCommand::SetTempo { bpm } => {
                self.controls.tempo_milli.store((bpm * 1000.0) as i32, Ordering::Relaxed);
            }
            UiCommand::SetRecording { on } => {
                if on {
                    for i in 0..TAP_COUNT {
                        self.ring_write[i] = 0;
                        self.ring_read[i] = 0;
                    }
                }
                self.controls.recording.store(on, Ordering::Relaxed);
            }
            UiCommand::ArmRings | UiCommand::FlushMix => {
                self.controls.flush.store(true, Ordering::Relaxed);
            }
        });

        let mix_playing = self.controls.mix_playing.load(Ordering::Relaxed);
        let recording = self.controls.recording.load(Ordering::Relaxed);
        let any_plugin = schedule.routes.iter().any(|r| r.enabled);

        if any_plugin || mix_playing {
            zero_output(&output, frames);
        } else {
            // Phase-1 sanity: dry duplex of input pair 0/1 → output pair 0/1.
            copy_pair(&input, &output, 0, 1, frames);
            accumulate_master(self, &input, 0, 1, frames);
        }

        if any_plugin || mix_playing {
            self.master_l[..frames].fill(0.0);
            self.master_r[..frames].fill(0.0);
        }

        if !mix_playing {
            for slot in 0..8 {
                let route = &schedule.routes[slot];
                if !route.enabled {
                    if recording {
                        for tap in 0..TAP_COUNT {
                            if self.ring_enabled[tap] && schedule.taps[tap].plugin_slot == slot as i32 {
                                write_silence_ring(self, tap, frames);
                            }
                        }
                    }
                    continue;
                }
                self.scratch_in_l[..frames].fill(0.0);
                self.scratch_in_r[..frames].fill(0.0);
                for feed in &route.feeds {
                    if feed.gain <= 0.0001 || feed.channel < 0 {
                        continue;
                    }
                    mix_input_channel(&input, feed.channel, feed.gain, feed.linked, frames, &mut self.scratch_in_l, &mut self.scratch_in_r);
                }
                vst3_host::process_slot(
                    slot as u32,
                    &self.scratch_in_l[..frames],
                    &self.scratch_in_r[..frames],
                    &mut self.scratch_out_l[..frames],
                    &mut self.scratch_out_r[..frames],
                );
                if route.return_channel >= 0 {
                    write_output_pair(&output, route.return_channel, frames, &self.scratch_out_l, &self.scratch_out_r);
                }
                hold_plugin_taps(self, &schedule, slot as i32, frames, recording);
            }
        }

        for tap_i in 0..TAP_COUNT {
            let tap = schedule.taps[tap_i];
            if tap.plugin_slot >= 0 {
                continue;
            }
            if tap.plugin_slot == MASTER_PLUGIN_SLOT {
                continue;
            }
            let peak = if tap.channel_l >= 0 {
                peak_from_input(&input, tap.channel_l, tap.channel_r, frames)
            } else {
                0.0
            };
            hold_peak(&self.peaks[tap_i], peak);
            if recording && self.ring_enabled[tap_i] {
                if tap.channel_l >= 0 {
                    write_ring_from_ptrs(self, tap_i, &input, tap.channel_l, tap.channel_r, frames);
                } else {
                    write_silence_ring(self, tap_i, frames);
                }
            }
        }

        hold_peak(&self.peaks[MASTER_TAP], peak_of(&self.master_l[..frames], &self.master_r[..frames]));
        if recording && self.ring_enabled[MASTER_TAP] {
            write_ring_slices(
                &mut self.rings_l,
                &mut self.rings_r,
                &mut self.ring_write,
                self.ring_cap,
                MASTER_TAP,
                &self.master_l[..frames],
                &self.master_r[..frames],
            );
        }

        if mix_playing {
            if self.controls.flush.swap(false, Ordering::Relaxed) {
                for i in 0..MIX_PLAY_MAX_LANES {
                    self.lane_read[i] = self.lane_write[i];
                }
            }
            mix_playback(self, &schedule, &output, frames);
        }

        let pos = self.pos.load(Ordering::Relaxed);
        self.pos.store(pos + frames as i64, Ordering::Relaxed);
        let _ = MAX_INTERNAL_BLOCK;
    }

    pub fn display_peaks(&self) -> [f32; TAP_COUNT] {
        std::array::from_fn(|i| display_level(self.peaks[i].load(Ordering::Relaxed)))
    }
}

/// Borrowed CoreAudio buffer list. Channel lookup is MixLink `locateChannel`.
#[derive(Clone, Copy)]
pub struct BufferList<'a> {
    pub buffers: &'a [AudioBuf],
}

#[derive(Clone, Copy)]
pub struct AudioBuf {
    pub data: *mut f32,
    pub channels: u32,
    pub frames: u32,
}

impl BufferList<'_> {
    pub fn locate(&self, channel: i32) -> Option<(*mut f32, usize)> {
        if channel < 0 {
            return None;
        }
        let mut base = 0i32;
        for buf in self.buffers {
            let ch = buf.channels as i32;
            if channel < base + ch {
                let offset = (channel - base) as usize;
                if buf.data.is_null() {
                    return None;
                }
                return Some((buf.data, offset.max(1) * 0 + offset)); // stride = channels if interleaved
            }
            base += ch;
        }
        None
    }

    pub fn locate_sample(&self, channel: i32, frame: usize) -> f32 {
        let mut base = 0i32;
        for buf in self.buffers {
            let ch = buf.channels as i32;
            if channel >= base && channel < base + ch {
                if buf.data.is_null() {
                    return 0.0;
                }
                let local = (channel - base) as usize;
                let stride = buf.channels as usize;
                // Interleaved: sample = data[frame * stride + local]
                // Non-interleaved: one channel per buffer (stride 1, local 0)
                let idx = if buf.channels <= 1 {
                    frame
                } else {
                    frame * stride + local
                };
                unsafe {
                    return *buf.data.add(idx);
                }
            }
            base += ch;
        }
        0.0
    }

    pub fn write_sample(&self, channel: i32, frame: usize, value: f32) {
        let mut base = 0i32;
        for buf in self.buffers {
            let ch = buf.channels as i32;
            if channel >= base && channel < base + ch {
                if buf.data.is_null() {
                    return;
                }
                let local = (channel - base) as usize;
                let stride = buf.channels as usize;
                let idx = if buf.channels <= 1 { frame } else { frame * stride + local };
                unsafe {
                    *buf.data.add(idx) = value;
                }
                return;
            }
            base += ch;
        }
    }
}

fn zero_output(output: &BufferList<'_>, frames: usize) {
    for buf in output.buffers {
        if buf.data.is_null() {
            continue;
        }
            let n = frames * buf.channels.max(1) as usize;
        unsafe {
            std::ptr::write_bytes(buf.data as *mut u8, 0, n * std::mem::size_of::<f32>());
        }
    }
}

fn mix_input_channel(
    input: &BufferList<'_>,
    channel: i32,
    gain: f32,
    linked: bool,
    frames: usize,
    dest_l: &mut [f32],
    dest_r: &mut [f32],
) {
    for i in 0..frames {
        let s = input.locate_sample(channel, i) * gain;
        dest_l[i] += s;
        if linked {
            dest_r[i] += input.locate_sample(channel + 1, i) * gain;
        } else {
            dest_r[i] += s;
        }
    }
}

fn copy_pair(input: &BufferList<'_>, output: &BufferList<'_>, left: i32, right: i32, frames: usize) {
    for i in 0..frames {
        output.write_sample(left, i, input.locate_sample(left, i));
        output.write_sample(right, i, input.locate_sample(right, i));
    }
}

fn accumulate_master(engine: &mut Engine, input: &BufferList<'_>, left: i32, right: i32, frames: usize) {
    for i in 0..frames {
        engine.master_l[i] = input.locate_sample(left, i);
        engine.master_r[i] = input.locate_sample(right, i);
    }
}

fn write_output_pair(output: &BufferList<'_>, left: i32, frames: usize, l: &[f32], r: &[f32]) {
    for i in 0..frames {
        output.write_sample(left, i, l[i]);
        output.write_sample(left + 1, i, r[i]);
    }
}

fn hold_peak(slot: &AtomicF32, peak: f32) {
    let cur = slot.load(Ordering::Relaxed);
    if peak > cur {
        slot.store(peak, Ordering::Relaxed);
    } else {
        slot.store(cur * 0.9, Ordering::Relaxed);
    }
}

fn peak_of(l: &[f32], r: &[f32]) -> f32 {
    let mut p = 0.0f32;
    for i in 0..l.len() {
        p = p.max(l[i].abs()).max(r[i].abs());
    }
    p
}

fn peak_from_input(input: &BufferList<'_>, l: i32, r: i32, frames: usize) -> f32 {
    let mut p = 0.0f32;
    for i in 0..frames {
        p = p.max(input.locate_sample(l, i).abs()).max(input.locate_sample(r, i).abs());
    }
    p
}

fn hold_plugin_taps(engine: &mut Engine, schedule: &Schedule, slot: i32, frames: usize, recording: bool) {
    for tap_i in 0..TAP_COUNT {
        if schedule.taps[tap_i].plugin_slot != slot {
            continue;
        }
        let peak = peak_of(&engine.scratch_out_l[..frames], &engine.scratch_out_r[..frames]);
        hold_peak(&engine.peaks[tap_i], peak);
        if recording && engine.ring_enabled[tap_i] {
            write_ring_slices(
                &mut engine.rings_l,
                &mut engine.rings_r,
                &mut engine.ring_write,
                engine.ring_cap,
                tap_i,
                &engine.scratch_out_l[..frames],
                &engine.scratch_out_r[..frames],
            );
        }
    }
}

fn write_silence_ring(engine: &mut Engine, tap: usize, frames: usize) {
    for _ in 0..frames {
        let w = engine.ring_write[tap];
        engine.rings_l[tap][w] = 0.0;
        engine.rings_r[tap][w] = 0.0;
        engine.ring_write[tap] = (w + 1) % engine.ring_cap;
    }
}

fn write_ring_slices(
    rings_l: &mut [Vec<f32>],
    rings_r: &mut [Vec<f32>],
    ring_write: &mut [usize],
    ring_cap: usize,
    tap: usize,
    l: &[f32],
    r: &[f32],
) {
    for i in 0..l.len() {
        let w = ring_write[tap];
        rings_l[tap][w] = l[i];
        rings_r[tap][w] = r[i];
        ring_write[tap] = (w + 1) % ring_cap;
    }
}

fn write_ring_from_ptrs(
    engine: &mut Engine,
    tap: usize,
    input: &BufferList<'_>,
    channel_l: i32,
    channel_r: i32,
    frames: usize,
) {
    for i in 0..frames {
        let w = engine.ring_write[tap];
        engine.rings_l[tap][w] = input.locate_sample(channel_l, i);
        engine.rings_r[tap][w] = input.locate_sample(channel_r, i);
        engine.ring_write[tap] = (w + 1) % engine.ring_cap;
    }
}

fn mix_playback(engine: &mut Engine, schedule: &Schedule, output: &BufferList<'_>, frames: usize) {
    let any_solo = schedule.any_solo;
    for (i, lane) in schedule.lanes.iter().enumerate() {
        if !lane.active {
            continue;
        }
        let cap = engine.lane_cap;
        let avail = (engine.lane_write[i] + cap - engine.lane_read[i]) % cap;
        let n = frames.min(avail);
        if n < frames {
            let _ = engine.ev_tx.try_push(EngineEvent::Underrun { lane: i as u8 });
        }
        let silent = lane.muted || (any_solo && !lane.is_main && !lane.soloed);
        for f in 0..frames {
            let (sl, sr) = if f < n {
                let r = engine.lane_read[i];
                let l = engine.lane_rings_l[i][r];
                let rr = engine.lane_rings_r[i][r];
                engine.lane_read[i] = (r + 1) % cap;
                (l, rr)
            } else {
                (0.0, 0.0)
            };
            if silent || lane.is_main {
                continue;
            }
            if lane.dest >= 0 {
                let existing_l = output.locate_sample(lane.dest, f);
                let existing_r = output.locate_sample(lane.dest + 1, f);
                output.write_sample(lane.dest, f, existing_l + sl * lane.gain_l);
                output.write_sample(lane.dest + 1, f, existing_r + sr * lane.gain_r);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    fn interleaved(frames: usize, l: f32, r: f32) -> (Vec<f32>, BufferList<'static>) {
        let mut data = Vec::with_capacity(frames * 2);
        for _ in 0..frames {
            data.push(l);
            data.push(r);
        }
        let ptr = data.as_mut_ptr();
        std::mem::forget(data);
        let buf = AudioBuf { data: ptr, channels: 2, frames: frames as u32 };
        let list = BufferList { buffers: Box::leak(Box::new([buf])) };
        (Vec::new(), list)
    }

    #[test]
    fn process_does_not_panic_on_silence() {
        let (mut engine, _h) = Engine::new(48_000);
        let buf = AudioBuf { data: std::ptr::null_mut(), channels: 2, frames: 64 };
        let list = BufferList { buffers: &[buf] };
        engine.process(list, list, 64, 0);
    }

    #[test]
    fn dry_process_copies_input_pair_and_meters() {
        let (mut engine, handles) = Engine::new(48_000);
        let (_keep, input) = interleaved(32, 0.5, -0.25);
        let mut out = vec![0.0f32; 64];
        let out_buf = AudioBuf { data: out.as_mut_ptr(), channels: 2, frames: 32 };
        let output = BufferList { buffers: &[out_buf] };
        engine.process(input, output, 32, 0);
        assert!((out[0] - 0.5).abs() < 1e-6);
        assert!((out[1] + 0.25).abs() < 1e-6);
        assert!(handles.peaks[0].load(Ordering::Relaxed) > 0.2);
    }

    #[test]
    fn mix_playing_skips_live_sends() {
        let (mut engine, handles) = Engine::new(48_000);
        let mut schedule = Schedule::empty();
        schedule.routes[0].enabled = true;
        schedule.routes[0].return_channel = 2;
        handles.schedule.store(std::sync::Arc::new(schedule));
        handles.controls.mix_playing.store(true, Ordering::Relaxed);
        let (_keep, input) = interleaved(16, 1.0, 1.0);
        let mut out = vec![99.0f32; 64];
        let out_buf = AudioBuf { data: out.as_mut_ptr(), channels: 4, frames: 16 };
        let output = BufferList { buffers: &[out_buf] };
        engine.process(input, output, 16, 0);
        assert_eq!(out[2], 0.0);
    }

    #[test]
    fn seek_flush_resets_lane_read() {
        let (mut engine, mut handles) = Engine::new(48_000);
        engine.lane_write[0] = 8;
        engine.lane_read[0] = 2;
        let _ = handles.cmd_tx.try_push(UiCommand::TransportSeek { sample: 0 });
        handles.controls.mix_playing.store(true, Ordering::Relaxed);
        handles.controls.flush.store(true, Ordering::Relaxed);
        let buf = AudioBuf { data: std::ptr::null_mut(), channels: 2, frames: 8 };
        let list = BufferList { buffers: &[buf] };
        engine.process(list, list, 8, 0);
        assert_eq!(engine.lane_read[0], engine.lane_write[0]);
    }

    #[test]
    fn underrun_zeros_inactive_lane() {
        let (mut engine, handles) = Engine::new(48_000);
        let mut schedule = Schedule::empty();
        schedule.lanes[0] = crate::schedule::LanePlayer {
            active: true,
            dest: 0,
            gain_l: 1.0,
            gain_r: 1.0,
            ..crate::schedule::LanePlayer::default()
        };
        handles.schedule.store(std::sync::Arc::new(schedule));
        handles.controls.mix_playing.store(true, Ordering::Relaxed);
        let mut out = vec![1.0f32; 32];
        let out_buf = AudioBuf { data: out.as_mut_ptr(), channels: 2, frames: 16 };
        let output = BufferList { buffers: &[out_buf] };
        let input = output;
        engine.process(input, output, 16, 0);
        assert_eq!(out[0], 0.0);
    }
}
