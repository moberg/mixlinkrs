//! TotalMix mirror: channels, send matrix, connection flag.

use std::collections::HashMap;

use osc::{addresses, fader_lin_from_db, OscValue};

use crate::types::{ChannelID, MixEvent, MixNode, MixerBus, MixerChannel, StripName};

#[derive(Clone, Debug)]
pub struct MixerState {
    pub inputs: Vec<MixerChannel>,
    pub playback: Vec<MixerChannel>,
    pub outputs: Vec<MixerChannel>,
    /// −∞ until TotalMix sends `/output/{main}/faderlin`.
    pub main_fader: f32,
    /// Output index whose faderlin updates `main_fader`.
    pub monitored_output: i32,
    pub dim: bool,
    pub mono: bool,
    pub speaker_b: bool,
    pub talkback: bool,
    pub connected: bool,
    pub last_osc_log: String,
    /// source ChannelID → dest output index → 0...1
    pub sends: HashMap<ChannelID, HashMap<i32, f32>>,
    /// source ChannelID → dest output index → TotalMix balpan (−1...+1)
    pub send_pans: HashMap<ChannelID, HashMap<i32, f32>>,
}

impl Default for MixerState {
    fn default() -> Self {
        Self::new()
    }
}

impl MixerState {
    pub fn new() -> Self {
        Self {
            inputs: adat_names(MixerBus::Input, 32),
            playback: {
                let mut pb = adat_names(MixerBus::Playback, 32);
                pb.push(MixerChannel::placeholder(MixerBus::Playback, 32, false, "PH"));
                pb
            },
            outputs: output_placeholders(),
            main_fader: 0.0,
            monitored_output: 0,
            dim: false,
            mono: false,
            speaker_b: false,
            talkback: false,
            connected: false,
            last_osc_log: String::new(),
            sends: HashMap::new(),
            send_pans: HashMap::new(),
        }
    }

    pub fn channel(&self, id: ChannelID) -> Option<&MixerChannel> {
        self.bus_channels(id.bus).iter().find(|ch| ch.id == id)
    }

    pub fn channel_mut(&mut self, id: ChannelID) -> Option<&mut MixerChannel> {
        self.bus_channels_mut(id.bus).iter_mut().find(|ch| ch.id == id)
    }

    pub fn update_channel(&mut self, id: ChannelID, mutate: impl FnOnce(&mut MixerChannel)) {
        let channels = self.bus_channels_mut(id.bus);
        if let Some(i) = channels.iter().position(|ch| ch.id == id) {
            mutate(&mut channels[i]);
        } else {
            let mut created =
                MixerChannel::placeholder(id.bus, id.index, false, default_name(id.bus, id.index));
            mutate(&mut created);
            channels.push(created);
            channels.sort_by_key(|ch| ch.id.index);
        }
    }

    pub fn send_level(&self, source: ChannelID, dest: i32) -> f32 {
        self.send_level_if_present(source, dest).unwrap_or(0.0)
    }

    /// `None` when TotalMix has not reported this mix node.
    pub fn send_level_if_present(&self, source: ChannelID, dest: i32) -> Option<f32> {
        self.sends.get(&source).and_then(|row| row.get(&dest)).copied()
    }

    pub fn set_send(&mut self, source: ChannelID, dest: i32, level: f32) {
        let row = self.sends.entry(source).or_default();
        row.insert(dest, level.clamp(0.0, 1.0));
    }

    pub fn send_pan_if_present(&self, source: ChannelID, dest: i32) -> Option<f32> {
        self.send_pans.get(&source).and_then(|row| row.get(&dest)).copied()
    }

    pub fn set_send_pan(&mut self, source: ChannelID, dest: i32, balpan: f32) {
        let row = self.send_pans.entry(source).or_default();
        row.insert(dest, balpan.clamp(-1.0, 1.0));
    }

    pub fn all_channels(&self, bus: MixerBus) -> Vec<MixerChannel> {
        let mut ch = self.bus_channels(bus).to_vec();
        ch.sort_by_key(|c| c.id.index);
        ch
    }

    /// Stereo-grouped strips (left of pair + monos), MixLink `mixer.strips(for:)`.
    pub fn strips(&self, bus: MixerBus) -> Vec<MixerChannel> {
        let mut out = Vec::new();
        let mut skip = -1i32;
        for ch in self.all_channels(bus) {
            if ch.id.index <= skip {
                continue;
            }
            if ch.stereo {
                skip = ch.id.index + 1;
            }
            out.push(ch);
        }
        out
    }

    fn bus_channels(&self, bus: MixerBus) -> &[MixerChannel] {
        match bus {
            MixerBus::Input => &self.inputs,
            MixerBus::Playback => &self.playback,
            MixerBus::Output => &self.outputs,
        }
    }

    fn bus_channels_mut(&mut self, bus: MixerBus) -> &mut Vec<MixerChannel> {
        match bus {
            MixerBus::Input => &mut self.inputs,
            MixerBus::Playback => &mut self.playback,
            MixerBus::Output => &mut self.outputs,
        }
    }
}

