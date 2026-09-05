//! Control-surface brain. UI and MIDI call these methods; they never send OSC.

use osc::{
    balpan_to_pan_unit, fader_db, fader_lin_to_amp, mix_balpan, mix_fader, mix_fader_lin,
    pan_unit_to_balpan, post_fader_lin, send_lin_from_post, strip_mute, strip_solo, OscSession,
    OscValue, FADER_LIN_0DB,
};

use crate::config::SessionConfig;
use crate::mixer::{apply_inbound, MixerState};
use crate::surface::SurfaceState;
use crate::types::{
    ChannelID, EffectRef, HardwareEffect, MixAssign, MixEvent, MixNode, MixerBus, MixerChannel,
    ReturnLane, ReturnStrip, RoutingSlot, SendDestination, StripBinding, ALL_SEND_LANES,
};

pub struct AnalogEngine {
    pub mixer: MixerState,
    pub surface: SurfaceState,
    pub config: SessionConfig,
    pub osc: OscSession,
}

impl AnalogEngine {
    pub fn new(
        mixer: MixerState,
        surface: SurfaceState,
        config: SessionConfig,
        osc: OscSession,
    ) -> Self {
        Self { mixer, surface, config, osc }
    }

    /// Drain inbound OSC. Sets `mixer.connected` from the session flag.
    /// TotalMix is the source of truth: dump on connect, then copy mix nodes into the UI.
    pub fn poll_osc(&mut self) {
        let was_connected = self.mixer.connected;
        if self.osc.is_connected() {
            self.mixer.connected = true;
        }
        let just_connected = self.mixer.connected && !was_connected;
        let log = self.osc.last_sent();
        if !log.is_empty() {
            self.mixer.last_osc_log = log;
        }
        let mut saw_mix = false;
        for msg in self.osc.poll() {
            let value = msg.values.first().cloned().unwrap_or(OscValue::Float(0.0));
            match apply_inbound(&mut self.mixer, &msg.address, &value) {
                Some(MixEvent::Mix(node)) => {
                    self.apply_inbound_mix(&node);
                    saw_mix = true;
                }
                Some(MixEvent::Pads | MixEvent::Name(_) | MixEvent::Layout) | None => {}
            }
        }
        if just_connected {
            self.osc.send_dump_requests();
        }
        if just_connected || saw_mix {
            self.sync_surface_from_total_mix();
        }
    }

    pub fn source_ids(&self, strip: usize) -> Vec<ChannelID> {
        let Some(b) = self.config.strips.get(strip) else {
            return Vec::new();
        };
        let mut ids = vec![b.channel_id()];
        if b.linked_stereo {
            ids.push(ChannelID::new(b.bus, b.index + 1));
        }
        ids
    }

    /// Global OSC addresses stereo from the left channel only.
    pub fn osc_sources(&self, strip: usize) -> Vec<ChannelID> {
        self.source_ids(strip).into_iter().take(1).collect()
    }

    pub fn apply_fader(&mut self, strip: usize, value: f32) {
        if let Some(s) = self.surface.strips.get_mut(strip) {
            s.fader = value;
        }
        self.write_assign_sends(strip);
        if self.config.sends_post_fader {
            self.write_all_aux(strip);
        }
    }

    pub fn apply_pan(&mut self, strip: usize, value: f32) {
        if let Some(s) = self.surface.strips.get_mut(strip) {
            s.pan = value;
        }
        let pan = pan_unit_to_balpan(value);
        let dests = self.pan_destinations(strip);
        for id in self.osc_sources(strip) {
            self.mixer.update_channel(id, |c| c.pan = value);
            for dest in &dests {
                self.osc.send_float(mix_balpan(id.bus.osc(), id.index, *dest), pan);
            }
        }
    }

    pub fn output_index(&self, dest: SendDestination) -> i32 {
        self.config.output_index(dest)
    }

    pub fn aux_a_output(&self) -> i32 {
        self.config
            .hardware_output(ReturnLane::SendA)
            .unwrap_or_else(|| self.output_index(self.config.aux_a))
    }

    pub fn aux_b_output(&self) -> i32 {
        self.config
            .hardware_output(ReturnLane::SendB)
            .unwrap_or_else(|| self.output_index(self.config.aux_b))
    }

    pub fn apply_aux_a(&mut self, strip: usize, value: f32) {
        self.apply_aux(strip, ReturnLane::SendA, value);
    }

    pub fn apply_aux_b(&mut self, strip: usize, value: f32) {
        self.apply_aux(strip, ReturnLane::SendB, value);
    }

    pub fn apply_aux(&mut self, strip: usize, lane: ReturnLane, value: f32) {
        if !lane.is_send() {
            return;
        }
        if let Some(s) = self.surface.strips.get_mut(strip) {
            s.set_aux(value, lane);
        }
        let dest = self.config.send_destination(lane);
        let written = self.aux_write_level(strip, value);
        self.write_aux(strip, dest, written);
    }

    pub fn apply_return_fader(&mut self, id: i32, value: f32) {
        self.upsert_return(id, |r| r.fader = value);
        self.write_return_fader(id, value, None, None);
    }

    pub fn apply_return_pan(&mut self, id: i32, value: f32) {
        self.upsert_return(id, |r| r.pan = value);
        self.write_return_pan(id, value);
    }

    pub fn return_source(&self, lane: ReturnLane) -> Option<(MixerBus, i32)> {
        self.config.return_source_id(lane).map(|id| (id.bus, id.index))
    }

