# MixLinkRs

Rust rewrite of [MixLink](https://github.com): a macOS control surface for
[TotalMix FX](https://www.rme-audio.de/totalmix-fx.html) 2.1 **Global OSC**,
with a mixer UI in the TotalMix 2.x visual language. A Novation Launch Control
XL Mk2 is treated as an 8-channel analog mixer (1:1). TotalMix remains the
hardware mixer.

Application Support stays `~/Library/Application Support/MixLink` so sessions
are shared with the Swift MixLink app.

## Requirements

- macOS 14+
- Rust 1.75+ (`rustup`)
- TotalMix FX 2.1+ with **Global OSC** enabled
- Network access on first VST3 build, to fetch the Steinberg SDK
- Optional: Launch Control XL Mk2
- Optional: RME Fireface (or any stereo-output device matching `audioDeviceContains`)

## Run

```bash
cargo run -p app
```

Default window is 1672×941. Mic permission is required for device input.

## TotalMix setup

1. Options → **Enable OSC Control**
2. Settings (F3) → OSC → controller in use, host `127.0.0.1`
3. Incoming **7001**, outgoing **9001**
4. Compatibility: **Global OSC**
5. Details: send changes, send status; turn **Follow Submix** off
6. Enable receive to hidden channels if a destination is not in the current layout
7. Do **not** Enable MIDI Control on the XL

On connect MixLinkRs sends `/sendall`, `/sendmix`, and `/sendstate`. Mix sends
use `/mix/in/{src}/{out}/faderlin` and `/mix/pb/...` (0-based). Pan is
`/mix/.../balpan` (−1…+1). Off nodes send `fader -300`.

## Launch Control XL

Use a **User** template (not Ableton Factory 1). Default map:

- Send A knobs CC 13–20
- Send B knobs CC 29–36
- Pan knobs CC 49–56
- Faders CC 77–84
- Focus notes 41–44 and 57–60
- Control notes 73–76 and 89–90

LED writes go through `midi-xl`. Rules:

- Write all 16 templates (`Set LEDs` `F0 00 20 29 02 11 78 <template> …`)
- One SysEx lists every LED. A second subset message blanks the rest.
- Never note-on 105–108 as LED addresses.
- Never send `0x77` (template select).

## VST3 SDK

```bash
MIXLINK_VST3_ARCHS=arm64 ./Scripts/build-vst3-sdk.sh
```

The script clones Steinberg repos at `v3.8.1_build_84` into `Vendor/vst3sdk`
(untracked). Same licence constraint as MixLink (GPLv3 or Steinberg proprietary).
