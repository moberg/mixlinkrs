//! Record EFFECTS/SETTINGS sidebar and Mix INSERTS column. 248 pt.

use analog::{AnalogEngine, HardwareEffect, PluginSlot, ReturnLane};
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

const FOOT_H: f32 = 40.0;

pub fn body_rect(window_w: f32, window_h: f32, page: Page) -> Rect {
    let x = window_w - Layout::SIDEBAR_WIDTH;
    let y = crate::chrome::HEADER_H;
    let sh = window_h - crate::chrome::HEADER_H - crate::chrome::footer_height(page);
    Rect { x, y, w: Layout::SIDEBAR_WIDTH, h: (sh - FOOT_H).max(0.0) }
}

pub fn max_scroll(view: &SidebarView<'_>, window_h: f32) -> f32 {
    let body_h = body_rect(0.0, window_h, view.page).h;
    let content = match view.page {
        Page::Record => record_content_h(view),
        Page::Mix => mix_content_h(view),
    };
    (content - body_h).max(0.0)
}

pub fn paint(view: &SidebarView<'_>, w: f32, h: f32) -> (Vec<DrawCmd>, Vec<(Rect, SidebarHit)>) {
    let mut cmds = Vec::new();
    let mut hits = Vec::new();
    let x = w - Layout::SIDEBAR_WIDTH;
    let y = crate::chrome::HEADER_H;
    let sh = h - crate::chrome::HEADER_H - crate::chrome::footer_height(view.page);
    cmds.push(DrawCmd::Layer);
    theme::hardware_surface(
        &mut cmds,
        Rect { x, y, w: Layout::SIDEBAR_WIDTH, h: sh },
        theme::SurfaceStyle::Sidebar,
    );
    theme::seam_v(&mut cmds, x, y, sh, true);

    let clip = body_rect(w, h, view.page);
    let max_scroll = max_scroll(view, h);
    let scroll = view.scroll.clamp(0.0, max_scroll);

    cmds.push(DrawCmd::Layer);
    cmds.push(DrawCmd::Clip { rect: clip });
    let mut yy = clip.y + 8.0 - scroll;
    if view.page == Page::Record {
        paint_record(view, &mut cmds, &mut hits, x, &mut yy, clip);
    } else {
        paint_mix(view, &mut cmds, &mut hits, x, &mut yy, clip);
    }
    hits.retain(|(r, _)| rects_overlap(*r, clip));

    cmds.push(DrawCmd::Layer);
    let by = y + sh - 36.0;
    theme::hardware_surface(
        &mut cmds,
        Rect { x, y: by - 4.0, w: Layout::SIDEBAR_WIDTH, h: FOOT_H },
        theme::SurfaceStyle::Sidebar,
    );
    theme::seam_h(&mut cmds, x, by - 4.0, Layout::SIDEBAR_WIDTH, false);
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

    if max_scroll > 0.5 {
        widgets::scrollbar(
            &mut cmds,
            Rect {
                x: x + Layout::SIDEBAR_WIDTH - 8.0,
                y: clip.y + 4.0,
                w: 5.0,
                h: (clip.h - 8.0).max(8.0),
            },
            scroll,
            max_scroll,
        );
    }
    (cmds, hits)
}

fn rects_overlap(a: Rect, b: Rect) -> bool {
    a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y
}

fn record_content_h(view: &SidebarView<'_>) -> f32 {
    const GAP: f32 = 6.0;
    const OUTER: f32 = 10.0;
    const HEAD_GAP: f32 = 12.0;
    let n_hw = view.engine.config.hardware_effects.len() as f32;
    let n_plug = view.engine.config.plugins.len() as f32;
    let mut h = 8.0;
    h += 18.0 + HEAD_GAP;
    h += Layout::HEADER_BUTTON + GAP;
    h += n_hw * (hardware_card_h() + GAP);
    h += OUTER - GAP;
    h += Layout::HEADER_BUTTON + GAP;
    h += n_plug * (plugin_card_h() + GAP);
    h += OUTER - GAP;
    h += 16.0 + 8.0;
    h += settings_content_h(view);
    h += 8.0;
    h
}

