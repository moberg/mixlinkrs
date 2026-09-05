//! Mix page left browser: MIXES / TAKES, 148 pt (MixLink `MixBrowserView`).

use std::collections::HashMap;

use project::{take_wav_name, MixDocument, MixLane, ProjectMeta};
use render::{DrawCmd, Rect};

use crate::theme;
use crate::widgets;

pub const WIDTH: f32 = 148.0;
const MIX_LIST_TOP: f32 = 28.0;
const ROW_H: f32 = 24.0;
const SECTION_GAP: f32 = 12.0;
const TAKES_HEADER_H: f32 = 20.0;
const EMPTY_MIXES: &str = "no mixes created yet";
const EMPTY_TAKES: &str = "no takes recorded yet";

/// One placeholder row when the mix list is empty so TAKES keeps its y.
fn mix_list_h(mix_count: usize) -> f32 {
    if mix_count == 0 {
        ROW_H
    } else {
        mix_count as f32 * ROW_H
    }
}

pub struct MixBrowserView<'a> {
    pub x: f32,
    pub y: f32,
    pub h: f32,
    pub mixes: &'a [MixDocument],
    pub selected_mix: Option<uuid::Uuid>,
    pub takes: &'a [i32],
    pub take_names: &'a HashMap<i32, String>,
    pub selected_take: Option<i32>,
    pub mixer_collapsed: bool,
    pub edit: Option<BrowserEdit>,
    pub edit_text: &'a str,
    pub caret: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrowserEdit {
    Mix(uuid::Uuid),
    Take(i32),
}

pub fn paint(view: &MixBrowserView<'_>) -> Vec<DrawCmd> {
    let mut cmds = Vec::new();
    cmds.push(DrawCmd::Layer);
    theme::hardware_surface(
        &mut cmds,
        Rect { x: view.x, y: view.y, w: WIDTH, h: view.h },
        theme::SurfaceStyle::Sidebar,
    );
    theme::seam_v(&mut cmds, view.x + WIDTH - 1.0, view.y, view.h, true);

    theme::text(
        &mut cmds,
        Rect { x: view.x + 10.0, y: view.y + 8.0, w: 70.0, h: 16.0 },
        "MIXES",
        11.0,
        theme::TEXT_DIM,
        true,
    );
    let mut y = view.y + MIX_LIST_TOP;
    if view.mixes.is_empty() {
        paint_placeholder(&mut cmds, view.x, y, EMPTY_MIXES);
        y += ROW_H;
    } else {
        for mix in view.mixes {
            // Arrangement selection: a take and a mix are never both highlighted.
            let sel = view.selected_take.is_none() && view.selected_mix == Some(mix.id);
            let editing = view.edit == Some(BrowserEdit::Mix(mix.id));
            paint_row(&mut cmds, view.x, y, &mix.name, sel, editing, view);
            y += ROW_H;
        }
    }

    theme::seam_h(&mut cmds, view.x + 8.0, y + SECTION_GAP * 0.5 - 1.0, WIDTH - 16.0, false);
    y += SECTION_GAP;
    theme::text(
        &mut cmds,
        Rect { x: view.x + 10.0, y, w: 120.0, h: 16.0 },
        "TAKES",
        11.0,
        theme::TEXT_DIM,
        true,
    );
    y += TAKES_HEADER_H;
    if view.takes.is_empty() {
        paint_placeholder(&mut cmds, view.x, y, EMPTY_TAKES);
    } else {
        for take in view.takes {
            let sel = view.selected_take == Some(*take);
            let title =
                ProjectMeta::take_title(*take, view.take_names.get(take).map(String::as_str));
            let editing = view.edit == Some(BrowserEdit::Take(*take));
            paint_row(&mut cmds, view.x, y, title, sel, editing, view);
            y += ROW_H;
        }
    }
    let _ = take_wav_name(1, MixLane::Main, "");
    if view.mixer_collapsed {
        cmds.extend(crate::mix_mixer::paint_collapsed_handle(
            view.x,
            view.y + view.h - crate::mix_mixer::HANDLE_H,
            WIDTH,
        ));
    }
    cmds
}

