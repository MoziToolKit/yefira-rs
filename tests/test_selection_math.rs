use yefira_rs::selection::{Direction, SelectionBox, SelectionManager};

#[test]
fn test_selection_box_bounds_and_volume() {
    // Arbitrary order of corners
    let p1 = [10, 64, -5];
    let p2 = [-2, 80, 15];

    let box_3d = SelectionBox::new(p1, p2);

    assert_eq!(box_3d.min(), [-2, 64, -5]);
    assert_eq!(box_3d.max(), [10, 80, 15]);

    // Size calculation: max - min + 1
    assert_eq!(box_3d.size_x(), 13);
    assert_eq!(box_3d.size_y(), 17);
    assert_eq!(box_3d.size_z(), 21);

    assert_eq!(box_3d.volume(), 13 * 17 * 21);

    // Contains checks
    assert!(box_3d.contains([0, 70, 0]));
    assert!(box_3d.contains([-2, 64, -5])); // min corner
    assert!(box_3d.contains([10, 80, 15])); // max corner
    assert!(!box_3d.contains([-3, 70, 0]));
    assert!(!box_3d.contains([11, 70, 0]));
    assert!(!box_3d.contains([0, 63, 0]));
    assert!(!box_3d.contains([0, 81, 0]));
    assert!(!box_3d.contains([0, 70, -6]));
    assert!(!box_3d.contains([0, 70, 16]));
}

#[test]
fn test_preset_above() {
    let center = [0, 64, 0];

    // Size 1: single block at center
    let s1 = SelectionBox::preset_above(center, 1).unwrap();
    assert_eq!(s1.size_x(), 1);
    assert_eq!(s1.size_y(), 1);
    assert_eq!(s1.size_z(), 1);
    assert_eq!(s1.min(), [0, 64, 0]);
    assert_eq!(s1.max(), [0, 64, 0]);

    // Size 15 (odd size centered on X/Z, extending upwards)
    let s15 = SelectionBox::preset_above(center, 15).unwrap();
    assert_eq!(s15.size_x(), 15);
    assert_eq!(s15.size_y(), 15);
    assert_eq!(s15.size_z(), 15);
    assert_eq!(s15.min(), [-7, 64, -7]);
    assert_eq!(s15.max(), [7, 78, 7]);

    // Size 16 (even size)
    let s16 = SelectionBox::preset_above(center, 16).unwrap();
    assert_eq!(s16.size_x(), 16);
    assert_eq!(s16.size_y(), 16);
    assert_eq!(s16.size_z(), 16);
    assert_eq!(s16.min(), [-7, 64, -7]);
    assert_eq!(s16.max(), [8, 79, 8]);

    // Invalid size 0
    assert!(SelectionBox::preset_above(center, 0).is_err());
}

#[test]
fn test_expand_and_shift() {
    let base = SelectionBox::new([0, 64, 0], [10, 74, 10]);

    // Expand UP by 5
    let up = base.expand(Direction::Up, 5);
    assert_eq!(up.size_y(), 16);
    assert_eq!(up.max()[1], 79);

    // Expand DOWN by 4
    let down = base.expand(Direction::Down, 4);
    assert_eq!(down.size_y(), 15);
    assert_eq!(down.min()[1], 60);

    // Shift EAST by 10 (X+)
    let shifted = base.shift(Direction::East, 10);
    assert_eq!(shifted.size_x(), 11);
    assert_eq!(shifted.min()[0], 10);
    assert_eq!(shifted.max()[0], 20);
}

#[test]
fn test_selection_manager_listener_and_persistence() {
    let mgr = SelectionManager::new();
    let counter = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter_clone = counter.clone();

    mgr.add_listener(move |sel, _dim| {
        if sel.is_some() {
            counter_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    });

    assert!(!mgr.has_selection());

    // Setting pos1 doesn't trigger box until pos2 is set
    mgr.set_pos1([0, 60, 0], "minecraft:overworld");
    assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 0);

    // Setting pos2 triggers selection listener
    mgr.set_pos2([15, 75, 15], "minecraft:overworld");
    assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(mgr.has_selection());

    // Persistence to temporary directory
    let temp_file = std::env::temp_dir().join("yefira_rs_test_selection.json");
    mgr.save_to_path(&temp_file).unwrap();

    let new_mgr = SelectionManager::new();
    let loaded = new_mgr.load_from_path(&temp_file).unwrap();
    assert!(loaded.is_some());
    let loaded_box = loaded.unwrap();
    assert_eq!(loaded_box.size_x(), 16);
    assert_eq!(loaded_box.size_y(), 16);
    assert_eq!(loaded_box.size_z(), 16);

    let _ = std::fs::remove_file(temp_file);
}