fn settings_content_h(view: &SidebarView<'_>) -> f32 {
    const GROUP: f32 = 6.0;
    let mut h = LABEL_H + GROUP + MENU_H + GROUP;
    if !view.project_suffix.is_empty() || !view.project_date.is_empty() {
        h += LABEL_H + GROUP + MENU_H + GROUP;
    }
    h += NEW_PROJECT_H + GROUP;
    h += LABEL_H + PICKER_GAP + MENU_H + GROUP;
    h += LABEL_H + PICKER_GAP + MENU_H + 4.0;
    h += 16.0;
    h
}

fn mix_content_h(view: &SidebarView<'_>) -> f32 {
    let insert_h = Layout::MODULE_PAD
        + Layout::HEADER_BUTTON
        + CARD_GAP
        + MENU_H
        + CARD_GAP
        + PLUGIN_PAD_H
        + Layout::MODULE_PAD;
    let lane = view.selected_lane.unwrap_or(MixLane::Main);
    let n = view
        .mix
        .and_then(|m| m.tracks.iter().find(|t| t.lane == lane))
        .map(|t| t.inserts.len())
        .unwrap_or(0) as f32;
    let mut h = 8.0;
    h += Layout::HEADER_BUTTON + CARD_GAP;
    h += 16.0 + CARD_GAP;
    if n > 0.0 {
        h += n * (insert_h + CARD_GAP);
    } else {
        h += 18.0;
    }
    h += 8.0;
    h
}

fn paint_record(
    view: &SidebarView<'_>,
    cmds: &mut Vec<DrawCmd>,
    hits: &mut Vec<(Rect, SidebarHit)>,
    x: f32,
    y: &mut f32,
    clip: Rect,
) {
    // MixLink SidebarView: VStack spacing 10, .padding(8).
    // Header→first content is MixLink 10 (EFFECTS) / 6 (SETTINGS); +2 so
    // the title is not flush against the first row.
    const SIDE: f32 = 8.0;
    const GAP: f32 = 6.0;
    const OUTER: f32 = 10.0;
    const HEAD_GAP: f32 = 12.0;
    let inner_x = x + SIDE;
    let inner_w = Layout::SIDEBAR_WIDTH - SIDE * 2.0;

    // MixLink EFFECTS: 13 semibold MixerTheme.primaryText
    theme::text(
        cmds,
        Rect { x: inner_x, y: *y, w: inner_w, h: 18.0 },
        "EFFECTS",
        13.0,
        theme::PRIMARY_TEXT,
        true,
    );
    *y += 18.0 + HEAD_GAP;

    row_plus(cmds, hits, x, *y, "Hardware effects", SidebarHit::AddHardware, true);
    *y += Layout::HEADER_BUTTON + GAP;
    for hw in &view.engine.config.hardware_effects {
        if *y > clip.y + clip.h {
            break;
        }
        paint_hardware_card(view, cmds, hits, inner_x, *y, inner_w, hw);
        *y += hardware_card_h() + GAP;
    }

    let can_add_plugin = view.engine.config.next_free_plugin_id().is_some();
    *y += OUTER - GAP;
    row_plus(cmds, hits, x, *y, "Plugins", SidebarHit::AddPlugin, can_add_plugin);
    *y += Layout::HEADER_BUTTON + GAP;
    for plug in &view.engine.config.plugins {
        if *y > clip.y + clip.h {
            break;
        }
        paint_plugin_card(view, cmds, hits, inner_x, *y, inner_w, plug);
        *y += plugin_card_h() + GAP;
    }

    if *y > clip.y + clip.h {
        return;
    }
    *y += OUTER - GAP;
    // MixLink SETTINGS: 12 semibold MixerTheme.secondaryText, VStack spacing 6.
    theme::text(
        cmds,
        Rect { x: inner_x, y: *y, w: inner_w, h: 16.0 },
        "SETTINGS",
        12.0,
        theme::SECONDARY_TEXT,
        true,
    );
    *y += 16.0 + 8.0;
    paint_settings(view, cmds, hits, inner_x, y, inner_w);
    let _ = ReturnLane::SendA;
}

