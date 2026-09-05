//! TotalMix FX 2.1 Global OSC: codec, addresses, fader scale, UDP session.

pub mod addresses;
pub mod codec;
pub mod scale;
pub mod session;

pub use addresses::{
    controlroom_dim, controlroom_mainmono, controlroom_speakerb, controlroom_talkback, mix_balpan,
    mix_fader, mix_fader_lin, mix_prefix, output_fader_lin, send_chan, strip_mute, strip_name,
    strip_solo, strip_stereo, Bus, SEND_ALL, SEND_MIX, SEND_STATE,
};
pub use codec::{decode, encode, encode_bundle, OscMessage, OscValue};
pub use scale::{
    balpan_to_pan_unit, fader_db, fader_lin_from_db, fader_lin_to_amp, pan_unit_to_balpan,
    post_fader_lin, send_lin_from_post, stereo_pan_amps, DB_FLOOR, DB_OFF, FADER_LIN_0DB, LIN_EPS,
};
pub use session::{OscError, OscSession};
