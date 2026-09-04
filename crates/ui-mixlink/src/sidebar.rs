//! Record EFFECTS/SETTINGS sidebar and Mix INSERTS column. 248 pt.

use analog::{AnalogEngine, ReturnLane};
use project::{MixDocument, MixLane, MixTrack};
use render::{DrawCmd, Rect};

use crate::chrome::Page;
use crate::overlay::TextFocus;
use crate::theme::{self, Layout};
use crate::widgets;

pub struct SidebarView<'a> {
    pub page: Page,
    pub engine: &'a AnalogEngine,
    pub project_name: &'a str,
    pub project_date: &'a str,
    pub project_suffix: &'a str,
    pub sample_rate: u32,
    pub buffer_frames: u32,
    pub latency_ms: f32,
    pub device_name: &'a str,
    pub mix: Option<&'a MixDocument>,
    pub selected_lane: Option<MixLane>,
    pub scroll: f32,
    pub focus: &'a TextFocus,
    pub caret: bool,
}

#[derive(Clone, Debug)]
pub enum SidebarHit {
    Page(Page),
    AddHardware,
    RemoveHardware(i32),
    EditHardwareName(i32),
    HardwareOutput(i32),
    HardwareInput(i32),
    AddPlugin,
    RemovePlugin(i32),
    EditPluginName(i32),
    PluginBundle(i32),
    PluginEdit(i32),
    PluginBypass(i32),
    PluginPlayback(i32),
    ProjectsFolder,
    ProjectName,
    NewProject,
    MixOut,
    AudioDevice,
    AudioBuffer,
    Channels,
    Settings,
    AddInsert,
    RemoveInsert(uuid::Uuid),
    InsertBundle(uuid::Uuid),
    InsertEdit(uuid::Uuid),
    InsertBypass(uuid::Uuid),
}

pub fn paint(view: &SidebarView<'_>, w: f32, h: f32) -> (Vec<DrawCmd>, Vec<(Rect, SidebarHit)>) {
    let mut cmds = Vec::new();
    let mut hits = Vec::new();
    let x = w - Layout::SIDEBAR_WIDTH;
    let y = crate::chrome::HEADER_H;
    let sh = h - crate::chrome::HEADER_H - Layout::FOOTER_H;
    theme::material_grain(&mut cmds, Rect { x, y, w: Layout::SIDEBAR_WIDTH, h: sh }, &theme::SIDEBAR_MAT);
    theme::seam_v(&mut cmds, x, y, sh, true);

    let body_y = y + 42.0;
    let body_h = sh - 42.0 - 40.0;
    let clip = Rect { x, y: body_y, w: Layout::SIDEBAR_WIDTH, h: body_h };

    let mut yy = body_y + 8.0 - view.scroll;
    if view.page == Page::Record {
        paint_record(view, &mut cmds, &mut hits, x, &mut yy, clip);
    } else {
        paint_mix(view, &mut cmds, &mut hits, x, &mut yy, clip);
    }

    theme::material_grain(&mut cmds, Rect { x, y, w: Layout::SIDEBAR_WIDTH, h: 42.0 }, &theme::SIDEBAR_MAT);
    widgets::hardware_pad(
        &mut cmds,
        Rect { x: x + 8.0, y: y + 8.0, w: 110.0, h: 26.0 },
        "Record",
        view.page == Page::Record,
        theme::PRIMARY_TEXT,
    );
    widgets::hardware_pad(
        &mut cmds,
        Rect { x: x + 126.0, y: y + 8.0, w: 110.0, h: 26.0 },
        "Mix",
        view.page == Page::Mix,
        theme::PRIMARY_TEXT,
    );
    hits.push((Rect { x: x + 8.0, y: y + 8.0, w: 110.0, h: 26.0 }, SidebarHit::Page(Page::Record)));
    hits.push((Rect { x: x + 126.0, y: y + 8.0, w: 110.0, h: 26.0 }, SidebarHit::Page(Page::Mix)));

    let by = y + sh - 36.0;
    theme::material_grain(&mut cmds, Rect { x, y: by - 4.0, w: Layout::SIDEBAR_WIDTH, h: 40.0 }, &theme::SIDEBAR_MAT);
    widgets::hardware_pad(
        &mut cmds,
        Rect { x: x + 136.0, y: by, w: 72.0, h: 26.0 },
        "Channels",
        false,
        theme::PRIMARY_TEXT,
    );
    widgets::icon_pad(&mut cmds, Rect { x: x + 214.0, y: by, w: 26.0, h: 26.0 }, "⚙", true);
    hits.push((Rect { x: x + 136.0, y: by, w: 72.0, h: 26.0 }, SidebarHit::Channels));
    hits.push((Rect { x: x + 214.0, y: by, w: 26.0, h: 26.0 }, SidebarHit::Settings));
    (cmds, hits)
}