// MixLink `HardwareEffectCard` / `PluginSlotView`:
//   VStack(alignment: .leading, spacing: 6) {
//     HStack(spacing: 6) { NameField; HardwareIconButton("minus") }
//     ChannelPicker(title: "Output"|"Input", ...)          // hardware
//     Menu { MenuLabel(plugin) }                           // plugin
//     HStack(spacing: 6) { SmallButton("Edit"); SmallButton("Bypass") }
//     HStack(spacing: 8) { Text("Software playback"); ChannelPicker(title: nil) }
//   }
//   .hardwareModule()   // padding 7, recessed card, thin light rim
// MixLink `ChannelPicker`: VStack(spacing: 4) { Text(11 medium textDim); Menu { MenuLabel } }
//   .menuStyle(.borderlessButton) .menuIndicator(.hidden)
// MixLink `MenuLabel` closed: HStack(spacing: 3) { Text; chevron.up.chevron.down }
//   .padding(.vertical, 5) .padding(.horizontal, 6) — screenshot Heat/EffectRack
//   drops the recessed well, leading chevron, no diamond.
// Sidebar EDIT/BYPASS stay MixLink compact 26 (`Layout::BUTTON_H`); mixer SOLO unchanged.
// NEW PROJECT is MixLink compact full-width; 27 is slightly larger than 26.
const CARD_GAP: f32 = 6.0;
const PICKER_GAP: f32 = 4.0;
const LABEL_H: f32 = 16.0;
const MENU_H: f32 = 24.0;
const PLUGIN_PAD_H: f32 = Layout::BUTTON_H;
const NEW_PROJECT_H: f32 = Layout::COMPACT_BUTTON_H;

fn hardware_card_h() -> f32 {
    Layout::MODULE_PAD
        + Layout::HEADER_BUTTON
        + CARD_GAP
        + LABEL_H
        + PICKER_GAP
        + MENU_H
        + CARD_GAP
        + LABEL_H
        + PICKER_GAP
        + MENU_H
        + Layout::MODULE_PAD
}

fn plugin_card_h() -> f32 {
    Layout::MODULE_PAD
        + Layout::HEADER_BUTTON
        + CARD_GAP
        + MENU_H
        + CARD_GAP
        + PLUGIN_PAD_H
        + CARD_GAP
        + MENU_H
        + Layout::MODULE_PAD
}

fn paint_hardware_card(
    view: &SidebarView<'_>,
    cmds: &mut Vec<DrawCmd>,
    hits: &mut Vec<(Rect, SidebarHit)>,
    x: f32,
    y: f32,
    w: f32,
    hw: &HardwareEffect,
) {
    let card = Rect { x, y, w, h: hardware_card_h() };
    widgets::hardware_module(cmds, card);
    let mut cy = card.y + Layout::MODULE_PAD;
    let cx = card.x + Layout::MODULE_PAD;
    let cw = card.w - Layout::MODULE_PAD * 2.0;
    name_row(
        cmds,
        hits,
        cx,
        cy,
        cw,
        if hw.name.is_empty() { "Name" } else { &hw.name },
        hw.name.is_empty(),
        *view.focus == TextFocus::HardwareName(hw.id),
        view.caret,
        SidebarHit::EditHardwareName(hw.id),
        SidebarHit::RemoveHardware(hw.id),
    );
    cy += Layout::HEADER_BUTTON + CARD_GAP;
    picker_stack(
        cmds,
        hits,
        cx,
        cy,
        cw,
        "Output",
        &view.engine.output_name(hw.output),
        SidebarHit::HardwareOutput(hw.id),
    );
    cy += LABEL_H + PICKER_GAP + MENU_H + CARD_GAP;
    picker_stack(
        cmds,
        hits,
        cx,
        cy,
        cw,
        "Input",
        &view.engine.display_name(analog::ChannelID::new(analog::MixerBus::Input, hw.input)),
        SidebarHit::HardwareInput(hw.id),
    );
}

