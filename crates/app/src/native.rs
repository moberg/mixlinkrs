//! AppKit overlays: Settings, Channels, file dialogs, menus, text fields.

#![allow(dead_code)]

/// Dock / Cmd-Tab icon. `cargo run` is a bare binary, so the bundle icon never
/// applies — set `NSApplication.applicationIconImage` once NSApp exists.
pub fn apply_app_icon() {
    #[cfg(target_os = "macos")]
    apply_app_icon_macos();
}

#[cfg(target_os = "macos")]
fn apply_app_icon_macos() {
    use objc2::ClassType;
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::{MainThreadMarker, NSData, NSSize};

    const ICON_PNG: &[u8] = include_bytes!("../assets/AppIcon.png");

    let Some(mtm) = MainThreadMarker::new() else {
        log::warn!("app icon: not on main thread");
        return;
    };
    let data = NSData::with_bytes(ICON_PNG);
    let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else {
        log::warn!("app icon: NSImage init failed");
        return;
    };
    // Bitmap is 1024² px; point size must be a Dock tile, not 1024pt.
    unsafe { image.setSize(NSSize { width: 128.0, height: 128.0 }) };
    let app = NSApplication::sharedApplication(mtm);
    unsafe { app.setApplicationIconImage(Some(&image)) };
}

pub fn pick_projects_folder() -> Option<std::path::PathBuf> {
    #[cfg(target_os = "macos")]
    {
        pick_folder_macos()
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

#[cfg(target_os = "macos")]
fn pick_folder_macos() -> Option<std::path::PathBuf> {
    use objc2::rc::Id;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send, msg_send_id};
    use objc2_foundation::NSString;

    let panel: Id<AnyObject> = unsafe { msg_send_id![class!(NSOpenPanel), openPanel] };
    let _: () = unsafe { msg_send![&*panel, setCanChooseFiles: false] };
    let _: () = unsafe { msg_send![&*panel, setCanChooseDirectories: true] };
    let _: () = unsafe { msg_send![&*panel, setAllowsMultipleSelection: false] };
    let _: () = unsafe { msg_send![&*panel, setCanCreateDirectories: true] };
    let title = NSString::from_str("Projects folder");
    let _: () = unsafe { msg_send![&*panel, setTitle: &*title] };
    let ok: isize = unsafe { msg_send![&*panel, runModal] };
    if ok != 1 {
        return None;
    }
    let url: Option<Id<AnyObject>> = unsafe { msg_send_id![&*panel, URL] };
    let path: Option<Id<NSString>> = unsafe { msg_send_id![&*url?, path] };
    path.map(|p| std::path::PathBuf::from(p.to_string()))
}

pub fn alert(message: &str) {
    log::info!("alert: {message}");
}

pub fn confirm_delete_mix() -> bool {
    #[cfg(target_os = "macos")]
    {
        confirm_delete_macos()
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

#[cfg(target_os = "macos")]
fn confirm_delete_macos() -> bool {
    use objc2::rc::Id;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send, msg_send_id};
    use objc2_foundation::NSString;

    let alert: Id<AnyObject> = unsafe { msg_send_id![class!(NSAlert), new] };
    let title = NSString::from_str("Delete this mix?");
    let info = NSString::from_str("The mix file will be removed from the project folder.");
    let delete = NSString::from_str("Delete");
    let cancel = NSString::from_str("Cancel");
    let _: () = unsafe { msg_send![&*alert, setMessageText: &*title] };
    let _: () = unsafe { msg_send![&*alert, setInformativeText: &*info] };
    let _: () = unsafe { msg_send![&*alert, addButtonWithTitle: &*delete] };
    let _: () = unsafe { msg_send![&*alert, addButtonWithTitle: &*cancel] };
    let code: isize = unsafe { msg_send![&*alert, runModal] };
    code == 1000
}