    pub fn return_muted(&self, lane: ReturnLane) -> bool {
        if self.surface.returns.iter().any(|r| r.id == lane as i32 && r.mute) {
            return true;
        }
        let Some(id) = self.config.return_source_id(lane) else {
            return false;
        };
        self.mixer.channel(id).is_some_and(|c| c.mute)
    }

    pub fn return_soloed(&self, lane: ReturnLane) -> bool {
        if self.surface.returns.iter().any(|r| r.id == lane as i32 && r.solo) {
            return true;
        }
        let Some(id) = self.config.return_source_id(lane) else {
            return false;
        };
        self.mixer.channel(id).is_some_and(|c| c.solo)
    }

    pub fn apply_return_mute(&mut self, lane: ReturnLane, on: bool) {
        self.upsert_return(lane as i32, |r| {
            r.mute = on;
            if on {
                r.solo = false;
            }
        });
        let Some(id) = self.config.return_source_id(lane) else {
            return;
        };
        self.mixer.update_channel(id, |c| {
            c.mute = on;
            if on {
                c.solo = false;
            }
        });
        self.osc.send_float(strip_mute(id.bus.osc(), id.index), if on { 1.0 } else { 0.0 });
        if on {
            self.osc.send_float(strip_solo(id.bus.osc(), id.index), 0.0);
        }
    }

    pub fn apply_return_solo(&mut self, lane: ReturnLane, on: bool) {
        self.upsert_return(lane as i32, |r| {
            r.solo = on;
            if on {
                r.mute = false;
            }
        });
        let Some(id) = self.config.return_source_id(lane) else {
            return;
        };
        self.mixer.update_channel(id, |c| {
            c.solo = on;
            if on {
                c.mute = false;
            }
        });
        self.osc.send_float(strip_solo(id.bus.osc(), id.index), if on { 1.0 } else { 0.0 });
        if on {
            self.osc.send_float(strip_mute(id.bus.osc(), id.index), 0.0);
        }
    }

    pub fn neutralize_plugin_send_mixes(&mut self) {
        let dests: Vec<SendDestination> = ALL_SEND_LANES
            .iter()
            .filter_map(|lane| self.config.send_destination(*lane))
            .filter(|dest| matches!(dest, SendDestination::Plugin(_)))
            .collect();
        for dest in dests {
            self.clear_send_mix(dest);
        }
    }

    pub fn rewrite_all_sends(&mut self) {
        for i in 0..self.surface.strips.len() {
            self.write_assign_sends(i);
            self.write_all_aux(i);
            let pan = self.surface.strips[i].pan;
            self.apply_pan(i, pan);
        }
    }

    pub fn apply_assign(&mut self, strip: usize, previous: MixAssign, assign: MixAssign) {
        if let Some(s) = self.surface.strips.get_mut(strip) {
            s.assign = assign;
        }
        if previous != assign {
            if let Some(dest) = self.config.listen_output(previous) {
                self.zero_assign_dest(strip, dest);
            }
        }
        self.write_assign_sends(strip);
    }

    pub fn apply_mute(&mut self, strip: usize, on: bool) {
        for id in self.osc_sources(strip) {
            self.mixer.update_channel(id, |c| {
                c.mute = on;
                if on {
                    c.solo = false;
                }
            });
            self.osc.send_float(strip_mute(id.bus.osc(), id.index), if on { 1.0 } else { 0.0 });
            if on {
                self.osc.send_float(strip_solo(id.bus.osc(), id.index), 0.0);
            }
        }
        if self.config.sends_post_fader {
            self.rewrite_aux_sends(None);
        }
    }

    pub fn apply_solo(&mut self, strip: usize, on: bool) {
        for id in self.osc_sources(strip) {
            self.mixer.update_channel(id, |c| {
                c.solo = on;
                if on {
                    c.mute = false;
                }
            });
            self.osc.send_float(strip_solo(id.bus.osc(), id.index), if on { 1.0 } else { 0.0 });
            if on {
                self.osc.send_float(strip_mute(id.bus.osc(), id.index), 0.0);
            }
        }
        if self.config.sends_post_fader {
            self.rewrite_aux_sends(None);
        }
    }

    pub fn toggle_mute(&mut self, strip: usize) {
        let Some(id) = self.osc_sources(strip).first().copied() else {
            return;
        };
        let on = !self.mixer.channel(id).is_some_and(|c| c.mute);
        self.apply_mute(strip, on);
    }

    pub fn strip_muted(&self, strip: usize) -> bool {
        if self.config.strips.get(strip).is_none() {
            return false;
        }
        if !self.config.is_strip_enabled(strip) {
            return true;
        }
        self.source_ids(strip).iter().any(|id| self.mixer.channel(*id).is_some_and(|c| c.mute))
    }

    pub fn strip_soloed(&self, strip: usize) -> bool {
        if self.config.strips.get(strip).is_none() {
            return false;
        }
        self.source_ids(strip).iter().any(|id| self.mixer.channel(*id).is_some_and(|c| c.solo))
    }

    pub fn any_solo_active(&self) -> bool {
        (0..self.surface.strips.len()).any(|i| self.strip_soloed(i))
            || ReturnLane::ALL.iter().any(|lane| self.return_soloed(*lane))
    }

    pub fn toggle_solo(&mut self, strip: usize) {
        let Some(id) = self.osc_sources(strip).first().copied() else {
            return;
        };
        let on = !self.mixer.channel(id).is_some_and(|c| c.solo);
        self.apply_solo(strip, on);
    }