#[test]
fn test_horizon_command_dispatch() {
    use yefira_rs::horizon::YefiraMod;

    let yefira = YefiraMod::default();
    let dim = "minecraft:overworld";

    // Unrelated command should return None
    assert!(
        yefira
            .handle_chat_command("gamemode creative", Some([0, 64, 0]), dim)
            .is_none()
    );

    // /yefira pos1 0 64 0
    let res1 = yefira.handle_chat_command("/yefira pos1 0 64 0", None, dim);
    assert!(res1.is_some());
    assert!(res1.unwrap().is_success());

    // /yefira pos2 10 74 10
    let res2 = yefira.handle_chat_command("yefira pos2 10 74 10", None, dim);
    assert!(res2.is_some());
    assert!(res2.unwrap().is_success());

    assert!(yefira.current_selection().is_some());
    let sel = yefira.current_selection().unwrap();
    assert_eq!(sel.size_x(), 11);
    assert_eq!(sel.size_y(), 11);
    assert_eq!(sel.size_z(), 11);

    // /yefira pos1 without args uses player pos
    let res3 = yefira.handle_chat_command("yefira pos1", Some([5, 60, 5]), dim);
    assert!(res3.is_some() && res3.unwrap().is_success());

    // /yefira ghost on / off / status
    let res_ghost_on = yefira.handle_chat_command("yefira ghost on", Some([5, 60, 5]), dim);
    assert!(res_ghost_on.is_some() && res_ghost_on.unwrap().is_success());
    assert!(yefira.ghost_mode.is_active());

    let res_ghost_status = yefira.handle_chat_command("yefira ghost status", None, dim);
    assert!(res_ghost_status.is_some() && res_ghost_status.unwrap().is_success());

    let res_ghost_off = yefira.handle_chat_command("yefira ghost off", None, dim);
    assert!(res_ghost_off.is_some() && res_ghost_off.unwrap().is_success());
    assert!(!yefira.ghost_mode.is_active());

    // /yefira clear
    let res_clear = yefira.handle_chat_command("/yefira clear", None, dim);
    assert!(res_clear.is_some());
    assert!(yefira.current_selection().is_none());
}

#[test]
fn test_gizmo_plane_ray_intersection() {
    use glam::DVec3;
    use yefira_rs::ghost::{
        AXIS_X, AXIS_Y, AXIS_Z, get_axis_direction, intersect_ray_with_axis_plane,
        intersect_ray_with_camera_plane,
    };

    let cam_pos = DVec3::new(5.0, 8.0, -10.0);
    let origin = DVec3::new(0.0, 5.0, 0.0);
    let cam_look = (origin - cam_pos).normalize();

    // 1. Single Axis Plane intersection along X
    let axis_x = get_axis_direction(AXIS_X);
    // Ray pointing directly at origin
    let ray_dir = (origin - cam_pos).normalize();
    let t0 = intersect_ray_with_axis_plane(cam_pos, ray_dir, origin, axis_x, cam_look);
    assert!((t0 - 0.0).abs() < 1e-4);

    // Ray pointing at origin + 3.0 along X
    let target_x = origin + DVec3::new(3.0, 0.0, 0.0);
    let ray_dir_x = (target_x - cam_pos).normalize();
    let t1 = intersect_ray_with_axis_plane(cam_pos, ray_dir_x, origin, axis_x, cam_look);
    assert!((t1 - 3.0).abs() < 1e-4);

    // 2. Single Axis Plane intersection along Y
    let axis_y = get_axis_direction(AXIS_Y);
    let target_y = origin + DVec3::new(0.0, -4.0, 0.0);
    let ray_dir_y = (target_y - cam_pos).normalize();
    let t_y = intersect_ray_with_axis_plane(cam_pos, ray_dir_y, origin, axis_y, cam_look);
    assert!((t_y - (-4.0)).abs() < 1e-4);

    // 3. Single Axis Plane intersection along Z
    let axis_z = get_axis_direction(AXIS_Z);
    let target_z = origin + DVec3::new(0.0, 0.0, 5.0);
    let ray_dir_z = (target_z - cam_pos).normalize();
    let t_z = intersect_ray_with_axis_plane(cam_pos, ray_dir_z, origin, axis_z, cam_look);
    assert!((t_z - 5.0).abs() < 1e-4);

    // 4. Camera plane intersection: hit point must lie on the camera plane passing through origin
    let hit_cam = intersect_ray_with_camera_plane(cam_pos, ray_dir_x, origin, cam_look);
    assert!((hit_cam - origin).dot(cam_look).abs() < 1e-4);
    // Ray from cam_pos along ray_dir_x reaches hit_cam
    let t_cam = (hit_cam - cam_pos).dot(ray_dir_x);
    assert!((cam_pos + ray_dir_x * t_cam - hit_cam).length() < 1e-4);
}

