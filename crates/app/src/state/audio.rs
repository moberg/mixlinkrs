//! Real-time audio engine, device stream, recorder, and mix player.

use std::collections::HashMap;
use std::time::Instant;

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
    pub record_started: Option<Instant>,
    pub playing: bool,
    pub plugin_refs: HashMap<uuid::Uuid, vst3_host::MixLinkVST3Ref>,
    pub insert_refs: HashMap<uuid::Uuid, vst3_host::MixLinkVST3Ref>,
    /// Per-stage Project vs Global chunk-state selection.
    pub plugin_state_scope: HashMap<uuid::Uuid, PluginStateScope>,
    /// PNG screenshots of plugin editors, keyed by stage id.
    /// Updated when that stage's editor closes (backing-scale capture).
    pub plugin_previews: HashMap<uuid::Uuid, Vec<u8>>,
    /// First-open fill-in only, when this stage has no persisted thumb yet.
    pub preview_due: HashMap<uuid::Uuid, Instant>,
}
