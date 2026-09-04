//! Launch Control XL / on-screen mixer surface.

use crate::config::SessionConfig;
use crate::types::{MixAssign, ReturnLane, ReturnStrip, SurfaceStrip};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackControlMode {
    BusAssign,
    Mute,
    Solo,
}

#[derive(Clone, Debug)]
pub struct SurfaceState {
    pub strips: Vec<SurfaceStrip>,
    pub returns: Vec<ReturnStrip>,
    pub track_control_mode: TrackControlMode,
}

impl Default for SurfaceState {
    fn default() -> Self {
        Self::new()
    }
}

impl SurfaceState {
    pub fn new() -> Self {
        Self {
            strips: (0..8).map(SurfaceStrip::new).collect(),
            returns: ReturnLane::ALL.iter().map(|lane| ReturnStrip::new(*lane as i32)).collect(),
            track_control_mode: TrackControlMode::BusAssign,
        }
    }

    pub fn load_returns(&mut self, config: &SessionConfig) {
        let previous: std::collections::HashMap<i32, ReturnStrip> =
            self.returns.iter().cloned().map(|r| (r.id, r)).collect();
        self.returns = ReturnLane::ALL
            .iter()
            .map(|lane| {
                let id = *lane as i32;
                let cfg = config.return_lane(id);
                let old = previous.get(&id);
                ReturnStrip {
                    id,
                    fader: cfg.map(|c| c.fader).or_else(|| old.map(|o| o.fader)).unwrap_or(0.0),
                    pan: cfg.map(|c| c.pan).unwrap_or(0.5),
                    mute: old.map(|o| o.mute).unwrap_or(false),
                    solo: old.map(|o| o.solo).unwrap_or(false),
                }
            })
            .collect();
    }

    pub fn toggle_mute_mode(&mut self) {
        self.track_control_mode = if self.track_control_mode == TrackControlMode::Mute {
            TrackControlMode::BusAssign
        } else {
            TrackControlMode::Mute
        };
    }

    pub fn toggle_solo_mode(&mut self) {
        self.track_control_mode = if self.track_control_mode == TrackControlMode::Solo {
            TrackControlMode::BusAssign
        } else {
            TrackControlMode::Solo
        };
    }

    pub fn toggle_assign(&mut self, strip: usize, target: MixAssign) {
        if let Some(s) = self.strips.get_mut(strip) {
            if s.assign == target {
                s.assign = MixAssign::Main;
            } else {
                s.assign = target;
            }
        }
    }
}
