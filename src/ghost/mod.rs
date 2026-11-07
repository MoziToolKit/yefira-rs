//! Ghost Mode and Server Anchor Protection for DCC interaction and free camera navigation.

use parking_lot::{RwLock, RwLockWriteGuard};
use std::sync::Arc;

use crate::selection::{SelectionBox, SelectionManager};

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
    fov_degrees: f32,
) -> Option<[f32; 2]> {
    let fwd = forward_vector(yaw, pitch);
    let right = right_vector(yaw);
    let up = up_vector(yaw, pitch);

    let rel = [
        pos[0] - camera_pos[0],
        pos[1] - camera_pos[1],
        pos[2] - camera_pos[2],
    ];
    let depth = rel[0] * fwd[0] + rel[1] * fwd[1] + rel[2] * fwd[2];
    if depth <= 0.1 {
        return None; // Behind camera
    }

    let aspect = (screen_size[0] / screen_size[1].max(1.0)) as f64;
    let effective_fov = if fov_degrees > 1.0 { fov_degrees } else { 70.0 };
    let fov_rad = (effective_fov as f64).to_radians();
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
    fov_degrees: f32,
) -> ([f64; 3], [f64; 3]) {
    let fwd = forward_vector(yaw, pitch);
    let right = right_vector(yaw);
    let up = up_vector(yaw, pitch);

    let aspect = (screen_size[0] / screen_size[1].max(1.0)) as f64;
    let effective_fov = if fov_degrees > 1.0 { fov_degrees } else { 70.0 };
    let fov_rad = (effective_fov as f64).to_radians();
    let tan_half_y = (fov_rad * 0.5).tan();
    let tan_half_x = tan_half_y * aspect;

    let nx = (2.0 * mouse[0] as f64 / screen_size[0] as f64) - 1.0;
    let ny = 1.0 - (2.0 * mouse[1] as f64 / screen_size[1] as f64);

    let dir = [
        fwd[0] + right[0] * (nx * tan_half_x) + up[0] * (ny * tan_half_y),
        fwd[1] + right[1] * (nx * tan_half_x) + up[1] * (ny * tan_half_y),
        fwd[2] + right[2] * (nx * tan_half_x) + up[2] * (ny * tan_half_y),
    ];
    let len = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2])
        .sqrt()
        .max(1e-6);
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

/// Gets the unit direction vector for coordinate axes (0=X, 1=Y, 2=Z).
pub fn get_axis_direction(axis: i32) -> glam::DVec3 {
    match axis {
        AXIS_X => glam::DVec3::new(1.0, 0.0, 0.0),
        AXIS_Y => glam::DVec3::new(0.0, 1.0, 0.0),
        AXIS_Z => glam::DVec3::new(0.0, 0.0, 1.0),
        _ => glam::DVec3::new(0.0, 1.0, 0.0),
    }
}

/// Returns the 3D world origin for a Gizmo handle corner.
pub fn get_gizmo_origin(
    corner: i32,
    p1: Option<[i32; 3]>,
    p2: Option<[i32; 3]>,
) -> Option<[f64; 3]> {
    match corner {
        CORNER_POS1 => p1.map(|p| [p[0] as f64 + 0.5, p[1] as f64 + 0.5, p[2] as f64 + 0.5]),
        CORNER_POS2 => p2.map(|p| [p[0] as f64 + 0.5, p[1] as f64 + 0.5, p[2] as f64 + 0.5]),
        CORNER_CENTER => match (p1, p2) {
            (Some(a), Some(b)) => {
                let min_x = a[0].min(b[0]) as f64;
                let max_x = a[0].max(b[0]) as f64;
                let min_y = a[1].min(b[1]) as f64;
                let max_y = a[1].max(b[1]) as f64;
                let min_z = a[2].min(b[2]) as f64;
                let max_z = a[2].max(b[2]) as f64;
                Some([
                    (min_x + max_x + 1.0) * 0.5,
                    (min_y + max_y + 1.0) * 0.5,
                    (min_z + max_z + 1.0) * 0.5,
                ])
            }
            _ => None,
        },
        _ => None,
    }
}

