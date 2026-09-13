//! Arrangement cursors: Ableton-style `[` / `]` trim and a timeline loupe.

use winit::event_loop::ActiveEventLoop;
use winit::window::{CursorIcon, CustomCursor, Window};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ArrCursor {
    #[default]
    Default,
    TrimLeft,
    TrimRight,
    Zoom,
    ColResize,
}

#[derive(Clone)]
pub struct TrimCursors {
    left: CustomCursor,
    right: CustomCursor,
    zoom: CustomCursor,
}

impl TrimCursors {
    pub fn create(event_loop: &ActiveEventLoop) -> Option<Self> {
        let left = event_loop.create_custom_cursor(bracket_source(true)?);
        let right = event_loop.create_custom_cursor(bracket_source(false)?);
        let zoom = event_loop.create_custom_cursor(glass_source()?);
        Some(Self { left, right, zoom })
    }

    pub fn apply(&self, window: &Window, kind: ArrCursor) {
        match kind {
            ArrCursor::Default => window.set_cursor(CursorIcon::Default),
            ArrCursor::TrimLeft => window.set_cursor(self.left.clone()),
            ArrCursor::TrimRight => window.set_cursor(self.right.clone()),
            ArrCursor::Zoom => window.set_cursor(self.zoom.clone()),
            ArrCursor::ColResize => window.set_cursor(CursorIcon::ColResize),
        }
    }
}

pub fn apply_fallback(window: &Window, kind: ArrCursor) {
    window.set_cursor(match kind {
        ArrCursor::Default => CursorIcon::Default,
        ArrCursor::TrimLeft => CursorIcon::WResize,
        ArrCursor::TrimRight => CursorIcon::EResize,
        ArrCursor::Zoom => CursorIcon::ZoomIn,
        ArrCursor::ColResize => CursorIcon::ColResize,
    });
}

fn bracket_source(left: bool) -> Option<winit::window::CustomCursorSource> {
    let (rgba, hx, hy) = bracket_rgba(left);
    CustomCursor::from_rgba(rgba, SIZE as u16, SIZE as u16, hx, hy).ok()
}

fn glass_source() -> Option<winit::window::CustomCursorSource> {
    let (rgba, hx, hy) = glass_rgba();
    CustomCursor::from_rgba(rgba, SIZE as u16, SIZE as u16, hx, hy).ok()
}

const SIZE: usize = 18;

fn bracket_rgba(left: bool) -> (Vec<u8>, u16, u16) {
    let mut ink = vec![false; SIZE * SIZE];
    let mut stroke = |x0: usize, y0: usize, x1: usize, y1: usize| {
        for y in y0..=y1 {
            for x in x0..=x1 {
                ink[y * SIZE + x] = true;
            }
        }
    };
    if left {
        stroke(5, 3, 5, 14);
        stroke(5, 3, 10, 3);
        stroke(5, 14, 10, 14);
    } else {
        stroke(12, 3, 12, 14);
        stroke(7, 3, 12, 3);
        stroke(7, 14, 12, 14);
    }
    let hx = if left { 5 } else { 12 };
    ink_to_rgba(&ink, hx, 9)
}

fn glass_rgba() -> (Vec<u8>, u16, u16) {
    let mut ink = vec![false; SIZE * SIZE];
    let mut plot = |x: i32, y: i32| {
        if (0..SIZE as i32).contains(&x) && (0..SIZE as i32).contains(&y) {
            ink[y as usize * SIZE + x as usize] = true;
        }
    };
    let cx = 7i32;
    let cy = 6i32;
    for y in 0..SIZE as i32 {
        for x in 0..SIZE as i32 {
            let dx = x - cx;
            let dy = y - cy;
            let d2 = dx * dx + dy * dy;
            if d2 >= 16 && d2 <= 30 {
                plot(x, y);
            }
        }
    }
    for i in 0..=4 {
        plot(11 + i, 10 + i);
        plot(12 + i, 10 + i);
        plot(11 + i, 11 + i);
    }
    ink_to_rgba(&ink, 7, 6)
}

fn ink_to_rgba(ink: &[bool], hx: u16, hy: u16) -> (Vec<u8>, u16, u16) {
    let mut rgba = vec![0u8; SIZE * SIZE * 4];
    for y in 0..SIZE {
        for x in 0..SIZE {
            if ink[y * SIZE + x] {
                continue;
            }
            let mut near = false;
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    let nx = x as i32 + dx;
                    let ny = y as i32 + dy;
                    if (0..SIZE as i32).contains(&nx)
                        && (0..SIZE as i32).contains(&ny)
                        && ink[ny as usize * SIZE + nx as usize]
                    {
                        near = true;
                    }
                }
            }
            if near {
                let i = (y * SIZE + x) * 4;
                rgba[i] = 0;
                rgba[i + 1] = 0;
                rgba[i + 2] = 0;
                rgba[i + 3] = 200;
            }
        }
    }
    for y in 0..SIZE {
        for x in 0..SIZE {
            if ink[y * SIZE + x] {
                let i = (y * SIZE + x) * 4;
                rgba[i] = 255;
                rgba[i + 1] = 255;
                rgba[i + 2] = 255;
                rgba[i + 3] = 255;
            }
        }
    }
    (rgba, hx, hy)
}
