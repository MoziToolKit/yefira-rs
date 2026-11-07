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
    assert!(yefira
        .handle_chat_command("gamemode creative", Some([0, 64, 0]), dim)
        .is_none());

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