/// Intersects ray with a plane containing axis_dir and facing the camera.
/// Returns parameter t along axis_dir.
pub fn intersect_ray_with_axis_plane(
    ray_origin: glam::DVec3,
    ray_dir: glam::DVec3,
    axis_origin: glam::DVec3,
    axis_dir: glam::DVec3,
    cam_look: glam::DVec3,
) -> f64 {
    let normal_raw = axis_dir.cross(cam_look).cross(axis_dir);
    let normal = if normal_raw.length_squared() < 1e-6 {
        cam_look
    } else {
        normal_raw.normalize()
    };

    let denom = ray_dir.dot(normal);
    if denom.abs() < 1e-6 {
        return 0.0;
    }

    let t = (axis_origin - ray_origin).dot(normal) / denom;
    let hit_point = ray_origin + ray_dir * t;
    (hit_point - axis_origin).dot(axis_dir)
}

/// Intersects ray with a camera-facing plane passing through plane_origin.
pub fn intersect_ray_with_camera_plane(
    ray_origin: glam::DVec3,
    ray_dir: glam::DVec3,
    plane_origin: glam::DVec3,
    cam_look: glam::DVec3,
) -> glam::DVec3 {
    let normal = cam_look;
    let denom = ray_dir.dot(normal);
    if denom.abs() < 1e-6 {
        return plane_origin;
    }
    let t = (plane_origin - ray_origin).dot(normal) / denom;
    ray_origin + ray_dir * t
}

/// Whether a block name represents a selectable solid block (non-air, non-fluid).
pub fn is_selectable_block(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let bare = name.strip_prefix("minecraft:").unwrap_or(name);
    !matches!(
        bare,
        "air" | "cave_air" | "void_air" | "water" | "flowing_water" | "lava" | "flowing_lava"
    )
}

/// Amanatides-Woo Fast Voxel Traversal algorithm for screen mouse raycasting.
pub fn raycast_block_from_mouse(
    origin: [f64; 3],
    dir: [f64; 3],
    max_dist: f64,
    get_block: impl Fn([i32; 3]) -> String,
) -> Option<[i32; 3]> {
    let mut x = origin[0].floor() as i32;
    let mut y = origin[1].floor() as i32;
    let mut z = origin[2].floor() as i32;

    let step_x = if dir[0] > 0.0 {
        1
    } else if dir[0] < 0.0 {
        -1
    } else {
        0
    };
    let step_y = if dir[1] > 0.0 {
        1
    } else if dir[1] < 0.0 {
        -1
    } else {
        0
    };
    let step_z = if dir[2] > 0.0 {
        1
    } else if dir[2] < 0.0 {
        -1
    } else {
        0
    };

    let t_delta_x = if dir[0] != 0.0 {
        (1.0 / dir[0]).abs()
    } else {
        f64::INFINITY
    };
    let t_delta_y = if dir[1] != 0.0 {
        (1.0 / dir[1]).abs()
    } else {
        f64::INFINITY
    };
    let t_delta_z = if dir[2] != 0.0 {
        (1.0 / dir[2]).abs()
    } else {
        f64::INFINITY
    };

    let mut t_max_x = if dir[0] > 0.0 {
        ((x as f64 + 1.0) - origin[0]) / dir[0]
    } else if dir[0] < 0.0 {
        (origin[0] - x as f64) / (-dir[0])
    } else {
        f64::INFINITY
    };

    let mut t_max_y = if dir[1] > 0.0 {
        ((y as f64 + 1.0) - origin[1]) / dir[1]
    } else if dir[1] < 0.0 {
        (origin[1] - y as f64) / (-dir[1])
    } else {
        f64::INFINITY
    };

    let mut t_max_z = if dir[2] > 0.0 {
        ((z as f64 + 1.0) - origin[2]) / dir[2]
    } else if dir[2] < 0.0 {
        (origin[2] - z as f64) / (-dir[2])
    } else {
        f64::INFINITY
    };

    let mut traveled = 0.0;
    while traveled <= max_dist {
        let block_name = get_block([x, y, z]);
        if is_selectable_block(&block_name) {
            return Some([x, y, z]);
        }

        if t_max_x < t_max_y {
            if t_max_x < t_max_z {
                traveled = t_max_x;
                x += step_x;
                t_max_x += t_delta_x;
            } else {
                traveled = t_max_z;
                z += step_z;
                t_max_z += t_delta_z;
            }
        } else if t_max_y < t_max_z {
            traveled = t_max_y;
            y += step_y;
            t_max_y += t_delta_y;
        } else {
            traveled = t_max_z;
            z += step_z;
            t_max_z += t_delta_z;
        }
    }
    None
}

