//! TotalMix FX 2.1 Global OSC addresses. Channel indices are 0-based.
//! Stereo pairs are addressed by the left channel.

/// Hardware / software bus used in `/sendchan` and strip paths.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Bus {
    Input,
    Playback,
    Output,
}

impl Bus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Playback => "playback",
            Self::Output => "output",
        }
    }

    pub fn from_strip_token(token: &str) -> Option<Self> {
        match token {
            "input" => Some(Self::Input),
            "playback" => Some(Self::Playback),
            "output" => Some(Self::Output),
            _ => None,
        }
    }

    pub fn from_mix_token(token: &str) -> Option<Self> {
        match token {
            "in" => Some(Self::Input),
            "pb" => Some(Self::Playback),
            _ => None,
        }
    }
}

pub const SEND_STATE: &str = "/sendstate";
pub const SEND_ALL: &str = "/sendall";
pub const SEND_MIX: &str = "/sendmix";

/// Dump one strip (name, stereo, mute, …). `/sendall` often drops stereo.
pub fn send_chan(bus: Bus, ch: i32) -> String {
    format!("/sendchan/{}/{}", bus.as_str(), ch)
}

pub fn mix_prefix(source_bus: Bus) -> &'static str {
    if source_bus == Bus::Playback {
        "pb"
    } else {
        "in"
    }
}

/// Send level 0...1 into a hardware output (left index of a stereo dest).
pub fn mix_fader_lin(source_bus: Bus, source: i32, dest: i32) -> String {
    format!("/mix/{}/{}/{}/faderlin", mix_prefix(source_bus), source, dest)
}

/// −300 dB = off (for clearing a mix node).
pub fn mix_fader(source_bus: Bus, source: i32, dest: i32) -> String {
    format!("/mix/{}/{}/{}/fader", mix_prefix(source_bus), source, dest)
}

/// Pan/balance into a mix node, −1...+1.
pub fn mix_balpan(source_bus: Bus, source: i32, dest: i32) -> String {
    format!("/mix/{}/{}/{}/balpan", mix_prefix(source_bus), source, dest)
}

pub fn strip_name(bus: Bus, channel: i32) -> String {
    format!("/{}/{}/name", bus.as_str(), channel)
}

pub fn strip_mute(bus: Bus, channel: i32) -> String {
    format!("/{}/{}/mute", bus.as_str(), channel)
}

pub fn strip_solo(bus: Bus, channel: i32) -> String {
    format!("/{}/{}/solo", bus.as_str(), channel)
}

pub fn strip_stereo(bus: Bus, channel: i32) -> String {
    format!("/{}/{}/stereo", bus.as_str(), channel)
}

pub fn output_fader_lin(channel: i32) -> String {
    format!("/output/{channel}/faderlin")
}

pub fn controlroom_dim() -> &'static str {
    "/controlroom/dim"
}

pub fn controlroom_mainmono() -> &'static str {
    "/controlroom/mainmono"
}

pub fn controlroom_speakerb() -> &'static str {
    "/controlroom/speakerb"
}

pub fn controlroom_talkback() -> &'static str {
    "/controlroom/talkback"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dump_and_strip_paths() {
        assert_eq!(send_chan(Bus::Input, 0), "/sendchan/input/0");
        assert_eq!(send_chan(Bus::Output, 32), "/sendchan/output/32");
        assert_eq!(mix_fader_lin(Bus::Input, 2, 14), "/mix/in/2/14/faderlin");
        assert_eq!(mix_fader(Bus::Playback, 0, 0), "/mix/pb/0/0/fader");
        assert_eq!(mix_balpan(Bus::Input, 4, 0), "/mix/in/4/0/balpan");
        assert_eq!(strip_mute(Bus::Input, 1), "/input/1/mute");
        assert_eq!(output_fader_lin(0), "/output/0/faderlin");
        assert_eq!(controlroom_dim(), "/controlroom/dim");
        assert_eq!(controlroom_mainmono(), "/controlroom/mainmono");
    }
}