/// Apply one inbound OSC value to mixer state. UI does not send OSC from here.
pub fn apply_inbound(mixer: &mut MixerState, address: &str, value: &OscValue) -> Option<MixEvent> {
    let parts: Vec<&str> = address.split('/').filter(|s| !s.is_empty()).collect();
    if parts.is_empty() {
        return None;
    }

    if parts.len() >= 2 && parts[0] == "controlroom" {
        let on = value.float_value() > 0.5;
        match parts[1] {
            "dim" => mixer.dim = on,
            "talkback" => mixer.talkback = on,
            "speakerb" => mixer.speaker_b = on,
            "mainmono" => mixer.mono = on,
            _ => {}
        }
        return None;
    }

    if parts.len() >= 3 && parts[0] == "output" {
        if let Ok(ch) = parts[1].parse::<i32>() {
            let id = ChannelID::new(MixerBus::Output, ch);
            match parts[2] {
                "name" => {
                    if let OscValue::String(s) = value {
                        mixer.update_channel(id, |c| {
                            c.name = s.clone();
                            c.from_total_mix = true;
                            if s.contains('/') {
                                c.stereo = true;
                            }
                        });
                    }
                }
                "faderlin" | "volume" => {
                    mixer.update_channel(id, |c| {
                        c.fader = value.float_value();
                        c.from_total_mix = true;
                    });
                    if ch == mixer.monitored_output {
                        mixer.main_fader = value.float_value();
                    }
                }
                "fader" => {
                    let lin = fader_lin_from_db(value.float_value());
                    mixer.update_channel(id, |c| {
                        c.fader = lin;
                        c.from_total_mix = true;
                    });
                    if ch == mixer.monitored_output {
                        mixer.main_fader = lin;
                    }
                }
                "stereo" => {
                    mixer.update_channel(id, |c| {
                        c.stereo = value.float_value() > 0.5;
                        c.from_total_mix = true;
                    });
                    return Some(MixEvent::Layout);
                }
                _ => {}
            }
            return None;
        }
    }

    if parts.len() >= 3 {
        if let (Some(bus), Ok(ch)) =
            (addresses::Bus::from_strip_token(parts[0]), parts[1].parse::<i32>())
        {
            let bus = MixerBus::from(bus);
            let id = ChannelID::new(bus, ch);
            match parts[2] {
                "name" => {
                    if let OscValue::String(s) = value {
                        mixer.update_channel(id, |c| {
                            c.name = s.clone();
                            c.from_total_mix = true;
                            if s.contains('/') {
                                c.stereo = true;
                            }
                        });
                        return Some(MixEvent::Name(StripName {
                            bus,
                            channel: ch,
                            name: s.clone(),
                        }));
                    }
                }
                "mute" => {
                    mixer.update_channel(id, |c| c.mute = value.float_value() > 0.5);
                    return Some(MixEvent::Pads);
                }
                "solo" => {
                    mixer.update_channel(id, |c| c.solo = value.float_value() > 0.5);
                    return Some(MixEvent::Pads);
                }
                "stereo" => {
                    mixer.update_channel(id, |c| {
                        c.stereo = value.float_value() > 0.5;
                        c.from_total_mix = true;
                    });
                    return Some(MixEvent::Layout);
                }
                _ => {}
            }
        }
    }

    if parts.len() >= 5 && parts[0] == "mix" {
        if let (Some(src_bus), Ok(src), Ok(dest)) = (
            addresses::Bus::from_mix_token(parts[1]),
            parts[2].parse::<i32>(),
            parts[3].parse::<i32>(),
        ) {
            let src_bus = MixerBus::from(src_bus);
            let id = ChannelID::new(src_bus, src);
            match parts[4] {
                "faderlin" | "volume" => {
                    mixer.set_send(id, dest, value.float_value());
                    return Some(MixEvent::Mix(MixNode {
                        source_bus: src_bus,
                        source: src,
                        dest,
                        fader_lin: Some(value.float_value()),
                        balpan: None,
                    }));
                }
                "fader" => {
                    let lin = fader_lin_from_db(value.float_value());
                    mixer.set_send(id, dest, lin);
                    return Some(MixEvent::Mix(MixNode {
                        source_bus: src_bus,
                        source: src,
                        dest,
                        fader_lin: Some(lin),
                        balpan: None,
                    }));
                }
                "balpan" => {
                    let pan = value.float_value();
                    mixer.set_send_pan(id, dest, pan);
                    mixer.update_channel(id, |c| c.pan = osc::balpan_to_pan_unit(pan));
                    return Some(MixEvent::Mix(MixNode {
                        source_bus: src_bus,
                        source: src,
                        dest,
                        fader_lin: None,
                        balpan: Some(value.float_value()),
                    }));
                }
                _ => {}
            }
        }
    }

    None
}

fn default_name(bus: MixerBus, index: i32) -> String {
    if index >= 32 {
        return "PH".into();
    }
    if bus == MixerBus::Output && index % 2 == 0 {
        return format!("ADAT {}/{}", index + 1, index + 2);
    }
    format!("ADAT {}", index + 1)
}

fn output_placeholders() -> Vec<MixerChannel> {
    let mut channels: Vec<MixerChannel> = (0..32)
        .map(|i| {
            MixerChannel::placeholder(
                MixerBus::Output,
                i,
                i % 2 == 0,
                default_name(MixerBus::Output, i),
            )
        })
        .collect();
    channels.push(MixerChannel::placeholder(MixerBus::Output, 32, true, "PH"));
    channels
}

fn adat_names(bus: MixerBus, count: i32) -> Vec<MixerChannel> {
    (0..count).map(|i| MixerChannel::placeholder(bus, i, false, default_name(bus, i))).collect()
}