fn paint_record(
    view: &SidebarView<'_>,
    cmds: &mut Vec<DrawCmd>,
    hits: &mut Vec<(Rect, SidebarHit)>,
    x: f32,
    y: &mut f32,
    clip: Rect,
) {
    section(cmds, x, *y, "EFFECTS");
    *y += 20.0;
    row_plus(cmds, hits, x, *y, "Hardware effects", SidebarHit::AddHardware, true);
    *y += 20.0;
    for hw in &view.engine.config.hardware_effects {
        if *y > clip.y + clip.h {
            break;
        }
        let card = Rect { x: x + 8.0, y: *y, w: Layout::SIDEBAR_WIDTH - 16.0, h: 78.0 };
        theme::fill(cmds, card, theme::RECESSED.middle);
        widgets::text_field(
            cmds,
            Rect { x: card.x + 6.0, y: card.y + 6.0, w: card.w - 36.0, h: 20.0 },
            if hw.name.is_empty() { "Name" } else { &hw.name },
            *view.focus == TextFocus::HardwareName(hw.id),
            view.caret,
        );
        widgets::icon_pad(cmds, Rect { x: card.x + card.w - 26.0, y: card.y + 6.0, w: 20.0, h: 20.0 }, "−", true);
        hits.push((Rect { x: card.x + 6.0, y: card.y + 6.0, w: card.w - 36.0, h: 20.0 }, SidebarHit::EditHardwareName(hw.id)));
        hits.push((Rect { x: card.x + card.w - 26.0, y: card.y + 6.0, w: 20.0, h: 20.0 }, SidebarHit::RemoveHardware(hw.id)));
        theme::text(cmds, Rect { x: card.x + 6.0, y: card.y + 30.0, w: 50.0, h: 16.0 }, "Output", 10.0, theme::TEXT_DIM, false);
        widgets::menu_label(
            cmds,
            Rect { x: card.x + 56.0, y: card.y + 28.0, w: card.w - 64.0, h: 20.0 },
            &view.engine.output_name(hw.output),
        );
        hits.push((Rect { x: card.x + 56.0, y: card.y + 28.0, w: card.w - 64.0, h: 20.0 }, SidebarHit::HardwareOutput(hw.id)));
        theme::text(cmds, Rect { x: card.x + 6.0, y: card.y + 52.0, w: 50.0, h: 16.0 }, "Input", 10.0, theme::TEXT_DIM, false);
        widgets::menu_label(
            cmds,
            Rect { x: card.x + 56.0, y: card.y + 50.0, w: card.w - 64.0, h: 20.0 },
            &view.engine.display_name(analog::ChannelID::new(analog::MixerBus::Input, hw.input)),
        );
        hits.push((Rect { x: card.x + 56.0, y: card.y + 50.0, w: card.w - 64.0, h: 20.0 }, SidebarHit::HardwareInput(hw.id)));
        *y += 84.0;
    }

    let can_add_plugin = view.engine.config.next_free_plugin_id().is_some();
    row_plus(cmds, hits, x, *y, "Plugins", SidebarHit::AddPlugin, can_add_plugin);
    *y += 20.0;
    for plug in &view.engine.config.plugins {
        if *y > clip.y + clip.h {
            break;
        }
        let card = Rect { x: x + 8.0, y: *y, w: Layout::SIDEBAR_WIDTH - 16.0, h: 96.0 };
        theme::fill(cmds, card, theme::RECESSED.middle);
        widgets::text_field(
            cmds,
            Rect { x: card.x + 6.0, y: card.y + 6.0, w: card.w - 36.0, h: 20.0 },
            if plug.name.is_empty() { "Plugin" } else { &plug.name },
            *view.focus == TextFocus::PluginName(plug.id),
            view.caret,
        );
        widgets::icon_pad(cmds, Rect { x: card.x + card.w - 26.0, y: card.y + 6.0, w: 20.0, h: 20.0 }, "−", true);
        hits.push((Rect { x: card.x + 6.0, y: card.y + 6.0, w: card.w - 36.0, h: 20.0 }, SidebarHit::EditPluginName(plug.id)));
        hits.push((Rect { x: card.x + card.w - 26.0, y: card.y + 6.0, w: 20.0, h: 20.0 }, SidebarHit::RemovePlugin(plug.id)));
        widgets::menu_label(
            cmds,
            Rect { x: card.x + 6.0, y: card.y + 28.0, w: card.w - 12.0, h: 20.0 },
            plug.bundle_path.as_deref().and_then(|p| std::path::Path::new(p).file_stem().and_then(|s| s.to_str())).unwrap_or("None"),
        );
        hits.push((Rect { x: card.x + 6.0, y: card.y + 28.0, w: card.w - 12.0, h: 20.0 }, SidebarHit::PluginBundle(plug.id)));
        widgets::hardware_pad(cmds, Rect { x: card.x + 6.0, y: card.y + 50.0, w: 56.0, h: 20.0 }, "Edit", false, theme::PRIMARY_TEXT);
        widgets::hardware_pad(cmds, Rect { x: card.x + 66.0, y: card.y + 50.0, w: 64.0, h: 20.0 }, "Bypass", plug.bypassed, theme::AMBER);
        hits.push((Rect { x: card.x + 6.0, y: card.y + 50.0, w: 56.0, h: 20.0 }, SidebarHit::PluginEdit(plug.id)));
        hits.push((Rect { x: card.x + 66.0, y: card.y + 50.0, w: 64.0, h: 20.0 }, SidebarHit::PluginBypass(plug.id)));
        theme::text(cmds, Rect { x: card.x + 6.0, y: card.y + 72.0, w: 90.0, h: 16.0 }, "Software playback", 9.0, theme::TEXT_DIM, false);
        widgets::menu_label(
            cmds,
            Rect { x: card.x + 100.0, y: card.y + 70.0, w: card.w - 108.0, h: 20.0 },
            &format!("{}/{}", plug.return_channel + 1, plug.return_channel + 2),
        );
        hits.push((Rect { x: card.x + 100.0, y: card.y + 70.0, w: card.w - 108.0, h: 20.0 }, SidebarHit::PluginPlayback(plug.id)));
        *y += 102.0;
    }

    if *y > clip.y + clip.h {
        return;
    }
    section(cmds, x, *y, "SETTINGS");
    *y += 20.0;
    theme::text(cmds, Rect { x: x + 10.0, y: *y, w: 200.0, h: 16.0 }, "Projects folder", 11.0, theme::TEXT_DIM, false);
    *y += 16.0;
    widgets::menu_label(cmds, Rect { x: x + 10.0, y: *y, w: Layout::SIDEBAR_WIDTH - 20.0, h: 22.0 }, view.project_name);
    hits.push((Rect { x: x + 10.0, y: *y, w: Layout::SIDEBAR_WIDTH - 20.0, h: 22.0 }, SidebarHit::ProjectsFolder));
    *y += 26.0;
    if !view.project_suffix.is_empty() || !view.project_date.is_empty() {
        theme::text(cmds, Rect { x: x + 10.0, y: *y, w: 80.0, h: 16.0 }, "Project", 11.0, theme::TEXT_DIM, false);
        *y += 16.0;
        theme::text(cmds, Rect { x: x + 10.0, y: *y, w: 90.0, h: 22.0 }, format!("{} -", view.project_date), 12.0, theme::TEXT_DIM, false);
        widgets::text_field(
            cmds,
            Rect { x: x + 100.0, y: *y, w: Layout::SIDEBAR_WIDTH - 114.0, h: 22.0 },
            view.project_suffix,
            *view.focus == TextFocus::ProjectName,
            view.caret,
        );
        hits.push((Rect { x: x + 100.0, y: *y, w: Layout::SIDEBAR_WIDTH - 114.0, h: 22.0 }, SidebarHit::ProjectName));
        *y += 28.0;
    }
    widgets::hardware_pad(cmds, Rect { x: x + 10.0, y: *y, w: 120.0, h: 24.0 }, "New project", false, theme::PRIMARY_TEXT);
    hits.push((Rect { x: x + 10.0, y: *y, w: 120.0, h: 24.0 }, SidebarHit::NewProject));
    *y += 32.0;
    theme::text(cmds, Rect { x: x + 10.0, y: *y, w: 200.0, h: 16.0 }, "Mix (Main Out)", 11.0, theme::TEXT_DIM, false);
    *y += 16.0;
    widgets::menu_label(
        cmds,
        Rect { x: x + 10.0, y: *y, w: Layout::SIDEBAR_WIDTH - 20.0, h: 22.0 },
        &view.engine.output_name(view.engine.config.main_output),
    );
    hits.push((Rect { x: x + 10.0, y: *y, w: Layout::SIDEBAR_WIDTH - 20.0, h: 22.0 }, SidebarHit::MixOut));
    *y += 26.0;
    theme::text(cmds, Rect { x: x + 10.0, y: *y, w: 200.0, h: 16.0 }, "Audio Device", 11.0, theme::TEXT_DIM, false);
    *y += 16.0;
    widgets::menu_label(cmds, Rect { x: x + 10.0, y: *y, w: Layout::SIDEBAR_WIDTH - 20.0, h: 22.0 }, view.device_name);
    hits.push((Rect { x: x + 10.0, y: *y, w: Layout::SIDEBAR_WIDTH - 20.0, h: 22.0 }, SidebarHit::AudioDevice));
    *y += 24.0;
    theme::text(
        cmds,
        Rect { x: x + 10.0, y: *y, w: Layout::SIDEBAR_WIDTH - 20.0, h: 16.0 },
        format!("{} Hz · {} frames · {:.1} ms", view.sample_rate, view.buffer_frames, view.latency_ms),
        10.0,
        theme::TEXT_DIM,
        false,
    );
    hits.push((Rect { x: x + 10.0, y: *y, w: Layout::SIDEBAR_WIDTH - 20.0, h: 16.0 }, SidebarHit::AudioBuffer));
    let _ = ReturnLane::SendA;
}