/// Screen-space Gizmo and Block hover detection aligned with Java 1:1.
pub fn update_gizmo_hover(
    state: &mut GhostState,
    selection_mgr: &SelectionManager,
    mouse: [f32; 2],
    screen_size: [f32; 2],
    fov_degrees: f32,
    get_block: impl Fn([i32; 3]) -> String,
) {
    let cam_pos = state.camera_pos;
    let yaw = state.yaw;
    let pitch = state.pitch;

    let mut closest_dist = 24.0_f32;
    let mut best_corner = CORNER_NONE;
    let mut best_axis = AXIS_NONE;

    let p1 = if state.dragging_corner != CORNER_NONE && state.drag_preview_pos1.is_some() {
        state.drag_preview_pos1
    } else {
        selection_mgr.pos1()
    };
    let p2 = if state.dragging_corner != CORNER_NONE && state.drag_preview_pos2.is_some() {
        state.drag_preview_pos2
    } else {
        selection_mgr.pos2()
    };

    // 1. Test Pos1 Handle
    if let Some(pos1) = p1 {
        let origin = [
            pos1[0] as f64 + 0.5,
            pos1[1] as f64 + 0.5,
            pos1[2] as f64 + 0.5,
        ];
        if let Some(screen_origin) =
            project_to_screen(origin, cam_pos, yaw, pitch, screen_size, fov_degrees)
        {
            let dist = ((origin[0] - cam_pos[0]).powi(2)
                + (origin[1] - cam_pos[1]).powi(2)
                + (origin[2] - cam_pos[2]).powi(2))
            .sqrt()
            .max(0.5);
            let axis_len = (dist * 0.15).max(0.3);

            let center_dist = ((mouse[0] - screen_origin[0]).powi(2)
                + (mouse[1] - screen_origin[1]).powi(2))
            .sqrt();
            if center_dist < 20.0 && center_dist < closest_dist {
                closest_dist = center_dist;
                best_corner = CORNER_POS1;
                best_axis = AXIS_CENTER;
            }

            for axis in 0..3 {
                let dir = get_axis_direction(axis);
                let end = [
                    origin[0] + dir.x * axis_len,
                    origin[1] + dir.y * axis_len,
                    origin[2] + dir.z * axis_len,
                ];
                if let Some(screen_end) =
                    project_to_screen(end, cam_pos, yaw, pitch, screen_size, fov_degrees)
                {
                    let d =
                        distance_point_to_segment_2d(mouse[0], mouse[1], screen_origin, screen_end);
                    if d < 18.0 && d < closest_dist {
                        closest_dist = d;
                        best_corner = CORNER_POS1;
                        best_axis = axis;
                    }
                }
            }
        }
    }

    // 2. Test Pos2 Handle
    if let Some(pos2) = p2 {
        let origin = [
            pos2[0] as f64 + 0.5,
            pos2[1] as f64 + 0.5,
            pos2[2] as f64 + 0.5,
        ];
        if let Some(screen_origin) =
            project_to_screen(origin, cam_pos, yaw, pitch, screen_size, fov_degrees)
        {
            let dist = ((origin[0] - cam_pos[0]).powi(2)
                + (origin[1] - cam_pos[1]).powi(2)
                + (origin[2] - cam_pos[2]).powi(2))
            .sqrt()
            .max(0.5);
            let axis_len = (dist * 0.15).max(0.3);

            let center_dist = ((mouse[0] - screen_origin[0]).powi(2)
                + (mouse[1] - screen_origin[1]).powi(2))
            .sqrt();
            if center_dist < 20.0 && center_dist < closest_dist {
                closest_dist = center_dist;
                best_corner = CORNER_POS2;
                best_axis = AXIS_CENTER;
            }

            for axis in 0..3 {
                let dir = get_axis_direction(axis);
                let end = [
                    origin[0] + dir.x * axis_len,
                    origin[1] + dir.y * axis_len,
                    origin[2] + dir.z * axis_len,
                ];
                if let Some(screen_end) =
                    project_to_screen(end, cam_pos, yaw, pitch, screen_size, fov_degrees)
                {
                    let d =
                        distance_point_to_segment_2d(mouse[0], mouse[1], screen_origin, screen_end);
                    if d < 18.0 && d < closest_dist {
                        closest_dist = d;
                        best_corner = CORNER_POS2;
                        best_axis = axis;
                    }
                }
            }
        }
    }

    // 3. Test Center Move Handle
    if let (Some(pos1), Some(pos2)) = (p1, p2) {
        let min_x = pos1[0].min(pos2[0]) as f64;
        let max_x = pos1[0].max(pos2[0]) as f64;
        let min_y = pos1[1].min(pos2[1]) as f64;
        let max_y = pos1[1].max(pos2[1]) as f64;
        let min_z = pos1[2].min(pos2[2]) as f64;
        let max_z = pos1[2].max(pos2[2]) as f64;
        let center = [
            (min_x + max_x + 1.0) * 0.5,
            (min_y + max_y + 1.0) * 0.5,
            (min_z + max_z + 1.0) * 0.5,
        ];

        if let Some(screen_center) =
            project_to_screen(center, cam_pos, yaw, pitch, screen_size, fov_degrees)
        {
            let dist = ((center[0] - cam_pos[0]).powi(2)
                + (center[1] - cam_pos[1]).powi(2)
                + (center[2] - cam_pos[2]).powi(2))
            .sqrt()
            .max(0.5);
            let axis_len = (dist * 0.15).max(0.3);

            let center_dist = ((mouse[0] - screen_center[0]).powi(2)
                + (mouse[1] - screen_center[1]).powi(2))
            .sqrt();
            if center_dist < 22.0 && center_dist < closest_dist {
                closest_dist = center_dist;
                best_corner = CORNER_CENTER;
                best_axis = AXIS_CENTER;
            }

            for axis in 0..3 {
                let dir = get_axis_direction(axis);
                let end = [
                    center[0] + dir.x * axis_len,
                    center[1] + dir.y * axis_len,
                    center[2] + dir.z * axis_len,
                ];
                if let Some(screen_end) =
                    project_to_screen(end, cam_pos, yaw, pitch, screen_size, fov_degrees)
                {
                    let d =
                        distance_point_to_segment_2d(mouse[0], mouse[1], screen_center, screen_end);
                    if d < 18.0 && d < closest_dist {
                        closest_dist = d;
                        best_corner = CORNER_CENTER;
                        best_axis = axis;
                    }
                }
            }
        }
    }

    state.hovered_corner = best_corner;
    state.hovered_axis = best_axis;

    // 4. Hovered block outline when not hovering any gizmo and no selection exists
    if best_corner == CORNER_NONE {
        if !selection_mgr.has_selection() {
            let (ray_origin, ray_dir) =
                screen_mouse_ray(mouse, screen_size, cam_pos, yaw, pitch, fov_degrees);
            state.hovered_block = raycast_block_from_mouse(ray_origin, ray_dir, 256.0, get_block);
        } else {
            state.hovered_block = None;
        }
    } else {
        state.hovered_block = None;
    }
}