fn paint_plugin_card(
    view: &SidebarView<'_>,
    cmds: &mut Vec<DrawCmd>,
    hits: &mut Vec<(Rect, SidebarHit)>,
    x: f32,
    y: f32,
    w: f32,
    plug: &PluginSlot,
) {
    let card = Rect { x, y, w, h: plugin_card_h() };
    widgets::hardware_module(cmds, card);
    let mut cy = card.y + Layout::MODULE_PAD;
    let cx = card.x + Layout::MODULE_PAD;
    let cw = card.w - Layout::MODULE_PAD * 2.0;
    name_row(
        cmds,
        hits,
        cx,
        cy,
        cw,
        if plug.name.is_empty() { "Name" } else { &plug.name },
        plug.name.is_empty(),
        *view.focus == TextFocus::PluginName(plug.id),
        view.caret,
        SidebarHit::EditPluginName(plug.id),
        SidebarHit::RemovePlugin(plug.id),
    );
    cy += Layout::HEADER_BUTTON + CARD_GAP;
    let bundle = plug
        .bundle_path
        .as_deref()
        .and_then(|p| std::path::Path::new(p).file_stem().and_then(|s| s.to_str()))
        .unwrap_or("No plugin");
    let bundle_row = Rect { x: cx, y: cy, w: cw, h: MENU_H };
    widgets::channel_picker(cmds, bundle_row, bundle, widgets::ChannelPickerStyle::plugin());
    hits.push((bundle_row, SidebarHit::PluginBundle(plug.id)));
    cy += MENU_H + CARD_GAP;
    let btn_gap = CARD_GAP;
    let btn_w = (cw - btn_gap) * 0.5;
    let btn_h = PLUGIN_PAD_H;
    widgets::hardware_pad(
        cmds,
        Rect { x: cx, y: cy, w: btn_w, h: btn_h },
        "Edit",
        false,
        theme::PRIMARY_TEXT,
    );
    widgets::hardware_pad(
        cmds,
        Rect { x: cx + btn_w + btn_gap, y: cy, w: btn_w, h: btn_h },
        "Bypass",
        plug.bypassed,
        theme::AMBER,
    );
    hits.push((Rect { x: cx, y: cy, w: btn_w, h: btn_h }, SidebarHit::PluginEdit(plug.id)));
    hits.push((
        Rect { x: cx + btn_w + btn_gap, y: cy, w: btn_w, h: btn_h },
        SidebarHit::PluginBypass(plug.id),
    ));
    cy += btn_h + CARD_GAP;
    // MixLink PluginSlotView: HStack spacing 8, 11 medium textDim + ChannelPicker(title: nil).
    const PLAYBACK_LABEL_W: f32 = 108.0;
    const PLAYBACK_GAP: f32 = 8.0;
    theme::text(
        cmds,
        Rect { x: cx, y: cy, w: PLAYBACK_LABEL_W, h: MENU_H },
        "Software playback",
        11.0,
        theme::TEXT_DIM,
        false,
    );
    let menu_x = cx + PLAYBACK_LABEL_W + PLAYBACK_GAP;
    let menu_w = (cx + cw - menu_x).max(36.0);
    let playback = Rect { x: menu_x, y: cy, w: menu_w, h: MENU_H };
    widgets::channel_picker(
        cmds,
        playback,
        &format!("{}/{}", plug.return_channel + 1, plug.return_channel + 2),
        widgets::ChannelPickerStyle::playback(),
    );
    hits.push((playback, SidebarHit::PluginPlayback(plug.id)));
}

