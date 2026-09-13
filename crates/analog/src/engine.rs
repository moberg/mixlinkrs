//! Control-surface brain. UI and MIDI call these methods; they never send OSC.

use std::thread;
use std::time::{Duration, Instant};

use osc::{
    balpan_to_pan_unit, fader_db, fader_lin_to_amp, mix_balpan, mix_fader, mix_fader_lin,
    pan_unit_to_balpan, post_fader_lin, send_lin_from_post, strip_mute, strip_solo, OscSession,
    OscValue, FADER_LIN_0DB, LIN_EPS,
};

use crate::config::SessionConfig;
use crate::mixer::{apply_inbound, MixerState};
use crate::surface::SurfaceState;
use crate::types::{
    ChainRef, ChannelID, HardwarePreset, MixAssign, MixEvent, MixNode, MixerBus, MixerChannel,
    ReturnLane, ReturnStrip, RoutingSlot, SendDestination, StripBinding, ALL_SEND_LANES,
};

/// Wait this long after the last dump mix node before writing missing volumes.
const MIX_BACKFILL_SETTLE: Duration = Duration::from_millis(300);
/// Short cosine fade so mute/unmute is not a hard cut in TotalMix.
const MUTE_FADE: Duration = Duration::from_millis(16);
const MUTE_FADE_STEPS: u32 = 8;

#[derive(Clone, Copy)]
enum MuteTarget {
    Strip(usize),
    Return(ReturnLane),
}

struct MuteRamp {
    target: MuteTarget,
    started: Instant,
    from: f32,
    to: f32,
}

impl MuteRamp {
    fn progress_at(&self, now: Instant) -> f32 {
        let elapsed = now.saturating_duration_since(self.started).as_secs_f32();
        (elapsed / MUTE_FADE.as_secs_f32()).clamp(0.0, 1.0)
    }

    fn weight_at(&self, now: Instant) -> f32 {
        let t = 0.5 * (1.0 - (self.progress_at(now) * std::f32::consts::PI).cos());
        self.from + (self.to - self.from) * t
    }

    fn weight(&self) -> f32 {
        self.weight_at(Instant::now())
    }
}

pub struct AnalogEngine {
    pub mixer: MixerState,
    pub surface: SurfaceState,
    pub config: SessionConfig,
    pub osc: OscSession,
    mix_backfill_at: Option<Instant>,
    mute_ramps: Vec<MuteRamp>,
}

impl AnalogEngine {
    pub fn new(
        mixer: MixerState,
        surface: SurfaceState,
        config: SessionConfig,
        osc: OscSession,
    ) -> Self {
        let mut engine = Self {
            mixer,
            surface,
            config,
            osc,
            mix_backfill_at: None,
            mute_ramps: Vec::new(),
        };
        engine.restore_software_levels();
        engine
    }

    /// Plugin send knobs and plugin-return faders live in MixLink, not TotalMix.
    /// Restore them before the dump so hardware pull cannot start from zeros.
    pub fn restore_software_levels(&mut self) {
        for i in 0..self.surface.strips.len() {
            for lane in ALL_SEND_LANES {
                if !matches!(self.config.send_destination(lane), Some(SendDestination::Plugin(_))) {
                    continue;
                }
                if let Some(value) = self.config.plugin_send(i, lane) {
                    self.surface.strips[i].set_aux(value, lane);
                }
            }
        }
        for lane in ReturnLane::ALL {
            if !self.config.is_plugin_return(lane) {
                continue;
            }
            let Some(fader) = self.config.return_lane(lane as i32).map(|r| r.fader) else {
                continue;
            };
            self.upsert_return(lane as i32, |r| r.fader = fader);
        }
    }

