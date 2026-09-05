use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};

use engine_api::{MIX_PLAY_MAX_LANES, STRIP_COUNT, TAP_COUNT};

use crate::taps::AudioTapBinding;

/// Per-tap print of the TotalMix Main mix (send to `main_output`).
#[derive(Clone, Copy, Debug)]
pub struct MixGain {
    pub gain: f32,
    pub pan: f32,
    pub muted: bool,
    pub into_master: bool,
}

impl Default for MixGain {
    fn default() -> Self {
        Self { gain: 1.0, pan: 0.5, muted: false, into_master: false }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct StripFeed {
    pub channel: i32,
    pub gain: f32,
    pub linked: bool,
}

#[derive(Clone, Debug)]
pub struct SendRoute {
    pub enabled: bool,
    pub return_channel: i32,
    pub feeds: [StripFeed; STRIP_COUNT],
}

impl Default for SendRoute {
    fn default() -> Self {
        Self {
            enabled: false,
            return_channel: -1,
            feeds: [StripFeed { channel: -1, gain: 0.0, linked: false }; STRIP_COUNT],
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct LanePlayer {
    pub active: bool,
    pub is_main: bool,
    pub dest: i32,
    pub muted: bool,
    pub soloed: bool,
    pub gain_l: f32,
    pub gain_r: f32,
    pub insert: usize,
}

impl Default for LanePlayer {
    fn default() -> Self {
        Self {
            active: false,
            is_main: false,
            dest: -1,
            muted: false,
            soloed: false,
            gain_l: 1.0,
            gain_r: 1.0,
            insert: usize::MAX,
        }
    }
}

/// Published graph. Swapped via ArcSwap; parameter tweaks use [`RtControls`].
#[derive(Clone, Debug)]
pub struct Schedule {
    pub routes: [SendRoute; 8],
    pub taps: [AudioTapBinding; TAP_COUNT],
    /// TotalMix mute/solo printed onto the take. Never Mix-page mixer state.
    pub record_muted: [bool; TAP_COUNT],
    /// Analog fader + live pan printed onto stereo stems and mix.wav.
    pub mix_gains: [MixGain; TAP_COUNT],
    pub lanes: [LanePlayer; MIX_PLAY_MAX_LANES],
    pub any_solo: bool,
    /// Mix-page Control Room: listen gain after the Main bus. Export is pre-CR.
    pub listen_amp: f32,
}

impl Schedule {
    pub fn empty() -> Self {
        let mut taps = [AudioTapBinding::silent(); TAP_COUNT];
        for i in 0..STRIP_COUNT {
            taps[i] = AudioTapBinding::hardware((i as i32) * 2, (i as i32) * 2 + 1);
        }
        taps[engine_api::MASTER_TAP] = AudioTapBinding::master_mix();
        Self {
            routes: Default::default(),
            taps,
            record_muted: [false; TAP_COUNT],
            mix_gains: [MixGain::default(); TAP_COUNT],
            lanes: [LanePlayer::default(); MIX_PLAY_MAX_LANES],
            any_solo: false,
            listen_amp: 1.0,
        }
    }
}

/// Lock-free fader/mute/solo table. A fader move must not rebuild the graph.
pub struct RtControls {
    pub strip_fader: [AtomicU32; STRIP_COUNT],
    pub strip_pan: [AtomicU32; STRIP_COUNT],
    pub strip_mute: [AtomicBool; STRIP_COUNT],
    pub strip_solo: [AtomicBool; STRIP_COUNT],
    pub strip_enabled: [AtomicBool; STRIP_COUNT],
    pub main_fader: AtomicU32,
    pub mix_playing: AtomicBool,
    pub recording: AtomicBool,
    pub flush: AtomicBool,
    pub any_send_enabled: AtomicBool,
    pub tempo_milli: AtomicI32,
}

impl Default for RtControls {
    fn default() -> Self {
        Self {
            strip_fader: std::array::from_fn(|_| AtomicU32::new(0)),
            strip_pan: std::array::from_fn(|_| AtomicU32::new(f32::to_bits(0.5))),
            strip_mute: std::array::from_fn(|_| AtomicBool::new(false)),
            strip_solo: std::array::from_fn(|_| AtomicBool::new(false)),
            strip_enabled: std::array::from_fn(|_| AtomicBool::new(true)),
            main_fader: AtomicU32::new(0),
            mix_playing: AtomicBool::new(false),
            recording: AtomicBool::new(false),
            flush: AtomicBool::new(false),
            any_send_enabled: AtomicBool::new(false),
            tempo_milli: AtomicI32::new(120_000),
        }
    }
}

impl RtControls {
    pub fn set_fader(&self, strip: usize, lin: f32) {
        if let Some(a) = self.strip_fader.get(strip) {
            a.store(lin.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
        }
    }

    pub fn fader(&self, strip: usize) -> f32 {
        f32::from_bits(self.strip_fader.get(strip).map(|a| a.load(Ordering::Relaxed)).unwrap_or(0))
    }
}
