//! Ghost Mode and Server Anchor Protection for DCC interaction and free camera navigation.

use parking_lot::{RwLock, RwLockWriteGuard};
use std::sync::Arc;

use crate::selection::SelectionBox;

// Gizmo constants
pub const CORNER_NONE: i32 = 0;
pub const CORNER_POS1: i32 = 1;
pub const CORNER_POS2: i32 = 2;
pub const CORNER_CENTER: i32 = 3;

pub const AXIS_NONE: i32 = -1;
pub const AXIS_X: i32 = 0;
pub const AXIS_Y: i32 = 1;
pub const AXIS_Z: i32 = 2;
pub const AXIS_CENTER: i32 = 3;

/// Ghost Mode Manager for client-side camera detachment, DCC navigation, and Gizmo interaction.
#[derive(Debug)]
pub struct GhostMode {
    inner: Arc<RwLock<GhostState>>,
}

#[derive(Debug, Clone)]
pub struct GhostState {
    pub active: bool,
    pub camera_pos: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub pivot_pos: [f64; 3],

    // DCC Navigation flags
    pub is_orbiting: bool,
    pub is_panning: bool,
    pub is_zooming: bool,

    // Gizmo Dragging State
    pub dragging_corner: i32, // 0 = None, 1 = Pos1, 2 = Pos2, 3 = Center
    pub dragging_axis: i32,   // -1 = None, 0 = X, 1 = Y, 2 = Z, 3 = Center
    pub initial_pos1: Option<[i32; 3]>,
    pub initial_pos2: Option<[i32; 3]>,
    pub drag_start_origin: Option<[f64; 3]>,
    pub drag_start_hit: Option<[f64; 3]>,
    pub drag_start_param: f64,
    pub drag_preview_pos1: Option<[i32; 3]>,
    pub drag_preview_pos2: Option<[i32; 3]>,
    pub drag_preview_selection: Option<SelectionBox>,

    // Hovered elements
    pub hovered_corner: i32,
    pub hovered_axis: i32,
    pub hovered_block: Option<[i32; 3]>,

    // Box creation by dragging
    pub is_box_creating: bool,
    pub box_create_start: Option<[i32; 3]>,
    pub box_create_current: Option<[i32; 3]>,
}

impl Default for GhostState {
    fn default() -> Self {
        Self {
            active: false,
            camera_pos: [0.0, 64.0, 0.0],
            yaw: 0.0,
            pitch: 0.0,
            pivot_pos: [0.0, 64.0, 0.0],
            is_orbiting: false,
            is_panning: false,
            is_zooming: false,
            dragging_corner: CORNER_NONE,
            dragging_axis: AXIS_NONE,
            initial_pos1: None,
            initial_pos2: None,
            drag_start_origin: None,
            drag_start_hit: None,
            drag_start_param: 0.0,
            drag_preview_pos1: None,
            drag_preview_pos2: None,
            drag_preview_selection: None,
            hovered_corner: CORNER_NONE,
            hovered_axis: AXIS_NONE,
            hovered_block: None,
            is_box_creating: false,
            box_create_start: None,
            box_create_current: None,
        }
    }
}

impl Default for GhostMode {
    fn default() -> Self {
        Self::new()
    }
}

/// Minecraft forward vector from yaw (yRot) and pitch (xRot).
pub fn forward_vector(yaw: f32, pitch: f32) -> [f64; 3] {
    let p_rad = (pitch as f64).to_radians();
    let y_rad = (yaw as f64).to_radians();
    let len = p_rad.cos();
    [-y_rad.sin() * len, -p_rad.sin(), y_rad.cos() * len]
}

/// Minecraft right vector from yaw (yRot).
pub fn right_vector(yaw: f32) -> [f64; 3] {
    let y_rad = (yaw as f64).to_radians();
    [-y_rad.cos(), 0.0, -y_rad.sin()]
}

/// Minecraft up vector from yaw and pitch: right.cross(forward).
pub fn up_vector(yaw: f32, pitch: f32) -> [f64; 3] {
    let f = forward_vector(yaw, pitch);
    let r = right_vector(yaw);
    [
        r[1] * f[2] - r[2] * f[1],
        r[2] * f[0] - r[0] * f[2],
        r[0] * f[1] - r[1] * f[0],
    ]
}

