//! Dedicated Chains window: Hardware presets/chains and plugin chains.

use analog::{AnalogEngine, ChainKind, MixerBus};
use render::{DrawCmd, Rect};

use crate::overlay::{self, TextFocus};
use crate::theme::{self, Layout};
use crate::widgets;

pub const CHAINS_WINDOW_W: f32 = 780.0;
pub const CHAINS_WINDOW_H: f32 = 640.0;

const PAD: f32 = 16.0;
const ROW: f32 = 24.0;
const GAP: f32 = 6.0;
const TAB_H: f32 = 26.0;
const SCROLL_GUTTER: f32 = 14.0;
const THUMB_W: f32 = 128.0;
const THUMB_H: f32 = 72.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChainsTab {
    Hardware,
    Plugins,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChainsHit {
    Tab(ChainsTab),
    Close,
    AddPreset,
    AddHardwareChain,
    AddPluginChain,
    DuplicatePreset(uuid::Uuid),
    DuplicateHardwareChain(uuid::Uuid),
    DuplicatePluginChain(uuid::Uuid),
    RemovePreset(uuid::Uuid),
    RemoveHardwareChain(uuid::Uuid),
    RemovePluginChain(uuid::Uuid),
    EditPresetName(uuid::Uuid),
    EditHardwareChainName(uuid::Uuid),
    EditPluginChainName(uuid::Uuid),
    PresetOutput(uuid::Uuid),
    PresetInput(uuid::Uuid),
    AddHardwareStage(uuid::Uuid),
    AddPluginStage(uuid::Uuid),
    HardwareStagePreset { chain: uuid::Uuid, index: usize },
    RemoveHardwareStage { chain: uuid::Uuid, index: usize },
    MoveHardwareStage { chain: uuid::Uuid, index: usize, delta: i32 },
    PluginStageBundle(uuid::Uuid),
    PluginStageEdit(uuid::Uuid),
    PluginStageBypass(uuid::Uuid),
    PluginStageScope(uuid::Uuid),
    PluginStageThumb(uuid::Uuid),
    RemovePluginStage(uuid::Uuid),
    MovePluginStage { chain: uuid::Uuid, index: usize, delta: i32 },
}

pub fn paint_chains(
    engine: &AnalogEngine,
    w: f32,
    h: f32,
    tab: ChainsTab,
    focus: &TextFocus,
    caret: bool,
    scroll: f32,
    scope_global: &std::collections::HashMap<uuid::Uuid, bool>,
    thumbs: &std::collections::HashSet<uuid::Uuid>,
) -> (Vec<DrawCmd>, Vec<(Rect, ChainsHit)>) {
    let mut cmds = Vec::new();
    let mut hits = Vec::new();
    let panel = overlay::settings_panel(w, h);
    widgets::document_window(&mut cmds, w, h, panel);

    let tab_y = panel.y + 12.0;
    let tab_w = 110.0;
    let hw_tab = Rect { x: panel.x + PAD, y: tab_y, w: tab_w, h: TAB_H };
    let pl_tab = Rect { x: hw_tab.x + tab_w + 8.0, y: tab_y, w: tab_w, h: TAB_H };
    widgets::hardware_pad(&mut cmds, hw_tab, "Hardware", tab == ChainsTab::Hardware, theme::PRIMARY_TEXT);
    widgets::hardware_pad(&mut cmds, pl_tab, "Plugins", tab == ChainsTab::Plugins, theme::PRIMARY_TEXT);
    hits.push((hw_tab, ChainsHit::Tab(ChainsTab::Hardware)));
    hits.push((pl_tab, ChainsHit::Tab(ChainsTab::Plugins)));

    let max_scroll = chains_max_scroll(engine, tab, w, h);
    let gutter = if max_scroll > 0.5 { SCROLL_GUTTER } else { 0.0 };
    let clip = Rect {
        x: panel.x + 8.0,
        y: tab_y + TAB_H + 10.0,
        w: panel.w - 16.0,
        h: (panel.h - (tab_y - panel.y) - TAB_H - 18.0).max(40.0),
    };
    let list_w = (clip.w - gutter).max(40.0);
    cmds.push(DrawCmd::Layer);
    cmds.push(DrawCmd::Clip { rect: clip });
    let mut y = clip.y + 4.0 - scroll;
    match tab {
        ChainsTab::Hardware => paint_hardware(engine, &mut cmds, &mut hits, clip.x, list_w, &mut y, focus, caret),
        ChainsTab::Plugins => paint_plugins(
            engine,
            &mut cmds,
            &mut hits,
            clip.x,
            list_w,
            &mut y,
            focus,
            caret,
            scope_global,
            thumbs,
        ),
    }
    hits.retain(|(r, hit)| {
        matches!(hit, ChainsHit::Tab(_)) || (r.y + r.h > clip.y && r.y < clip.y + clip.h)
    });
    cmds.push(DrawCmd::Layer);
    if max_scroll > 0.5 {
        widgets::scrollbar(
            &mut cmds,
            Rect {
                x: clip.x + list_w + 4.0,
                y: clip.y + 4.0,
                w: 5.0,
                h: (clip.h - 8.0).max(8.0),
            },
            scroll,
            max_scroll,
        );
    }

    let close = Rect {
        x: w - overlay::WINDOW_CLOSE_PAD - overlay::WINDOW_CLOSE_W,
        y: h - overlay::WINDOW_CLOSE_PAD - overlay::WINDOW_CLOSE_H,
        w: overlay::WINDOW_CLOSE_W,
        h: overlay::WINDOW_CLOSE_H,
    };
    widgets::window_close(&mut cmds, close);
    hits.push((close, ChainsHit::Close));
    (cmds, hits)
}

pub fn chains_max_scroll(engine: &AnalogEngine, tab: ChainsTab, w: f32, h: f32) -> f32 {
    let panel = overlay::settings_panel(w, h);
    let clip_h = (panel.h - 12.0 - TAB_H - 18.0).max(40.0);
    let content = match tab {
        ChainsTab::Hardware => hardware_content_h(&engine.config),
        ChainsTab::Plugins => plugin_content_h(&engine.config),
    };
    (content - clip_h).max(0.0)
}

fn hardware_content_h(config: &analog::SessionConfig) -> f32 {
    let mut h = 8.0;
    h += ROW + GAP;
    h += config.hardware_presets.len() as f32 * (preset_h() + GAP);
    h += 20.0 + ROW + GAP;
    h += config.hardware_chains.iter().map(|c| hardware_chain_h(c.stages.len()) + GAP).sum::<f32>();
    h
}

fn plugin_content_h(config: &analog::SessionConfig) -> f32 {
    let mut h = 8.0 + ROW + GAP;
    h += config.plugin_chains.iter().map(plugin_chain_h).sum::<f32>();
    h
}

const LABEL_H: f32 = 16.0;

fn preset_h() -> f32 {
    Layout::MODULE_PAD * 2.0 + Layout::HEADER_BUTTON + GAP + LABEL_H + 4.0 + ROW
}

fn hardware_chain_h(stages: usize) -> f32 {
    Layout::MODULE_PAD * 2.0
        + Layout::HEADER_BUTTON
        + GAP
        + ROW
        + stages as f32 * (ROW + GAP)
}

fn plugin_stage_h(stage: &analog::PluginStage) -> f32 {
    if stage.is_loaded() {
        THUMB_H + GAP
    } else {
        ROW * 2.0 + GAP
    }
}

fn plugin_chain_h(chain: &analog::PluginChain) -> f32 {
    Layout::MODULE_PAD * 2.0
        + Layout::HEADER_BUTTON
        + GAP
        + ROW
        + chain.stages.iter().map(plugin_stage_h).sum::<f32>()
        + GAP
}

fn paint_hardware(
    engine: &AnalogEngine,
    cmds: &mut Vec<DrawCmd>,
    hits: &mut Vec<(Rect, ChainsHit)>,
    x: f32,
    w: f32,
    y: &mut f32,
    focus: &TextFocus,
    caret: bool,
) {
    section_plus(cmds, hits, x, *y, w, "Devices", ChainsHit::AddPreset);
    *y += ROW + GAP;
    for preset in &engine.config.hardware_presets {
        paint_preset(engine, cmds, hits, x, *y, w, preset, focus, caret);
        *y += preset_h() + GAP;
    }
    *y += 8.0;
    section_plus(cmds, hits, x, *y, w, "Hardware chains", ChainsHit::AddHardwareChain);
    *y += ROW + GAP;
    for chain in &engine.config.hardware_chains {
        paint_hw_chain(engine, cmds, hits, x, *y, w, chain, focus, caret);
        *y += hardware_chain_h(chain.stages.len()) + GAP;
    }
}

fn paint_plugins(
    engine: &AnalogEngine,
    cmds: &mut Vec<DrawCmd>,
    hits: &mut Vec<(Rect, ChainsHit)>,
    x: f32,
    w: f32,
    y: &mut f32,
    focus: &TextFocus,
    caret: bool,
    scope_global: &std::collections::HashMap<uuid::Uuid, bool>,
    thumbs: &std::collections::HashSet<uuid::Uuid>,
) {
    section_plus(cmds, hits, x, *y, w, "Plugin chains", ChainsHit::AddPluginChain);
    *y += ROW + GAP;
    for chain in &engine.config.plugin_chains {
        paint_plugin_chain(
            engine,
            cmds,
            hits,
            x,
            *y,
            w,
            chain,
            focus,
            caret,
            scope_global,
            thumbs,
        );
        *y += plugin_chain_h(chain) + GAP;
    }
}

fn paint_preset(
    engine: &AnalogEngine,
    cmds: &mut Vec<DrawCmd>,
    hits: &mut Vec<(Rect, ChainsHit)>,
    x: f32,
    y: f32,
    w: f32,
    preset: &analog::HardwarePreset,
    focus: &TextFocus,
    caret: bool,
) {
    let card = Rect { x, y, w, h: preset_h() };
    widgets::hardware_module(cmds, card);
    let cx = x + Layout::MODULE_PAD;
    let cw = w - Layout::MODULE_PAD * 2.0;
    let mut cy = y + Layout::MODULE_PAD;
    name_row(
        cmds,
        hits,
        cx,
        cy,
        cw,
        &preset.name,
        &preset.title(),
        *focus == TextFocus::HardwarePresetName(preset.id),
        caret,
        ChainsHit::EditPresetName(preset.id),
        ChainsHit::DuplicatePreset(preset.id),
        ChainsHit::RemovePreset(preset.id),
    );
    cy += Layout::HEADER_BUTTON + GAP;
    let col_w = (cw - GAP) * 0.5;
    labeled_picker(
        cmds,
        hits,
        cx,
        cy,
        col_w,
        "Output",
        &engine.output_name(preset.output),
        ChainsHit::PresetOutput(preset.id),
    );
    labeled_picker(
        cmds,
        hits,
        cx + col_w + GAP,
        cy,
        col_w,
        "Input",
        &engine.display_name(analog::ChannelID::new(MixerBus::Input, preset.input)),
        ChainsHit::PresetInput(preset.id),
    );
}

fn paint_hw_chain(
    engine: &AnalogEngine,
    cmds: &mut Vec<DrawCmd>,
    hits: &mut Vec<(Rect, ChainsHit)>,
    x: f32,
    y: f32,
    w: f32,
    chain: &analog::HardwareChain,
    focus: &TextFocus,
    caret: bool,
) {
    let card = Rect { x, y, w, h: hardware_chain_h(chain.stages.len()) };
    widgets::hardware_module(cmds, card);
    let cx = x + Layout::MODULE_PAD;
    let cw = w - Layout::MODULE_PAD * 2.0;
    let mut cy = y + Layout::MODULE_PAD;
    name_row(
        cmds,
        hits,
        cx,
        cy,
        cw,
        &chain.name,
        &chain.title(),
        *focus == TextFocus::HardwareChainName(chain.id),
        caret,
        ChainsHit::EditHardwareChainName(chain.id),
        ChainsHit::DuplicateHardwareChain(chain.id),
        ChainsHit::RemoveHardwareChain(chain.id),
    );
    cy += Layout::HEADER_BUTTON + GAP;
    let add = Rect { x: cx, y: cy, w: 72.0, h: ROW };
    widgets::hardware_pad(cmds, add, "Add stage", false, theme::PRIMARY_TEXT);
    hits.push((add, ChainsHit::AddHardwareStage(chain.id)));
    cy += ROW + GAP;
    for (i, preset_id) in chain.stages.iter().enumerate() {
        let title = engine
            .config
            .hardware_preset(*preset_id)
            .map(|p| {
                format!(
                    "{}  Out {} → In {}",
                    p.title(),
                    p.output + 1,
                    p.input + 1
                )
            })
            .unwrap_or_else(|| "Missing device".into());
        let row = Rect { x: cx, y: cy, w: cw - 72.0, h: ROW };
        widgets::channel_picker(cmds, row, &title, widgets::ChannelPickerStyle::value());
        hits.push((row, ChainsHit::HardwareStagePreset { chain: chain.id, index: i }));
        let up = Rect { x: cx + cw - 68.0, y: cy, w: 20.0, h: ROW };
        let down = Rect { x: cx + cw - 46.0, y: cy, w: 20.0, h: ROW };
        let minus = Rect { x: cx + cw - 24.0, y: cy, w: 24.0, h: ROW };
        widgets::icon_pad(cmds, up, "↑", i > 0);
        widgets::icon_pad(cmds, down, "↓", i + 1 < chain.stages.len());
        widgets::icon_pad(cmds, minus, "−", true);
        if i > 0 {
            hits.push((up, ChainsHit::MoveHardwareStage { chain: chain.id, index: i, delta: -1 }));
        }
        if i + 1 < chain.stages.len() {
            hits.push((down, ChainsHit::MoveHardwareStage { chain: chain.id, index: i, delta: 1 }));
        }
        hits.push((minus, ChainsHit::RemoveHardwareStage { chain: chain.id, index: i }));
        cy += ROW + GAP;
    }
}

fn paint_plugin_chain(
    engine: &AnalogEngine,
    cmds: &mut Vec<DrawCmd>,
    hits: &mut Vec<(Rect, ChainsHit)>,
    x: f32,
    y: f32,
    w: f32,
    chain: &analog::PluginChain,
    focus: &TextFocus,
    caret: bool,
    scope_global: &std::collections::HashMap<uuid::Uuid, bool>,
    thumbs: &std::collections::HashSet<uuid::Uuid>,
) {
    let card = Rect { x, y, w, h: plugin_chain_h(chain) };
    widgets::hardware_module(cmds, card);
    let cx = x + Layout::MODULE_PAD;
    let cw = w - Layout::MODULE_PAD * 2.0;
    let mut cy = y + Layout::MODULE_PAD;
    name_row(
        cmds,
        hits,
        cx,
        cy,
        cw,
        &chain.name,
        &chain.title(),
        *focus == TextFocus::PluginChainName(chain.id),
        caret,
        ChainsHit::EditPluginChainName(chain.id),
        ChainsHit::DuplicatePluginChain(chain.id),
        ChainsHit::RemovePluginChain(chain.id),
    );
    cy += Layout::HEADER_BUTTON + GAP;
    let add = Rect { x: cx, y: cy, w: 72.0, h: ROW };
    widgets::hardware_pad(cmds, add, "Add stage", false, theme::PRIMARY_TEXT);
    hits.push((add, ChainsHit::AddPluginStage(chain.id)));
    cy += ROW + GAP;
    for (i, stage) in chain.stages.iter().enumerate() {
        let bundle = stage
            .bundle_path
            .as_deref()
            .and_then(|p| std::path::Path::new(p).file_stem().and_then(|s| s.to_str()))
            .unwrap_or("No plugin");
        let kind = if stage.is_loaded() { "Plugin" } else { "Empty" };
        let thumb_slot = stage.is_loaded();
        let right = if thumb_slot { THUMB_W + 8.0 } else { 0.0 };
        theme::text(
            cmds,
            Rect { x: cx, y: cy, w: 56.0, h: ROW },
            kind,
            10.0,
            theme::SECONDARY_TEXT,
            true,
        );
        let menu = Rect { x: cx + 60.0, y: cy, w: (cw - 160.0 - right).max(80.0), h: ROW };
        widgets::channel_picker(cmds, menu, bundle, widgets::ChannelPickerStyle::plugin());
        hits.push((menu, ChainsHit::PluginStageBundle(stage.id)));
        let tools_r = cx + cw - right;
        let up = Rect { x: tools_r - 96.0, y: cy, w: 20.0, h: ROW };
        let down = Rect { x: tools_r - 74.0, y: cy, w: 20.0, h: ROW };
        let minus = Rect { x: tools_r - 52.0, y: cy, w: 24.0, h: ROW };
        widgets::icon_pad(cmds, up, "↑", i > 0);
        widgets::icon_pad(cmds, down, "↓", i + 1 < chain.stages.len());
        widgets::icon_pad(cmds, minus, "−", true);
        if i > 0 {
            hits.push((up, ChainsHit::MovePluginStage { chain: chain.id, index: i, delta: -1 }));
        }
        if i + 1 < chain.stages.len() {
            hits.push((down, ChainsHit::MovePluginStage { chain: chain.id, index: i, delta: 1 }));
        }
        hits.push((minus, ChainsHit::RemovePluginStage(stage.id)));
        if thumb_slot {
            let thumb = Rect { x: cx + cw - THUMB_W, y: cy, w: THUMB_W, h: THUMB_H };
            widgets::plugin_thumb(cmds, thumb, stage.id.as_u128(), thumbs.contains(&stage.id));
            hits.push((thumb, ChainsHit::PluginStageThumb(stage.id)));
        }
        cy += ROW + GAP;
        let edit = Rect { x: cx + 60.0, y: cy, w: 72.0, h: ROW };
        let bypass = Rect { x: cx + 140.0, y: cy, w: 72.0, h: ROW };
        let scope = Rect { x: cx + 220.0, y: cy, w: 80.0, h: ROW };
        let is_global = scope_global.get(&stage.id).copied().unwrap_or(false);
        let scope_label = if is_global { "Global" } else { "Project" };
        widgets::hardware_pad(cmds, edit, "Edit", false, theme::PRIMARY_TEXT);
        widgets::hardware_pad(cmds, bypass, "Bypass", stage.bypassed, theme::AMBER);
        widgets::hardware_pad(cmds, scope, scope_label, is_global, theme::PRIMARY_TEXT);
        hits.push((edit, ChainsHit::PluginStageEdit(stage.id)));
        hits.push((bypass, ChainsHit::PluginStageBypass(stage.id)));
        if stage.is_loaded() {
            hits.push((scope, ChainsHit::PluginStageScope(stage.id)));
        }
        cy += if thumb_slot { (THUMB_H - ROW).max(ROW) } else { ROW + GAP };
        let _ = engine;
    }
}

fn section_plus(
    cmds: &mut Vec<DrawCmd>,
    hits: &mut Vec<(Rect, ChainsHit)>,
    x: f32,
    y: f32,
    w: f32,
    title: &str,
    add: ChainsHit,
) {
    theme::text(cmds, Rect { x, y, w: w - 30.0, h: ROW }, title, 12.0, theme::SECONDARY_TEXT, true);
    let plus = Rect { x: x + w - Layout::HEADER_BUTTON, y, w: Layout::HEADER_BUTTON, h: Layout::HEADER_BUTTON };
    widgets::icon_pad(cmds, plus, "+", true);
    hits.push((plus, add));
}

fn name_row(
    cmds: &mut Vec<DrawCmd>,
    hits: &mut Vec<(Rect, ChainsHit)>,
    x: f32,
    y: f32,
    w: f32,
    name: &str,
    placeholder: &str,
    focused: bool,
    caret: bool,
    edit: ChainsHit,
    dup: ChainsHit,
    remove: ChainsHit,
) {
    let empty = name.is_empty();
    let shown = if focused {
        if caret {
            format!("{name}|")
        } else {
            name.to_string()
        }
    } else if empty {
        placeholder.to_string()
    } else {
        name.to_string()
    };
    let field = Rect { x, y, w: w - 80.0, h: Layout::HEADER_BUTTON };
    theme::text(
        cmds,
        field,
        shown,
        12.0,
        if !focused && empty { theme::TEXT_DIM } else { theme::PRIMARY_TEXT },
        true,
    );
    hits.push((field, edit));
    let copy = Rect { x: x + w - 76.0, y, w: 48.0, h: Layout::HEADER_BUTTON };
    let minus = Rect {
        x: x + w - Layout::HEADER_BUTTON,
        y,
        w: Layout::HEADER_BUTTON,
        h: Layout::HEADER_BUTTON,
    };
    widgets::hardware_pad(cmds, copy, "Copy", false, theme::PRIMARY_TEXT);
    widgets::icon_pad(cmds, minus, "−", true);
    hits.push((copy, dup));
    hits.push((minus, remove));
}

fn labeled_picker(
    cmds: &mut Vec<DrawCmd>,
    hits: &mut Vec<(Rect, ChainsHit)>,
    x: f32,
    y: f32,
    w: f32,
    label: &str,
    value: &str,
    hit: ChainsHit,
) {
    theme::text(cmds, Rect { x, y, w, h: LABEL_H }, label, 11.0, theme::TEXT_DIM, false);
    let r = Rect { x, y: y + LABEL_H + 4.0, w, h: ROW };
    widgets::channel_picker(cmds, r, value, widgets::ChannelPickerStyle::value());
    hits.push((r, hit));
}

#[cfg(test)]
mod tests {
    use analog::{AnalogEngine, MixerState, OscSession, SessionConfig, SurfaceState};

    use super::*;

    fn engine() -> AnalogEngine {
        AnalogEngine::new(
            MixerState::new(),
            SurfaceState::new(),
            SessionConfig::new(),
            OscSession::new(),
        )
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
    fn hardware_tab_paints_presets_and_chains() {
        let engine = engine();
        let empty = std::collections::HashMap::new();
        let (cmds, hits) = paint_chains(
            &engine,
            CHAINS_WINDOW_W,
            CHAINS_WINDOW_H,
            ChainsTab::Hardware,
            &TextFocus::None,
            false,
            0.0,
            &empty,
            &std::collections::HashSet::new(),
        );
        let labels = texts(&cmds);
        assert!(!labels.contains(&"Chains"), "{labels:?}");
        assert!(labels.contains(&"HARDWARE"), "{labels:?}");
        assert!(labels.contains(&"PLUGINS"), "{labels:?}");
        assert!(labels.contains(&"Devices"), "{labels:?}");
        assert!(labels.contains(&"Output"), "{labels:?}");
        assert!(labels.contains(&"Input"), "{labels:?}");
        assert!(hits.iter().any(|(_, h)| matches!(h, ChainsHit::AddPreset)));
        assert!(hits.iter().any(|(_, h)| matches!(h, ChainsHit::AddHardwareChain)));
        assert!(hits.iter().any(|(_, h)| matches!(h, ChainsHit::DuplicatePreset(_))));
        assert!(hits.iter().any(|(_, h)| matches!(h, ChainsHit::PresetOutput(_))));
        assert!(hits.iter().any(|(_, h)| matches!(h, ChainsHit::PresetInput(_))));
        let plugins = hits
            .iter()
            .find(|(_, h)| matches!(h, ChainsHit::Tab(ChainsTab::Plugins)))
            .map(|(r, _)| *r)
            .expect("Plugins tab hit");
        assert!(widgets::contains(plugins, plugins.x + 8.0, plugins.y + 8.0));
        assert!(
            chains_max_scroll(&engine, ChainsTab::Hardware, CHAINS_WINDOW_W, 360.0) > 0.5,
            "short window should overflow"
        );
        let (short, short_hits) = paint_chains(
            &engine,
            CHAINS_WINDOW_W,
            360.0,
            ChainsTab::Hardware,
            &TextFocus::None,
            false,
            0.0,
            &empty,
            &std::collections::HashSet::new(),
        );
        let thumb = short.iter().find_map(|c| match c {
            DrawCmd::RoundedRect { rect, .. } if rect.w <= 6.0 && rect.h >= 18.0 => Some(*rect),
            _ => None,
        });
        let thumb = thumb.expect("overflow list should paint a scrollbar thumb");
        for (r, hit) in &short_hits {
            if matches!(hit, ChainsHit::Tab(_) | ChainsHit::Close) {
                continue;
            }
            assert!(
                r.x + r.w <= thumb.x + 0.5,
                "content {hit:?} overlaps scrollbar at x={}",
                thumb.x
            );
        }
    }

    #[test]
    fn loaded_stage_paints_editor_thumb() {
        let mut engine = engine();
        let stage = analog::PluginStage {
            id: uuid::Uuid::from_u128(7),
            name: "Rack".into(),
            bundle_path: Some("/tmp/EffectRack.vst3".into()),
            class_uid: None,
            bypassed: false,
        };
        engine.config.plugin_chains[0].stages.push(stage.clone());
        let mut thumbs = std::collections::HashSet::new();
        thumbs.insert(stage.id);
        let empty = std::collections::HashMap::new();
        let (cmds, hits) = paint_chains(
            &engine,
            CHAINS_WINDOW_W,
            CHAINS_WINDOW_H,
            ChainsTab::Plugins,
            &TextFocus::None,
            false,
            0.0,
            &empty,
            &thumbs,
        );
        assert!(cmds.iter().any(|c| matches!(c, DrawCmd::Thumb { id, .. } if *id == stage.id.as_u128())));
        assert!(hits.iter().any(|(_, h)| matches!(h, ChainsHit::PluginStageThumb(id) if *id == stage.id)));
    }

    #[test]
    fn loaded_stage_without_capture_paints_placeholder() {
        let mut engine = engine();
        engine.config.plugin_chains[0].stages.push(analog::PluginStage {
            id: uuid::Uuid::from_u128(8),
            name: "Rack".into(),
            bundle_path: Some("/tmp/EffectRack.vst3".into()),
            class_uid: None,
            bypassed: false,
        });
        let thumbs = std::collections::HashSet::new();
        let empty = std::collections::HashMap::new();
        let (cmds, hits) = paint_chains(
            &engine,
            CHAINS_WINDOW_W,
            CHAINS_WINDOW_H,
            ChainsTab::Plugins,
            &TextFocus::None,
            false,
            0.0,
            &empty,
            &thumbs,
        );
        let labels: Vec<_> = cmds
            .iter()
            .filter_map(|c| match c {
                DrawCmd::Text(t) => Some(t.text.as_str()),
                _ => None,
            })
            .collect();
        assert!(labels.contains(&"Edit"), "{labels:?}");
        assert!(!cmds.iter().any(|c| matches!(c, DrawCmd::Thumb { .. })));
        assert!(hits.iter().any(|(_, h)| matches!(h, ChainsHit::PluginStageThumb(_))));
    }

    #[test]
    fn plugins_tab_paints_return_and_duplicate() {
        let engine = engine();
        let empty = std::collections::HashMap::new();
        let (cmds, hits) = paint_chains(
            &engine,
            CHAINS_WINDOW_W,
            CHAINS_WINDOW_H,
            ChainsTab::Plugins,
            &TextFocus::None,
            false,
            0.0,
            &empty,
            &std::collections::HashSet::new(),
        );
        let labels = texts(&cmds);
        assert!(labels.iter().any(|t| t.contains("FX A")), "{labels:?}");
        assert!(hits.iter().any(|(_, h)| matches!(h, ChainsHit::AddPluginChain)));
        assert!(hits.iter().any(|(_, h)| matches!(h, ChainsHit::DuplicatePluginChain(_))));
        assert!(!labels.iter().any(|t| t.contains("Software playback")), "{labels:?}");
        assert!(!hits.iter().any(|(_, h)| matches!(h, ChainsHit::PresetOutput(_))));
    }

    #[test]
    fn empty_focused_name_does_not_restore_title() {
        let mut engine = engine();
        let id = engine.config.hardware_chains[0].id;
        engine.config.rename_hardware_chain(id, "");
        let empty = std::collections::HashMap::new();
        let (cmds, _) = paint_chains(
            &engine,
            CHAINS_WINDOW_W,
            CHAINS_WINDOW_H,
            ChainsTab::Hardware,
            &TextFocus::HardwareChainName(id),
            true,
            0.0,
            &empty,
            &std::collections::HashSet::new(),
        );
        let labels = texts(&cmds);
        assert!(labels.contains(&"|"), "{labels:?}");
        assert!(!labels.contains(&"Hardware chain"), "{labels:?}");
        assert!(!labels.contains(&"Hardware chain|"), "{labels:?}");
    }
}

pub fn chain_kind_label(kind: ChainKind) -> &'static str {
    match kind {
        ChainKind::Hardware => "Hardware",
        ChainKind::Plugin => "Plugin",
    }
}