fn paint_row(
    cmds: &mut Vec<DrawCmd>,
    x: f32,
    y: f32,
    title: impl AsRef<str>,
    selected: bool,
    editing: bool,
    view: &MixBrowserView<'_>,
) {
    if selected || editing {
        theme::fill(cmds, Rect { x, y, w: WIDTH, h: 22.0 }, theme::BROWSER_SELECTED);
    }
    if editing {
        widgets::text_field(
            cmds,
            Rect { x: x + 4.0, y: y + 1.0, w: WIDTH - 8.0, h: 20.0 },
            view.edit_text,
            true,
            view.caret,
        );
        return;
    }
    theme::text(
        cmds,
        Rect { x: x + 10.0, y, w: WIDTH - 16.0, h: 22.0 },
        title.as_ref(),
        12.0,
        if selected { theme::TEXT } else { theme::TEXT_DIM },
        selected,
    );
}

fn paint_placeholder(cmds: &mut Vec<DrawCmd>, x: f32, y: f32, s: &str) {
    theme::text_italic(
        cmds,
        Rect { x: x + 10.0, y, w: WIDTH - 16.0, h: 22.0 },
        s,
        12.0,
        theme::TEXT_DIM,
    );
}

pub fn hit(view: &MixBrowserView<'_>, x: f32, y: f32) -> Option<BrowserHit> {
    if x < view.x || x > view.x + WIDTH || y < view.y || y > view.y + view.h {
        return None;
    }
    if view.mixer_collapsed && y >= view.y + view.h - crate::mix_mixer::HANDLE_H {
        return Some(BrowserHit::Mixer);
    }
    let mut yy = view.y + MIX_LIST_TOP;
    if view.mixes.is_empty() {
        yy += ROW_H;
    } else {
        for mix in view.mixes {
            if y >= yy && y < yy + ROW_H {
                return Some(BrowserHit::Mix(mix.id));
            }
            yy += ROW_H;
        }
    }
    yy += SECTION_GAP + TAKES_HEADER_H;
    for take in view.takes {
        if y >= yy && y < yy + ROW_H {
            return Some(BrowserHit::Take(*take));
        }
        yy += ROW_H;
    }
    None
}

pub fn row_rect(view: &MixBrowserView<'_>, hit: BrowserHit) -> Option<Rect> {
    match hit {
        BrowserHit::Mix(id) => {
            let i = view.mixes.iter().position(|m| m.id == id)?;
            Some(Rect { x: view.x, y: view.y + MIX_LIST_TOP + i as f32 * ROW_H, w: WIDTH, h: 22.0 })
        }
        BrowserHit::Take(number) => {
            let i = view.takes.iter().position(|n| *n == number)?;
            let y = view.y
                + MIX_LIST_TOP
                + mix_list_h(view.mixes.len())
                + SECTION_GAP
                + TAKES_HEADER_H
                + i as f32 * ROW_H;
            Some(Rect { x: view.x, y, w: WIDTH, h: 22.0 })
        }
        _ => None,
    }
}

#[derive(Clone, Copy, Debug)]
pub enum BrowserHit {
    Mix(uuid::Uuid),
    Take(i32),
    Mixer,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_names() -> HashMap<i32, String> {
        HashMap::new()
    }

