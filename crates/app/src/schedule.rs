//! Publish the RT schedule and lock-free strip controls.

use std::sync::Arc;

use analog::ReturnLane;
use engine::{AudioTapBinding, Engine, LanePlayer, MixGain, Schedule, StripFeed};
use engine_api::{MASTER_TAP, MIX_PLAY_MAX_LANES, TAP_COUNT};
use project::MixLane;

use crate::state::AppState;

impl AppState {
    pub(crate) fn sync_rt(&mut self) {
        for i in 0..8 {
            self.engine_handles.controls.set_fader(i, self.analog.surface.strips[i].fader);
        }
        self.publish_schedule();
    }

    pub(crate) fn restart_audio(&mut self) {
        self._stream = None;
        let engine_addr = self.engine as usize;
        let needle = self.analog.config.audio_device_contains.clone();
        let frames = self.analog.config.audio_buffer_frames.map(|n| n as u32);
        match audio_io::DeviceStream::open_named(
            &needle,
            frames,
            Box::new(move |input, output, timing| unsafe {
                let engine = engine_addr as *mut Engine;
                (*engine).process(input, output, timing.frames as usize, timing.host_time);
            }),
        ) {
            Ok(s) => {
                log::info!("audio {} Hz / {} frames", s.sample_rate(), s.buffer_frames());
                self._stream = Some(s);
            }
            Err(e) => log::warn!("audio: {e}"),
        }
    }

    pub(crate) fn publish_schedule(&self) {
        let mut schedule = Schedule::empty();
        for (i, strip) in self.analog.config.strips.iter().enumerate().take(8) {
            // Mono strips tap one input twice; record DSP pans that into a stereo file.
            schedule.taps[i] = AudioTapBinding::hardware(
                strip.index,
                if strip.linked_stereo { strip.index + 1 } else { strip.index },
            );
        }
        for lane in analog::ReturnLane::ALL {
            let tap = 8 + lane as usize;
            if tap >= TAP_COUNT {
                continue;
            }
            if let Some(id) = self.analog.config.return_source_id(lane) {
                schedule.taps[tap] = AudioTapBinding::hardware(id.index, id.index + 1);
            }
        }
        schedule.taps[MASTER_TAP] = AudioTapBinding::master_mix();
        self.publish_record_main_mix(&mut schedule);

        for slot in 0..8 {
            let id = slot as i32;
            let Some(plugin) = self.analog.config.plugin(id) else {
                continue;
            };
            if !plugin.is_loaded() {
                continue;
            }
            let used = analog::ALL_SEND_LANES.iter().any(|lane| {
                matches!(
                    self.analog.config.send_destination(*lane),
                    Some(analog::SendDestination::Plugin(pid)) if pid == id
                )
            }) || matches!(
                self.analog.config.send_destination(ReturnLane::Bus1),
                Some(analog::SendDestination::Plugin(pid)) if pid == id
            ) || matches!(
                self.analog.config.send_destination(ReturnLane::Bus2),
                Some(analog::SendDestination::Plugin(pid)) if pid == id
            );
            if !used {
                continue;
            }
            schedule.routes[slot].enabled = true;
            schedule.routes[slot].return_channel = plugin.return_channel;
            for (i, strip) in self.analog.config.strips.iter().enumerate().take(8) {
                schedule.routes[slot].feeds[i] = StripFeed {
                    channel: strip.index,
                    gain: self.analog.plugin_send_gain(id, i),
                    linked: strip.linked_stereo,
                };
            }
        }

        let play_tracks = self.mixer_tracks();
        schedule.any_solo = play_tracks.iter().any(|t| t.solo);
        let main_dest = self.analog.config.mix_playback_channel(analog::MixLane::Main);
        for (i, track) in play_tracks.iter().take(MIX_PLAY_MAX_LANES).enumerate() {
            // Mix page: strip, send, and return are the same — clips into Main.
            let dest = if track.lane == MixLane::Main { main_dest } else { -1 };
            let amp = osc::fader_lin_to_amp(track.fader);
            let pan = if track.lane == MixLane::Main { 0.5 } else { track.pan };
            let (gain_l, gain_r) = osc::stereo_pan_amps(amp, pan);
            schedule.lanes[i] = LanePlayer {
                active: self.playing,
                is_main: track.lane == MixLane::Main,
                dest,
                muted: track.mute,
                soloed: track.solo,
                gain_l,
                gain_r,
                insert: usize::MAX,
            };
        }
        schedule.listen_amp = osc::fader_lin_to_amp(self.control_room_fader);
        self.engine_handles.schedule.store(Arc::new(schedule));
    }

    /// Print analog fader/mute and the live pan knob onto each stem.
    /// Mix-page faders stay playback-only.

    pub(crate) fn publish_record_main_mix(&self, schedule: &mut Schedule) {
        let any_solo = self.analog.any_solo_active();
        for i in 0..8 {
            let muted = analog_performance_muted(
                self.analog.strip_muted(i),
                self.analog.strip_soloed(i),
                any_solo,
            );
            let enabled = self.analog.config.is_strip_enabled(i);
            schedule.record_muted[i] = muted;
            schedule.mix_gains[i] = MixGain {
                gain: osc::fader_lin_to_amp(self.analog.strip_main_mix_lin(i)),
                pan: self.analog.strip_record_pan(i),
                muted,
                into_master: enabled && !muted,
            };
        }
        for lane in ReturnLane::ALL {
            let tap = 8 + lane as usize;
            if tap >= MASTER_TAP {
                continue;
            }
            let muted = analog_performance_muted(
                self.analog.return_muted(lane),
                self.analog.return_soloed(lane),
                any_solo,
            );
            let enabled = self.analog.config.is_return_enabled(lane);
            schedule.record_muted[tap] = muted;
            schedule.mix_gains[tap] = MixGain {
                gain: osc::fader_lin_to_amp(self.analog.return_main_mix_lin(lane)),
                pan: self.analog.return_record_pan(lane),
                muted,
                into_master: enabled && !muted,
            };
        }
    }
}

fn analog_performance_muted(muted: bool, soloed: bool, any_solo: bool) -> bool {
    muted || (any_solo && !soloed)
}
