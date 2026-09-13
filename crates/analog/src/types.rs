//! Mixer / routing types shared by session, surface, and AnalogEngine.

use osc::Bus as OscBus;
use serde::de::{self, Deserializer};
use serde::ser::Serializer;
use serde::{Deserialize, Serialize};

/// TotalMix bus. Serde names match MixLink (`input` / `playback` / `output`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MixerBus {
    Input,
    Playback,
    Output,
}

impl MixerBus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Playback => "playback",
            Self::Output => "output",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Input => "HARDWARE INPUTS",
            Self::Playback => "SOFTWARE PLAYBACK",
            Self::Output => "HARDWARE OUTPUTS",
        }
    }

    pub fn osc(self) -> OscBus {
        match self {
            Self::Input => OscBus::Input,
            Self::Playback => OscBus::Playback,
            Self::Output => OscBus::Output,
        }
    }
}

impl From<MixerBus> for OscBus {
    fn from(bus: MixerBus) -> Self {
        bus.osc()
    }
}

impl From<OscBus> for MixerBus {
    fn from(bus: OscBus) -> Self {
        match bus {
            OscBus::Input => Self::Input,
            OscBus::Playback => Self::Playback,
            OscBus::Output => Self::Output,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChannelID {
    pub bus: MixerBus,
    pub index: i32,
}

impl ChannelID {
    pub fn new(bus: MixerBus, index: i32) -> Self {
        Self { bus, index }
    }
}

/// Fader assign: Main, or exclusive Bus 1 / Bus 2.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MixAssign {
    Main,
    Bus1,
    Bus2,
}

impl MixAssign {
    pub const ALL: [MixAssign; 3] = [MixAssign::Main, MixAssign::Bus1, MixAssign::Bus2];
}

/// Return / send lane. Raw values match MixLink.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum ReturnLane {
    SendA = 0,
    SendB = 1,
    Bus1 = 2,
    Bus2 = 3,
    SendC = 4,
    SendD = 5,
    SendE = 6,
    SendF = 7,
}

pub const MAX_SEND_COUNT: i32 = 6;

pub const ALL_SEND_LANES: [ReturnLane; 6] = [
    ReturnLane::SendA,
    ReturnLane::SendB,
    ReturnLane::SendC,
    ReturnLane::SendD,
    ReturnLane::SendE,
    ReturnLane::SendF,
];

pub const SEND_LANES: [ReturnLane; 2] = [ReturnLane::SendA, ReturnLane::SendB];

pub const BUS_LANES: [ReturnLane; 2] = [ReturnLane::Bus1, ReturnLane::Bus2];

impl ReturnLane {
    pub const ALL: [ReturnLane; 8] = [
        ReturnLane::SendA,
        ReturnLane::SendB,
        ReturnLane::Bus1,
        ReturnLane::Bus2,
        ReturnLane::SendC,
        ReturnLane::SendD,
        ReturnLane::SendE,
        ReturnLane::SendF,
    ];

    pub fn from_i32(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::SendA),
            1 => Some(Self::SendB),
            2 => Some(Self::Bus1),
            3 => Some(Self::Bus2),
            4 => Some(Self::SendC),
            5 => Some(Self::SendD),
            6 => Some(Self::SendE),
            7 => Some(Self::SendF),
            _ => None,
        }
    }

    pub fn is_send(self) -> bool {
        ALL_SEND_LANES.contains(&self)
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::SendA => "Send A",
            Self::SendB => "Send B",
            Self::SendC => "Send C",
            Self::SendD => "Send D",
            Self::SendE => "Send E",
            Self::SendF => "Send F",
            Self::Bus1 => "Bus 1",
            Self::Bus2 => "Bus 2",
        }
    }

    pub fn strip_title(self) -> &'static str {
        match self {
            Self::SendA => "A",
            Self::SendB => "B",
            Self::SendC => "C",
            Self::SendD => "D",
            Self::SendE => "E",
            Self::SendF => "F",
            Self::Bus1 => "1",
            Self::Bus2 => "2",
        }
    }

    pub fn routing_slot(self) -> RoutingSlot {
        match self {
            Self::SendA => RoutingSlot::SendA,
            Self::SendB => RoutingSlot::SendB,
            Self::SendC => RoutingSlot::SendC,
            Self::SendD => RoutingSlot::SendD,
            Self::SendE => RoutingSlot::SendE,
            Self::SendF => RoutingSlot::SendF,
            Self::Bus1 => RoutingSlot::Bus1,
            Self::Bus2 => RoutingSlot::Bus2,
        }
    }
}

