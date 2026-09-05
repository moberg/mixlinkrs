//! Real-time audio engine, device stream, recorder, and mix player.

use std::collections::HashMap;

use engine::{Engine, EngineHandles};

pub(crate) struct Audio {
    pub engine: *mut Engine,
    pub engine_handles: EngineHandles,
    pub _stream: Option<audio_io::DeviceStream>,
    pub recorder: Option<crate::record::Recorder>,
    pub mix_player: Option<crate::mix_play::MixPlayer>,
    pub recording: bool,
    pub playing: bool,
    pub plugin_refs: HashMap<i32, vst3_host::MixLinkVST3Ref>,
    pub insert_refs: HashMap<uuid::Uuid, vst3_host::MixLinkVST3Ref>,
}