/// 3D World coordinates to 2D Screen pixel projection.
pub fn project_to_screen(
    pos: [f64; 3],
    camera_pos: [f64; 3],
    yaw: f32,
    pitch: f32,
    screen_size: [f32; 2],
) -> Option<[f32; 2]> {
    let fwd = forward_vector(yaw, pitch);
    let right = right_vector(yaw);
    let up = up_vector(yaw, pitch);

    let rel = [pos[0] - camera_pos[0], pos[1] - camera_pos[1], pos[2] - camera_pos[2]];
    let depth = rel[0] * fwd[0] + rel[1] * fwd[1] + rel[2] * fwd[2];
    if depth <= 0.1 {
        return None; // Behind camera
    }

    let aspect = (screen_size[0] / screen_size[1].max(1.0)) as f64;
    let fov_rad = 70.0_f64.to_radians();
    let tan_half_y = (fov_rad * 0.5).tan();
    let tan_half_x = tan_half_y * aspect;

    let x_view = rel[0] * right[0] + rel[1] * right[1] + rel[2] * right[2];
    let y_view = rel[0] * up[0] + rel[1] * up[1] + rel[2] * up[2];

    let ndc_x = x_view / (depth * tan_half_x);
    let ndc_y = y_view / (depth * tan_half_y);

    let screen_x = ((ndc_x + 1.0) * 0.5) * screen_size[0] as f64;
    let screen_y = ((1.0 - ndc_y) * 0.5) * screen_size[1] as f64;

    Some([screen_x as f32, screen_y as f32])
}

/// Generates a 3D ray from 2D screen mouse coordinates.
pub fn screen_mouse_ray(
    mouse: [f32; 2],
    screen_size: [f32; 2],
    camera_pos: [f64; 3],
    yaw: f32,
    pitch: f32,
) -> ([f64; 3], [f64; 3]) {
    let fwd = forward_vector(yaw, pitch);
    let right = right_vector(yaw);
    let up = up_vector(yaw, pitch);

    let aspect = (screen_size[0] / screen_size[1].max(1.0)) as f64;
    let fov_rad = 70.0_f64.to_radians();
    let tan_half_y = (fov_rad * 0.5).tan();
    let tan_half_x = tan_half_y * aspect;

    let nx = (2.0 * mouse[0] as f64 / screen_size[0] as f64) - 1.0;
    let ny = 1.0 - (2.0 * mouse[1] as f64 / screen_size[1] as f64);

    let dir = [
        fwd[0] + right[0] * (nx * tan_half_x) + up[0] * (ny * tan_half_y),
        fwd[1] + right[1] * (nx * tan_half_x) + up[1] * (ny * tan_half_y),
        fwd[2] + right[2] * (nx * tan_half_x) + up[2] * (ny * tan_half_y),
    ];
    let len = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt().max(1e-6);
    let dir_norm = [dir[0] / len, dir[1] / len, dir[2] / len];
    (camera_pos, dir_norm)
}

/// Distance from a 2D point to a 2D line segment.
pub fn distance_point_to_segment_2d(px: f32, py: f32, s1: [f32; 2], s2: [f32; 2]) -> f32 {
    let vx = s2[0] - s1[0];
    let vy = s2[1] - s1[1];
    let wx = px - s1[0];
    let wy = py - s1[1];

    let len_sq = vx * vx + vy * vy;
    if len_sq < 1e-6 {
        return ((px - s1[0]).powi(2) + (py - s1[1]).powi(2)).sqrt();
    }

    let t = ((wx * vx + wy * vy) / len_sq).clamp(0.0, 1.0);
    let proj_x = s1[0] + t * vx;
    let proj_y = s1[1] + t * vy;

    ((px - proj_x).powi(2) + (py - proj_y).powi(2)).sqrt()
}