fn paint_settings(
    view: &SidebarView<'_>,
    cmds: &mut Vec<DrawCmd>,
    hits: &mut Vec<(Rect, SidebarHit)>,
    x: f32,
    y: &mut f32,
    w: f32,
) {
    const GROUP: f32 = 6.0;
    field_label(cmds, x, *y, w, "Projects folder");
    *y += LABEL_H + GROUP;
    let folder = Rect { x, y: *y, w, h: MENU_H };
    widgets::channel_picker(cmds, folder, view.project_name, widgets::ChannelPickerStyle::value());
    hits.push((folder, SidebarHit::ProjectsFolder));
    *y += MENU_H + GROUP;
    if !view.project_suffix.is_empty() || !view.project_date.is_empty() {
        field_label(cmds, x, *y, w, "Project");
        *y += LABEL_H + GROUP;
        let row = Rect { x, y: *y, w, h: MENU_H };
        widgets::recessed_field(cmds, row);
        // MixLink project well is `.padding(.horizontal, 6)`; 8 matches MenuLabel.
        let date = format!("{} -", view.project_date);
        theme::text(
            cmds,
            Rect { x: x + 8.0, y: *y, w: 90.0, h: MENU_H },
            date,
            12.0,
            theme::TEXT_DIM,
            false,
        );
        let name = if *view.focus == TextFocus::ProjectName && view.caret {
            format!("{}|", view.project_suffix)
        } else {
            view.project_suffix.to_string()
        };
        theme::text(
            cmds,
            Rect { x: x + 98.0, y: *y, w: w - 106.0, h: MENU_H },
            name,
            12.0,
            theme::PRIMARY_TEXT,
            false,
        );
        hits.push((Rect { x: x + 98.0, y: *y, w: w - 106.0, h: MENU_H }, SidebarHit::ProjectName));
        *y += MENU_H + GROUP;
    }
    widgets::hardware_pad(
        cmds,
        Rect { x, y: *y, w, h: NEW_PROJECT_H },
        "New project",
        false,
        theme::PRIMARY_TEXT,
    );
    hits.push((Rect { x, y: *y, w, h: NEW_PROJECT_H }, SidebarHit::NewProject));
    *y += NEW_PROJECT_H + GROUP;
    picker_stack(
        cmds,
        hits,
        x,
        *y,
        w,
        "Mix (Main Out)",
        &view.engine.output_name(view.engine.config.main_output),
        SidebarHit::MixOut,
    );
    *y += LABEL_H + PICKER_GAP + MENU_H + GROUP;
    picker_stack(cmds, hits, x, *y, w, "Audio Device", view.device_name, SidebarHit::AudioDevice);
    *y += LABEL_H + PICKER_GAP + MENU_H + 4.0;
    theme::text(
        cmds,
        Rect { x, y: *y, w, h: 16.0 },
        format!(
            "{} Hz · {} frames · {:.1} ms",
            view.sample_rate, view.buffer_frames, view.latency_ms
        ),
        10.0,
        theme::TEXT_DIM,
        false,
    );
    hits.push((Rect { x, y: *y, w, h: 16.0 }, SidebarHit::AudioBuffer));
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
    let plus = Rect {
        x: x + Layout::SIDEBAR_WIDTH - 8.0 - Layout::HEADER_BUTTON,
        y: *y,
        w: Layout::HEADER_BUTTON,
        h: Layout::HEADER_BUTTON,
    };
    theme::text(
        cmds,
        Rect { x: x + 8.0, y: *y, w: 180.0, h: Layout::HEADER_BUTTON },
        lane.title().to_uppercase(),
        13.0,
        theme::PRIMARY_TEXT,
        true,
    );
    widgets::icon_pad(cmds, plus, "+", true);
    hits.push((plus, SidebarHit::AddInsert));
    *y += Layout::HEADER_BUTTON + CARD_GAP;
    theme::text(
        cmds,
        Rect { x: x + 8.0, y: *y, w: 120.0, h: 16.0 },
        "INSERTS",
        11.0,
        theme::SECONDARY_TEXT,
        true,
    );
    *y += 16.0 + CARD_GAP;
    let track: Option<&MixTrack> = view.mix.and_then(|m| m.tracks.iter().find(|t| t.lane == lane));
    if let Some(track) = track {
        let insert_h = Layout::MODULE_PAD
            + Layout::HEADER_BUTTON
            + CARD_GAP
            + MENU_H
            + CARD_GAP
            + PLUGIN_PAD_H
            + Layout::MODULE_PAD;
        for insert in &track.inserts {
            if *y > clip.y + clip.h {
                break;
            }
            let card = Rect { x: x + 8.0, y: *y, w: Layout::SIDEBAR_WIDTH - 16.0, h: insert_h };
            widgets::hardware_module(cmds, card);
            let cx = card.x + Layout::MODULE_PAD;
            let cy0 = card.y + Layout::MODULE_PAD;
            let cw = card.w - Layout::MODULE_PAD * 2.0;
            let icon = Rect {
                x: cx + cw - Layout::HEADER_BUTTON,
                y: cy0,
                w: Layout::HEADER_BUTTON,
                h: Layout::HEADER_BUTTON,
            };
            theme::text(
                cmds,
                Rect {
                    x: cx,
                    y: cy0,
                    w: cw - Layout::HEADER_BUTTON - CARD_GAP,
                    h: Layout::HEADER_BUTTON,
                },
                insert.title(),
                12.0,
                theme::PRIMARY_TEXT,
                true,
            );
            widgets::icon_pad(cmds, icon, "−", true);
            hits.push((icon, SidebarHit::RemoveInsert(insert.id)));
            let menu = Rect { x: cx, y: cy0 + Layout::HEADER_BUTTON + CARD_GAP, w: cw, h: MENU_H };
            widgets::channel_picker(
                cmds,
                menu,
                insert
                    .bundle_path
                    .as_deref()
                    .and_then(|p| std::path::Path::new(p).file_name().and_then(|s| s.to_str()))
                    .unwrap_or("No plugin"),
                widgets::ChannelPickerStyle::plugin(),
            );
            hits.push((menu, SidebarHit::InsertBundle(insert.id)));
            let by = menu.y + MENU_H + CARD_GAP;
            let btn_w = (cw - CARD_GAP) * 0.5;
            let btn_h = PLUGIN_PAD_H;
            widgets::hardware_pad(
                cmds,
                Rect { x: cx, y: by, w: btn_w, h: btn_h },
                "Edit",
                false,
                theme::PRIMARY_TEXT,
            );
            widgets::hardware_pad(
                cmds,
                Rect { x: cx + btn_w + CARD_GAP, y: by, w: btn_w, h: btn_h },
                "Bypass",
                insert.bypassed,
                theme::AMBER,
            );
            hits.push((
                Rect { x: cx, y: by, w: btn_w, h: btn_h },
                SidebarHit::InsertEdit(insert.id),
            ));
            hits.push((
                Rect { x: cx + btn_w + CARD_GAP, y: by, w: btn_w, h: btn_h },
                SidebarHit::InsertBypass(insert.id),
            ));
            *y += insert_h + CARD_GAP;
        }
    } else {
        theme::text(
            cmds,
            Rect { x: x + 8.0, y: *y, w: 200.0, h: 18.0 },
            "Select a channel",
            11.0,
            theme::TEXT_DIM,
            false,
        );
    }
}

