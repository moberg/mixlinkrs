//! Publish the RT schedule and lock-free strip controls.

use std::sync::Arc;

use analog::{ChainKind, ReturnLane, SessionConfig};
use engine::{AudioTapBinding, Engine, InsertStage, LanePlayer, MixGain, Schedule, StripFeed};
use engine_api::{MASTER_TAP, MIX_PLAY_MAX_LANES, TAP_COUNT};
use project::MixLane;

use crate::state::{AppState, Audio};

impl Audio {
    pub(crate) fn sample_rate(&self) -> f64 {
        self._stream.as_ref().map(|s| s.sample_rate() as f64).unwrap_or(48_000.0)
    }

    pub(crate) fn restart_audio(&mut self, config: &SessionConfig) {
        self._stream = None;
        let engine_addr = self.engine as usize;
        let needle = config.audio_device_contains.clone();
        let frames = config.audio_buffer_frames.map(|n| n as u32);
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
}

impl AppState {
    pub(crate) fn sync_rt(&mut self) {
        for i in 0..8 {
            self.audio
                .engine_handles
                .controls
                .set_fader(i, self.surface.analog.surface.strips[i].fader);
        }
        self.publish_schedule();
    }

    pub(crate) fn publish_schedule(&self) {
        let cfg = &self.surface.analog.config;
        let mut schedule = Schedule::empty();
        for (i, strip) in cfg.strips.iter().enumerate().take(8) {
            // Mono strips tap one input twice; record DSP pans that into a stereo file.
            schedule.taps[i] = if strip.has_input {
                AudioTapBinding::hardware(
                    strip.index,
                    if strip.linked_stereo { strip.index + 1 } else { strip.index },
                )
            } else {
                AudioTapBinding::silent()
            };
        }
        schedule.taps[MASTER_TAP] = AudioTapBinding::master_mix();
        self.publish_record_main_mix(&mut schedule);

        let mut plugin_slots = std::collections::HashMap::new();
        for chain in &cfg.plugin_chains {
            if !chain.is_loaded() {
                continue;
            }
            let used = analog::ReturnLane::ALL.iter().any(|lane| {
                matches!(
                    cfg.send_destination(*lane),
                    Some(analog::SendDestination::Plugin(pid)) if pid == chain.id
                )
            });
            if !used {
                continue;
            }
            let mut route = engine::SendRoute {
                enabled: true,
                return_channel: chain.return_channel,
                ..engine::SendRoute::default()
            };
            for (i, strip) in cfg.strips.iter().enumerate().take(8) {
                route.feeds[i] = StripFeed {
                    channel: if strip.has_input { strip.index } else { -1 },
                    gain: self.surface.analog.plugin_send_gain(chain.id, i),
                    linked: strip.linked_stereo,
                };
            }
            route.stages = chain
                .stages
                .iter()
                .filter_map(|stage| {
                    let inst = self.audio.plugin_refs.get(&stage.id).copied()?;
                    Some(InsertStage { instance: inst as usize, bypassed: stage.bypassed })
                })
                .collect();
            plugin_slots.insert(chain.id, schedule.routes.len() as i32);
            schedule.routes.push(route);
        }
        for lane in analog::ReturnLane::ALL {
            let tap = 8 + lane as usize;
            if tap >= TAP_COUNT {
                continue;
            }
            match cfg.chain_ref(lane) {
                Some(r) if r.kind == ChainKind::Plugin => {
                    if let Some(&slot) = plugin_slots.get(&r.id) {
                        schedule.taps[tap] = AudioTapBinding::plugin(slot);
                    }
                }
                Some(_) => {
                    if let Some(id) = cfg.return_source_id(lane) {
                        schedule.taps[tap] = AudioTapBinding::hardware(id.index, id.index + 1);
                    }
                }
                None => {}
            }
        }

        let play_tracks = self.mixer_tracks();
        schedule.any_solo = play_tracks.iter().any(|t| t.solo);
        let main_dest = cfg.mix_playback_channel(analog::MixLane::Main);
        for (i, track) in play_tracks.iter().take(MIX_PLAY_MAX_LANES).enumerate() {
            // Mix page: strip, send, and return are the same — clips into Main.
            let dest = if track.lane == MixLane::Main { main_dest } else { -1 };
            let amp = osc::fader_lin_to_amp(track.fader);
            let pan = if track.lane == MixLane::Main { 0.5 } else { track.pan };
            let (gain_l, gain_r) = osc::stereo_pan_amps(amp, pan);
            let mut insert_stages = Vec::new();
            let mut hw_out = -1;
            let mut hw_in = -1;
            let mut hw_enabled = false;
            if let Some(chain) = track.effect_chain {
                match chain.kind {
                    ChainKind::Plugin => {
                        if let Some(pc) = cfg.plugin_chain(chain.id) {
                            insert_stages = pc
                                .stages
                                .iter()
                                .filter_map(|stage| {
                                    let inst = self.audio.plugin_refs.get(&stage.id).copied()?;
                                    Some(InsertStage {
                                        instance: inst as usize,
                                        bypassed: stage.bypassed,
                                    })
                                })
                                .collect();
                        }
                    }
                    ChainKind::Hardware => {
                        if let Some(hc) = cfg.hardware_chain(chain.id) {
                            if let (Some(&first), Some(&last)) =
                                (hc.stages.first(), hc.stages.last())
                            {
                                if let (Some(a), Some(b)) =
                                    (cfg.hardware_preset(first), cfg.hardware_preset(last))
                                {
                                    hw_out = a.output;
                                    hw_in = b.input;
                                    hw_enabled = track.hardware_chain_enabled;
                                }
                            }
                        }
                    }
                }
            }
            schedule.lanes[i] = LanePlayer {
                active: self.audio.playing,
                is_main: track.lane == MixLane::Main,
                dest,
                muted: track.mute,
                soloed: track.solo,
                gain_l,
                gain_r,
                insert: usize::MAX,
                insert_stages,
                hw_out,
                hw_in,
                hw_enabled,
            };
        }
        schedule.listen_amp = osc::fader_lin_to_amp(self.surface.control_room_fader);
        self.audio.engine_handles.schedule.store(Arc::new(schedule));
    }

    /// Print analog fader/mute and the live pan knob onto each stem.
    /// Mix-page faders stay playback-only.

    pub(crate) fn publish_record_main_mix(&self, schedule: &mut Schedule) {
        let any_solo = self.surface.analog.any_solo_active();
        for i in 0..8 {
            let muted = analog_performance_muted(
                self.surface.analog.strip_muted(i),
                self.surface.analog.strip_soloed(i),
                any_solo,
            );
            let enabled = self.surface.analog.config.is_strip_enabled(i);
            schedule.record_muted[i] = muted;
            schedule.mix_gains[i] = MixGain {
                gain: osc::fader_lin_to_amp(self.surface.analog.strip_main_mix_lin(i)),
                pan: self.surface.analog.strip_record_pan(i),
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
                self.surface.analog.return_muted(lane),
                self.surface.analog.return_soloed(lane),
                any_solo,
            );
            let enabled = self.surface.analog.config.is_return_enabled(lane);
            schedule.record_muted[tap] = muted;
            schedule.mix_gains[tap] = MixGain {
                gain: osc::fader_lin_to_amp(self.surface.analog.return_main_mix_lin(lane)),
                pan: self.surface.analog.return_record_pan(lane),
                muted,
                into_master: enabled && !muted,
            };
        }
    }
}

fn analog_performance_muted(muted: bool, soloed: bool, any_solo: bool) -> bool {
    muted || (any_solo && !soloed)
}