impl GhostState {
    #[allow(clippy::too_many_arguments)]
    pub fn start_gizmo_drag(
        &mut self,
        corner: i32,
        axis: i32,
        initial_p1: Option<[i32; 3]>,
        initial_p2: Option<[i32; 3]>,
        ray_origin: glam::DVec3,
        ray_dir: glam::DVec3,
        cam_look: glam::DVec3,
    ) {
        self.dragging_corner = corner;
        self.dragging_axis = axis;
        self.initial_pos1 = initial_p1;
        self.initial_pos2 = initial_p2;
        self.drag_preview_pos1 = initial_p1;
        self.drag_preview_pos2 = initial_p2;
        if let (Some(p1), Some(p2)) = (initial_p1, initial_p2) {
            self.drag_preview_selection = Some(SelectionBox::new(p1, p2));
        }

        if let Some(origin) = get_gizmo_origin(corner, initial_p1, initial_p2) {
            self.drag_start_origin = Some(origin);
            let origin_d = glam::DVec3::from(origin);
            if axis == AXIS_CENTER {
                let hit = intersect_ray_with_camera_plane(ray_origin, ray_dir, origin_d, cam_look);
                self.drag_start_hit = Some(hit.to_array());
            } else {
                let axis_dir = get_axis_direction(axis);
                self.drag_start_param = intersect_ray_with_axis_plane(
                    ray_origin, ray_dir, origin_d, axis_dir, cam_look,
                );
            }
        }
    }

