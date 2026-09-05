//! MixLink look: paint + hit from Copy layout structs. Not rustest Live widgets.

pub mod arrangement;
pub mod chrome;
pub mod deck;
pub mod hit;
pub mod mix_browser;
pub mod mix_mixer;
pub mod mixer;
pub mod overlay;
pub mod sidebar;
pub mod theme;
pub mod widgets;

pub use chrome::{Page, HEADER_H};
pub use mixer::MixerLayout;
pub use theme::{Color, Layout, SurfaceStyle};