    pub fn write_assign_sends(&mut self, strip: usize) {
        let Some(s) = self.surface.strips.get(strip) else {
            return;
        };
        let assign = s.assign;
        let level = if self.config.is_strip_enabled(strip) { s.fader } else { 0.0 };
        if let Some(dest) = self.config.listen_output(assign) {
            self.write_send(strip, dest, level);
        }
        for other in MixAssign::ALL {
            if other == assign {
                continue;
            }
            if let Some(dest) = self.config.listen_output(other) {
                self.zero_assign_dest(strip, dest);
            }
        }
    }

    pub fn apply_inbound_mix(&mut self, node: &MixNode) {
        for i in 0..self.config.strips.len() {
            if !self.config.strips[i].enabled {
                continue;
            }
            if self.config.strips[i].bus != node.source_bus {
                continue;
            }
            let sources: Vec<i32> = self.source_ids(i).iter().map(|id| id.index).collect();
            if !sources.contains(&node.source) {
                continue;
            }
            let assigned = self.config.listen_output(self.surface.strips[i].assign);
            if let (Some(assigned), Some(v)) = (assigned, node.fader_lin) {
                if node.dest == assigned {
                    self.surface.strips[i].fader = v;
                }
            }
            if let (Some(assigned), Some(pan)) = (assigned, node.balpan) {
                if node.dest == assigned {
                    self.surface.strips[i].pan = balpan_to_pan_unit(pan);
                }
            }
        }
        for lane in ReturnLane::ALL {
            let Some(source) = self.return_source(lane) else {
                continue;
            };
            if node.source_bus != source.0
                || node.source != source.1
                || node.dest != self.config.main_output
            {
                continue;
            }
            if let Some(v) = node.fader_lin {
                if self.config.is_return_enabled(lane) {
                    self.set_return_fader_state(lane as i32, v);
                }
            }
            if let Some(pan) = node.balpan {
                self.upsert_return(lane as i32, |r| r.pan = balpan_to_pan_unit(pan));
            }
        }
    }

    /// Copy TotalMix mix-matrix levels onto every on-screen control.
    pub fn sync_surface_from_total_mix(&mut self) {
        self.pull_strip_faders_from_sends();
        self.pull_strip_pans_from_sends();
        self.pull_send_levels_from_total_mix(None, None, false);
        self.pull_return_faders_from_sends();
        self.pull_return_pans_from_sends();
        self.pull_main_fader_from_total_mix();
    }

    /// TotalMix send from this strip into the Main mix (`main_output`).
    pub fn strip_main_mix_lin(&self, strip: usize) -> f32 {
        if !self.config.is_strip_enabled(strip) {
            return 0.0;
        }
        for src in self.osc_sources(strip) {
            if let Some(level) = self.mixer.send_level_if_present(src, self.config.main_output) {
                return level;
            }
        }
        if self.surface.strips.get(strip).is_some_and(|s| s.assign == MixAssign::Main) {
            return self.surface.strips[strip].fader;
        }
        0.0
    }

    pub fn strip_main_mix_pan(&self, strip: usize) -> f32 {
        for src in self.osc_sources(strip) {
            if let Some(pan) = self.mixer.send_pan_if_present(src, self.config.main_output) {
                return balpan_to_pan_unit(pan);
            }
        }
        self.surface.strips.get(strip).map(|s| s.pan).unwrap_or(0.5)
    }

    /// Pan printed onto a recorded stem: analog / XL knob, not a stale Main send.
    pub fn strip_record_pan(&self, strip: usize) -> f32 {
        self.surface.strips.get(strip).map(|s| s.pan).unwrap_or(0.5)
    }

    /// TotalMix send from a return/bus source into the Main mix.
    pub fn return_main_mix_lin(&self, lane: ReturnLane) -> f32 {
        if !self.config.is_return_enabled(lane) {
            return 0.0;
        }
        if let Some(source) = self.return_source(lane) {
            let id = ChannelID::new(source.0, source.1);
            if let Some(level) = self.mixer.send_level_if_present(id, self.config.main_output) {
                return level;
            }
        }
        self.surface
            .returns
            .iter()
            .find(|r| r.id == lane as i32)
            .map(|r| r.fader)
            .unwrap_or(0.0)
    }

    pub fn return_main_mix_pan(&self, lane: ReturnLane) -> f32 {
        if let Some(source) = self.return_source(lane) {
            let id = ChannelID::new(source.0, source.1);
            if let Some(pan) = self.mixer.send_pan_if_present(id, self.config.main_output) {
                return balpan_to_pan_unit(pan);
            }
        }
        self.return_record_pan(lane)
    }

    pub fn return_record_pan(&self, lane: ReturnLane) -> f32 {
        self.surface
            .returns
            .iter()
            .find(|r| r.id == lane as i32)
            .map(|r| r.pan)
            .unwrap_or(0.5)
    }

    pub fn pull_return_faders_from_sends(&mut self) {
        for lane in ReturnLane::ALL {
            if !self.config.is_return_enabled(lane) {
                continue;
            }
            let Some(source) = self.return_source(lane) else {
                continue;
            };
            let id = ChannelID::new(source.0, source.1);
            if let Some(level) = self.mixer.send_level_if_present(id, self.config.main_output) {
                self.set_return_fader_state(lane as i32, level);
            }
        }
    }