impl GhostMode {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(GhostState::default())),
        }
    }

    #[inline]
    pub fn is_active(&self) -> bool {
        self.inner.read().active
    }

    pub fn set_active(&self, active: bool) {
        let mut state = self.inner.write();
        state.active = active;
        if !active {
            state.is_orbiting = false;
            state.is_panning = false;
            state.is_zooming = false;
            state.dragging_corner = CORNER_NONE;
            state.dragging_axis = AXIS_NONE;
            state.drag_preview_selection = None;
            state.is_box_creating = false;
        }
    }

    /// Initializes ghost camera position from live camera pose.
    pub fn init_from_camera(
        &self,
        eye_pos: [f64; 3],
        yaw: f32,
        pitch: f32,
        selection_center: Option<[f64; 3]>,
    ) {
        let mut state = self.inner.write();
        state.active = true;
        state.camera_pos = eye_pos;
        state.yaw = yaw;
        state.pitch = pitch;
        if let Some(center) = selection_center {
            state.pivot_pos = center;
        } else {
            let fwd = forward_vector(yaw, pitch);
            state.pivot_pos = [
                eye_pos[0] + fwd[0] * 8.0,
                eye_pos[1] + fwd[1] * 8.0,
                eye_pos[2] + fwd[2] * 8.0,
            ];
        }
        state.is_orbiting = false;
        state.is_panning = false;
        state.is_zooming = false;
        state.dragging_corner = CORNER_NONE;
        state.dragging_axis = AXIS_NONE;
        state.drag_preview_selection = None;
        state.is_box_creating = false;
    }

    pub fn focus_selection(&self, center: [f64; 3], max_side: f64) {
        let mut state = self.inner.write();
        state.pivot_pos = center;
        let dist = (max_side * 1.5).clamp(6.0, 150.0);
        let fwd = forward_vector(state.yaw, state.pitch);
        state.camera_pos = [
            center[0] - fwd[0] * dist,
            center[1] - fwd[1] * dist,
            center[2] - fwd[2] * dist,
        ];
    }

    pub fn toggle(&self) -> bool {
        let mut state = self.inner.write();
        state.active = !state.active;
        let active = state.active;
        if !active {
            state.is_orbiting = false;
            state.is_panning = false;
            state.is_zooming = false;
            state.dragging_corner = CORNER_NONE;
            state.dragging_axis = AXIS_NONE;
            state.drag_preview_selection = None;
            state.is_box_creating = false;
        }
        active
    }

    pub fn state(&self) -> GhostState {
        self.inner.read().clone()
    }

    pub fn write_state(&self) -> RwLockWriteGuard<'_, GhostState> {
        self.inner.write()
    }

    pub fn update_camera(&self, pos: [f64; 3], yaw: f32, pitch: f32) {
        let mut state = self.inner.write();
        state.camera_pos = pos;
        state.yaw = yaw;
        state.pitch = pitch;
    }

    pub fn set_pivot(&self, pivot: [f64; 3]) {
        self.inner.write().pivot_pos = pivot;
    }

    pub fn set_drag_preview(&self, sel: Option<SelectionBox>) {
        self.inner.write().drag_preview_selection = sel;
    }

    pub fn set_hovered_block(&self, pos: Option<[i32; 3]>) {
        self.inner.write().hovered_block = pos;
    }
}

/// Server-side ghost anchor to pin players and prevent survival damage/gravity drift.
#[derive(Debug, Default)]
pub struct GhostAnchorManager {
    anchors: RwLock<std::collections::HashMap<String, AnchorRecord>>,
}

#[derive(Debug, Clone)]
pub struct AnchorRecord {
    pub dimension: String,
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl GhostAnchorManager {
    pub fn new() -> Self {
        Self {
            anchors: RwLock::new(std::collections::HashMap::new()),
        }
    }

    pub fn pin_player(&self, player_id: &str, dimension: &str, pos: [f64; 3]) {
        self.anchors.write().insert(
            player_id.to_string(),
            AnchorRecord {
                dimension: dimension.to_string(),
                x: pos[0],
                y: pos[1],
                z: pos[2],
            },
        );
    }

    pub fn unpin_player(&self, player_id: &str) -> Option<AnchorRecord> {
        self.anchors.write().remove(player_id)
    }

    pub fn is_pinned(&self, player_id: &str) -> bool {
        self.anchors.read().contains_key(player_id)
    }

    pub fn get_anchor(&self, player_id: &str) -> Option<AnchorRecord> {
        self.anchors.read().get(player_id).cloned()
    }

    pub fn clear(&self) {
        self.anchors.write().clear();
    }
}