fn paint_mix(
    view: &SidebarView<'_>,
    cmds: &mut Vec<DrawCmd>,
    hits: &mut Vec<(Rect, SidebarHit)>,
    x: f32,
    y: &mut f32,
    clip: Rect,
) {
    let lane = view.selected_lane.unwrap_or(MixLane::Main);
    theme::text(cmds, Rect { x: x + 10.0, y: *y, w: 200.0, h: 18.0 }, lane.title().to_uppercase(), 13.0, theme::PRIMARY_TEXT, true);
    widgets::icon_pad(cmds, Rect { x: x + Layout::SIDEBAR_WIDTH - 34.0, y: *y, w: 22.0, h: 22.0 }, "+", true);
    hits.push((Rect { x: x + Layout::SIDEBAR_WIDTH - 34.0, y: *y, w: 22.0, h: 22.0 }, SidebarHit::AddInsert));
    *y += 24.0;
    theme::text(cmds, Rect { x: x + 10.0, y: *y, w: 120.0, h: 16.0 }, "INSERTS", 11.0, theme::TEXT_DIM, true);
    *y += 20.0;
    let track: Option<&MixTrack> = view.mix.and_then(|m| m.tracks.iter().find(|t| t.lane == lane));
    if let Some(track) = track {
        for insert in &track.inserts {
            if *y > clip.y + clip.h {
                break;
            }
            let card = Rect { x: x + 8.0, y: *y, w: Layout::SIDEBAR_WIDTH - 16.0, h: 72.0 };
            theme::fill(cmds, card, theme::RECESSED.middle);
            theme::text(cmds, Rect { x: card.x + 6.0, y: card.y + 4.0, w: card.w - 32.0, h: 18.0 }, insert.title(), 12.0, theme::TEXT, true);
            widgets::icon_pad(cmds, Rect { x: card.x + card.w - 26.0, y: card.y + 4.0, w: 20.0, h: 20.0 }, "−", true);
            hits.push((Rect { x: card.x + card.w - 26.0, y: card.y + 4.0, w: 20.0, h: 20.0 }, SidebarHit::RemoveInsert(insert.id)));
            widgets::menu_label(
                cmds,
                Rect { x: card.x + 6.0, y: card.y + 24.0, w: card.w - 12.0, h: 20.0 },
                insert.bundle_path.as_deref().and_then(|p| std::path::Path::new(p).file_name().and_then(|s| s.to_str())).unwrap_or("None"),
            );
            hits.push((Rect { x: card.x + 6.0, y: card.y + 24.0, w: card.w - 12.0, h: 20.0 }, SidebarHit::InsertBundle(insert.id)));
            widgets::hardware_pad(cmds, Rect { x: card.x + 6.0, y: card.y + 46.0, w: 56.0, h: 20.0 }, "Edit", false, theme::PRIMARY_TEXT);
            widgets::hardware_pad(cmds, Rect { x: card.x + 66.0, y: card.y + 46.0, w: 64.0, h: 20.0 }, "Bypass", insert.bypassed, theme::AMBER);
            hits.push((Rect { x: card.x + 6.0, y: card.y + 46.0, w: 56.0, h: 20.0 }, SidebarHit::InsertEdit(insert.id)));
            hits.push((Rect { x: card.x + 66.0, y: card.y + 46.0, w: 64.0, h: 20.0 }, SidebarHit::InsertBypass(insert.id)));
            *y += 78.0;
        }
    } else {
        theme::text(cmds, Rect { x: x + 10.0, y: *y, w: 200.0, h: 18.0 }, "Select a channel", 11.0, theme::TEXT_DIM, false);
    }
}