    pub fn pull_strip_faders_from_sends(&mut self) {
        for i in 0..self.surface.strips.len() {
            if !self.config.is_strip_enabled(i) {
                continue;
            }
            let Some(dest) = self.config.listen_output(self.surface.strips[i].assign) else {
                continue;
            };
            for src in self.osc_sources(i) {
                if let Some(level) = self.mixer.send_level_if_present(src, dest) {
                    self.surface.strips[i].fader = level;
                    break;
                }
            }
        }
    }

    pub fn pull_strip_pans_from_sends(&mut self) {
        for i in 0..self.surface.strips.len() {
            if !self.config.is_strip_enabled(i) {
                continue;
            }
            let Some(dest) = self.config.listen_output(self.surface.strips[i].assign) else {
                continue;
            };
            for src in self.osc_sources(i) {
                if let Some(pan) = self.mixer.send_pan_if_present(src, dest) {
                    self.surface.strips[i].pan = balpan_to_pan_unit(pan);
                    break;
                }
            }
        }
    }

    pub fn pull_return_pans_from_sends(&mut self) {
        for lane in ReturnLane::ALL {
            if !self.config.is_return_enabled(lane) {
                continue;
            }
            let Some(source) = self.return_source(lane) else {
                continue;
            };
            let id = ChannelID::new(source.0, source.1);
            if let Some(pan) = self.mixer.send_pan_if_present(id, self.config.main_output) {
                self.upsert_return(lane as i32, |r| r.pan = balpan_to_pan_unit(pan));
            }
        }
    }

    pub fn pull_main_fader_from_total_mix(&mut self) {
        let id = ChannelID::new(MixerBus::Output, self.config.main_output);
        if let Some(ch) = self.mixer.channel(id) {
            if ch.from_total_mix {
                self.mixer.main_fader = ch.fader;
            }
        }
    }

    pub fn pull_send_levels_from_total_mix(
        &mut self,
        strip: Option<usize>,
        lane: Option<ReturnLane>,
        reset_unreadable: bool,
    ) {
        let strips: Vec<usize> = match strip {
            Some(i) => vec![i],
            None => (0..self.surface.strips.len()).collect(),
        };
        let lanes: Vec<ReturnLane> = match lane {
            Some(l) => vec![l],
            None => ALL_SEND_LANES.to_vec(),
        };
        for i in strips {
            if i >= self.surface.strips.len() || !self.config.is_strip_enabled(i) {
                continue;
            }
            for send_lane in &lanes {
                if !send_lane.is_send() {
                    continue;
                }
                let dest = self.config.send_destination(*send_lane);
                let index = match dest {
                    Some(SendDestination::Output(index)) => Some(index),
                    _ => None,
                };
                if index.is_none() {
                    if reset_unreadable {
                        self.surface.strips[i].set_aux(0.0, *send_lane);
                    }
                    continue;
                }
                let index = index.unwrap();
                let mut pulled = None;
                for src in self.osc_sources(i) {
                    if let Some(level) = self.mixer.send_level_if_present(src, index) {
                        pulled = Some(level);
                        break;
                    }
                }
                if let Some(pulled) = pulled {
                    if self.config.sends_post_fader {
                        if let Some(recovered) =
                            send_lin_from_post(pulled, self.surface.strips[i].fader)
                        {
                            self.surface.strips[i].set_aux(recovered, *send_lane);
                        }
                    } else {
                        self.surface.strips[i].set_aux(pulled, *send_lane);
                    }
                } else if reset_unreadable {
                    self.surface.strips[i].set_aux(0.0, *send_lane);
                }
            }
        }
    }

    pub fn apply_default_fader(&mut self, lane: ReturnLane, write_osc: bool) {
        let level = self.default_fader(lane);
        self.set_return_fader_state(lane as i32, level);
        if write_osc {
            self.write_return_fader(lane as i32, level, None, None);
        }
        if lane.is_send() {
            self.pull_send_levels_from_total_mix(None, Some(lane), true);
        }
    }

    fn default_fader(&self, lane: ReturnLane) -> f32 {
        if self.config.effect_ref(lane).is_some() {
            FADER_LIN_0DB
        } else {
            0.0
        }
    }

    /// Writes saved return/bus faders to TotalMix. Does not invent 0 dB.
    pub fn apply_launch_fader_defaults(&mut self, write_osc: bool) {
        if write_osc {
            for lane in ReturnLane::ALL {
                let level = self
                    .surface
                    .returns
                    .iter()
                    .find(|r| r.id == lane as i32)
                    .map(|r| r.fader)
                    .or_else(|| self.config.return_lane(lane as i32).map(|c| c.fader))
                    .unwrap_or(0.0);
                self.write_return_fader(lane as i32, level, None, None);
            }
        }
        self.pull_send_levels_from_total_mix(None, None, false);
        self.pull_return_faders_from_sends();
    }

    pub fn rewrite_aux_sends(&mut self, strip: Option<usize>) {
        let strips: Vec<usize> = match strip {
            Some(i) => vec![i],
            None => (0..self.surface.strips.len()).collect(),
        };
        for i in strips {
            if i < self.surface.strips.len() {
                self.write_all_aux(i);
            }
        }
    }

    pub fn apply_channel_enable(&mut self, strip: usize) {
        self.write_assign_sends(strip);
        self.write_all_aux(strip);
    }

