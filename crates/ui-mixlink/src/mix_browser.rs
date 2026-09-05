//! Mix page left browser: MIXES / TAKES, 148 pt (MixLink `MixBrowserView`).

use project::{take_wav_name, MixDocument, MixLane};
use render::{DrawCmd, Rect};

use crate::theme;

pub const WIDTH: f32 = 148.0;
const MIX_LIST_TOP: f32 = 28.0;
const ROW_H: f32 = 24.0;
const SECTION_GAP: f32 = 12.0;
const TAKES_HEADER_H: f32 = 20.0;

pub struct MixBrowserView<'a> {
    pub x: f32,
    pub y: f32,
    pub h: f32,
    pub mixes: &'a [MixDocument],
    pub selected_mix: Option<uuid::Uuid>,
    pub takes: &'a [i32],
    pub selected_take: Option<i32>,
    pub mixer_collapsed: bool,
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
    for mix in view.mixes {
        // Arrangement selection: a take and a mix are never both highlighted.
        let sel = view.selected_take.is_none() && view.selected_mix == Some(mix.id);
        paint_row(&mut cmds, view.x, y, &mix.name, sel);
        y += ROW_H;
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
    for take in view.takes {
        let sel = view.selected_take == Some(*take);
        paint_row(&mut cmds, view.x, y, format!("Take {take}"), sel);
        y += 24.0;
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

fn paint_row(cmds: &mut Vec<DrawCmd>, x: f32, y: f32, title: impl AsRef<str>, selected: bool) {
    if selected {
        theme::fill(cmds, Rect { x, y, w: WIDTH, h: 22.0 }, theme::BROWSER_SELECTED);
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

pub fn hit(view: &MixBrowserView<'_>, x: f32, y: f32) -> Option<BrowserHit> {
    if x < view.x || x > view.x + WIDTH || y < view.y || y > view.y + view.h {
        return None;
    }
    if view.mixer_collapsed && y >= view.y + view.h - crate::mix_mixer::HANDLE_H {
        return Some(BrowserHit::Mixer);
    }
    let mut yy = view.y + MIX_LIST_TOP;
    for mix in view.mixes {
        if y >= yy && y < yy + ROW_H {
            return Some(BrowserHit::Mix(mix.id));
        }
        yy += ROW_H;
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
                + view.mixes.len() as f32 * ROW_H
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

    #[test]
    fn drawer_cover_is_opaque() {
        let cmds = paint(&MixBrowserView {
            x: 0.0,
            y: 40.0,
            h: 400.0,
            mixes: &[],
            selected_mix: None,
            takes: &[],
            selected_take: None,
            mixer_collapsed: false,
        });
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
        let view = MixBrowserView {
            x: 0.0,
            y: 40.0,
            h: 400.0,
            mixes: &[],
            selected_mix: None,
            takes: &[],
            selected_take: None,
            mixer_collapsed: true,
        };
        let foot = 40.0 + 400.0 - crate::mix_mixer::HANDLE_H + 4.0;
        assert!(matches!(hit(&view, 20.0, foot), Some(BrowserHit::Mixer)));
        let open = MixBrowserView { mixer_collapsed: false, ..view };
        assert!(hit(&open, 20.0, foot).is_none());
    }

    #[test]
    fn row_rect_matches_hit() {
        let mix = MixDocument::empty("Mix 1", 2);
        let id = mix.id;
        let view = MixBrowserView {
            x: 0.0,
            y: 40.0,
            h: 400.0,
            mixes: std::slice::from_ref(&mix),
            selected_mix: Some(id),
            takes: &[7, 20],
            selected_take: None,
            mixer_collapsed: false,
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
        let cmds = paint(&MixBrowserView {
            x: 0.0,
            y: 40.0,
            h: 400.0,
            mixes: &[],
            selected_mix: None,
            takes: &[20],
            selected_take: Some(20),
            mixer_collapsed: false,
        });
        let seam_y = 40.0 + MIX_LIST_TOP + SECTION_GAP * 0.5 - 1.0;
        let has_seam = cmds.iter().any(|c| match c {
            DrawCmd::Rect { rect, .. } => {
                rect.h <= 1.5 && rect.w > 80.0 && (rect.y - seam_y).abs() < 1.0
            }
            _ => false,
        });
        assert!(has_seam, "expected a horizontal seam above TAKES");
    }
}