    pub fn handle_gizmo_drag(
        &mut self,
        ray_origin: glam::DVec3,
        ray_dir: glam::DVec3,
        cam_look: glam::DVec3,
    ) {
        if self.dragging_corner == CORNER_NONE || self.dragging_axis == AXIS_NONE {
            return;
        }
        let Some(origin) = self.drag_start_origin else {
            return;
        };
        let origin_d = glam::DVec3::from(origin);

        let (dx, dy, dz) = if self.dragging_axis == AXIS_CENTER {
            let Some(start_hit) = self.drag_start_hit else {
                return;
            };
            let hit = intersect_ray_with_camera_plane(ray_origin, ray_dir, origin_d, cam_look);
            let offset = hit - glam::DVec3::from(start_hit);
            (
                offset.x.round() as i32,
                offset.y.round() as i32,
                offset.z.round() as i32,
            )
        } else {
            let axis_dir = get_axis_direction(self.dragging_axis);
            let current_param =
                intersect_ray_with_axis_plane(ray_origin, ray_dir, origin_d, axis_dir, cam_look);
            let delta_param = current_param - self.drag_start_param;
            let block_delta = delta_param.round() as i32;
            (
                if self.dragging_axis == AXIS_X {
                    block_delta
                } else {
                    0
                },
                if self.dragging_axis == AXIS_Y {
                    block_delta
                } else {
                    0
                },
                if self.dragging_axis == AXIS_Z {
                    block_delta
                } else {
                    0
                },
            )
        };

        if self.dragging_corner == CORNER_POS1
            && let Some(p1) = self.initial_pos1
        {
            let new_p1 = [p1[0] + dx, p1[1] + dy, p1[2] + dz];
            self.drag_preview_pos1 = Some(new_p1);
            if let Some(p2) = self.initial_pos2 {
                self.drag_preview_selection = Some(SelectionBox::new(new_p1, p2));
            }
        } else if self.dragging_corner == CORNER_POS2
            && let Some(p2) = self.initial_pos2
        {
            let new_p2 = [p2[0] + dx, p2[1] + dy, p2[2] + dz];
            self.drag_preview_pos2 = Some(new_p2);
            if let Some(p1) = self.initial_pos1 {
                self.drag_preview_selection = Some(SelectionBox::new(p1, new_p2));
            }
        } else if self.dragging_corner == CORNER_CENTER
            && let (Some(p1), Some(p2)) = (self.initial_pos1, self.initial_pos2)
        {
            let new_p1 = [p1[0] + dx, p1[1] + dy, p1[2] + dz];
            let new_p2 = [p2[0] + dx, p2[1] + dy, p2[2] + dz];
            self.drag_preview_pos1 = Some(new_p1);
            self.drag_preview_pos2 = Some(new_p2);
            self.drag_preview_selection = Some(SelectionBox::new(new_p1, new_p2));
        }
    }

    pub fn finish_gizmo_drag(&mut self) -> Option<([i32; 3], [i32; 3])> {
        let result = if self.dragging_corner != CORNER_NONE {
            if let (Some(p1), Some(p2)) = (self.drag_preview_pos1, self.drag_preview_pos2) {
                if Some(p1) != self.initial_pos1 || Some(p2) != self.initial_pos2 {
                    Some((p1, p2))
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        self.dragging_corner = CORNER_NONE;
        self.dragging_axis = AXIS_NONE;
        self.initial_pos1 = None;
        self.initial_pos2 = None;
        self.drag_preview_pos1 = None;
        self.drag_preview_pos2 = None;
        self.drag_preview_selection = None;
        self.drag_start_origin = None;
        self.drag_start_hit = None;
        self.drag_start_param = 0.0;
        result
    }

    pub fn cancel_gizmo_drag(&mut self) {
        self.dragging_corner = CORNER_NONE;
        self.dragging_axis = AXIS_NONE;
        self.initial_pos1 = None;
        self.initial_pos2 = None;
        self.drag_preview_pos1 = None;
        self.drag_preview_pos2 = None;
        self.drag_preview_selection = None;
        self.drag_start_origin = None;
        self.drag_start_hit = None;
        self.drag_start_param = 0.0;
    }

    pub fn start_box_create(&mut self, start_pos: [i32; 3]) {
        self.is_box_creating = true;
        self.box_create_start = Some(start_pos);
        self.box_create_current = Some(start_pos);
    }

    pub fn update_box_create(&mut self, cur_pos: [i32; 3]) {
        if self.is_box_creating {
            self.box_create_current = Some(cur_pos);
        }
    }

    pub fn finish_box_create(&mut self) -> Option<([i32; 3], [i32; 3])> {
        let res = if self.is_box_creating {
            match (self.box_create_start, self.box_create_current) {
                (Some(s), Some(c)) => Some((s, c)),
                _ => None,
            }
        } else {
            None
        };
        self.is_box_creating = false;
        self.box_create_start = None;
        self.box_create_current = None;
        res
    }

    pub fn cancel_box_create(&mut self) {
        self.is_box_creating = false;
        self.box_create_start = None;
        self.box_create_current = None;
    }
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