    pub fn retarget(&mut self, slot: RoutingSlot, dest: SendDestination) {
        let previous = self.config.slot_destination(slot);
        if previous == dest {
            return;
        }
        self.clear_send_mix(previous);
        self.config.set_slot_destination(slot, dest);
        if let Some(lane) = ReturnLane::ALL.iter().find(|l| l.routing_slot() == slot) {
            if lane.is_send() {
                self.pull_send_levels_from_total_mix(None, Some(*lane), true);
            }
        }
        self.rewrite_all_sends();
    }

    pub fn remap_strip(&mut self, strip: usize, id: ChannelID, linked_stereo: bool) {
        if strip >= self.config.strips.len() {
            return;
        }
        let next = StripBinding {
            id: self.config.strips[strip].id,
            bus: id.bus,
            index: id.index,
            linked_stereo,
            enabled: self.config.strips[strip].enabled,
        };
        if self.config.strips[strip] == next {
            return;
        }
        self.silence_strip(strip);
        self.config.strips[strip] = next;
        self.write_assign_sends(strip);
        self.pull_send_levels_from_total_mix(Some(strip), None, true);
        let pan = self.surface.strips[strip].pan;
        self.apply_pan(strip, pan);
    }

    pub fn display_name(&self, id: ChannelID) -> String {
        let channel = self.hardware_name(id);
        if let Some(gear) = self.resolved_gear_name(id) {
            format!("{channel} - {gear}")
        } else {
            channel
        }
    }

    pub fn hardware_label(ch: &MixerChannel) -> String {
        let name =
            if ch.name.is_empty() { format!("ADAT {}", ch.id.index + 1) } else { ch.name.clone() };
        if name.contains('/') || name == "PH" {
            return name;
        }
        if ch.stereo {
            format!("{}/{}", name, ch.id.index + 2)
        } else {
            name
        }
    }

    pub fn update_hardware_effect(&mut self, id: i32, change: impl FnOnce(&mut HardwareEffect)) {
        let Some(i) = self.config.hardware_effects.iter().position(|h| h.id == id) else {
            return;
        };
        let previous = self.config.hardware_effects[i].clone();
        change(&mut self.config.hardware_effects[i]);
        let next = self.config.hardware_effects[i].clone();
        if previous.output != next.output {
            self.clear_hardware_send(previous.output);
        }
        self.config.sync_legacy_dests();
        if previous.output != next.output {
            for lane in ALL_SEND_LANES {
                if matches!(self.config.effect_ref(lane), Some(EffectRef::Hardware(hid)) if hid == id)
                {
                    self.pull_send_levels_from_total_mix(None, Some(lane), true);
                }
            }
        }
        self.rewrite_all_sends();
        self.apply_all_returns();
    }

    pub fn apply_all_returns(&mut self) {
        for lane in ReturnLane::ALL {
            self.apply_return_mix(lane as i32);
        }
    }

    pub fn apply_return_mix(&mut self, id: i32) {
        let ret = self.surface.returns.iter().find(|r| r.id == id);
        let cfg = self.config.return_lane(id);
        let fader = ret.map(|r| r.fader).or_else(|| cfg.map(|c| c.fader)).unwrap_or(0.0);
        let pan = ret.map(|r| r.pan).or_else(|| cfg.map(|c| c.pan)).unwrap_or(0.5);
        self.write_return_fader(id, fader, None, None);
        self.write_return_pan(id, pan);
    }

    pub fn clear_send_mix(&mut self, dest: SendDestination) {
        match dest {
            SendDestination::Output(index) => self.clear_hardware_send(index),
            SendDestination::Plugin(id) => {
                if let Some(out) = self.config.plugin(id).map(|p| p.send_output) {
                    self.clear_hardware_send(out);
                }
            }
        }
    }

    fn write_all_aux(&mut self, strip: usize) {
        for lane in ALL_SEND_LANES {
            let send = self.surface.strips[strip].aux(lane);
            let dest = self.config.send_destination(lane);
            let value = self.aux_write_level(strip, send);
            self.write_aux(strip, dest, value);
        }
    }

    fn aux_write_level(&self, strip: usize, send: f32) -> f32 {
        if !self.config.is_strip_enabled(strip) {
            return 0.0;
        }
        if !self.config.sends_post_fader {
            return send;
        }
        if self.strip_silenced_for_post_send(strip) {
            return 0.0;
        }
        post_fader_lin(send, self.surface.strips[strip].fader)
    }

    fn strip_silenced_for_post_send(&self, strip: usize) -> bool {
        let Some(id) = self.osc_sources(strip).first().copied() else {
            return true;
        };
        if self.mixer.channel(id).is_some_and(|c| c.mute) {
            return true;
        }
        if self.any_solo_active() && !self.mixer.channel(id).is_some_and(|c| c.solo) {
            return true;
        }
        false
    }

    fn silence_strip(&mut self, strip: usize) {
        self.zero_assign_dest(strip, self.config.main_output);
        if let Some(bus1) = self.config.listen_output(MixAssign::Bus1) {
            self.zero_assign_dest(strip, bus1);
        }
        if let Some(bus2) = self.config.listen_output(MixAssign::Bus2) {
            self.zero_assign_dest(strip, bus2);
        }
        for lane in ALL_SEND_LANES {
            let dest = self.config.send_destination(lane);
            self.write_aux(strip, dest, 0.0);
        }
    }

    fn write_aux(&mut self, strip: usize, dest: Option<SendDestination>, value: f32) {
        let Some(dest) = dest else {
            return;
        };
        let Some(index) = hardware_index(dest) else {
            return;
        };
        self.write_send(strip, index, value);
    }