    fn view<'a>(
        mixes: &'a [MixDocument],
        takes: &'a [i32],
        names: &'a HashMap<i32, String>,
    ) -> MixBrowserView<'a> {
        MixBrowserView {
            x: 0.0,
            y: 40.0,
            h: 400.0,
            mixes,
            selected_mix: mixes.first().map(|m| m.id),
            takes,
            take_names: names,
            selected_take: None,
            mixer_collapsed: false,
            edit: None,
            edit_text: "",
            caret: false,
        }
    }

    #[test]
    fn drawer_cover_is_opaque() {
        let names = empty_names();
        let cmds = paint(&view(&[], &[], &names));
        assert!(matches!(cmds.first(), Some(DrawCmd::Layer)));
        let opaque = cmds.iter().any(|c| match c {
            DrawCmd::VertGradient { rect, top, bottom } => {
                rect.w >= WIDTH - 0.5 && rect.h > 10.0 && top[3] >= 0.99 && bottom[3] >= 0.99
            }
            DrawCmd::Rect { rect, color } => {
                rect.w >= WIDTH - 0.5 && rect.h >= 399.0 && color[3] >= 0.99
            }
            _ => false,
        });
        assert!(opaque, "drawer must fully cover the timeline");
    }

    #[test]
    fn collapsed_mixer_hits_drawer_footer() {
        let names = empty_names();
        let closed = MixBrowserView { mixer_collapsed: true, ..view(&[], &[], &names) };
        let foot = 40.0 + 400.0 - crate::mix_mixer::HANDLE_H + 4.0;
        assert!(matches!(hit(&closed, 20.0, foot), Some(BrowserHit::Mixer)));
        let open = MixBrowserView { mixer_collapsed: false, ..closed };
        assert!(hit(&open, 20.0, foot).is_none());
    }

    #[test]
    fn row_rect_matches_hit() {
        let mix = MixDocument::empty("Mix 1", 2);
        let id = mix.id;
        let names = empty_names();
        let takes = [7, 20];
        let view = MixBrowserView {
            selected_mix: Some(id),
            ..view(std::slice::from_ref(&mix), &takes, &names)
        };
        let mix_rect = row_rect(&view, BrowserHit::Mix(id)).unwrap();
        assert!(
            matches!(hit(&view, 20.0, mix_rect.y + 4.0), Some(BrowserHit::Mix(found)) if found == id)
        );
        let take_rect = row_rect(&view, BrowserHit::Take(20)).unwrap();
        assert!(matches!(hit(&view, 20.0, take_rect.y + 4.0), Some(BrowserHit::Take(20))));
    }

    #[test]
    fn paints_seam_between_mixes_and_takes() {
        let names = empty_names();
        let takes = [20];
        let cmds = paint(&MixBrowserView { selected_take: Some(20), ..view(&[], &takes, &names) });
        let seam_y = 40.0 + MIX_LIST_TOP + ROW_H + SECTION_GAP * 0.5 - 1.0;
        let has_seam = cmds.iter().any(|c| match c {
            DrawCmd::Rect { rect, .. } => {
                rect.h <= 1.5 && rect.w > 80.0 && (rect.y - seam_y).abs() < 1.0
            }
            _ => false,
        });
        assert!(has_seam, "expected a horizontal seam above TAKES");
    }

    #[test]
    fn empty_lists_paint_italic_placeholders() {
        let names = empty_names();
        let cmds = paint(&view(&[], &[], &names));
        let placeholders: Vec<_> = cmds
            .iter()
            .filter_map(|c| match c {
                DrawCmd::Text(t) if t.italic => Some(t.text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(placeholders, [EMPTY_MIXES, EMPTY_TAKES]);
        let placeholder_y = 40.0 + MIX_LIST_TOP + 4.0;
        assert!(hit(&view(&[], &[], &names), 20.0, placeholder_y).is_none());
        let takes_placeholder_y = 40.0 + MIX_LIST_TOP + ROW_H + SECTION_GAP + TAKES_HEADER_H + 4.0;
        assert!(hit(&view(&[], &[], &names), 20.0, takes_placeholder_y).is_none());
    }

    #[test]
    fn take_row_uses_display_name() {
        let mut names = empty_names();
        names.insert(7, "Kick stem".into());
        let takes = [7];
        let cmds = paint(&view(&[], &takes, &names));
        let title = cmds.iter().find_map(|c| match c {
            DrawCmd::Text(t) if t.text == "Kick stem" => Some(t.text.as_str()),
            _ => None,
        });
        assert_eq!(title, Some("Kick stem"));
        assert!(!cmds.iter().any(|c| match c {
            DrawCmd::Text(t) => t.text == "Take 7",
            _ => false,
        }));
    }
}
