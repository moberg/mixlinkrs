use engine_api::{MASTER_PLUGIN_SLOT, MASTER_TAP, STRIP_COUNT, TAP_COUNT as API_TAP};

pub const TAP_COUNT: usize = API_TAP;

#[derive(Clone, Copy, Debug)]
pub struct AudioTapBinding {
    pub channel_l: i32,
    pub channel_r: i32,
    pub plugin_slot: i32,
}

impl AudioTapBinding {
    pub fn silent() -> Self {
        Self { channel_l: -1, channel_r: -1, plugin_slot: -1 }
    }

    pub fn master_mix() -> Self {
        Self { channel_l: -1, channel_r: -1, plugin_slot: MASTER_PLUGIN_SLOT }
    }

    pub fn hardware(l: i32, r: i32) -> Self {
        Self { channel_l: l, channel_r: r, plugin_slot: -1 }
    }

    pub fn plugin(slot: i32) -> Self {
        Self { channel_l: -1, channel_r: -1, plugin_slot: slot }
    }
}

/// MixLink `displayLevel`: −60…0 dB → 0…1.
pub fn display_level(peak: f32) -> f32 {
    if peak <= 0.000001 {
        return 0.0;
    }
    let db = 20.0 * peak.log10();
    ((db + 60.0) / 60.0).clamp(0.0, 1.0)
}

pub fn strip_tap(i: usize) -> usize {
    i.min(STRIP_COUNT - 1)
}

/// Returns occupy slots 8 + ReturnLane.rawValue (MixLink AudioTap).
pub fn return_tap(lane_raw: usize) -> usize {
    8 + lane_raw.min(7)
}

pub fn master_tap() -> usize {
    MASTER_TAP
}
