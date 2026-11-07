use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use mtk_sync::{LiveSyncSession, SyncLifecycle};
use yefira_rs::config::YefiraConfig;
use yefira_rs::horizon::YefiraMod;
use yefira_rs::scanner::InMemoryWorld;

#[test]
fn test_live_sync_closed_loop_with_libmozitoolkit() {
    let _ = env_logger::builder().is_test(true).try_init();

    let config = YefiraConfig {
        port: 0, // Dynamic port
        host: "127.0.0.1".to_string(),
        auto_start_on_world_load: false, // We start manually
        ..Default::default()
    };

    // 1. Set up YefiraMod with an in-memory test world
    let yefira = YefiraMod::new(config);
    let mut world = InMemoryWorld::new();

    // Place initial baseline blocks
    world.set_block([0, 64, 0], "minecraft:stone");
    world.set_block([1, 64, 0], "minecraft:oak_log");
    world.set_block([2, 64, 0], "minecraft:gold_block");
    world.set_block([0, 65, 0], "minecraft:glass");

    let world_arc = Arc::new(world);
    yefira.attach_world_provider(world_arc.clone());

    // 2. Set authoritative selection in yefira (e.g. 16x16x16 volume)
    yefira
        .selection_manager
        .set_selection([0, 60, 0], [15, 75, 15], "minecraft:overworld");

    // 3. Start Yefira WebSocket server on dynamic port
    let bound_port = yefira.server_manager.start("127.0.0.1", 0).unwrap();
    assert!(yefira.server_manager.is_running());

    // 4. Initialize libmozitoolkit LiveSyncSession
    let mut mtk_session = LiveSyncSession::new(None, None, None);
    let ws_url = format!("ws://127.0.0.1:{}", bound_port);

    log::info!("Connecting libmozitoolkit LiveSyncSession to {}", ws_url);
    mtk_session
        .start(&ws_url, false, 0)
        .expect("mtk_session should initiate connection");

    // 5. Wait for live sync handshake & baseline commit
    let start_wait = Instant::now();
    let mut live_reached = false;

    while start_wait.elapsed() < Duration::from_secs(5) {
        let lifecycle = mtk_session.lifecycle();
        if lifecycle == SyncLifecycle::Live {
            live_reached = true;
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }

    assert!(
        live_reached,
        "libmozitoolkit LiveSyncSession failed to reach Live lifecycle state! Current: {:?}",
        mtk_session.lifecycle()
    );

    // 6. Verify initial baseline data arrived in libmozitoolkit's VoxelStorage
    {
        let storage = mtk_session.get_storage();
        assert_eq!(
            storage.get_block(0, 64, 0),
            "minecraft:stone",
            "Voxel at [0, 64, 0] should be stone"
        );
        assert_eq!(
            storage.get_block(1, 64, 0),
            "minecraft:oak_log",
            "Voxel at [1, 64, 0] should be oak_log"
        );
        assert_eq!(
            storage.get_block(2, 64, 0),
            "minecraft:gold_block",
            "Voxel at [2, 64, 0] should be gold_block"
        );
        assert_eq!(
            storage.get_block(0, 65, 0),
            "minecraft:glass",
            "Voxel at [0, 65, 0] should be glass"
        );
    }

    // 7. Test Real-time Delta Streaming:
    // Modify blocks in Minecraft (Horizon), queue update, and tick server
    log::info!("Queuing real-time delta block update: [5, 64, 5] -> minecraft:diamond_block");
    yefira.on_block_changed(
        "minecraft:overworld",
        [5, 64, 5],
        "minecraft:air",
        "minecraft:diamond_block",
    );

    // Flush deltas on server tick
    yefira.on_server_tick();

    // 8. Wait for libmozitoolkit to receive and apply DELTA_OP
    let delta_wait = Instant::now();
    let mut delta_applied = false;

    while delta_wait.elapsed() < Duration::from_secs(3) {
        let storage = mtk_session.get_storage();
        if storage.get_block(5, 64, 5) == "minecraft:diamond_block" {
            delta_applied = true;
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }

    assert!(
        delta_applied,
        "libmozitoolkit failed to apply real-time DELTA_OP for diamond_block! Current state at [5, 64, 5]: {}",
        mtk_session.get_storage().get_block(5, 64, 5)
    );

    // 9. Clean shutdown
    log::info!("Live sync closed loop successfully verified! Shutting down session and server");
    mtk_session.stop();
    yefira.server_manager.stop();
}
