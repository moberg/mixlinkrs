//! Arrangement view: selection, zoom, scroll, grid, locate, clip preview.

use project::{ArrSelection, MixClip, MixGrid, MixLane};

impl Default for Timeline {
    fn default() -> Self {
        Self {
            selected_lane: None,
            selection: ArrSelection::default(),
            pixels_per_bar: 48.0,
            scroll_x: 0.0,
            scroll_y: 0.0,
            grid: MixGrid::Bar1,
            grid_enabled: true,
            tempo: 120.0,
            locate_frame: 0,
            arrangement_origin: 0,
            clip_preview: None,
            clip_readout: None,
            automation_armed: false,
        }
    }
}

pub(crate) struct Timeline {
    pub selected_lane: Option<MixLane>,
    pub selection: ArrSelection,
    pub pixels_per_bar: f32,
    pub scroll_x: f32,
    pub scroll_y: f32,
    pub grid: MixGrid,
    pub grid_enabled: bool,
    pub tempo: f64,
    pub locate_frame: i64,
    pub arrangement_origin: i64,
    pub clip_preview: Option<Vec<(MixLane, MixClip)>>,
    pub clip_readout: Option<String>,
    pub automation_armed: bool,
}
