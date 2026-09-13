//! Real-time audio engine, device stream, recorder, and mix player.

use std::collections::HashMap;

use engine::{Engine, EngineHandles};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum PluginStateScope {
    #[default]
    Project,
    Global,
}

impl PluginStateScope {
    pub fn toggle(self) -> Self {
        match self {
            Self::Project => Self::Global,
            Self::Global => Self::Project,
        }
    }
}

pub(crate) struct Audio {
    pub engine: *mut Engine,
    pub engine_handles: EngineHandles,
    pub _stream: Option<audio_io::DeviceStream>,
    pub recorder: Option<crate::record::Recorder>,
    pub mix_player: Option<crate::mix_play::MixPlayer>,
    pub recording: bool,
    pub playing: bool,
    pub plugin_refs: HashMap<uuid::Uuid, vst3_host::MixLinkVST3Ref>,
    pub insert_refs: HashMap<uuid::Uuid, vst3_host::MixLinkVST3Ref>,
    /// Per-stage Project vs Global chunk-state selection.
    pub plugin_state_scope: HashMap<uuid::Uuid, PluginStateScope>,
}
