//! Widget / input / layout core. Paint + hit-test live in `ui-mixlink`;
//! this crate is the shared geometry and input vocabulary.

#![forbid(unsafe_op_in_unsafe_fn)]

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }

    pub fn inset(&self, dx: f32, dy: f32) -> Self {
        Self {
            x: self.x + dx,
            y: self.y + dy,
            w: (self.w - dx * 2.0).max(0.0),
            h: (self.h - dy * 2.0).max(0.0),
        }
    }

    pub fn intersect(&self, other: &Rect) -> Option<Rect> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let r = (self.x + self.w).min(other.x + other.w);
        let b = (self.y + self.h).min(other.y + other.h);
        if r > x && b > y {
            Some(Rect { x, y, w: r - x, h: b - y })
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InputEvent {
    MouseMoved { x: f32, y: f32 },
    MouseDown { x: f32, y: f32, button: MouseButton },
    MouseUp { x: f32, y: f32, button: MouseButton },
    Scroll { dx: f32, dy: f32 },
    Key { key: u32, pressed: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
    pub super_key: bool,
}

/// Pixels of movement that promote a press into a drag (matches MixLink clip).
pub const CLICK_DRAG_THRESHOLD: f32 = 2.0;
/// Empty-lane bar-select threshold.
pub const SELECT_DRAG_THRESHOLD: f32 = 3.0;
/// Ruler zoom vs locate threshold.
pub const RULER_DRAG_THRESHOLD: f32 = 3.0;
