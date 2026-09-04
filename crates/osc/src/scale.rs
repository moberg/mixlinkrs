//! Approximate TotalMix fader scale: −∞ … +6 dB.

/// Linear fader that maps to 0 dB on [`fader_db`].
pub const FADER_LIN_0DB: f32 = 65.0 / 71.0;

/// Treat this (and below) as −∞ / off in faderlin space.
pub const LIN_EPS: f32 = 0.001;

/// TotalMix “off node” in dB.
pub const DB_OFF: f32 = -300.0;

/// dB at or below this maps back to linear 0.
pub const DB_FLOOR: f32 = -120.0;

/// `lin <= eps` → −300, else `-65 + lin * 71`.
pub fn fader_db(lin: f32) -> f32 {
    if lin <= LIN_EPS {
        DB_OFF
    } else {
        -65.0 + lin * 71.0
    }
}

pub fn fader_lin_from_db(db: f32) -> f32 {
    if db <= DB_FLOOR {
        0.0
    } else {
        ((db + 65.0) / 71.0).clamp(0.0, 1.0)
    }
}

/// TotalMix `faderlin` (0…1) → linear amplitude. 0 dB is [`FADER_LIN_0DB`].
pub fn fader_lin_to_amp(lin: f32) -> f32 {
    if lin <= LIN_EPS {
        0.0
    } else {
        10f32.powf(fader_db(lin) / 20.0)
    }
}

/// Post-fader send in TotalMix faderlin space. Unity fader leaves `send` unchanged.
pub fn post_fader_lin(send: f32, fader: f32) -> f32 {
    if send <= LIN_EPS || fader <= LIN_EPS {
        0.0
    } else {
        (send + fader - FADER_LIN_0DB).clamp(0.0, 1.0)
    }
}

/// Recover a send amount from a post-fader mix node. `None` when the fader is at −∞.
pub fn send_lin_from_post(mix: f32, fader: f32) -> Option<f32> {
    if fader <= LIN_EPS {
        return None;
    }
    if mix <= LIN_EPS {
        return Some(0.0);
    }
    Some((mix - fader + FADER_LIN_0DB).clamp(0.0, 1.0))
}

/// Pan unit 0…1 → TotalMix balpan −1…+1.
pub fn pan_unit_to_balpan(unit: f32) -> f32 {
    unit * 2.0 - 1.0
}

/// TotalMix balpan −1…+1 → pan unit 0…1.
pub fn balpan_to_pan_unit(pan: f32) -> f32 {
    (pan + 1.0) / 2.0
}