#[test]
fn test_ghost_gizmo_drag_lifecycle() {
    use glam::DVec3;
    use yefira_rs::ghost::{AXIS_CENTER, AXIS_X, CORNER_CENTER, CORNER_POS1, GhostState};

    let mut state = GhostState::default();
    let cam_pos = DVec3::new(0.0, 64.0, -10.0);
    let cam_look = DVec3::new(0.0, 0.0, 1.0);
    let p1 = [0, 64, 0];
    let p2 = [10, 74, 10];

    // --- Case 1: Drag Pos1 along X axis by +3 blocks ---
    let origin_p1 = DVec3::new(0.5, 64.5, 0.5);
    let ray_dir_start = (origin_p1 - cam_pos).normalize();

    state.start_gizmo_drag(
        CORNER_POS1,
        AXIS_X,
        Some(p1),
        Some(p2),
        cam_pos,
        ray_dir_start,
        cam_look,
    );

    assert_eq!(state.dragging_corner, CORNER_POS1);
    assert_eq!(state.dragging_axis, AXIS_X);

    // Drag mouse to hit offset +2.8 along X -> rounds to +3 blocks
    let ray_dir_drag = (origin_p1 + DVec3::new(2.8, 0.0, 0.0) - cam_pos).normalize();
    state.handle_gizmo_drag(cam_pos, ray_dir_drag, cam_look);

    assert_eq!(state.drag_preview_pos1, Some([3, 64, 0]));
    assert_eq!(state.drag_preview_pos2, Some(p2));

    let finished = state.finish_gizmo_drag();
    assert_eq!(finished, Some(([3, 64, 0], p2)));
    assert_eq!(state.dragging_corner, 0);

    // --- Case 2: Drag Center on Camera Plane by (+2, -3, 0) ---
    let center_origin = DVec3::new(5.5, 69.5, 5.5);
    let ray_center_start = (center_origin - cam_pos).normalize();

    state.start_gizmo_drag(
        CORNER_CENTER,
        AXIS_CENTER,
        Some(p1),
        Some(p2),
        cam_pos,
        ray_center_start,
        cam_look,
    );

    let ray_center_drag = (center_origin + DVec3::new(2.1, -3.2, 0.0) - cam_pos).normalize();
    state.handle_gizmo_drag(cam_pos, ray_center_drag, cam_look);

    assert_eq!(state.drag_preview_pos1, Some([2, 61, 0]));
    assert_eq!(state.drag_preview_pos2, Some([12, 71, 10]));

    // Cancel resets drag without committing
    state.cancel_gizmo_drag();
    assert_eq!(state.dragging_corner, 0);
    assert_eq!(state.drag_preview_pos1, None);
}

#[test]
fn test_voxel_dda_raycast() {
    use std::collections::HashMap;
    use yefira_rs::ghost::{is_selectable_block, raycast_block_from_mouse};

    assert!(!is_selectable_block(""));
    assert!(!is_selectable_block("minecraft:air"));
    assert!(!is_selectable_block("cave_air"));
    assert!(!is_selectable_block("minecraft:water"));
    assert!(!is_selectable_block("minecraft:lava"));
    assert!(is_selectable_block("minecraft:stone"));
    assert!(is_selectable_block("oak_planks"));

    let mut world: HashMap<[i32; 3], &'static str> = HashMap::new();
    world.insert([5, 10, 5], "minecraft:stone");
    world.insert([5, 15, 5], "minecraft:water"); // fluids ignored
    world.insert([5, 20, 5], "minecraft:air"); // air ignored

    let get_block = |pos: [i32; 3]| -> String {
        world
            .get(&pos)
            .copied()
            .unwrap_or("minecraft:air")
            .to_string()
    };

    // Ray from top straight down along Y-
    let ray_origin = [5.5, 25.0, 5.5];
    let ray_dir = [0.0, -1.0, 0.0];
    let hit = raycast_block_from_mouse(ray_origin, ray_dir, 50.0, get_block);
    assert_eq!(hit, Some([5, 10, 5]));

    // Ray missing the block returns None
    let ray_miss = [10.5, 25.0, 10.5];
    let miss_hit = raycast_block_from_mouse(ray_miss, ray_dir, 50.0, get_block);
    assert_eq!(miss_hit, None);
}

#[test]
fn test_ghost_box_creation_lifecycle() {
    use yefira_rs::ghost::GhostState;

    let mut state = GhostState::default();

    state.start_box_create([10, 64, 10]);
    assert!(state.is_box_creating);
    assert_eq!(state.box_create_start, Some([10, 64, 10]));
    assert_eq!(state.box_create_current, Some([10, 64, 10]));

    state.update_box_create([15, 70, 18]);
    assert_eq!(state.box_create_current, Some([15, 70, 18]));

    let finished = state.finish_box_create();
    assert_eq!(finished, Some(([10, 64, 10], [15, 70, 18])));
    assert!(!state.is_box_creating);
}