fn name_row(
    cmds: &mut Vec<DrawCmd>,
    hits: &mut Vec<(Rect, SidebarHit)>,
    x: f32,
    y: f32,
    w: f32,
    title: &str,
    placeholder: bool,
    focused: bool,
    caret: bool,
    edit: SidebarHit,
    remove: SidebarHit,
) {
    let icon = Rect {
        x: x + w - Layout::HEADER_BUTTON,
        y,
        w: Layout::HEADER_BUTTON,
        h: Layout::HEADER_BUTTON,
    };
    let name = Rect { x, y, w: w - Layout::HEADER_BUTTON - CARD_GAP, h: Layout::HEADER_BUTTON };
    let shown = if focused && caret { format!("{title}|") } else { title.to_string() };
    theme::text(
        cmds,
        name,
        shown,
        12.0,
        if placeholder { theme::TEXT_DIM } else { theme::PRIMARY_TEXT },
        true,
    );
    widgets::icon_pad(cmds, icon, "−", true);
    hits.push((name, edit));
    hits.push((icon, remove));
}

fn picker_stack(
    cmds: &mut Vec<DrawCmd>,
    hits: &mut Vec<(Rect, SidebarHit)>,
    x: f32,
    y: f32,
    w: f32,
    label: &str,
    value: &str,
    hit: SidebarHit,
) {
    field_label(cmds, x, y, w, label);
    let menu = Rect { x, y: y + LABEL_H + PICKER_GAP, w, h: MENU_H };
    widgets::channel_picker(cmds, menu, value, widgets::ChannelPickerStyle::value());
    hits.push((menu, hit));
}

fn field_label(cmds: &mut Vec<DrawCmd>, x: f32, y: f32, w: f32, label: &str) {
    theme::text(cmds, Rect { x, y, w, h: LABEL_H }, label, 11.0, theme::TEXT_DIM, false);
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
    // MixLink effectSection: 11 semibold MixerTheme.secondaryText, headerMini 22.
    theme::text(
        cmds,
        Rect { x: x + 8.0, y, w: 180.0, h: Layout::HEADER_BUTTON },
        title,
        11.0,
        theme::SECONDARY_TEXT,
        true,
    );
    let plus = Rect {
        x: x + Layout::SIDEBAR_WIDTH - 8.0 - Layout::HEADER_BUTTON,
        y,
        w: Layout::HEADER_BUTTON,
        h: Layout::HEADER_BUTTON,
    };
    widgets::icon_pad(cmds, plus, "+", enabled);
    if enabled {
        hits.push((plus, add));
    }
}

pub fn hit(hits: &[(Rect, SidebarHit)], x: f32, y: f32) -> Option<SidebarHit> {
    hits.iter().rev().find(|(r, _)| widgets::contains(*r, x, y)).map(|(_, h)| h.clone())
}
