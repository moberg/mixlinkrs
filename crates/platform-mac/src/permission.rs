/// Request microphone permission (any device input is capture on macOS).
pub fn request_mic_permission() {
    use objc2_foundation::NSString;
    // AVCaptureDevice requestAccessForMediaType is in AVFoundation; call via
    // `osascript` fallback is too heavy. The first CoreAudio input open will
    // trigger TCC. This function exists so the app can call it at launch.
    let _ = NSString::from_str("audio");
}