    fn is_listen_output(&self, index: i32) -> bool {
        MixAssign::ALL.iter().any(|a| self.config.listen_output(*a) == Some(index))
    }

    fn clear_hardware_send(&mut self, index: i32) {
        if self.is_listen_output(index) {
            return;
        }
        for i in 0..self.surface.strips.len() {
            self.zero_assign_dest(i, index);
        }
    }

    fn zero_assign_dest(&mut self, strip: usize, dest: i32) {
        self.write_send(strip, dest, 0.0);
    }

    fn pan_destinations(&self, strip: usize) -> Vec<i32> {
        let mut dests = Vec::new();
        if let Some(dest) = self.config.listen_output(self.surface.strips[strip].assign) {
            dests.push(dest);
        }
        for lane in ALL_SEND_LANES {
            if let Some(SendDestination::Output(index)) = self.config.send_destination(lane) {
                dests.push(index);
            }
        }
        dests
    }

    fn set_return_fader_state(&mut self, id: i32, value: f32) {
        self.upsert_return(id, |r| r.fader = value);
        if let Some(row) = self.config.returns.iter_mut().find(|r| r.id == id) {
            row.fader = value;
        }
    }

    fn upsert_return(&mut self, id: i32, change: impl FnOnce(&mut ReturnStrip)) {
        if let Some(row) = self.surface.returns.iter_mut().find(|r| r.id == id) {
            change(row);
        } else {
            let mut row = ReturnStrip::new(id);
            change(&mut row);
            self.surface.returns.push(row);
        }
    }

    fn hardware_name(&self, id: ChannelID) -> String {
        if let Some(ch) = self.mixer.channel(id) {
            return Self::hardware_label(ch);
        }
        match id.bus {
            MixerBus::Input => format!("In {}", id.index + 1),
            MixerBus::Output => format!("Out {}", id.index + 1),
            MixerBus::Playback => format!("PB {}", id.index + 1),
        }
    }

    fn resolved_gear_name(&self, id: ChannelID) -> Option<String> {
        let own = self.config.gear_name(id);
        let own = own.trim();
        if !own.is_empty() {
            return Some(own.to_string());
        }
        let ch = self.mixer.channel(id)?;
        if !ch.stereo {
            return None;
        }
        let right = self.config.gear_name(ChannelID::new(id.bus, id.index + 1));
        let right = right.trim();
        if right.is_empty() {
            None
        } else {
            Some(right.to_string())
        }
    }

    fn write_return_fader(
        &mut self,
        id: i32,
        value: f32,
        dest: Option<i32>,
        source: Option<(MixerBus, i32)>,
    ) {
        let Some(lane) = ReturnLane::from_i32(id) else {
            return;
        };
        let Some(src) = source.or_else(|| self.return_source(lane)) else {
            return;
        };
        let dest = dest.unwrap_or(self.config.main_output);
        let channel = ChannelID::new(src.0, src.1);
        let written = if self.config.is_return_enabled(lane) { value } else { 0.0 };
        self.mixer.set_send(channel, dest, written);
        self.osc.send_float(mix_fader_lin(src.0.osc(), src.1, dest), written);
        self.osc.send_float(mix_fader(src.0.osc(), src.1, dest), fader_db(written));
    }

    fn write_return_pan(&mut self, id: i32, value: f32) {
        let Some(lane) = ReturnLane::from_i32(id) else {
            return;
        };
        let Some(src) = self.return_source(lane) else {
            return;
        };
        self.osc.send_float(
            mix_balpan(src.0.osc(), src.1, self.config.main_output),
            pan_unit_to_balpan(value),
        );
    }

    pub fn persist(&mut self) {
        #[cfg(not(test))]
        {
            self.config.save();
        }
    }

    /// MixLink `PluginHost.syncSends` gain for one plugin slot / strip.
    #[must_use]
    pub fn plugin_send_gain(&self, slot: i32, strip: usize) -> f32 {
        let Some(row) = self.surface.strips.get(strip) else {
            return 0.0;
        };
        if !self.config.is_strip_enabled(strip) {
            return 0.0;
        }
        let mut gain = 0.0;
        for lane in crate::types::ALL_SEND_LANES {
            if matches!(self.config.send_destination(lane), Some(SendDestination::Plugin(id)) if id == slot)
            {
                let mut amount = row.aux(lane);
                if self.config.sends_post_fader {
                    amount *= fader_lin_to_amp(row.fader);
                }
                gain += amount;
            }
        }
        if row.assign == MixAssign::Bus1
            && matches!(
                self.config.send_destination(ReturnLane::Bus1),
                Some(SendDestination::Plugin(id)) if id == slot
            )
        {
            gain += row.fader;
        }
        if row.assign == MixAssign::Bus2
            && matches!(
                self.config.send_destination(ReturnLane::Bus2),
                Some(SendDestination::Plugin(id)) if id == slot
            )
        {
            gain += row.fader;
        }
        if gain > 0.0 && self.config.sends_post_fader && self.strip_silenced_for_post_send(strip) {
            return 0.0;
        }
        gain
    }

    pub fn selected_name(&self, id: ChannelID) -> String {
        self.resolved_gear_name(id).unwrap_or_else(|| self.hardware_name(id))
    }

