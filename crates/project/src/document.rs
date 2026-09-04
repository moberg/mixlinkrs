//! Mix document types and take WAV naming. JSON keys match MixLink Codable.

use analog::{ReturnLane, ALL_SEND_LANES, BUS_LANES, MAX_SEND_COUNT};
use serde::de::{self, Deserializer};
use serde::ser::{SerializeStruct, Serializer};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// TotalMix 0 dB on the faderlin scale (`65/71`).
pub const FADER_LIN_0DB: f32 = 65.0 / 71.0;

pub const KNOB_COUNT: usize = 6;

/// Arrangement / mix-mixer lane. Wire format is MixLink `{kind, index}`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MixLane {
    Strip(i32),
    ReturnLane(ReturnLane),
    Main,
}

impl MixLane {
    pub fn id(self) -> String {
        match self {
            Self::Strip(i) => format!("ch-{i}"),
            Self::ReturnLane(lane) => format!("ret-{}", lane as i32),
            Self::Main => "main".into(),
        }
    }

    pub fn title(self) -> String {
        match self {
            Self::Strip(i) => format!("Ch {}", i + 1),
            Self::ReturnLane(lane) if lane.is_send() => format!("Ret {}", lane.strip_title()),
            Self::ReturnLane(lane) => {
                format!("Bus {}", if lane == ReturnLane::Bus1 { 1 } else { 2 })
            }
            Self::Main => "Main".into(),
        }
    }

    pub fn short_title(self) -> String {
        match self {
            Self::Strip(i) => format!("{}", i + 1),
            Self::ReturnLane(lane) => lane.strip_title().into(),
            Self::Main => "M".into(),
        }
    }

    pub fn default_lanes(send_count: i32) -> Vec<Self> {
        let mut lanes: Vec<Self> = (0..8).map(Self::Strip).collect();
        let n = send_count.clamp(2, MAX_SEND_COUNT) as usize;
        lanes.extend(ALL_SEND_LANES[..n].iter().copied().map(Self::ReturnLane));
        lanes.extend(BUS_LANES.iter().copied().map(Self::ReturnLane));
        lanes.push(Self::Main);
        lanes
    }
}

impl From<analog::MixLane> for MixLane {
    fn from(lane: analog::MixLane) -> Self {
        match lane {
            analog::MixLane::Strip(i) => Self::Strip(i),
            analog::MixLane::ReturnLane(r) => Self::ReturnLane(r),
            analog::MixLane::Main => Self::Main,
        }
    }
}

impl From<MixLane> for analog::MixLane {
    fn from(lane: MixLane) -> Self {
        match lane {
            MixLane::Strip(i) => Self::Strip(i),
            MixLane::ReturnLane(r) => Self::ReturnLane(r),
            MixLane::Main => Self::Main,
        }
    }
}

