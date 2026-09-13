//! Record EFFECTS/SETTINGS sidebar and Mix INSERTS column. 248 pt.

use analog::{AnalogEngine, ChainKind, ReturnLane};
use project::{MixDocument, MixLane};
use render::{DrawCmd, Rect};

use crate::chrome::Page;
use crate::overlay::TextFocus;
use crate::theme::{self, Layout};
use crate::widgets;

pub struct SidebarView<'a> {
    pub page: Page,
    pub engine: &'a AnalogEngine,
    pub sample_rate: u32,
    pub buffer_frames: u32,
    pub latency_ms: f32,
    pub device_name: &'a str,
    pub mix: Option<&'a MixDocument>,
    pub selected_lane: Option<MixLane>,
    pub scroll: f32,
    pub focus: &'a TextFocus,
    pub caret: bool,
    pub thumbs: &'a std::collections::HashSet<uuid::Uuid>,
}

#[derive(Clone, Debug)]
pub enum SidebarHit {
    OpenChains,
    AssignReturn { lane: ReturnLane },
    AssignPlayback { id: uuid::Uuid },
    OpenPlugin { id: uuid::Uuid },
    AssignMix,
    ClearMixChain,
    ToggleMixHardware,
    AudioDevice,
    AudioBuffer,
    Channels,
    Settings,
}

const FOOT_H: f32 = 40.0;

pub fn body_rect(window_w: f32, window_h: f32, page: Page) -> Rect {
    let x = window_w - Layout::SIDEBAR_WIDTH;
    let y = crate::chrome::HEADER_H;
    let sh = window_h - crate::chrome::HEADER_H - crate::chrome::footer_height(page);
    let dock = if page == Page::Record { settings_dock_h() } else { 0.0 };
    Rect { x, y, w: Layout::SIDEBAR_WIDTH, h: (sh - FOOT_H - dock).max(0.0) }
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
    if view.page == Page::Record {
        paint_settings_dock(view, &mut cmds, &mut hits, x, y + sh - FOOT_H - settings_dock_h());
    }
    let by = y + sh - 36.0;
    theme::hardware_surface(
        &mut cmds,
        Rect { x, y: by - 4.0, w: Layout::SIDEBAR_WIDTH, h: FOOT_H },
        theme::SurfaceStyle::Sidebar,
    );
    theme::seam_h(&mut cmds, x, by - 4.0, Layout::SIDEBAR_WIDTH, false);
    widgets::hardware_pad(
        &mut cmds,
        Rect { x: x + 56.0, y: by, w: 72.0, h: 26.0 },
        "Effects",
        false,
        theme::PRIMARY_TEXT,
    );
    widgets::hardware_pad(
        &mut cmds,
        Rect { x: x + 136.0, y: by, w: 72.0, h: 26.0 },
        "Channels",
        false,
        theme::PRIMARY_TEXT,
    );
    widgets::icon_pad(&mut cmds, Rect { x: x + 214.0, y: by, w: 26.0, h: 26.0 }, "⚙", true);
    hits.push((Rect { x: x + 56.0, y: by, w: 72.0, h: 26.0 }, SidebarHit::OpenChains));
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
    const GAP: f32 = 8.0;
    const HEAD_GAP: f32 = 12.0;
    let mut h = 8.0;
    h += 18.0 + HEAD_GAP;
    for lane in record_lanes(view) {
        let chain = view.engine.config.chain_ref(lane);
        h += assign_row_h(
            is_plugin_assign(chain),
            &plugin_stages(&view.engine.config, chain),
            false,
        ) + GAP;
    }
    h += Layout::HEADER_BUTTON + GAP;
    h += 8.0;
    h
}

fn settings_heading_h() -> f32 {
    16.0 + 8.0
}

fn settings_content_h() -> f32 {
    LABEL_H + PICKER_GAP + MENU_H + 4.0 + 16.0
}

fn settings_dock_h() -> f32 {
    8.0 + settings_heading_h() + settings_content_h() + 8.0
}