impl Serialize for ReturnLane {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_i32(*self as i32)
    }
}

impl<'de> Deserialize<'de> for ReturnLane {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let v = i32::deserialize(deserializer)?;
        Self::from_i32(v).ok_or_else(|| de::Error::custom(format!("unknown ReturnLane {v}")))
    }
}

/// Where a send lands. A bare JSON int decodes as [`SendDestination::Output`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SendDestination {
    Output(i32),
    Plugin(uuid::Uuid),
}

impl Serialize for SendDestination {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut c = serializer.serialize_struct("SendDestination", 2)?;
        match self {
            Self::Output(index) => {
                c.serialize_field("kind", "output")?;
                c.serialize_field("index", index)?;
            }
            Self::Plugin(id) => {
                c.serialize_field("kind", "plugin")?;
                c.serialize_field("id", id)?;
            }
        }
        c.end()
    }
}

impl<'de> Deserialize<'de> for SendDestination {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;

        impl<'de> de::Visitor<'de> for Visitor {
            type Value = SendDestination;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a send destination object or a bare output index")
            }

            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(SendDestination::Output(v as i32))
            }

            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(SendDestination::Output(v as i32))
            }

            fn visit_map<A: de::MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut kind: Option<String> = None;
                let mut index: Option<i32> = None;
                let mut id: Option<uuid::Uuid> = None;
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "kind" => kind = Some(map.next_value()?),
                        "index" => index = Some(map.next_value()?),
                        "id" => id = Some(map.next_value()?),
                        _ => {
                            let _: de::IgnoredAny = map.next_value()?;
                        }
                    }
                }
                match kind.as_deref() {
                    Some("plugin") => {
                        if let Some(id) = id {
                            Ok(SendDestination::Plugin(id))
                        } else {
                            let index = index.unwrap_or(0);
                            Ok(SendDestination::Plugin(uuid::Uuid::from_u128(index as u128)))
                        }
                    }
                    _ => {
                        let index = index.ok_or_else(|| de::Error::missing_field("index"))?;
                        Ok(SendDestination::Output(index))
                    }
                }
            }
        }

        deserializer.deserialize_any(Visitor)
    }
}

/// Legacy single-effect pointer. Read on load only; assignments are [`ChainRef`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EffectRef {
    Hardware(i32),
    Plugin(i32),
}

impl Serialize for EffectRef {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut c = serializer.serialize_struct("EffectRef", 2)?;
        match self {
            Self::Hardware(id) => {
                c.serialize_field("kind", "hardware")?;
                c.serialize_field("id", id)?;
            }
            Self::Plugin(id) => {
                c.serialize_field("kind", "plugin")?;
                c.serialize_field("id", id)?;
            }
        }
        c.end()
    }
}

impl<'de> Deserialize<'de> for EffectRef {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            kind: String,
            id: i32,
        }
        let raw = Raw::deserialize(deserializer)?;
        match raw.kind.as_str() {
            "plugin" => Ok(Self::Plugin(raw.id)),
            _ => Ok(Self::Hardware(raw.id)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChainKind {
    Hardware,
    Plugin,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ChainRef {
    pub kind: ChainKind,
    pub id: uuid::Uuid,
}

impl ChainRef {
    pub fn hardware(id: uuid::Uuid) -> Self {
        Self { kind: ChainKind::Hardware, id }
    }

    pub fn plugin(id: uuid::Uuid) -> Self {
        Self { kind: ChainKind::Plugin, id }
    }
}

impl Serialize for ChainRef {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut c = serializer.serialize_struct("ChainRef", 2)?;
        match self.kind {
            ChainKind::Hardware => c.serialize_field("kind", "hardware")?,
            ChainKind::Plugin => c.serialize_field("kind", "plugin")?,
        }
        c.serialize_field("id", &self.id)?;
        c.end()
    }
}

impl<'de> Deserialize<'de> for ChainRef {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            kind: String,
            id: uuid::Uuid,
        }
        let raw = Raw::deserialize(deserializer)?;
        match raw.kind.as_str() {
            "plugin" => Ok(Self::plugin(raw.id)),
            _ => Ok(Self::hardware(raw.id)),
        }
    }
}