impl Serialize for MixLane {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Strip(index) => {
                let mut c = serializer.serialize_struct("MixLane", 2)?;
                c.serialize_field("kind", "strip")?;
                c.serialize_field("index", index)?;
                c.end()
            }
            Self::ReturnLane(lane) => {
                let mut c = serializer.serialize_struct("MixLane", 2)?;
                c.serialize_field("kind", "return")?;
                c.serialize_field("index", &(*lane as i32))?;
                c.end()
            }
            Self::Main => {
                let mut c = serializer.serialize_struct("MixLane", 1)?;
                c.serialize_field("kind", "main")?;
                c.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for MixLane {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            kind: String,
            #[serde(default)]
            index: Option<i32>,
        }
        let raw = Raw::deserialize(deserializer)?;
        match raw.kind.as_str() {
            "strip" => Ok(Self::Strip(raw.index.unwrap_or(0))),
            "return" => {
                let raw_index = raw.index.unwrap_or(0);
                Ok(Self::ReturnLane(ReturnLane::from_i32(raw_index).unwrap_or(ReturnLane::SendA)))
            }
            _ => Ok(Self::Main),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MixClip {
    pub id: Uuid,
    pub source_take: i32,
    pub source_lane: MixLane,
    pub source_file: String,
    pub source_start_frame: i64,
    pub source_frame_count: i64,
    pub mix_start_frame: i64,
}

impl MixClip {
    pub fn mix_end_frame(&self) -> i64 {
        self.mix_start_frame + self.source_frame_count
    }

    pub fn slice(&self, start: i64, count: i64) -> Self {
        let offset = start.max(0);
        Self {
            id: Uuid::new_v4(),
            source_take: self.source_take,
            source_lane: self.source_lane,
            source_file: self.source_file.clone(),
            source_start_frame: self.source_start_frame + offset,
            source_frame_count: (self.source_frame_count - offset).min(count).max(0),
            mix_start_frame: self.mix_start_frame,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MixKnobMap {
    pub insert_id: Uuid,
    pub parameter_id: u32,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MixInsert {
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub bundle_path: Option<String>,
    #[serde(default)]
    pub class_uid: Option<String>,
    pub bypassed: bool,
}

impl MixInsert {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            bundle_path: None,
            class_uid: None,
            bypassed: false,
        }
    }

    pub fn is_loaded(&self) -> bool {
        self.bundle_path.as_ref().is_some_and(|p| !p.is_empty())
    }

    pub fn title(&self) -> &str {
        if self.name.is_empty() {
            "Plugin"
        } else {
            &self.name
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MixAutomationTarget {
    Volume,
    Pan,
    Knob(i32),
    InsertParam { insert_id: Uuid, parameter_id: u32 },
}

impl Serialize for MixAutomationTarget {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Volume => {
                let mut c = serializer.serialize_struct("MixAutomationTarget", 1)?;
                c.serialize_field("kind", "volume")?;
                c.end()
            }
            Self::Pan => {
                let mut c = serializer.serialize_struct("MixAutomationTarget", 1)?;
                c.serialize_field("kind", "pan")?;
                c.end()
            }
            Self::Knob(index) => {
                let mut c = serializer.serialize_struct("MixAutomationTarget", 2)?;
                c.serialize_field("kind", "knob")?;
                c.serialize_field("index", index)?;
                c.end()
            }
            Self::InsertParam { insert_id, parameter_id } => {
                let mut c = serializer.serialize_struct("MixAutomationTarget", 3)?;
                c.serialize_field("kind", "param")?;
                c.serialize_field("insertID", insert_id)?;
                c.serialize_field("parameterID", parameter_id)?;
                c.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for MixAutomationTarget {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            kind: String,
            #[serde(default)]
            index: Option<i32>,
            #[serde(default, rename = "insertID")]
            insert_id: Option<Uuid>,
            #[serde(default, rename = "parameterID")]
            parameter_id: Option<u32>,
        }
        let raw = Raw::deserialize(deserializer)?;
        match raw.kind.as_str() {
            "pan" => Ok(Self::Pan),
            "knob" => Ok(Self::Knob(raw.index.unwrap_or(0))),
            "param" => Ok(Self::InsertParam {
                insert_id: raw.insert_id.ok_or_else(|| de::Error::missing_field("insertID"))?,
                parameter_id: raw
                    .parameter_id
                    .ok_or_else(|| de::Error::missing_field("parameterID"))?,
            }),
            _ => Ok(Self::Volume),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MixAutomationPoint {
    pub frame: i64,
    pub value: f32,
}

impl Eq for MixAutomationPoint {}
impl std::hash::Hash for MixAutomationPoint {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.frame.hash(state);
        self.value.to_bits().hash(state);
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MixAutomationLane {
    pub target: MixAutomationTarget,
    pub points: Vec<MixAutomationPoint>,
}

impl Eq for MixAutomationLane {}
impl std::hash::Hash for MixAutomationLane {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.target.hash(state);
        self.points.hash(state);
    }
}

impl MixAutomationLane {
    pub fn value_at(&self, frame: i64, fallback: f32) -> f32 {
        if self.points.is_empty() {
            return fallback;
        }
        if frame <= self.points[0].frame {
            return self.points[0].value;
        }
        let last = self.points.len() - 1;
        if frame >= self.points[last].frame {
            return self.points[last].value;
        }
        for i in 1..self.points.len() {
            let a = self.points[i - 1];
            let b = self.points[i];
            if frame <= b.frame {
                let span = (b.frame - a.frame).max(1) as f32;
                let t = (frame - a.frame) as f32 / span;
                return a.value + (b.value - a.value) * t;
            }
        }
        fallback
    }

    pub fn write(&mut self, frame: i64, value: f32) {
        if let Some(i) = self.points.iter().position(|p| p.frame >= frame) {
            if self.points[i].frame == frame {
                self.points[i].value = value;
            } else {
                self.points.insert(i, MixAutomationPoint { frame, value });
            }
        } else {
            self.points.push(MixAutomationPoint { frame, value });
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MixTrack {
    pub id: Uuid,
    pub lane: MixLane,
    pub name: String,
    pub fader: f32,
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
    pub knobs: Vec<f32>,
    pub knob_maps: Vec<Option<MixKnobMap>>,
    pub clips: Vec<MixClip>,
    pub inserts: Vec<MixInsert>,
    pub automation: Vec<MixAutomationLane>,
}

impl MixTrack {
    pub fn empty(lane: MixLane, name: Option<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.unwrap_or_else(|| lane.title()),
            lane,
            fader: FADER_LIN_0DB,
            pan: 0.5,
            mute: false,
            solo: false,
            knobs: vec![0.0; KNOB_COUNT],
            knob_maps: vec![None; KNOB_COUNT],
            clips: Vec::new(),
            inserts: Vec::new(),
            automation: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.clips.is_empty()
    }

    /// Default mix template always has Ch 1–8. Hide slots the take never recorded.
    pub fn is_unused_template_strip(&self) -> bool {
        match self.lane {
            MixLane::Strip(i) if self.clips.is_empty() && self.inserts.is_empty() => {
                self.name == MixLane::Strip(i).title()
            }
            _ => false,
        }
    }

    pub fn automation_value(&self, target: MixAutomationTarget, frame: i64, fallback: f32) -> f32 {
        self.automation
            .iter()
            .find(|lane| lane.target == target)
            .map(|lane| lane.value_at(frame, fallback))
            .unwrap_or(fallback)
    }

    pub fn write_automation(&mut self, target: MixAutomationTarget, frame: i64, value: f32) {
        if let Some(lane) = self.automation.iter_mut().find(|l| l.target == target) {
            lane.write(frame, value);
        } else {
            let mut lane = MixAutomationLane { target, points: Vec::new() };
            lane.write(frame, value);
            self.automation.push(lane);
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MixDocument {
    pub id: Uuid,
    pub name: String,
    pub tracks: Vec<MixTrack>,
    #[serde(default)]
    pub start_frame: i64,
}

impl MixDocument {
    pub fn empty(name: impl Into<String>, send_count: i32) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            tracks: MixLane::default_lanes(send_count)
                .into_iter()
                .map(|lane| MixTrack::empty(lane, None))
                .collect(),
            start_frame: 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.tracks.iter().filter(|t| t.lane != MixLane::Main).all(|t| t.is_empty())
    }

    pub fn track(&self, lane: MixLane) -> Option<&MixTrack> {
        self.tracks.iter().find(|t| t.lane == lane)
    }

    pub fn track_mut(&mut self, lane: MixLane) -> Option<&mut MixTrack> {
        self.tracks.iter_mut().find(|t| t.lane == lane)
    }

    pub fn ensure_track(&mut self, lane: MixLane, name: Option<String>) -> usize {
        if let Some(i) = self.tracks.iter().position(|t| t.lane == lane) {
            return i;
        }
        self.tracks.push(MixTrack::empty(lane, name));
        self.tracks.len() - 1
    }

    pub fn ensure_main_bus(&mut self) {
        let i = self.ensure_track(MixLane::Main, Some("Main".into()));
        self.tracks[i].clips.clear();
        self.tracks[i].name = "Main".into();
        self.start_frame = self.start_frame.max(0);
    }

    pub fn last_clip_end(&self) -> i64 {
        self.tracks
            .iter()
            .filter(|t| t.lane != MixLane::Main)
            .flat_map(|t| t.clips.iter())
            .map(MixClip::mix_end_frame)
            .max()
            .unwrap_or(0)
    }

    pub fn copy_clips(&self, ids: &[Uuid]) -> Option<MixPasteboard> {
        for track in &self.tracks {
            let clips: Vec<MixClip> =
                track.clips.iter().filter(|c| ids.contains(&c.id)).cloned().collect();
            if !clips.is_empty() {
                return Some(MixPasteboard { clips, source_lane: track.lane });
            }
        }
        None
    }

    pub fn paste_clips(&mut self, board: &MixPasteboard, dest: MixLane, at: i64) {
        let Some(track) = self.track_mut(dest) else {
            return;
        };
        let origin = board.clips.iter().map(|c| c.mix_start_frame).min().unwrap_or(0);
        for clip in &board.clips {
            let mut next = clip.clone();
            next.id = Uuid::new_v4();
            next.mix_start_frame = at + (clip.mix_start_frame - origin);
            track.clips.push(next);
        }
    }

    pub fn export_file_name(&self) -> String {
        let trimmed = self.name.trim();
        let mapped: String = (if trimmed.is_empty() { "Mix" } else { trimmed })
            .chars()
            .map(|ch| match ch {
                '/' | ':' | '\\' | ' ' => '-',
                c => c,
            })
            .collect();
        let collapsed = collapse_dashes(&mapped);
        let trimmed = collapsed.trim_matches('-');
        let stem = if trimmed.is_empty() { "Mix" } else { trimmed };
        format!("{stem}.wav")
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MixPasteboard {
    pub clips: Vec<MixClip>,
    pub source_lane: MixLane,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MixArrangement {
    Mix(Uuid),
    Take(i32),
}

impl Serialize for MixArrangement {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Mix(id) => {
                let mut c = serializer.serialize_struct("MixArrangement", 2)?;
                c.serialize_field("kind", "mix")?;
                c.serialize_field("id", id)?;
                c.end()
            }
            Self::Take(number) => {
                let mut c = serializer.serialize_struct("MixArrangement", 2)?;
                c.serialize_field("kind", "take")?;
                c.serialize_field("number", number)?;
                c.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for MixArrangement {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            kind: String,
            #[serde(default)]
            id: Option<Uuid>,
            #[serde(default)]
            number: Option<i32>,
        }
        let raw = Raw::deserialize(deserializer)?;
        match raw.kind.as_str() {
            "take" => Ok(Self::Take(raw.number.unwrap_or(0))),
            _ => Ok(Self::Mix(raw.id.ok_or_else(|| de::Error::missing_field("id"))?)),
        }
    }
}

/// Bar grid step. Raw values are MixLink `0.0625…8`; titles `"1/16"`…`"8"`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MixGrid {
    Bar16,
    Bar8,
    Bar4,
    Bar2,
    Bar1,
    Bars2,
    Bars4,
    Bars8,
}

impl MixGrid {
    pub const ALL: [MixGrid; 8] = [
        Self::Bar16,
        Self::Bar8,
        Self::Bar4,
        Self::Bar2,
        Self::Bar1,
        Self::Bars2,
        Self::Bars4,
        Self::Bars8,
    ];

    pub fn raw(self) -> f64 {
        match self {
            Self::Bar16 => 0.0625,
            Self::Bar8 => 0.125,
            Self::Bar4 => 0.25,
            Self::Bar2 => 0.5,
            Self::Bar1 => 1.0,
            Self::Bars2 => 2.0,
            Self::Bars4 => 4.0,
            Self::Bars8 => 8.0,
        }
    }

    pub fn from_raw(value: f64) -> Self {
        Self::ALL
            .into_iter()
            .find(|g| (g.raw() - value).abs() < 1e-9)
            .unwrap_or(Self::Bar1)
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Bar16 => "1/16",
            Self::Bar8 => "1/8",
            Self::Bar4 => "1/4",
            Self::Bar2 => "1/2",
            Self::Bar1 => "1",
            Self::Bars2 => "2",
            Self::Bars4 => "4",
            Self::Bars8 => "8",
        }
    }

    pub fn help(self) -> String {
        match self {
            Self::Bar16 | Self::Bar8 | Self::Bar4 | Self::Bar2 => format!("{} bar", self.title()),
            Self::Bar1 => "1 bar".into(),
            _ => format!("{} bars", self.title()),
        }
    }
}

impl Default for MixGrid {
    fn default() -> Self {
        Self::Bar1
    }
}

impl Serialize for MixGrid {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_f64(self.raw())
    }
}

impl<'de> Deserialize<'de> for MixGrid {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self::from_raw(f64::deserialize(deserializer)?))
    }
}

/// Musical time at a sample rate and tempo. 4 beats per bar.
pub struct MixTime;

impl MixTime {
    pub fn frames_per_beat(tempo: f64, sample_rate: f64) -> f64 {
        (sample_rate * 60.0 / tempo.max(20.0)).max(1.0)
    }

    pub fn frames_per_bar(tempo: f64, sample_rate: f64) -> f64 {
        Self::frames_per_beat(tempo, sample_rate) * 4.0
    }

    pub fn bar_of(frame: i64, tempo: f64, sample_rate: f64) -> f64 {
        frame as f64 / Self::frames_per_bar(tempo, sample_rate)
    }

    pub fn frame_from_bar(bar: f64, tempo: f64, sample_rate: f64) -> i64 {
        (bar * Self::frames_per_bar(tempo, sample_rate)).round() as i64
    }

    /// Snap `frame` to `step_bars`, relative to `origin` (arrangement start).
    pub fn snap(frame: i64, step_bars: f64, tempo: f64, sample_rate: f64, origin: i64) -> i64 {
        let step = Self::frames_per_bar(tempo, sample_rate) * step_bars.max(1.0 / 64.0);
        let relative = (frame - origin) as f64;
        origin + ((relative / step).round() * step) as i64
    }

    /// `bar.beat.tick` (1-based beat/tick). Negative frames keep a leading `-`.
    pub fn format_position(frame: i64, tempo: f64, sample_rate: f64) -> String {
        let negative = frame < 0;
        let beats = frame.unsigned_abs() as f64 / Self::frames_per_beat(tempo, sample_rate);
        let bar = (beats / 4.0) as i64;
        let beat = (beats % 4.0) as i64 + 1;
        let tick = ((beats * 4.0) % 4.0) as i64 + 1;
        format!("{}{bar}.{beat}.{tick}", if negative { "-" } else { "" })
    }

    pub fn format_clock(frame: i64, sample_rate: f64) -> String {
        Self::format_clock_seconds(frame as f64 / sample_rate.max(1.0))
    }

    pub fn format_clock_seconds(seconds: f64) -> String {
        if !seconds.is_finite() {
            return "0:00".into();
        }
        let negative = seconds < 0.0;
        let t = seconds.abs();
        let hours = t as i64 / 3600;
        let minutes = (t as i64 % 3600) / 60;
        let secs = t % 60.0;
        let prefix = if negative { "-" } else { "" };
        let tenths = (secs * 10.0).round() / 10.0;
        if (tenths - tenths.round()).abs() < 0.001 {
            let whole = tenths.round() as i64;
            if hours > 0 {
                format!("{prefix}{hours}:{minutes:02}:{whole:02}")
            } else {
                format!("{prefix}{minutes}:{whole:02}")
            }
        } else if hours > 0 {
            format!("{prefix}{hours}:{minutes:02}:{tenths:04.1}")
        } else {
            format!("{prefix}{minutes}:{tenths:04.1}")
        }
    }
}

/// Deterministic clip id for take-view lanes (MixLink `MixClipID.take`).
pub fn take_clip_id(number: i32, file: &str) -> Uuid {
    let data = format!("mixlink.take.{number}.{file}");
    let mut bytes = [0u8; 16];
    for (i, b) in data.bytes().enumerate() {
        bytes[i % 16] ^= b;
    }
    Uuid::from_bytes(bytes)
}

#[derive(Clone, Debug, PartialEq)]
pub struct TakeFile {
    pub lane: MixLane,
    pub name: String,
    pub filename: String,
    pub frame_count: i64,
    pub sample_rate: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TakeInfo {
    pub number: i32,
    pub files: Vec<TakeFile>,
}

impl TakeInfo {
    pub fn frame_count(&self) -> i64 {
        self.files.iter().map(|f| f.frame_count).max().unwrap_or(0)
    }
}

/// `/ : \ space` → `-`, collapse `--`, empty → `Track`.
pub fn sanitize_take_name(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return "Track".into();
    }
    let mapped: String = trimmed
        .chars()
        .map(|ch| match ch {
            '/' | ':' | '\\' | ' ' => '-',
            c => c,
        })
        .collect();
    let collapsed = collapse_dashes(&mapped);
    let trimmed = collapsed.trim_matches('-');
    if trimmed.is_empty() {
        "Track".into()
    } else {
        trimmed.into()
    }
}

fn collapse_dashes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_dash = false;
    for ch in s.chars() {
        if ch == '-' {
            if !prev_dash {
                out.push(ch);
            }
            prev_dash = true;
        } else {
            prev_dash = false;
            out.push(ch);
        }
    }
    out
}

/// Recorder filenames. There is no `take-` prefix.
///
/// `{take}-ch-{01..08}-{name}.wav`
/// `{take}-ret-{A|B|C|D|E|F}-{name}.wav`
/// `{take}-bus-{01|02}-{name}.wav`
/// `{take}-mix.wav`
pub fn take_wav_name(take: i32, lane: MixLane, name: &str) -> String {
    match lane {
        MixLane::Strip(i) => {
            format!("{take}-ch-{:02}-{}.wav", i + 1, sanitize_take_name(name))
        }
        MixLane::ReturnLane(lane) if lane.is_send() => {
            format!("{take}-ret-{}-{}.wav", lane.strip_title(), sanitize_take_name(name))
        }
        MixLane::ReturnLane(lane) => {
            let n = if lane == ReturnLane::Bus1 { 1 } else { 2 };
            format!("{take}-bus-{n:02}-{}.wav", sanitize_take_name(name))
        }
        MixLane::Main => format!("{take}-mix.wav"),
    }
}

/// Inverse of [`take_wav_name`]. Name dashes become spaces.
pub fn parse_take_wav(filename: &str) -> Option<(i32, MixLane, String)> {
    let stem = filename.strip_suffix(".wav").or_else(|| filename.strip_suffix(".WAV")).unwrap_or(filename);
    let mut parts = stem.splitn(4, '-');
    let take = parts.next()?.parse::<i32>().ok()?;
    match parts.next()? {
        "ch" => {
            let ch = parts.next()?.parse::<i32>().ok()?;
            let name = parts.next().unwrap_or("").replace('-', " ");
            let name = if name.is_empty() { format!("Ch {ch}") } else { name };
            Some((take, MixLane::Strip((ch - 1).max(0)), name))
        }
        "ret" => {
            let letter = parts.next()?;
            let lane = ALL_SEND_LANES.iter().copied().find(|l| l.strip_title() == letter)?;
            let name = parts.next().unwrap_or("").replace('-', " ");
            let name = if name.is_empty() { format!("Ret {letter}") } else { name };
            Some((take, MixLane::ReturnLane(lane), name))
        }
        "bus" => {
            let n = parts.next()?.parse::<i32>().ok()?;
            let lane = if n == 1 { ReturnLane::Bus1 } else { ReturnLane::Bus2 };
            let name = parts.next().unwrap_or("").replace('-', " ");
            let name = if name.is_empty() { format!("Bus {n}") } else { name };
            Some((take, MixLane::ReturnLane(lane), name))
        }
        "mix" => Some((take, MixLane::Main, "Mix".into())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEMPO: f64 = 120.0;
    const SR: f64 = 48_000.0;

    #[test]
    fn frames_per_bar_at_120() {
        assert_eq!(MixTime::frames_per_beat(TEMPO, SR), 24_000.0);
        assert_eq!(MixTime::frames_per_bar(TEMPO, SR), 96_000.0);
    }

    #[test]
    fn bar_frame_roundtrip() {
        for bar in [-4.0, -1.0, 0.0, 1.0, 8.0] {
            let frame = MixTime::frame_from_bar(bar, TEMPO, SR);
            let back = MixTime::bar_of(frame, TEMPO, SR);
            assert!((back - bar).abs() < 1e-9, "{bar} -> {frame} -> {back}");
        }
    }

    #[test]
    fn snap_origin_relative_including_negative() {
        let origin = MixTime::frame_from_bar(4.0, TEMPO, SR);
        let left = MixTime::frame_from_bar(2.3, TEMPO, SR);
        let snapped = MixTime::snap(left, 1.0, TEMPO, SR, origin);
        assert_eq!(snapped, MixTime::frame_from_bar(2.0, TEMPO, SR));

        let before_origin = origin - 48_000;
        let snapped = MixTime::snap(before_origin, 1.0, TEMPO, SR, origin);
        // −0.5 bar from origin rounds away from zero onto bar 3.
        assert_eq!(snapped, MixTime::frame_from_bar(3.0, TEMPO, SR));

        let far_left = MixTime::frame_from_bar(-2.2, TEMPO, SR);
        let snapped = MixTime::snap(far_left, 1.0, TEMPO, SR, 0);
        assert_eq!(snapped, MixTime::frame_from_bar(-2.0, TEMPO, SR));
    }

    #[test]
    fn format_position_origin_and_negative() {
        assert_eq!(MixTime::format_position(0, TEMPO, SR), "0.1.1");
        let one_beat = MixTime::frames_per_beat(TEMPO, SR) as i64;
        assert_eq!(MixTime::format_position(one_beat, TEMPO, SR), "0.2.1");
        assert_eq!(MixTime::format_position(-one_beat, TEMPO, SR), "-0.2.1");
        let one_bar = MixTime::frames_per_bar(TEMPO, SR) as i64;
        assert_eq!(MixTime::format_position(one_bar, TEMPO, SR), "1.1.1");
        assert_eq!(MixTime::format_position(-one_bar, TEMPO, SR), "-1.1.1");
    }

    #[test]
    fn format_clock_variants() {
        assert_eq!(MixTime::format_clock_seconds(0.0), "0:00");
        assert_eq!(MixTime::format_clock_seconds(65.0), "1:05");
        assert_eq!(MixTime::format_clock_seconds(-1.5), "-0:01.5");
        assert_eq!(MixTime::format_clock_seconds(3661.0), "1:01:01");
    }

    #[test]
    fn take_wav_name_fixtures() {
        assert_eq!(
            take_wav_name(3, MixLane::Strip(0), "Rytm"),
            "3-ch-01-Rytm.wav"
        );
        assert_eq!(
            take_wav_name(3, MixLane::ReturnLane(ReturnLane::SendA), "BigSky"),
            "3-ret-A-BigSky.wav"
        );
        assert_eq!(
            take_wav_name(3, MixLane::ReturnLane(ReturnLane::Bus1), "x"),
            "3-bus-01-x.wav"
        );
        assert_eq!(take_wav_name(3, MixLane::Main, "ignored"), "3-mix.wav");
    }

    #[test]
    fn sanitize_collapses_and_falls_back() {
        assert_eq!(sanitize_take_name("A / B:C"), "A-B-C");
        assert_eq!(sanitize_take_name("  "), "Track");
        assert_eq!(sanitize_take_name("a  b"), "a-b");
    }

    #[test]
    fn parse_take_wav_fixtures() {
        let (take, lane, name) = parse_take_wav("3-ch-01-Rytm.wav").unwrap();
        assert_eq!((take, lane, name.as_str()), (3, MixLane::Strip(0), "Rytm"));
        let (take, lane, name) = parse_take_wav("3-ret-A-BigSky.wav").unwrap();
        assert_eq!(
            (take, lane, name.as_str()),
            (3, MixLane::ReturnLane(ReturnLane::SendA), "BigSky")
        );
        let (take, lane, name) = parse_take_wav("3-bus-01-x.wav").unwrap();
        assert_eq!(
            (take, lane, name.as_str()),
            (3, MixLane::ReturnLane(ReturnLane::Bus1), "x")
        );
        let (take, lane, name) = parse_take_wav("3-mix.wav").unwrap();
        assert_eq!((take, lane, name.as_str()), (3, MixLane::Main, "Mix"));
    }

    #[test]
    fn mix_grid_titles() {
        let titles: Vec<_> = MixGrid::ALL.iter().map(|g| g.title()).collect();
        assert_eq!(titles, ["1/16", "1/8", "1/4", "1/2", "1", "2", "4", "8"]);
        assert_eq!(MixGrid::default(), MixGrid::Bar1);
        assert_eq!(MixGrid::from_raw(0.0625), MixGrid::Bar16);
    }

    #[test]
    fn mix_document_json_roundtrip() {
        let mut mix = MixDocument::empty("Mix 1", 2);
        mix.start_frame = 48000;
        let insert_id = Uuid::from_u128(0x1111);
        mix.tracks[0].clips.push(MixClip {
            id: Uuid::from_u128(0xaaaa),
            source_take: 3,
            source_lane: MixLane::Strip(0),
            source_file: "3-ch-01-Rytm.wav".into(),
            source_start_frame: 0,
            source_frame_count: 96_000,
            mix_start_frame: 0,
        });
        mix.tracks[0].inserts.push(MixInsert {
            id: insert_id,
            name: "BigSky".into(),
            bundle_path: Some("/Library/Audio/Plug-Ins/VST3/BigSky.vst3".into()),
            class_uid: Some("abcd".into()),
            bypassed: false,
        });
        mix.tracks[0].write_automation(MixAutomationTarget::Volume, 0, 0.5);
        mix.tracks[0].write_automation(MixAutomationTarget::Knob(2), 100, 0.25);
        mix.tracks[0].write_automation(
            MixAutomationTarget::InsertParam { insert_id, parameter_id: 7 },
            200,
            0.8,
        );

        let json = serde_json::to_string_pretty(&mix).unwrap();
        assert!(json.contains("\"sourceTake\""));
        assert!(json.contains("\"startFrame\""));
        assert!(json.contains("\"kind\": \"strip\""));
        let back: MixDocument = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, mix.id);
        assert_eq!(back.name, mix.name);
        assert_eq!(back.start_frame, 48000);
        assert_eq!(back.tracks[0].clips.len(), 1);
        assert_eq!(back.tracks[0].clips[0].source_file, "3-ch-01-Rytm.wav");
        assert_eq!(back.tracks[0].inserts[0].name, "BigSky");
        assert_eq!(back.tracks[0].automation.len(), 3);
        assert_eq!(
            back.tracks[0].automation_value(MixAutomationTarget::Volume, 0, 0.0),
            0.5
        );
    }

    #[test]
    fn mix_lane_json_matches_mixlink() {
        let strip = serde_json::to_value(MixLane::Strip(3)).unwrap();
        assert_eq!(strip, serde_json::json!({"kind":"strip","index":3}));
        let ret = serde_json::to_value(MixLane::ReturnLane(ReturnLane::SendC)).unwrap();
        assert_eq!(ret, serde_json::json!({"kind":"return","index":4}));
        let main = serde_json::to_value(MixLane::Main).unwrap();
        assert_eq!(main, serde_json::json!({"kind":"main"}));
        let decoded: MixLane = serde_json::from_value(serde_json::json!({
            "kind": "return",
            "index": 99
        }))
        .unwrap();
        assert_eq!(decoded, MixLane::ReturnLane(ReturnLane::SendA));
    }
}