    pub fn effect_display_name(&self, ref_: EffectRef) -> String {
        match ref_ {
            EffectRef::Hardware(id) => self
                .config
                .hardware_effect(id)
                .map(|h| h.title())
                .unwrap_or_else(|| "Hardware".into()),
            EffectRef::Plugin(id) => {
                self.config.plugin(id).map(|p| p.title()).unwrap_or_else(|| "Plugin".into())
            }
        }
    }

    pub fn return_display_name(&self, lane: ReturnLane) -> String {
        match self.config.effect_ref(lane) {
            Some(r) => self.effect_display_name(r),
            None => "No effect".into(),
        }
    }

    pub fn output_name(&self, index: i32) -> String {
        self.display_name(ChannelID::new(MixerBus::Output, index))
    }

    pub fn destination_name(&self, dest: SendDestination) -> String {
        match dest {
            SendDestination::Output(i) => self.output_name(i),
            SendDestination::Plugin(id) => {
                self.config.plugin(id).map(|p| p.title()).unwrap_or_else(|| "Plugin".into())
            }
        }
    }

    pub fn set_strip_source(&mut self, strip: usize, id: ChannelID) {
        let linked = self.mixer.channel(id).map(|c| c.stereo).unwrap_or_else(|| {
            self.config.strips.get(strip).map(|s| s.linked_stereo).unwrap_or(true)
        });
        self.remap_strip(strip, id, linked);
        self.persist();
    }

    pub fn set_gear_name(&mut self, id: ChannelID, name: &str) {
        let trimmed = name.trim();
        let key = SessionConfig::gear_key(id);
        if trimmed.is_empty() {
            self.config.gear_names.remove(&key);
        } else {
            self.config.gear_names.insert(key, trimmed.to_string());
        }
        if self.mixer.channel(id).is_some_and(|c| c.stereo) {
            let right = ChannelID::new(id.bus, id.index + 1);
            let rkey = SessionConfig::gear_key(right);
            if trimmed.is_empty() {
                self.config.gear_names.remove(&rkey);
            } else {
                self.config.gear_names.insert(rkey, trimmed.to_string());
            }
        }
        self.persist();
    }

    pub fn set_return_effect(&mut self, lane: ReturnLane, ref_: Option<EffectRef>) {
        let previous_source = self.return_source(lane);
        if let Some(prev) = self.config.send_destination(lane) {
            self.clear_send_mix(prev);
        }
        self.config.set_return_effect(lane, ref_);
        if lane.is_send() {
            self.pull_send_levels_from_total_mix(None, Some(lane), true);
        }
        self.rewrite_all_sends();
        self.apply_default_fader(lane, true);
        if let Some(src) = previous_source {
            self.silence_return(lane as i32, Some(src), None);
        }
        self.apply_return_mix(lane as i32);
        self.persist();
    }

    pub fn add_effect_return(&mut self) {
        if self.config.effect_return_count >= crate::types::MAX_SEND_COUNT {
            return;
        }
        let next = ALL_SEND_LANES[self.config.effect_return_count as usize];
        self.config.effect_return_count += 1;
        self.config.ensure_return_lane(next);
        self.apply_default_fader(next, true);
        self.surface.load_returns(&self.config);
        self.persist();
    }

    pub fn remove_last_effect_return(&mut self) {
        if self.config.effect_return_count <= 2 {
            return;
        }
        let lane = ALL_SEND_LANES[self.config.effect_return_count as usize - 1];
        self.silence_return(lane as i32, None, None);
        if let Some(dest) = self.config.send_destination(lane) {
            self.clear_send_mix(dest);
        }
        for strip in &mut self.surface.strips {
            strip.set_aux(0.0, lane);
        }
        self.config.set_return_effect(lane, None);
        self.config.effect_return_count -= 1;
        self.rewrite_all_sends();
        self.surface.load_returns(&self.config);
        self.persist();
    }

    pub fn set_pan_knobs_control_send_c(&mut self, on: bool) {
        self.config.pan_knobs_control_send_c = on;
        self.persist();
    }

    pub fn set_sends_post_fader(&mut self, on: bool) {
        self.config.sends_post_fader = on;
        self.rewrite_aux_sends(None);
        self.persist();
    }

    pub fn set_hardware_strips(&mut self, on: bool) {
        self.config.hardware_strips = on;
        self.persist();
    }

    pub fn add_hardware_effect(&mut self) {
        let mut used_out: std::collections::HashSet<i32> =
            self.config.hardware_effects.iter().map(|h| h.output).collect();
        used_out.insert(self.config.main_output);
        let used_in: std::collections::HashSet<i32> =
            self.config.hardware_effects.iter().map(|h| h.input).collect();
        let output = next_free_pair(&used_out, 0);
        let input = next_free_pair(&used_in, 0);
        self.config.insert_hardware_effect(output, input, "");
        self.persist();
    }

    pub fn remove_hardware_effect(&mut self, id: i32) {
        for lane in ReturnLane::ALL {
            if matches!(self.config.effect_ref(lane), Some(EffectRef::Hardware(hid)) if hid == id) {
                self.silence_return(lane as i32, None, None);
                if let Some(dest) = self.config.send_destination(lane) {
                    self.clear_send_mix(dest);
                }
            }
        }
        self.config.remove_hardware_effect(id);
        self.rewrite_all_sends();
        self.apply_all_returns();
        self.persist();
    }

    pub fn set_hardware_effect_name(&mut self, id: i32, name: &str) {
        self.update_hardware_effect(id, |h| h.name = name.trim().to_string());
        self.persist();
    }

