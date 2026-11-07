//! Selection Box and Gizmo visual styling constants.
//! Aligned with Yefira Java SelectionStyle and Horizon 26.3.

/// Selection box (Committed)
pub const COLOR_NORMAL_STROKE: u32 = 0xE600FFFF; // Cyan outline (alpha ~0.90)
pub const COLOR_NORMAL_FILL: u32 = 0x3300FFFF;   // Subtle cyan fill (alpha ~0.20)
pub const WIDTH_NORMAL_STROKE: f32 = 2.0;

pub const COLOR_OVERSIZED_STROKE: u32 = 0xF0FF2222; // Alert red outline
pub const COLOR_OVERSIZED_FILL: u32 = 0x40FF2222;   // Subtle red fill
pub const WIDTH_OVERSIZED_STROKE: f32 = 2.5;

/// Drag preview box (Active dragging in Ghost Mode)
pub const COLOR_PREVIEW_NORMAL_STROKE: u32 = 0xE600FF00; // Lime green outline
pub const COLOR_PREVIEW_NORMAL_FILL: u32 = 0x4000FF00;   // Translucent green fill
pub const WIDTH_PREVIEW_STROKE: f32 = 2.5;

pub const COLOR_PREVIEW_OVERSIZED_STROKE: u32 = 0xF0FF3300; // Red-orange outline
pub const COLOR_PREVIEW_OVERSIZED_FILL: u32 = 0x4DFF3300;   // Red-orange fill

/// Free cursor hover block outline
pub const COLOR_HOVER_BLOCK_STROKE: u32 = 0xFF00FFEE;
pub const COLOR_HOVER_BLOCK_FILL: u32 = 0x3300FFEE;
pub const WIDTH_HOVER_BLOCK_STROKE: f32 = 3.0;

/// Box creation drag preview
pub const COLOR_BOX_CREATE_NORMAL_STROKE: u32 = 0xFF00FF55;
pub const COLOR_BOX_CREATE_NORMAL_FILL: u32 = 0x4000FF55;
pub const COLOR_BOX_CREATE_OVERSIZED_STROKE: u32 = 0xFFFF3300;
pub const COLOR_BOX_CREATE_OVERSIZED_FILL: u32 = 0x4DFF3300;
pub const WIDTH_BOX_CREATE_STROKE: f32 = 3.5;

/// Gizmo axes and handles
pub const COLOR_HANDLE_X: u32 = 0xFFFF3333; // Red
pub const COLOR_HANDLE_Y: u32 = 0xFF33FF33; // Green
pub const COLOR_HANDLE_Z: u32 = 0xFF3388FF; // Blue
pub const COLOR_HANDLE_CENTER: u32 = 0xFFFFCC00; // Gold
pub const COLOR_HANDLE_HIGHLIGHT: u32 = 0xFFFFFF55; // Bright Yellow
pub const COLOR_HANDLE_POS1: u32 = 0xFF00FFCC; // Cyan-Green
pub const COLOR_HANDLE_POS2: u32 = 0xFFFF00CC; // Magenta

pub const HANDLE_AXIS_RATIO: f32 = 0.15;
pub const HANDLE_CENTER_RATIO: f32 = 0.018;

#[inline]
pub fn argb_to_f32_rgba(argb: u32) -> [f32; 4] {
    let a = ((argb >> 24) & 0xFF) as f32 / 255.0;
    let r = ((argb >> 16) & 0xFF) as f32 / 255.0;
    let g = ((argb >> 8) & 0xFF) as f32 / 255.0;
    let b = (argb & 0xFF) as f32 / 255.0;
    [r, g, b, a]
}