fn mix_content_h(view: &SidebarView<'_>) -> f32 {
    let mut h = 8.0;
    h += 18.0 + 12.0;
    if let Some(lane) = view.selected_lane {
        let chain = view.mix.and_then(|m| m.track(lane)).and_then(|t| t.effect_chain);
        h += assign_row_h(false, &plugin_stages(&view.engine.config, chain), chain.is_some())
            + CARD_GAP;
        if chain.is_some_and(|c| c.kind == ChainKind::Hardware) {
            h += Layout::HEADER_BUTTON + CARD_GAP;
        }
        h += Layout::HEADER_BUTTON;
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
    const SIDE: f32 = 8.0;
    const GAP: f32 = 8.0;
    const HEAD_GAP: f32 = 12.0;
    let inner_x = x + SIDE;
    let inner_w = Layout::SIDEBAR_WIDTH - SIDE * 2.0;

    theme::text(
        cmds,
        Rect { x: inner_x, y: *y, w: inner_w, h: 18.0 },
        "EFFECTS",
        13.0,
        theme::PRIMARY_TEXT,
        true,
    );
    *y += 18.0 + HEAD_GAP;

    for lane in record_lanes(view) {
        if *y > clip.y + clip.h {
            break;
        }
        let chain = view.engine.config.chain_ref(lane);
        paint_assign_row(
            cmds,
            hits,
            inner_x,
            *y,
            inner_w,
            lane.title(),
            chain,
            &view.engine.config,
            SidebarHit::AssignReturn { lane },
            None,
            true,
            view.thumbs,
        );
        *y += assign_row_h(
            is_plugin_assign(chain),
            &plugin_stages(&view.engine.config, chain),
            false,
        ) + GAP;
    }

    let open = Rect { x: inner_x, y: *y, w: inner_w, h: Layout::HEADER_BUTTON };
    widgets::hardware_pad(cmds, open, "Open Effects", false, theme::PRIMARY_TEXT);
    hits.push((open, SidebarHit::OpenChains));
    *y += Layout::HEADER_BUTTON + GAP;
}

const CARD_GAP: f32 = 6.0;
const PICKER_GAP: f32 = 4.0;
const LABEL_H: f32 = 16.0;
const MENU_H: f32 = 24.0;
const STAGE_GAP: f32 = 6.0;
const THUMB_W: f32 = 112.0;
const THUMB_H: f32 = 63.0;

fn record_lanes(view: &SidebarView<'_>) -> Vec<ReturnLane> {
    let mut lanes = view.engine.config.visible_send_lanes();
    lanes.extend(analog::BUS_LANES);
    lanes
}

fn stage_row_h(stage: &analog::PluginStage) -> f32 {
    if stage.is_loaded() {
        THUMB_H + STAGE_GAP
    } else {
        MENU_H + STAGE_GAP
    }
}

fn assign_inner_h(playback: bool, stages: &[analog::PluginStage], clear: bool) -> f32 {
    let mut h = LABEL_H + PICKER_GAP + MENU_H;
    if playback {
        h += 8.0 + LABEL_H + PICKER_GAP + MENU_H;
    }
    if !stages.is_empty() {
        h += 8.0 + stages.iter().map(stage_row_h).sum::<f32>();
    }
    if clear {
        h += 14.0;
    } else {
        h += 8.0;
    }
    h
}

fn assign_row_h(playback: bool, stages: &[analog::PluginStage], clear: bool) -> f32 {
    Layout::MODULE_PAD * 2.0 + assign_inner_h(playback, stages, clear)
}

fn is_plugin_assign(chain: Option<analog::ChainRef>) -> bool {
    chain.is_some_and(|r| r.kind == ChainKind::Plugin)
}

fn plugin_stages(
    config: &analog::SessionConfig,
    chain: Option<analog::ChainRef>,
) -> &[analog::PluginStage] {
    chain
        .filter(|r| r.kind == ChainKind::Plugin)
        .and_then(|r| config.plugin_chain(r.id))
        .map(|c| c.stages.as_slice())
        .unwrap_or(&[])
}

fn stage_label(stage: &analog::PluginStage) -> String {
    if !stage.is_loaded() {
        return "No plugin".into();
    }
    if !stage.name.is_empty() {
        return stage.title();
    }
    stage
        .bundle_path
        .as_deref()
        .and_then(|p| std::path::Path::new(p).file_stem().and_then(|s| s.to_str()))
        .unwrap_or("No plugin")
        .to_string()
}

fn paint_assign_row(
    cmds: &mut Vec<DrawCmd>,
    hits: &mut Vec<(Rect, SidebarHit)>,
    x: f32,
    y: f32,
    w: f32,
    title: &str,
    chain: Option<analog::ChainRef>,
    config: &analog::SessionConfig,
    assign: SidebarHit,
    clear: Option<SidebarHit>,
    show_playback: bool,
    thumbs: &std::collections::HashSet<uuid::Uuid>,
) {
    let (label, kind) = match chain {
        Some(r) => {
            let name = config.chain_title(r);
            let kind = match r.kind {
                ChainKind::Hardware => "Hardware",
                ChainKind::Plugin => "Plugin",
            };
            (name, kind)
        }
        None => ("No effect".into(), ""),
    };
    let playback = show_playback && is_plugin_assign(chain);
    let stages = plugin_stages(config, chain);
    let card_h = assign_row_h(playback, stages, clear.is_some() && chain.is_some());
    widgets::hardware_module(cmds, Rect { x, y, w, h: card_h });
    let x = x + Layout::MODULE_PAD;
    let y = y + Layout::MODULE_PAD;
    let w = w - Layout::MODULE_PAD * 2.0;
    theme::text(cmds, Rect { x, y, w: w - 48.0, h: LABEL_H }, title, 11.0, theme::TEXT_DIM, false);
    if !kind.is_empty() {
        theme::text(
            cmds,
            Rect { x: x + w - 64.0, y, w: 64.0, h: LABEL_H },
            kind,
            10.0,
            theme::SECONDARY_TEXT,
            false,
        );
    }
    let menu = Rect { x, y: y + LABEL_H + PICKER_GAP, w, h: MENU_H };
    widgets::channel_picker(cmds, menu, &label, widgets::ChannelPickerStyle::value());
    hits.push((menu, assign));
    let mut extra_y = y + LABEL_H + PICKER_GAP + MENU_H + 8.0;
    if show_playback {
        if let Some(r) = chain.filter(|r| r.kind == ChainKind::Plugin) {
            if let Some(pc) = config.plugin_chain(r.id) {
                theme::text(
                    cmds,
                    Rect { x, y: extra_y, w, h: LABEL_H },
                    "Software playback",
                    11.0,
                    theme::TEXT_DIM,
                    false,
                );
                extra_y += LABEL_H + PICKER_GAP;
                let pb = Rect { x, y: extra_y, w, h: MENU_H };
                widgets::channel_picker(
                    cmds,
                    pb,
                    &analog::SessionConfig::playback_pair_label(pc.return_channel),
                    widgets::ChannelPickerStyle::playback(),
                );
                hits.push((pb, SidebarHit::AssignPlayback { id: pc.id }));
                extra_y += MENU_H + 8.0;
            }
        }
    }
    for stage in stages {
        let loaded = stage.is_loaded();
        let row_h = if loaded { THUMB_H } else { MENU_H };
        theme::text(
            cmds,
            Rect {
                x,
                y: extra_y,
                w: if loaded { (w - THUMB_W - 6.0).max(0.0) } else { w },
                h: row_h,
            },
            stage_label(stage),
            11.0,
            if loaded { theme::PRIMARY_TEXT } else { theme::TEXT_DIM },
            false,
        );
        if loaded {
            let thumb = Rect { x: x + w - THUMB_W, y: extra_y, w: THUMB_W, h: THUMB_H };
            widgets::plugin_thumb(cmds, thumb, stage.id.as_u128(), thumbs.contains(&stage.id));
            hits.push((thumb, SidebarHit::OpenPlugin { id: stage.id }));
        }
        extra_y += stage_row_h(stage);
    }
    if let Some(clear) = clear {
        if chain.is_some() {
            let minus = Rect {
                x: x + w - Layout::HEADER_BUTTON,
                y: extra_y,
                w: Layout::HEADER_BUTTON,
                h: 12.0,
            };
            theme::text(cmds, minus, "Clear", 10.0, theme::TEXT_DIM, false);
            hits.push((minus, clear));
        }
    }
}

fn paint_settings_dock(
    view: &SidebarView<'_>,
    cmds: &mut Vec<DrawCmd>,
    hits: &mut Vec<(Rect, SidebarHit)>,
    x: f32,
    y: f32,
) {
    const SIDE: f32 = 8.0;
    let inner_x = x + SIDE;
    let inner_w = Layout::SIDEBAR_WIDTH - SIDE * 2.0;
    theme::hardware_surface(
        cmds,
        Rect { x, y, w: Layout::SIDEBAR_WIDTH, h: settings_dock_h() },
        theme::SurfaceStyle::Sidebar,
    );
    theme::seam_h(cmds, x, y, Layout::SIDEBAR_WIDTH, false);
    let mut yy = y + 8.0;
    theme::text(
        cmds,
        Rect { x: inner_x, y: yy, w: inner_w, h: 16.0 },
        "SETTINGS",
        12.0,
        theme::SECONDARY_TEXT,
        true,
    );
    yy += settings_heading_h();
    paint_settings(view, cmds, hits, inner_x, &mut yy, inner_w);
}

fn paint_settings(
    view: &SidebarView<'_>,
    cmds: &mut Vec<DrawCmd>,
    hits: &mut Vec<(Rect, SidebarHit)>,
    x: f32,
    y: &mut f32,
    w: f32,
) {
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
    _clip: Rect,
) {
    const SIDE: f32 = 8.0;
    let inner_x = x + SIDE;
    let inner_w = Layout::SIDEBAR_WIDTH - SIDE * 2.0;
    let Some(lane) = view.selected_lane else {
        theme::text(
            cmds,
            Rect { x: inner_x, y: *y, w: inner_w, h: 18.0 },
            "Select a channel",
            11.0,
            theme::TEXT_DIM,
            false,
        );
        return;
    };
    theme::text(
        cmds,
        Rect { x: inner_x, y: *y, w: inner_w, h: 18.0 },
        lane.title().to_uppercase(),
        13.0,
        theme::PRIMARY_TEXT,
        true,
    );
    *y += 18.0 + 12.0;
    let track = view.mix.and_then(|m| m.track(lane));
    let chain = track.and_then(|t| t.effect_chain);
    paint_assign_row(
        cmds,
        hits,
        inner_x,
        *y,
        inner_w,
        "CHAIN",
        chain,
        &view.engine.config,
        SidebarHit::AssignMix,
        Some(SidebarHit::ClearMixChain),
        false,
        view.thumbs,
    );
    *y += assign_row_h(false, &plugin_stages(&view.engine.config, chain), chain.is_some())
        + CARD_GAP;
    if chain.is_some_and(|c| c.kind == ChainKind::Hardware) {
        let on = track.map(|t| t.hardware_chain_enabled).unwrap_or(true);
        let toggle = Rect { x: inner_x, y: *y, w: Layout::HEADER_BUTTON, h: Layout::HEADER_BUTTON };
        widgets::enable_toggle(cmds, toggle, on);
        theme::text(
            cmds,
            Rect {
                x: inner_x + Layout::HEADER_BUTTON + 8.0,
                y: *y,
                w: 120.0,
                h: Layout::HEADER_BUTTON,
            },
            if on { "Hardware on" } else { "Hardware off" },
            11.0,
            theme::PRIMARY_TEXT,
            false,
        );
        hits.push((
            Rect { x: inner_x, y: *y, w: inner_w, h: Layout::HEADER_BUTTON },
            SidebarHit::ToggleMixHardware,
        ));
        *y += Layout::HEADER_BUTTON + CARD_GAP;
    }
    let open = Rect { x: inner_x, y: *y, w: inner_w, h: Layout::HEADER_BUTTON };
    widgets::hardware_pad(cmds, open, "Open Effects", false, theme::PRIMARY_TEXT);
    hits.push((open, SidebarHit::OpenChains));
    *y += Layout::HEADER_BUTTON;
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

pub fn hit(hits: &[(Rect, SidebarHit)], x: f32, y: f32) -> Option<SidebarHit> {
    hits.iter().rev().find(|(r, _)| widgets::contains(*r, x, y)).map(|(_, h)| h.clone())
}

#[cfg(test)]
mod tests {
    use analog::{AnalogEngine, MixerState, OscSession, SessionConfig, SurfaceState};
    use project::MixDocument;

    use super::*;

    fn test_engine() -> AnalogEngine {
        AnalogEngine::new(
            MixerState::new(),
            SurfaceState::new(),
            SessionConfig::new(),
            OscSession::new(),
        )
    }

    fn record_view<'a>(
        engine: &'a AnalogEngine,
        focus: &'a TextFocus,
        thumbs: &'a std::collections::HashSet<uuid::Uuid>,
    ) -> SidebarView<'a> {
        SidebarView {
            page: Page::Record,
            engine,
            sample_rate: 48_000,
            buffer_frames: 128,
            latency_ms: 2.7,
            device_name: "Test Device",
            mix: None,
            selected_lane: None,
            scroll: 0.0,
            focus,
            caret: false,
            thumbs,
        }
    }

    fn texts(cmds: &[DrawCmd]) -> Vec<&str> {
        cmds.iter()
            .filter_map(|c| match c {
                DrawCmd::Text(t) => Some(t.text.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn record_settings_keeps_audio_device_without_projects_folder() {
        let mut engine = test_engine();
        engine.config.hardware_effects.clear();
        engine.config.plugins.clear();
        let focus = TextFocus::None;
        let thumbs = std::collections::HashSet::new();
        let view = record_view(&engine, &focus, &thumbs);
        let (cmds, hits) = paint(&view, 1200.0, 800.0);
        let labels = texts(&cmds);
        assert!(labels.contains(&"SETTINGS"), "{labels:?}");
        assert!(labels.contains(&"Audio Device"), "{labels:?}");
        assert!(!labels.contains(&"Projects folder"), "{labels:?}");
        assert!(hits.iter().any(|(_, h)| matches!(h, SidebarHit::AudioDevice)));
        assert!(hits.iter().any(|(_, h)| matches!(h, SidebarHit::Settings)));
        assert!(hits.iter().any(|(_, h)| matches!(h, SidebarHit::OpenChains)));
        assert!(labels.contains(&"EFFECTS"), "{labels:?}");
        assert!(labels.contains(&"OPEN EFFECTS"), "{labels:?}");
        let settings_y = cmds.iter().find_map(|c| match c {
            DrawCmd::Text(t) if t.text == "SETTINGS" => Some(t.rect.y),
            _ => None,
        });
        let settings_y = settings_y.expect("SETTINGS heading");
        let foot = crate::chrome::footer_height(Page::Record);
        assert!(
            settings_y > 800.0 - foot - FOOT_H - settings_dock_h() - 1.0,
            "SETTINGS should dock above the Effects/Channels footer, y={settings_y}"
        );
    }

    #[test]
    fn record_plugin_assign_shows_software_playback() {
        let mut engine = test_engine();
        let id = engine.config.plugin_chains[0].id;
        engine.config.set_return_chain(ReturnLane::SendB, Some(analog::ChainRef::plugin(id)));
        let focus = TextFocus::None;
        let thumbs = std::collections::HashSet::new();
        let view = record_view(&engine, &focus, &thumbs);
        let (cmds, hits) = paint(&view, 1200.0, 800.0);
        let labels = texts(&cmds);
        assert!(labels.contains(&"Software playback"), "{labels:?}");
        assert!(labels.contains(&"3/4"), "{labels:?}");
        assert!(hits.iter().any(|(_, h)| matches!(h, SidebarHit::AssignPlayback { id: hit } if *hit == id)));
        assert!(!hits.iter().any(|(_, h)| matches!(h, SidebarHit::AssignPlayback { id: hit } if *hit != id)));
    }

    fn add_named_stage(engine: &mut AnalogEngine, chain: uuid::Uuid, name: &str) -> uuid::Uuid {
        let stage = analog::PluginStage {
            id: uuid::Uuid::new_v4(),
            name: name.into(),
            bundle_path: Some(format!("/plugins/{name}.vst3")),
            class_uid: None,
            bypassed: false,
        };
        let id = stage.id;
        engine.config.plugin_chain_mut(chain).unwrap().stages.push(stage);
        id
    }

    #[test]
    fn record_plugin_assign_lists_stages_and_open() {
        let mut engine = test_engine();
        let chain = engine.config.plugin_chains[0].id;
        let stage = add_named_stage(&mut engine, chain, "Valhalla");
        engine.config.set_return_chain(ReturnLane::SendB, Some(analog::ChainRef::plugin(chain)));
        let focus = TextFocus::None;
        let mut thumbs = std::collections::HashSet::new();
        thumbs.insert(stage);
        let view = record_view(&engine, &focus, &thumbs);
        let (cmds, hits) = paint(&view, 1200.0, 800.0);
        let labels = texts(&cmds);
        assert!(labels.contains(&"Valhalla"), "{labels:?}");
        assert!(!labels.contains(&"Edit"), "{labels:?}");
        assert!(!labels.iter().any(|t| t.eq_ignore_ascii_case("OPEN")), "{labels:?}");
        assert!(cmds.iter().any(|c| matches!(c, DrawCmd::Thumb { id, .. } if *id == stage.as_u128())));
        assert!(hits.iter().any(|(_, h)| matches!(h, SidebarHit::OpenPlugin { id } if *id == stage)));
        let hardware_lane = engine.config.chain_ref(ReturnLane::SendA);
        assert!(hardware_lane.is_some_and(|r| r.kind == ChainKind::Hardware));
        assert_eq!(hits.iter().filter(|(_, h)| matches!(h, SidebarHit::OpenPlugin { .. })).count(), 1);
    }

    #[test]
    fn mix_plugin_assign_lists_stages_and_open() {
        let mut engine = test_engine();
        let chain = engine.config.plugin_chains[0].id;
        let stage = add_named_stage(&mut engine, chain, "Valhalla");
        let mut mix = MixDocument::empty("Mix 1", 2);
        let lane = MixLane::Strip(0);
        if let Some(track) = mix.track_mut(lane) {
            track.effect_chain = Some(analog::ChainRef::plugin(chain));
        }
        let focus = TextFocus::None;
        let thumbs = std::collections::HashSet::new();
        let view = SidebarView {
            page: Page::Mix,
            engine: &engine,
            sample_rate: 48_000,
            buffer_frames: 128,
            latency_ms: 2.7,
            device_name: "Test Device",
            mix: Some(&mix),
            selected_lane: Some(lane),
            scroll: 0.0,
            focus: &focus,
            caret: false,
            thumbs: &thumbs,
        };
        let (cmds, hits) = paint(&view, 1200.0, 800.0);
        let labels = texts(&cmds);
        assert!(labels.contains(&"Valhalla"), "{labels:?}");
        assert!(labels.contains(&"Edit"), "{labels:?}");
        assert!(!labels.contains(&"Software playback"), "{labels:?}");
        assert!(!cmds.iter().any(|c| matches!(c, DrawCmd::Thumb { .. })));
        assert!(hits.iter().any(|(_, h)| matches!(h, SidebarHit::OpenPlugin { id } if *id == stage)));
    }

    #[test]
    fn empty_plugin_stage_shows_no_plugin_and_disables_open() {
        let mut engine = test_engine();
        let chain = engine.config.plugin_chains[0].id;
        engine.config.rename_plugin_chain(chain, "Big reverb");
        engine.config.plugin_chain_mut(chain).unwrap().stages.push(analog::PluginStage {
            id: uuid::Uuid::new_v4(),
            name: "FX A".into(),
            bundle_path: None,
            class_uid: None,
            bypassed: false,
        });
        engine.config.set_return_chain(ReturnLane::SendB, Some(analog::ChainRef::plugin(chain)));
        let focus = TextFocus::None;
        let thumbs = std::collections::HashSet::new();
        let view = record_view(&engine, &focus, &thumbs);
        let (cmds, hits) = paint(&view, 1200.0, 800.0);
        let labels = texts(&cmds);
        assert!(labels.contains(&"Big reverb"), "{labels:?}");
        assert!(labels.contains(&"No plugin"), "{labels:?}");
        assert!(!labels.contains(&"FX A"), "{labels:?}");
        assert!(!labels.contains(&"Edit"), "{labels:?}");
        assert!(!labels.iter().any(|t| t.eq_ignore_ascii_case("OPEN")), "{labels:?}");
        assert!(!hits.iter().any(|(_, h)| matches!(h, SidebarHit::OpenPlugin { .. })), "{hits:?}");
    }
}