    pub fn set_hardware_effect_io(&mut self, id: i32, output: Option<i32>, input: Option<i32>) {
        if input.is_some() {
            for lane in ReturnLane::ALL {
                if matches!(self.config.effect_ref(lane), Some(EffectRef::Hardware(hid)) if hid == id)
                {
                    self.silence_return(lane as i32, None, None);
                }
            }
        }
        self.update_hardware_effect(id, |h| {
            if let Some(o) = output {
                h.output = o;
            }
            if let Some(i) = input {
                h.input = i;
            }
        });
        self.persist();
    }

    pub fn add_plugin(&mut self) {
        let Some(id) = self.config.next_free_plugin_id() else {
            return;
        };
        let used_out: std::collections::HashSet<i32> =
            self.config.plugins.iter().map(|p| p.send_output).collect();
        let used_ret: std::collections::HashSet<i32> =
            self.config.plugins.iter().map(|p| p.return_channel).collect();
        self.config.plugins.push(crate::types::PluginSlot {
            id,
            name: String::new(),
            bundle_path: None,
            class_uid: None,
            send_output: next_free_pair(&used_out, 14),
            input_channel: 14,
            return_channel: next_free_pair(&used_ret, 2),
            return_dest: 0,
            bypassed: false,
            return_fader: 0.75,
            return_pan: 0.5,
        });
        self.persist();
    }

    pub fn remove_plugin(&mut self, id: i32) {
        for lane in ReturnLane::ALL {
            if matches!(self.config.effect_ref(lane), Some(EffectRef::Plugin(pid)) if pid == id) {
                self.silence_return(lane as i32, None, None);
            }
        }
        self.config.remove_plugin_slot(id);
        self.rewrite_all_sends();
        self.apply_all_returns();
        self.persist();
    }

    pub fn set_plugin_bundle(&mut self, id: i32, path: Option<String>, name: Option<String>) {
        if let Some(p) = self.config.plugins.iter_mut().find(|p| p.id == id) {
            p.bundle_path = path;
            p.class_uid = None;
            if let Some(n) = name {
                p.name = n;
            }
        }
        self.apply_all_returns();
        self.persist();
    }

    pub fn set_plugin_bypass(&mut self, id: i32, bypassed: bool) {
        if let Some(p) = self.config.plugins.iter_mut().find(|p| p.id == id) {
            p.bypassed = bypassed;
        }
        self.persist();
    }

    pub fn set_plugin_name(&mut self, id: i32, name: &str) {
        if let Some(p) = self.config.plugins.iter_mut().find(|p| p.id == id) {
            p.name = name.trim().to_string();
        }
        self.persist();
    }

    pub fn set_plugin_playback(&mut self, id: i32, pair: i32) {
        for lane in ReturnLane::ALL {
            if matches!(self.config.effect_ref(lane), Some(EffectRef::Plugin(pid)) if pid == id) {
                self.silence_return(lane as i32, None, None);
            }
        }
        if let Some(p) = self.config.plugins.iter_mut().find(|p| p.id == id) {
            p.return_channel = pair;
        }
        self.apply_all_returns();
        self.persist();
    }

    pub fn set_main_output(&mut self, index: i32) {
        let previous = self.config.main_output;
        if previous == index {
            return;
        }
        self.retarget(RoutingSlot::Mix, SendDestination::Output(index));
        self.mixer.monitored_output = index;
        self.mixer.main_fader = 0.0;
        self.persist();
    }

    pub fn set_audio_device(&mut self, contains: &str) {
        self.config.audio_device_contains = contains.to_string();
        self.persist();
    }

    pub fn set_audio_buffer_frames(&mut self, frames: i32) {
        if frames <= 0 {
            return;
        }
        self.config.audio_buffer_frames = Some(frames);
        self.persist();
    }

    pub fn toggle_return_mute(&mut self, lane: ReturnLane) {
        let on = !self.return_muted(lane);
        self.apply_return_mute(lane, on);
    }

    pub fn toggle_return_solo(&mut self, lane: ReturnLane) {
        let on = !self.return_soloed(lane);
        self.apply_return_solo(lane, on);
    }

    pub fn toggle_return_enabled(&mut self, lane: ReturnLane) {
        let on = !self.config.is_return_enabled(lane);
        self.config.set_return_enabled(lane, on);
        self.apply_return_mix(lane as i32);
        self.persist();
    }

    pub fn silence_return(&mut self, id: i32, source: Option<(MixerBus, i32)>, dest: Option<i32>) {
        self.write_return_fader(id, 0.0, dest, source);
    }

    /// Writes both `/faderlin` and `/fader` (dB, −300 when off).
    fn write_send(&mut self, strip: usize, dest: i32, value: f32) {
        for id in self.osc_sources(strip) {
            self.mixer.set_send(id, dest, value);
            self.osc.send_float(mix_fader_lin(id.bus.osc(), id.index, dest), value);
            self.osc.send_float(mix_fader(id.bus.osc(), id.index, dest), fader_db(value));
        }
    }
}

fn hardware_index(dest: SendDestination) -> Option<i32> {
    match dest {
        SendDestination::Output(index) => Some(index),
        SendDestination::Plugin(_) => None,
    }
}

fn next_free_pair(used: &std::collections::HashSet<i32>, start: i32) -> i32 {
    let mut i = start;
    while used.contains(&i) {
        i += 2;
    }
    i
}