fn section(cmds: &mut Vec<DrawCmd>, x: f32, y: f32, title: &str) {
    theme::text(cmds, Rect { x: x + 10.0, y, w: 200.0, h: 18.0 }, title, 13.0, theme::PRIMARY_TEXT, true);
}

fn row_plus(
    cmds: &mut Vec<DrawCmd>,
    hits: &mut Vec<(Rect, SidebarHit)>,
    x: f32,
    y: f32,
    title: &str,
    add: SidebarHit,
    enabled: bool,
) {
    theme::text(cmds, Rect { x: x + 10.0, y, w: 180.0, h: 18.0 }, title, 11.0, theme::TEXT_DIM, true);
    widgets::icon_pad(cmds, Rect { x: x + Layout::SIDEBAR_WIDTH - 34.0, y, w: 22.0, h: 18.0 }, "+", enabled);
    if enabled {
        hits.push((Rect { x: x + Layout::SIDEBAR_WIDTH - 34.0, y, w: 22.0, h: 18.0 }, add));
    }
}

pub fn hit(hits: &[(Rect, SidebarHit)], x: f32, y: f32) -> Option<SidebarHit> {
    hits.iter().rev().find(|(r, _)| widgets::contains(*r, x, y)).map(|(_, h)| h.clone())
}