    /// Drain inbound OSC. Sets `mixer.connected` from the session flag.
    /// TotalMix is the source of truth: dump on connect, then copy mix nodes into the UI.
    /// After the dump goes quiet, push any on-screen volume TotalMix never reported.
    pub fn poll_osc(&mut self) {
        let was_connected = self.mixer.connected;
        if self.osc.is_connected() {
            self.mixer.connected = true;
        }
        let just_connected = self.mixer.connected && !was_connected;
        self.tick_mute_fades();
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
            self.arm_mix_backfill();
        }
        if just_connected || saw_mix {
            self.sync_surface_from_total_mix();
            self.isolate_plugin_playback_from_hardware();
        }
        if saw_mix {
            self.arm_mix_backfill();
        }
        if self.mix_backfill_at.is_some_and(|at| Instant::now() >= at) {
            self.mix_backfill_at = None;
            self.push_unreported_volumes();
        }
    }

    /// Ask TotalMix for a full dump, then push any volumes it never sends back.
    pub fn request_total_mix_dump(&mut self) {
        self.mixer.clear_reported_sends();
        self.osc.send_dump_requests();
        self.arm_mix_backfill();
    }

    fn arm_mix_backfill(&mut self) {
        self.mix_backfill_at = Some(Instant::now() + MIX_BACKFILL_SETTLE);
    }

    /// Push MixLink values TotalMix never mentioned. TotalMix omits silent
    /// nodes — never invent 0, or a restart would wipe the hardware mix.
    pub fn push_unreported_volumes(&mut self) {
        for i in 0..self.surface.strips.len() {
            if !self.config.is_strip_enabled(i)
                || !self.config.strips.get(i).is_some_and(|s| s.has_input)
            {
                continue;
            }
            self.push_unreported_strip_assign(i);
            for lane in ALL_SEND_LANES {
                let dest = self.config.send_destination(lane);
                let Some(SendDestination::Output(index)) = dest else {
                    continue;
                };
                if !self.strip_send_unreported(i, index) {
                    continue;
                }
                let value = self.aux_write_level(i, lane, self.surface.strips[i].aux(lane));
                if value > LIN_EPS {
                    self.write_aux(i, dest, value);
                }
            }
        }
        for lane in ReturnLane::ALL {
            if !self.config.is_return_enabled(lane) {
                continue;
            }
            let Some(source) = self.return_source(lane) else {
                continue;
            };
            let id = ChannelID::new(source.0, source.1);
            if self.mixer.send_reported(id, self.config.main_output) {
                continue;
            }
            let level = self
                .surface
                .returns
                .iter()
                .find(|r| r.id == lane as i32)
                .map(|r| r.fader)
                .unwrap_or(0.0);
            if level > LIN_EPS {
                self.apply_return_mix(lane as i32);
            }
        }
    }

    fn push_unreported_strip_assign(&mut self, strip: usize) {
        let Some(s) = self.surface.strips.get(strip) else {
            return;
        };
        let assign = s.assign;
        let Some(dest) = self.config.listen_output(assign) else {
            return;
        };
        if self.strip_send_unreported(strip, dest) && s.fader > LIN_EPS {
            // Write the assigned dest only. write_assign_sends zeros Bus1/Bus2.
            self.write_send(strip, dest, s.fader);
        }
    }

    fn strip_send_unreported(&self, strip: usize, dest: i32) -> bool {
        self.osc_sources(strip).iter().all(|src| !self.mixer.send_reported(*src, dest))
    }

    pub fn source_ids(&self, strip: usize) -> Vec<ChannelID> {
        let Some(b) = self.config.strips.get(strip) else {
            return Vec::new();
        };
        if !b.has_input {
            return Vec::new();
        }
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
        if matches!(self.config.send_destination(lane), Some(SendDestination::Plugin(_))) {
            self.config.set_plugin_send(strip, lane, value);
            self.persist();
        }
        let dest = self.config.send_destination(lane);
        let written = self.aux_write_level(strip, lane, value);
        self.write_aux(strip, dest, written);
    }

    pub fn apply_return_fader(&mut self, id: i32, value: f32) {
        self.set_return_fader_state(id, value);
        self.write_return_fader(id, value, None, None);
        if ReturnLane::from_i32(id).is_some_and(|lane| self.config.is_plugin_return(lane)) {
            self.persist();
        }
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
        let was_solo = self.return_soloed(lane);
        if !on {
            self.write_return_mix_scaled(lane, 0.0);
        }
        self.upsert_return(lane as i32, |r| {
            r.mute = on;
            if on {
                r.solo = false;
            }
        });
        if let Some(id) = self.config.return_source_id(lane) {
            self.mixer.update_channel(id, |c| {
                c.mute = on;
                if on {
                    c.solo = false;
                }
            });
            if !on {
                self.osc.send_float(strip_mute(id.bus.osc(), id.index), 0.0);
            }
            if on {
                self.osc.send_float(strip_solo(id.bus.osc(), id.index), 0.0);
            }
        }
        if on && was_solo {
            self.rewrite_mix_for_solo();
        }
        self.begin_mute_fade(MuteTarget::Return(lane), on);
    }

    pub fn apply_return_solo(&mut self, lane: ReturnLane, on: bool) {
        self.upsert_return(lane as i32, |r| {
            r.solo = on;
            if on {
                r.mute = false;
            }
        });
        if let Some(id) = self.config.return_source_id(lane) {
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
        self.rewrite_mix_for_solo();
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
        let was_solo = self.strip_soloed(strip);
        if !on {
            // Still muted in TotalMix: drop the send first so unmute is silent.
            self.write_strip_mix_scaled(strip, 0.0);
        }
        for id in self.osc_sources(strip) {
            self.mixer.update_channel(id, |c| {
                c.mute = on;
                if on {
                    c.solo = false;
                }
            });
            if !on {
                self.osc.send_float(strip_mute(id.bus.osc(), id.index), 0.0);
            }
            if on {
                self.osc.send_float(strip_solo(id.bus.osc(), id.index), 0.0);
            }
        }
        if on && was_solo {
            self.rewrite_mix_for_solo();
        }
        self.begin_mute_fade(MuteTarget::Strip(strip), on);
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
        self.rewrite_mix_for_solo();
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
        self.any_strip_solo_active() || self.any_return_solo_active()
    }

    fn any_strip_solo_active(&self) -> bool {
        (0..self.surface.strips.len()).any(|i| self.strip_soloed(i))
    }

    fn any_return_solo_active(&self) -> bool {
        ReturnLane::ALL.iter().any(|lane| self.return_soloed(*lane))
    }

    pub fn toggle_solo(&mut self, strip: usize) {
        let Some(id) = self.osc_sources(strip).first().copied() else {
            return;
        };
        let on = !self.mixer.channel(id).is_some_and(|c| c.solo);
        self.apply_solo(strip, on);
    }

    fn rewrite_mix_for_solo(&mut self) {
        for i in 0..self.surface.strips.len() {
            if self.mute_fade_covers(MuteTarget::Strip(i)) {
                continue;
            }
            self.write_assign_sends(i);
        }
        if self.config.sends_post_fader {
            self.rewrite_aux_sends(None);
        }
    }

    fn mute_fade_covers(&self, target: MuteTarget) -> bool {
        self.mute_ramps.iter().any(|r| match (r.target, target) {
            (MuteTarget::Strip(a), MuteTarget::Strip(b)) => a == b,
            (MuteTarget::Return(a), MuteTarget::Return(b)) => a == b,
            _ => false,
        })
    }

    fn mute_fade_weight(&self, target: MuteTarget) -> f32 {
        self.mute_ramps
            .iter()
            .find(|r| match (r.target, target) {
                (MuteTarget::Strip(a), MuteTarget::Strip(b)) => a == b,
                (MuteTarget::Return(a), MuteTarget::Return(b)) => a == b,
                _ => false,
            })
            .map(MuteRamp::weight)
            .unwrap_or(1.0)
    }

    fn begin_mute_fade(&mut self, target: MuteTarget, fading_out: bool) {
        let from = if self.mute_fade_covers(target) {
            self.mute_fade_weight(target)
        } else if fading_out {
            1.0
        } else {
            0.0
        };
        self.mute_ramps.retain(|r| match (r.target, target) {
            (MuteTarget::Strip(a), MuteTarget::Strip(b)) => a != b,
            (MuteTarget::Return(a), MuteTarget::Return(b)) => a != b,
            _ => true,
        });
        let started = if cfg!(test) { Instant::now() - MUTE_FADE / 4 } else { Instant::now() };
        self.mute_ramps.push(MuteRamp {
            target,
            started,
            from,
            to: if fading_out { 0.0 } else { 1.0 },
        });
        if cfg!(test) {
            self.tick_mute_fades();
        } else {
            self.run_mute_fade_steps();
        }
    }

    fn run_mute_fade_steps(&mut self) {
        let slice = MUTE_FADE / MUTE_FADE_STEPS;
        for step in 1..=MUTE_FADE_STEPS {
            let now = Instant::now();
            for ramp in &mut self.mute_ramps {
                ramp.started = now - slice * step;
            }
            self.tick_mute_fades();
            if step < MUTE_FADE_STEPS && !self.mute_ramps.is_empty() {
                thread::sleep(slice);
            }
        }
    }

    fn tick_mute_fades(&mut self) {
        if self.mute_ramps.is_empty() {
            return;
        }
        let now = Instant::now();
        let steps: Vec<(MuteTarget, f32, bool)> = self
            .mute_ramps
            .iter()
            .map(|ramp| {
                (
                    ramp.target,
                    ramp.weight_at(now),
                    now.saturating_duration_since(ramp.started) >= MUTE_FADE,
                )
            })
            .collect();
        let mut done = Vec::new();
        for (i, (target, gain, finished)) in steps.into_iter().enumerate() {
            match target {
                MuteTarget::Strip(strip) => self.write_strip_mix_scaled(strip, gain),
                MuteTarget::Return(lane) => self.write_return_mix_scaled(lane, gain),
            }
            if finished {
                done.push(i);
            }
        }
        for i in done.into_iter().rev() {
            let ramp = self.mute_ramps.remove(i);
            if ramp.to <= 0.0 {
                self.send_mute_osc(ramp.target, true);
                // Put the real send back. TotalMix mute holds silence; the
                // dump must still see the fader or a restart shows −∞.
                match ramp.target {
                    MuteTarget::Strip(strip) => self.write_strip_mix_scaled(strip, 1.0),
                    MuteTarget::Return(lane) => self.write_return_mix_scaled(lane, 1.0),
                }
            }
        }
    }

    fn send_mute_osc(&mut self, target: MuteTarget, on: bool) {
        match target {
            MuteTarget::Strip(strip) => {
                for id in self.osc_sources(strip) {
                    self.osc.send_float(strip_mute(id.bus.osc(), id.index), if on { 1.0 } else { 0.0 });
                }
            }
            MuteTarget::Return(lane) => {
                if let Some(id) = self.config.return_source_id(lane) {
                    self.osc.send_float(strip_mute(id.bus.osc(), id.index), if on { 1.0 } else { 0.0 });
                }
            }
        }
    }

    fn write_strip_mix_scaled(&mut self, strip: usize, gain: f32) {
        let Some(s) = self.surface.strips.get(strip) else {
            return;
        };
        let assign = s.assign;
        let level = if self.config.is_strip_enabled(strip) { s.fader * gain } else { 0.0 };
        if let Some(dest) = self.config.listen_output(assign) {
            self.write_send(strip, dest, level);
        }
        if !self.config.sends_post_fader {
            return;
        }
        for lane in ALL_SEND_LANES {
            let dest = self.config.send_destination(lane);
            let send = self.surface.strips[strip].aux(lane);
            self.write_aux(strip, dest, post_fader_lin(send, self.surface.strips[strip].fader) * gain);
        }
    }

    fn write_return_mix_scaled(&mut self, lane: ReturnLane, gain: f32) {
        let level = self
            .surface
            .returns
            .iter()
            .find(|r| r.id == lane as i32)
            .map(|r| r.fader)
            .unwrap_or(0.0)
            * gain;
        self.write_return_fader(lane as i32, level, None, None);
    }

    #[cfg(test)]
    pub fn finish_mute_fades(&mut self) {
        let past = Instant::now() - MUTE_FADE;
        for ramp in &mut self.mute_ramps {
            ramp.started = past;
        }
        self.tick_mute_fades();
    }

    pub fn write_assign_sends(&mut self, strip: usize) {
        let Some(s) = self.surface.strips.get(strip) else {
            return;
        };
        let assign = s.assign;
        let level = if self.config.is_strip_enabled(strip) && !self.assign_silenced_by_solo(strip, assign)
        {
            s.fader
        } else {
            0.0
        };
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
            if !self.config.strips[i].enabled || !self.config.strips[i].has_input {
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
                if node.dest == assigned && !self.mute_fade_covers(MuteTarget::Strip(i)) {
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
                if self.config.is_return_enabled(lane) && !self.mute_fade_covers(MuteTarget::Return(lane))
                {
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
        self.infer_strip_assigns_from_sends();
        self.pull_strip_faders_from_sends();
        self.pull_strip_pans_from_sends();
        self.pull_send_levels_from_total_mix(None, None, false);
        self.pull_return_faders_from_sends();
        self.pull_return_pans_from_sends();
        self.pull_main_fader_from_total_mix();
    }

    /// Assign is routing, stored in TotalMix as which listen dest has the fader.
    /// Session does not persist it — recover from the dump before pulling levels.
    fn infer_strip_assigns_from_sends(&mut self) {
        for i in 0..self.surface.strips.len() {
            if !self.config.is_strip_enabled(i) {
                continue;
            }
            if let Some(assign) = self.inferred_assign(i) {
                self.surface.strips[i].assign = assign;
            }
        }
    }

    fn inferred_assign(&self, strip: usize) -> Option<MixAssign> {
        let mut best: Option<(MixAssign, f32)> = None;
        for assign in MixAssign::ALL {
            let Some(dest) = self.config.listen_output(assign) else {
                continue;
            };
            let mut level = None;
            for src in self.osc_sources(strip) {
                if let Some(v) = self.mixer.send_level_if_present(src, dest) {
                    level = Some(v);
                    break;
                }
            }
            let Some(level) = level else {
                continue;
            };
            if level <= LIN_EPS {
                continue;
            }
            if best.is_none_or(|(_, prev)| level > prev) {
                best = Some((assign, level));
            }
        }
        best.map(|(assign, _)| assign)
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
        self.surface.returns.iter().find(|r| r.id == lane as i32).map(|r| r.fader).unwrap_or(0.0)
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
        self.surface.returns.iter().find(|r| r.id == lane as i32).map(|r| r.pan).unwrap_or(0.5)
    }

    pub fn pull_return_faders_from_sends(&mut self) {
        for lane in ReturnLane::ALL {
            if self.mute_fade_covers(MuteTarget::Return(lane)) {
                continue;
            }
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
            if self.mute_fade_covers(MuteTarget::Strip(i)) {
                continue;
            }
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
                    // Plugin dest has no TotalMix mix node. Never wipe MixLink engine gains.
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
        if self.config.chain_ref(lane).is_some() {
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
            if i < self.surface.strips.len() && !self.mute_fade_covers(MuteTarget::Strip(i)) {
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
            has_input: true,
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

    pub fn update_hardware_preset(&mut self, id: uuid::Uuid, change: impl FnOnce(&mut HardwarePreset)) {
        let Some(i) = self.config.hardware_presets.iter().position(|h| h.id == id) else {
            return;
        };
        let previous = self.config.hardware_presets[i].clone();
        change(&mut self.config.hardware_presets[i]);
        let next = self.config.hardware_presets[i].clone();
        if previous.output != next.output {
            self.clear_hardware_send(previous.output);
        }
        self.config.sync_legacy_dests();
        if previous.output != next.output {
            for lane in ALL_SEND_LANES {
                if self.lane_uses_preset(lane, id) {
                    self.pull_send_levels_from_total_mix(None, Some(lane), true);
                }
            }
        }
        self.rewrite_all_sends();
        self.apply_all_returns();
    }

    fn lane_uses_preset(&self, lane: ReturnLane, preset: uuid::Uuid) -> bool {
        let Some(ChainRef { kind: crate::types::ChainKind::Hardware, id }) =
            self.config.chain_ref(lane)
        else {
            return false;
        };
        self.config.hardware_chain(id).is_some_and(|c| c.stages.contains(&preset))
    }

    pub fn apply_all_returns(&mut self) {
        for lane in ReturnLane::ALL {
            self.apply_return_mix(lane as i32);
        }
        self.isolate_plugin_playback_from_hardware();
    }

    /// Plugin wet is written to a software-playback pair. TotalMix's default
    /// 1:1 would also send that pair to the matching hardware output — Heat
    /// sits on 5/6 in this session, which is also Galaxy's playback pair.
    pub fn isolate_plugin_playback_from_hardware(&mut self) {
        let main = self.config.main_output;
        let outs: Vec<i32> = self
            .config
            .hardware_io_pairs()
            .into_iter()
            .filter(|out| *out != main)
            .collect();
        let pairs: Vec<i32> = self
            .config
            .plugin_chains
            .iter()
            .map(|c| c.return_channel)
            .filter(|pb| *pb >= 0)
            .collect();
        for pb in pairs {
            let src = ChannelID::new(MixerBus::Playback, pb);
            for &out in &outs {
                if self.mixer.send_level_if_present(src, out).is_some_and(|v| v > LIN_EPS) {
                    self.write_playback_send(pb, out, 0.0);
                }
            }
        }
    }

    fn write_playback_send(&mut self, playback: i32, dest: i32, value: f32) {
        let channel = ChannelID::new(MixerBus::Playback, playback);
        self.mixer.set_send(channel, dest, value);
        self.osc.send_float(mix_fader_lin(MixerBus::Playback.osc(), playback, dest), value);
        self.osc.send_float(mix_fader(MixerBus::Playback.osc(), playback, dest), fader_db(value));
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
            SendDestination::Plugin(_) => {}
        }
    }

    fn write_all_aux(&mut self, strip: usize) {
        for lane in ALL_SEND_LANES {
            let send = self.surface.strips[strip].aux(lane);
            let dest = self.config.send_destination(lane);
            let value = self.aux_write_level(strip, lane, send);
            self.write_aux(strip, dest, value);
        }
    }

    fn aux_write_level(&self, strip: usize, lane: ReturnLane, send: f32) -> f32 {
        if !self.config.is_strip_enabled(strip) {
            return 0.0;
        }
        if !self.config.sends_post_fader {
            return send;
        }
        if self.aux_silenced_by_solo(strip, lane) {
            return 0.0;
        }
        post_fader_lin(send, self.surface.strips[strip].fader)
    }

    /// Dry mix bus (Main / other buses). Mute stays a TotalMix `/mute` so we
    /// do not write 0 into the fader.
    fn strip_silenced_on_mix(&self, strip: usize) -> bool {
        let Some(id) = self.osc_sources(strip).first().copied() else {
            return true;
        };
        self.any_solo_active() && !self.mixer.channel(id).is_some_and(|c| c.solo)
    }

    /// Keep feeding a soloed return (or a soloed strip's post-fader sends).
    /// Starving the send is why return solo used to kill hardware and plugins.
    fn aux_silenced_by_solo(&self, strip: usize, lane: ReturnLane) -> bool {
        if self.any_return_solo_active() {
            return !self.return_soloed(lane);
        }
        if self.any_strip_solo_active() {
            return !self.strip_soloed(strip);
        }
        false
    }

    fn assign_silenced_by_solo(&self, strip: usize, assign: MixAssign) -> bool {
        let lane = match assign {
            MixAssign::Bus1 => Some(ReturnLane::Bus1),
            MixAssign::Bus2 => Some(ReturnLane::Bus2),
            MixAssign::Main => None,
        };
        if let Some(lane) = lane {
            if self.return_soloed(lane) {
                return false;
            }
        }
        self.strip_silenced_on_mix(strip)
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
        let Some(lane) = ReturnLane::from_i32(id) else {
            return;
        };
        if !self.config.is_plugin_return(lane) {
            return;
        }
        self.config.ensure_return_lane(lane);
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

    /// MixLink `PluginHost.syncSends` gain for one plugin chain / strip.
    #[must_use]
    pub fn plugin_send_gain(&self, chain: uuid::Uuid, strip: usize) -> f32 {
        let Some(row) = self.surface.strips.get(strip) else {
            return 0.0;
        };
        if !self.config.is_strip_enabled(strip) || self.strip_muted(strip) {
            return 0.0;
        }
        let mut gain = 0.0;
        for lane in crate::types::ALL_SEND_LANES {
            if matches!(self.config.send_destination(lane), Some(SendDestination::Plugin(id)) if id == chain)
            {
                if self.config.sends_post_fader && self.aux_silenced_by_solo(strip, lane) {
                    continue;
                }
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
                Some(SendDestination::Plugin(id)) if id == chain
            )
            && !(self.config.sends_post_fader
                && self.assign_silenced_by_solo(strip, MixAssign::Bus1))
        {
            gain += row.fader;
        }
        if row.assign == MixAssign::Bus2
            && matches!(
                self.config.send_destination(ReturnLane::Bus2),
                Some(SendDestination::Plugin(id)) if id == chain
            )
            && !(self.config.sends_post_fader
                && self.assign_silenced_by_solo(strip, MixAssign::Bus2))
        {
            gain += row.fader;
        }
        gain
    }

    pub fn selected_name(&self, id: ChannelID) -> String {
        self.resolved_gear_name(id).unwrap_or_else(|| self.hardware_name(id))
    }

    pub fn effect_display_name(&self, ref_: ChainRef) -> String {
        self.config.chain_title(ref_)
    }

    pub fn return_display_name(&self, lane: ReturnLane) -> String {
        match self.config.chain_ref(lane) {
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
            SendDestination::Plugin(id) => self
                .config
                .plugin_chain(id)
                .map(|p| p.title())
                .unwrap_or_else(|| "Plugin".into()),
        }
    }

    pub fn set_strip_source(&mut self, strip: usize, id: ChannelID) {
        let linked = self.mixer.channel(id).map(|c| c.stereo).unwrap_or_else(|| {
            self.config.strips.get(strip).map(|s| s.linked_stereo).unwrap_or(true)
        });
        self.remap_strip(strip, id, linked);
        self.persist();
    }

    pub fn clear_strip_source(&mut self, strip: usize) {
        if strip >= self.config.strips.len() {
            return;
        }
        if !self.config.strips[strip].has_input {
            return;
        }
        self.silence_strip(strip);
        self.config.strips[strip].has_input = false;
        self.persist();
    }

    pub fn strip_display_name(&self, strip: usize) -> String {
        match self.config.strips.get(strip) {
            Some(b) if b.has_input => self.selected_name(b.channel_id()),
            _ => "No input".into(),
        }
    }

    /// Apply per-project strip sources and enable flags. Remaps TotalMix when a
    /// strip's input changes. An empty list is ignored so older sidecars can seed.
    pub fn apply_project_strips(&mut self, project: &[StripBinding]) -> bool {
        if project.is_empty() {
            return false;
        }
        let mut changed = false;
        for (i, binding) in project.iter().take(self.config.strips.len()).enumerate() {
            if self.config.strips[i] == *binding {
                continue;
            }
            if binding.has_input {
                self.remap_strip(i, binding.channel_id(), binding.linked_stereo);
            } else {
                if self.config.strips[i].has_input {
                    self.silence_strip(i);
                }
                self.config.strips[i] = binding.clone();
            }
            if self.config.strips[i].enabled != binding.enabled {
                self.config.strips[i].enabled = binding.enabled;
                self.apply_channel_enable(i);
            }
            changed = true;
        }
        changed
    }

    pub fn set_gear_name(&mut self, id: ChannelID, name: &str) {
        let key = SessionConfig::gear_key(id);
        if name.is_empty() {
            self.config.gear_names.remove(&key);
        } else {
            self.config.gear_names.insert(key, name.to_string());
        }
        if self.mixer.channel(id).is_some_and(|c| c.stereo) {
            let right = ChannelID::new(id.bus, id.index + 1);
            let rkey = SessionConfig::gear_key(right);
            if name.is_empty() {
                self.config.gear_names.remove(&rkey);
            } else {
                self.config.gear_names.insert(rkey, name.to_string());
            }
        }
        self.persist();
    }

    pub fn set_return_chain(&mut self, lane: ReturnLane, ref_: Option<ChainRef>) {
        let previous_source = self.return_source(lane);
        if let Some(prev) = self.config.send_destination(lane) {
            self.clear_send_mix(prev);
        }
        self.config.set_return_chain(lane, ref_);
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

    pub fn set_return_effect(&mut self, lane: ReturnLane, ref_: Option<ChainRef>) {
        self.set_return_chain(lane, ref_);
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
        for i in 0..self.surface.strips.len() {
            self.surface.strips[i].set_aux(0.0, lane);
            self.config.set_plugin_send(i, lane, 0.0);
        }
        self.config.set_return_chain(lane, None);
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

    pub fn add_hardware_preset(&mut self) {
        let mut used_out: std::collections::HashSet<i32> =
            self.config.hardware_presets.iter().map(|h| h.output).collect();
        used_out.insert(self.config.main_output);
        let used_in: std::collections::HashSet<i32> =
            self.config.hardware_presets.iter().map(|h| h.input).collect();
        let output = next_free_pair(&used_out, 0);
        let input = next_free_pair(&used_in, 0);
        self.config.add_hardware_preset(output, input, "");
        self.persist();
    }

    pub fn add_hardware_chain(&mut self) {
        let stages = self.config.hardware_presets.first().map(|p| vec![p.id]).unwrap_or_default();
        self.config.add_hardware_chain("", stages);
        self.persist();
    }

    pub fn add_hardware_chain_stage(&mut self, chain: uuid::Uuid, preset: uuid::Uuid) {
        self.apply_hardware_chain_edit(chain, |cfg| cfg.add_hardware_chain_stage(chain, preset));
    }

    pub fn remove_hardware_chain_stage_at(&mut self, chain: uuid::Uuid, index: usize) {
        self.apply_hardware_chain_edit(chain, |cfg| {
            cfg.remove_hardware_chain_stage_at(chain, index)
        });
    }

    pub fn move_hardware_chain_stage(&mut self, chain: uuid::Uuid, index: usize, delta: i32) {
        self.apply_hardware_chain_edit(chain, |cfg| {
            cfg.move_hardware_chain_stage(chain, index, delta)
        });
    }

    pub fn set_hardware_chain_stage(&mut self, chain: uuid::Uuid, index: usize, preset: uuid::Uuid) {
        self.apply_hardware_chain_edit(chain, |cfg| {
            if let Some(slot) = cfg.hardware_chain_mut(chain).and_then(|c| c.stages.get_mut(index)) {
                *slot = preset;
            }
        });
    }

    fn apply_hardware_chain_edit(
        &mut self,
        chain: uuid::Uuid,
        edit: impl FnOnce(&mut crate::config::SessionConfig),
    ) {
        for lane in ReturnLane::ALL {
            if self.config.chain_ref(lane).is_some_and(|r| r.id == chain) {
                if let Some(dest) = self.config.send_destination(lane) {
                    self.clear_send_mix(dest);
                }
                self.silence_return(lane as i32, None, None);
            }
        }
        edit(&mut self.config);
        self.config.sync_legacy_dests();
        self.rewrite_all_sends();
        self.apply_all_returns();
        self.persist();
    }

    pub fn add_plugin_chain(&mut self) {
        let pair = self.config.next_free_playback_pair();
        self.config.add_plugin_chain("", pair);
        self.persist();
    }

    pub fn remove_hardware_preset(&mut self, id: uuid::Uuid) {
        for lane in ReturnLane::ALL {
            if self.lane_uses_preset(lane, id) {
                self.silence_return(lane as i32, None, None);
                if let Some(dest) = self.config.send_destination(lane) {
                    self.clear_send_mix(dest);
                }
            }
        }
        self.config.remove_hardware_preset(id);
        self.rewrite_all_sends();
        self.apply_all_returns();
        self.persist();
    }

    pub fn remove_hardware_chain(&mut self, id: uuid::Uuid) {
        for lane in ReturnLane::ALL {
            if self.config.chain_ref(lane).is_some_and(|r| r.id == id) {
                self.silence_return(lane as i32, None, None);
                if let Some(dest) = self.config.send_destination(lane) {
                    self.clear_send_mix(dest);
                }
            }
        }
        self.config.remove_hardware_chain(id);
        self.rewrite_all_sends();
        self.apply_all_returns();
        self.persist();
    }

    pub fn set_hardware_preset_name(&mut self, id: uuid::Uuid, name: &str) {
        self.config.rename_hardware_preset(id, name);
        self.persist();
    }

    pub fn set_hardware_chain_name(&mut self, id: uuid::Uuid, name: &str) {
        self.config.rename_hardware_chain(id, name);
        self.persist();
    }

    pub fn set_plugin_chain_name(&mut self, id: uuid::Uuid, name: &str) {
        self.config.rename_plugin_chain(id, name);
        self.persist();
    }

    pub fn set_hardware_preset_io(&mut self, id: uuid::Uuid, output: Option<i32>, input: Option<i32>) {
        if input.is_some() {
            for lane in ReturnLane::ALL {
                if self.lane_uses_preset(lane, id) {
                    self.silence_return(lane as i32, None, None);
                }
            }
        }
        self.update_hardware_preset(id, |h| {
            if let Some(o) = output {
                h.output = o;
            }
            if let Some(i) = input {
                h.input = i;
            }
        });
        self.persist();
    }

    pub fn remove_plugin_chain(&mut self, id: uuid::Uuid) {
        for lane in ReturnLane::ALL {
            if self.config.chain_ref(lane).is_some_and(|r| r.id == id) {
                self.silence_return(lane as i32, None, None);
            }
        }
        self.config.remove_plugin_chain(id);
        self.rewrite_all_sends();
        self.apply_all_returns();
        self.persist();
    }

    pub fn set_plugin_stage_bundle(
        &mut self,
        id: uuid::Uuid,
        path: Option<String>,
        name: Option<String>,
    ) {
        if let Some(p) = self.config.plugin_stage_mut(id) {
            p.bundle_path = path;
            p.class_uid = None;
            if let Some(n) = name {
                p.name = n;
            }
        }
        self.apply_all_returns();
        self.persist();
    }

    pub fn set_plugin_stage_bypass(&mut self, id: uuid::Uuid, bypassed: bool) {
        if let Some(p) = self.config.plugin_stage_mut(id) {
            p.bypassed = bypassed;
        }
        self.persist();
    }

    pub fn set_plugin_chain_playback(&mut self, id: uuid::Uuid, pair: i32) {
        if !self.config.playback_pair_selectable(pair, id) {
            return;
        }
        for lane in ReturnLane::ALL {
            if self.config.chain_ref(lane).is_some_and(|r| r.id == id) {
                self.silence_return(lane as i32, None, None);
            }
        }
        if let Some(p) = self.config.plugin_chain_mut(id) {
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
