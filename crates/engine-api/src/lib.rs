//! UI ↔ RT command and event types. Keep this crate allocation-light.

#[derive(Clone, Debug)]
pub enum UiCommand {
    TransportPlay,
    TransportStop,
    TransportSeek { sample: i64 },
    SetTempo { bpm: f64 },
    SetRecording { on: bool },
    ArmRings,
    FlushMix,
}

#[derive(Clone, Debug)]
pub enum EngineEvent {
    Xrun,
    RecordOverrun { tap: u8 },
    Underrun { lane: u8 },
    RecordingStopped,
}

pub const TAP_COUNT: usize = 17;
pub const STRIP_COUNT: usize = 8;
pub const MASTER_TAP: usize = 16;
pub const MIX_PLAY_MAX_LANES: usize = 20;
pub const MASTER_PLUGIN_SLOT: i32 = -2;