pub const MAX_PLUGIN_STAGES: usize = 8;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HardwarePreset {
    pub id: uuid::Uuid,
    pub name: String,
    pub output: i32,
    pub input: i32,
}

impl HardwarePreset {
    pub fn new(name: impl Into<String>, output: i32, input: i32) -> Self {
        Self { id: uuid::Uuid::new_v4(), name: name.into(), output, input }
    }

    pub fn title(&self) -> String {
        if self.name.is_empty() {
            "Device".into()
        } else {
            self.name.clone()
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HardwareChain {
    pub id: uuid::Uuid,
    pub name: String,
    #[serde(default)]
    pub stages: Vec<uuid::Uuid>,
}

impl HardwareChain {
    pub fn new(name: impl Into<String>, stages: Vec<uuid::Uuid>) -> Self {
        Self { id: uuid::Uuid::new_v4(), name: name.into(), stages }
    }

    pub fn title(&self) -> String {
        if self.name.is_empty() {
            "Hardware chain".into()
        } else {
            self.name.clone()
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginStage {
    pub id: uuid::Uuid,
    pub name: String,
    #[serde(default)]
    pub bundle_path: Option<String>,
    #[serde(default, rename = "classUID")]
    pub class_uid: Option<String>,
    #[serde(default)]
    pub bypassed: bool,
}

impl PluginStage {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            name: name.into(),
            bundle_path: None,
            class_uid: None,
            bypassed: false,
        }
    }

    pub fn is_loaded(&self) -> bool {
        self.bundle_path.as_ref().is_some_and(|p| !p.is_empty())
    }

    pub fn title(&self) -> String {
        if self.name.is_empty() {
            "Plugin".into()
        } else {
            self.name.clone()
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginChain {
    pub id: uuid::Uuid,
    pub name: String,
    pub return_channel: i32,
    #[serde(default)]
    pub stages: Vec<PluginStage>,
}

impl PluginChain {
    pub fn new(name: impl Into<String>, return_channel: i32) -> Self {
        Self { id: uuid::Uuid::new_v4(), name: name.into(), return_channel, stages: Vec::new() }
    }

    pub fn is_loaded(&self) -> bool {
        self.stages.iter().any(PluginStage::is_loaded)
    }

    pub fn title(&self) -> String {
        if self.name.is_empty() {
            "Plugin chain".into()
        } else {
            self.name.clone()
        }
    }
}

pub fn unique_copy_name(existing: &[String], base: &str) -> String {
    let stem = if base.trim().is_empty() { "Untitled" } else { base.trim() };
    let candidate = format!("{stem} copy");
    if !existing.iter().any(|n| n == &candidate) {
        return candidate;
    }
    for i in 2..1000 {
        let next = format!("{stem} copy {i}");
        if !existing.iter().any(|n| n == &next) {
            return next;
        }
    }
    format!("{stem} copy {}", uuid::Uuid::new_v4())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SurfaceStrip {
    pub id: i32,
    #[serde(default)]
    pub fader: f32,
    #[serde(default = "center_pan")]
    pub pan: f32,
    #[serde(default)]
    pub aux_a: f32,
    #[serde(default)]
    pub aux_b: f32,
    #[serde(default)]
    pub aux_c: f32,
    #[serde(default)]
    pub aux_d: f32,
    #[serde(default)]
    pub aux_e: f32,
    #[serde(default)]
    pub aux_f: f32,
    #[serde(default)]
    pub assign: MixAssign,
}

fn center_pan() -> f32 {
    0.5
}

impl SurfaceStrip {
    pub fn new(id: i32) -> Self {
        Self {
            id,
            fader: 0.0,
            pan: 0.5,
            aux_a: 0.0,
            aux_b: 0.0,
            aux_c: 0.0,
            aux_d: 0.0,
            aux_e: 0.0,
            aux_f: 0.0,
            assign: MixAssign::Main,
        }
    }

    pub fn aux(&self, lane: ReturnLane) -> f32 {
        match lane {
            ReturnLane::SendA => self.aux_a,
            ReturnLane::SendB => self.aux_b,
            ReturnLane::SendC => self.aux_c,
            ReturnLane::SendD => self.aux_d,
            ReturnLane::SendE => self.aux_e,
            ReturnLane::SendF => self.aux_f,
            ReturnLane::Bus1 | ReturnLane::Bus2 => 0.0,
        }
    }

    pub fn set_aux(&mut self, value: f32, lane: ReturnLane) {
        match lane {
            ReturnLane::SendA => self.aux_a = value,
            ReturnLane::SendB => self.aux_b = value,
            ReturnLane::SendC => self.aux_c = value,
            ReturnLane::SendD => self.aux_d = value,
            ReturnLane::SendE => self.aux_e = value,
            ReturnLane::SendF => self.aux_f = value,
            ReturnLane::Bus1 | ReturnLane::Bus2 => {}
        }
    }
}

fn is_zero_f32(v: &f32) -> bool {
    *v == 0.0
}

/// Per-strip plugin-send aux. Keys match [`SurfaceStrip`] (`auxA`…`auxF`).
/// Hardware send knobs are not stored — TotalMix is that mix.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginStripSends {
    pub id: i32,
    #[serde(default, skip_serializing_if = "is_zero_f32")]
    pub aux_a: f32,
    #[serde(default, skip_serializing_if = "is_zero_f32")]
    pub aux_b: f32,
    #[serde(default, skip_serializing_if = "is_zero_f32")]
    pub aux_c: f32,
    #[serde(default, skip_serializing_if = "is_zero_f32")]
    pub aux_d: f32,
    #[serde(default, skip_serializing_if = "is_zero_f32")]
    pub aux_e: f32,
    #[serde(default, skip_serializing_if = "is_zero_f32")]
    pub aux_f: f32,
}

impl PluginStripSends {
    pub fn new(id: i32) -> Self {
        Self { id, ..Self::default() }
    }

    pub fn aux(&self, lane: ReturnLane) -> f32 {
        match lane {
            ReturnLane::SendA => self.aux_a,
            ReturnLane::SendB => self.aux_b,
            ReturnLane::SendC => self.aux_c,
            ReturnLane::SendD => self.aux_d,
            ReturnLane::SendE => self.aux_e,
            ReturnLane::SendF => self.aux_f,
            ReturnLane::Bus1 | ReturnLane::Bus2 => 0.0,
        }
    }

    pub fn set_aux(&mut self, value: f32, lane: ReturnLane) {
        match lane {
            ReturnLane::SendA => self.aux_a = value,
            ReturnLane::SendB => self.aux_b = value,
            ReturnLane::SendC => self.aux_c = value,
            ReturnLane::SendD => self.aux_d = value,
            ReturnLane::SendE => self.aux_e = value,
            ReturnLane::SendF => self.aux_f = value,
            ReturnLane::Bus1 | ReturnLane::Bus2 => {}
        }
    }

    pub fn is_empty(&self) -> bool {
        ALL_SEND_LANES.iter().all(|lane| self.aux(*lane) == 0.0)
    }
}

impl Default for MixAssign {
    fn default() -> Self {
        Self::Main
    }
}

/// On-screen return: wet mix only.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReturnStrip {
    pub id: i32,
    #[serde(default)]
    pub fader: f32,
    #[serde(default = "center_pan")]
    pub pan: f32,
    #[serde(default)]
    pub mute: bool,
    #[serde(default)]
    pub solo: bool,
}

impl ReturnStrip {
    pub fn new(id: i32) -> Self {
        Self { id, fader: 0.0, pan: 0.5, mute: false, solo: false }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MixerChannel {
    pub id: ChannelID,
    pub name: String,
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
    pub fader: f32,
    pub stereo: bool,
    pub from_total_mix: bool,
}

impl MixerChannel {
    pub fn placeholder(bus: MixerBus, index: i32, stereo: bool, name: impl Into<String>) -> Self {
        Self {
            id: ChannelID::new(bus, index),
            name: name.into(),
            pan: 0.5,
            mute: false,
            solo: false,
            fader: 0.75,
            stereo,
            from_total_mix: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StripBinding {
    pub id: i32,
    pub bus: MixerBus,
    pub index: i32,
    pub linked_stereo: bool,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// False when the strip has no hardware input.
    #[serde(default = "default_true")]
    pub has_input: bool,
}

fn default_true() -> bool {
    true
}

impl StripBinding {
    pub fn new(id: i32, bus: MixerBus, index: i32, linked_stereo: bool) -> Self {
        Self { id, bus, index, linked_stereo, enabled: true, has_input: true }
    }

    pub fn channel_id(&self) -> ChannelID {
        ChannelID::new(self.bus, self.index)
    }

    pub fn source(&self) -> Option<ChannelID> {
        self.has_input.then(|| self.channel_id())
    }

    pub fn display_name(&self) -> String {
        let n = self.index + 1;
        let bus = match self.bus {
            MixerBus::Input => "Input",
            MixerBus::Playback => "Playback",
            MixerBus::Output => "Output",
        };
        if self.linked_stereo {
            format!("{bus} {n}/{}", n + 1)
        } else {
            format!("{bus} {n}")
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReturnLaneConfig {
    pub id: i32,
    pub input: i32,
    #[serde(default)]
    pub effect: Option<EffectRef>,
    pub fader: f32,
    pub pan: f32,
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl ReturnLaneConfig {
    pub fn new(
        id: i32,
        input: i32,
        effect: Option<EffectRef>,
        fader: f32,
        pan: f32,
        name: impl Into<String>,
    ) -> Self {
        Self { id, input, effect, fader, pan, name: name.into(), enabled: true }
    }

    pub fn defaults() -> Vec<Self> {
        vec![
            Self::new(0, 16, None, 0.0, 0.5, ""),
            Self::new(1, 18, None, 0.0, 0.5, ""),
            Self::new(2, 20, None, 0.0, 0.5, ""),
            Self::new(3, 22, None, 0.0, 0.5, ""),
        ]
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginSlot {
    pub id: i32,
    pub name: String,
    #[serde(default)]
    pub bundle_path: Option<String>,
    #[serde(default, rename = "classUID")]
    pub class_uid: Option<String>,
    pub send_output: i32,
    pub input_channel: i32,
    pub return_channel: i32,
    pub return_dest: i32,
    pub bypassed: bool,
    #[serde(default = "default_return_fader")]
    pub return_fader: f32,
    #[serde(default = "center_pan")]
    pub return_pan: f32,
}

fn default_return_fader() -> f32 {
    0.75
}

impl PluginSlot {
    pub fn is_loaded(&self) -> bool {
        self.bundle_path.as_ref().is_some_and(|p| !p.is_empty())
    }

    pub fn title(&self) -> String {
        if self.name.is_empty() {
            format!("Plugin {}", self.id + 1)
        } else {
            self.name.clone()
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HardwareEffect {
    pub id: i32,
    pub name: String,
    pub output: i32,
    pub input: i32,
}

impl HardwareEffect {
    pub fn title(&self) -> String {
        if self.name.is_empty() {
            format!("Hardware {}", self.id + 1)
        } else {
            self.name.clone()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RoutingSlot {
    Mix,
    SendA,
    SendB,
    SendC,
    SendD,
    SendE,
    SendF,
    Bus1,
    Bus2,
}

impl RoutingSlot {
    pub fn title(self) -> &'static str {
        match self {
            Self::Mix => "Mix (Main Out)",
            Self::SendA => "Send A",
            Self::SendB => "Send B",
            Self::SendC => "Send C",
            Self::SendD => "Send D",
            Self::SendE => "Send E",
            Self::SendF => "Send F",
            Self::Bus1 => "Bus 1",
            Self::Bus2 => "Bus 2",
        }
    }

    pub fn lane(self) -> Option<ReturnLane> {
        match self {
            Self::Mix => None,
            Self::SendA => Some(ReturnLane::SendA),
            Self::SendB => Some(ReturnLane::SendB),
            Self::SendC => Some(ReturnLane::SendC),
            Self::SendD => Some(ReturnLane::SendD),
            Self::SendE => Some(ReturnLane::SendE),
            Self::SendF => Some(ReturnLane::SendF),
            Self::Bus1 => Some(ReturnLane::Bus1),
            Self::Bus2 => Some(ReturnLane::Bus2),
        }
    }
}

/// Mix-page lane (arrangement / playback dest).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MixLane {
    Strip(i32),
    ReturnLane(ReturnLane),
    Main,
}

/// Inbound mix-matrix node from TotalMix.
#[derive(Clone, Debug, PartialEq)]
pub struct MixNode {
    pub source_bus: MixerBus,
    pub source: i32,
    pub dest: i32,
    pub fader_lin: Option<f32>,
    pub balpan: Option<f32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StripName {
    pub bus: MixerBus,
    pub channel: i32,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MixEvent {
    Mix(MixNode),
    Name(StripName),
    Layout,
    Pads,
}
