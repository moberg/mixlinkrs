//! Mix page left browser: MIXES / TAKES, 148 pt (MixLink `MixBrowserView`).

use project::{take_wav_name, MixDocument, MixLane};
use render::{DrawCmd, Rect};

use crate::theme;

pub const WIDTH: f32 = 148.0;

pub struct MixBrowserView<'a> {
    pub x: f32,
    pub y: f32,
    pub h: f32,
    pub mixes: &'a [MixDocument],
    pub selected_mix: Option<uuid::Uuid>,
    pub takes: &'a [i32],
    pub selected_take: Option<i32>,
}

pub fn paint(view: &MixBrowserView<'_>) -> Vec<DrawCmd> {
    let mut cmds = Vec::new();
    theme::fill(&mut cmds, Rect { x: view.x, y: view.y, w: WIDTH, h: view.h }, [0.0, 0.0, 0.0, 0.28]);
    theme::seam_v(&mut cmds, view.x + WIDTH - 1.0, view.y, view.h, true);

    theme::text(&mut cmds, Rect { x: view.x + 10.0, y: view.y + 8.0, w: 70.0, h: 16.0 }, "MIXES", 10.0, theme::TEXT_DIM, true);
    crate::widgets::icon_pad(&mut cmds, Rect { x: view.x + WIDTH - 52.0, y: view.y + 6.0, w: 20.0, h: 18.0 }, "+", true);
    crate::widgets::icon_pad(&mut cmds, Rect { x: view.x + WIDTH - 28.0, y: view.y + 6.0, w: 20.0, h: 18.0 }, "−", true);
    let mut y = view.y + 28.0;
    for mix in view.mixes {
        let sel = view.selected_mix == Some(mix.id);
        if sel {
            theme::fill(
                &mut cmds,
                Rect { x: view.x + 4.0, y, w: WIDTH - 8.0, h: 22.0 },
                [theme::ORANGE[0], theme::ORANGE[1], theme::ORANGE[2], 0.22],
            );
        }
        theme::text(
            &mut cmds,
            Rect { x: view.x + 10.0, y, w: WIDTH - 16.0, h: 22.0 },
            &mix.name,
            11.0,
            if sel { theme::TEXT } else { theme::TEXT_DIM },
            sel,
        );
        y += 24.0;
    }

    y += 12.0;
    theme::text(&mut cmds, Rect { x: view.x + 10.0, y, w: 120.0, h: 16.0 }, "TAKES", 10.0, theme::TEXT_DIM, true);
    y += 20.0;
    for take in view.takes {
        let sel = view.selected_take == Some(*take);
        if sel {
            theme::fill(
                &mut cmds,
                Rect { x: view.x + 4.0, y, w: WIDTH - 8.0, h: 22.0 },
                [theme::BLUE[0], theme::BLUE[1], theme::BLUE[2], 0.22],
            );
        }
        theme::text(
            &mut cmds,
            Rect { x: view.x + 10.0, y, w: WIDTH - 16.0, h: 22.0 },
            format!("Take {take}"),
            11.0,
            if sel { theme::TEXT } else { theme::TEXT_DIM },
            sel,
        );
        y += 24.0;
    }
    let _ = take_wav_name(1, MixLane::Main, "");
    cmds
}

pub fn hit(view: &MixBrowserView<'_>, x: f32, y: f32) -> Option<BrowserHit> {
    if x < view.x || x > view.x + WIDTH || y < view.y || y > view.y + view.h {
        return None;
    }
    if y >= view.y + 6.0 && y < view.y + 24.0 {
        if x >= view.x + WIDTH - 52.0 && x < view.x + WIDTH - 32.0 {
            return Some(BrowserHit::NewMix);
        }
        if x >= view.x + WIDTH - 28.0 && x < view.x + WIDTH - 8.0 {
            return Some(BrowserHit::DeleteMix);
        }
    }
    let mut yy = view.y + 28.0;
    for mix in view.mixes {
        if y >= yy && y < yy + 24.0 {
            return Some(BrowserHit::Mix(mix.id));
        }
        yy += 24.0;
    }
    yy += 32.0;
    for take in view.takes {
        if y >= yy && y < yy + 24.0 {
            return Some(BrowserHit::Take(*take));
        }
        yy += 24.0;
    }
    None
}

#[derive(Clone, Copy, Debug)]
pub enum BrowserHit {
    Mix(uuid::Uuid),
    Take(i32),
    NewMix,
    DeleteMix,
}
