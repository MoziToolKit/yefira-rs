use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use glam_mtk::IVec3 as MtkIVec3;
use mtk_sync::{LiveSyncSession, SyncLifecycle};
use yefira_rs::config::YefiraConfig;
use yefira_rs::horizon::YefiraMod;
use yefira_rs::selection::{SelectionBox, get_world_storage_path, save_to_path};

fn unique_test_dir(name: &str) -> PathBuf {
    let base = std::env::temp_dir().join("yefira_loaded_sync_tests");
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = base.join(format!("{}_{}_{}", name, pid, nanos));
    fs::create_dir_all(&dir).expect("failed to create test directory");
    dir
}

#[test]
fn test_file_loaded_selection_transmits_voxels_and_verifies_hash() {
    let _ = env_logger::builder().is_test(true).try_init();

    let world_dir = unique_test_dir("file_loaded_sync");
    let storage_path = get_world_storage_path(&world_dir);

    // 1. Create a selection file with non-trivial bounds (simulating pre-existing map)
    let saved_box = SelectionBox::new([10, 60, 10], [25, 75, 25]);
    save_to_path(
        &storage_path,
        saved_box.min(),
        saved_box.max(),
        "minecraft:overworld",
    )
    .expect("save selection");

    let config = YefiraConfig {
        port: 0,
        host: "127.0.0.1".to_string(),
        auto_start_on_world_load: false,
        ..Default::default()
    };

    let yefira = YefiraMod::new(config);

    // 2. Simulate world start and session initialization
    yefira.server_manager.reset_session_for_new_world();
    let server_uuid1 = yefira.server_manager.server_uuid();
    let epoch1 = yefira.server_manager.epoch();
    assert_ne!(server_uuid1, [0u8; 16]);
    assert!(epoch1 >= 1);

    // Load selection from storage
    let loaded = yefira
        .selection_manager
        .load_saved_selection(&storage_path, Some("minecraft:overworld"));
    assert!(loaded, "Selection must be loaded from file");
    assert!(yefira.selection_manager.has_selection());

    // 3. Simulate chunk arrival and block scanning
    yefira
        .world_provider
        .set_block([12, 64, 12], "minecraft:stone");
    yefira
        .world_provider
        .set_block([13, 64, 12], "minecraft:birch_planks");
    yefira
        .world_provider
        .set_block([14, 64, 12], "minecraft:bookshelf");

    let scan = yefira_rs::scanner::CanonicalVoxelScanner::scan_baseline(
        &*yefira.world_provider,
        &saved_box,
        "minecraft:overworld",
        || false,
    )
    .expect("scan baseline");

    assert!(scan.state_hash != 0, "State hash must be non-zero");
    yefira.server_manager.set_cached_state_hash(scan.state_hash);
    yefira
        .scan_revision
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

    // 4. Start WebSocket live sync server
    let bound_port = yefira.server_manager.start("127.0.0.1", 0).unwrap();
    let ws_url = format!("ws://127.0.0.1:{}", bound_port);

    // 5. Connect DCC client (libmozitoolkit LiveSyncSession) without previous cache
    let mut client = LiveSyncSession::new(None, None, None);
    client
        .start(&ws_url, false, 0)
        .expect("client must initiate connection");

    let start_wait = Instant::now();
    let mut live_reached = false;
    while start_wait.elapsed() < Duration::from_secs(5) {
        if client.lifecycle() == SyncLifecycle::Live {
            live_reached = true;
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }

    assert!(
        live_reached,
        "Client must reach Live state upon receiving baseline"
    );

    // Verify voxels were received from file-loaded selection
    {
        let storage = client.get_storage();
        assert_eq!(storage.get_block(12, 64, 12), "minecraft:stone");
        assert_eq!(storage.get_block(13, 64, 12), "minecraft:birch_planks");
        assert_eq!(storage.get_block(14, 64, 12), "minecraft:bookshelf");
    }

    client.stop();

    // 6. Test 0ms Instant Reconnect (VerifiedSkipRebuild)
    // Connect with matching state hash, epoch, bounds, and UUID
    let cur_uuid = yefira.server_manager.server_uuid();
    let cur_epoch = yefira.server_manager.epoch();
    let cur_hash = yefira.server_manager.cached_state_hash();
    let min_glam = MtkIVec3::new(saved_box.min()[0], saved_box.min()[1], saved_box.min()[2]);
    let size_glam = MtkIVec3::new(
        saved_box.size_x() as i32,
        saved_box.size_y() as i32,
        saved_box.size_z() as i32,
    );

    let mut fast_client = LiveSyncSession::new(None, None, None);
    fast_client
        .start_with_cache(
            &ws_url,
            false,
            0,
            cur_uuid,
            cur_epoch,
            1, // seq
            min_glam,
            size_glam,
            cur_hash,
            "minecraft:overworld".to_string(),
        )
        .expect("start_with_cache");

    let fast_wait = Instant::now();
    let mut fast_live = false;
    while fast_wait.elapsed() < Duration::from_secs(3) {
        if fast_client.lifecycle() == SyncLifecycle::Live {
            fast_live = true;
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }

    assert!(
        fast_live,
        "Fast reconnect client must reach Live state with VerifiedSkipRebuild"
    );

    fast_client.stop();

    // 7. Verify that restarting the server rotates UUID and invalidates cache
    yefira.server_manager.reset_session_for_new_world();
    let server_uuid2 = yefira.server_manager.server_uuid();
    let epoch2 = yefira.server_manager.epoch();

    assert_ne!(
        server_uuid1, server_uuid2,
        "Server UUID must rotate on session reset"
    );
    assert_eq!(epoch2, epoch1 + 1, "Epoch must increment on session reset");

    // Reconnecting with old UUID now triggers FULL_BASELINE_REQUIRED instead of stale VerifiedSkipRebuild
    // Update authoritative state hash for new session
    yefira.server_manager.set_cached_state_hash(scan.state_hash);

    let mut mismatch_client = LiveSyncSession::new(None, None, None);
    mismatch_client
        .start_with_cache(
            &ws_url,
            false,
            0,
            server_uuid1, // Stale UUID!
            epoch1,       // Stale Epoch!
            1,
            min_glam,
            size_glam,
            cur_hash,
            "minecraft:overworld".to_string(),
        )
        .expect("start_with_cache mismatch");

    let mismatch_wait = Instant::now();
    let mut mismatch_live = false;
    while mismatch_wait.elapsed() < Duration::from_secs(5) {
        if mismatch_client.lifecycle() == SyncLifecycle::Live {
            mismatch_live = true;
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }

    assert!(
        mismatch_live,
        "Mismatch client must gracefully fall back to FullBaselineRequired and reach Live"
    );

    {
        let storage = mismatch_client.get_storage();
        assert_eq!(storage.get_block(12, 64, 12), "minecraft:stone");
    }

    mismatch_client.stop();
    yefira.server_manager.stop();
    let _ = fs::remove_dir_all(&world_dir);
}

#[test]
fn test_resync_request_retransmits_baseline() {
    let _ = env_logger::builder().is_test(true).try_init();

    let config = YefiraConfig {
        port: 0,
        host: "127.0.0.1".to_string(),
        auto_start_on_world_load: false,
        ..Default::default()
    };

    let yefira = YefiraMod::new(config);
    let sel = SelectionBox::new([0, 60, 0], [10, 70, 10]);
    yefira
        .selection_manager
        .set_selection(sel.min(), sel.max(), "minecraft:overworld");

    yefira
        .world_provider
        .set_block([5, 64, 5], "minecraft:diamond_block");
    let scan = yefira_rs::scanner::CanonicalVoxelScanner::scan_baseline(
        &*yefira.world_provider,
        &sel,
        "minecraft:overworld",
        || false,
    )
    .expect("scan baseline");
    yefira.server_manager.set_cached_state_hash(scan.state_hash);

    let bound_port = yefira.server_manager.start("127.0.0.1", 0).unwrap();
    let ws_url = format!("ws://127.0.0.1:{}", bound_port);

    let mut client = LiveSyncSession::new(None, None, None);
    client.start(&ws_url, false, 0).expect("connect");

    let start_wait = Instant::now();
    while start_wait.elapsed() < Duration::from_secs(5) {
        if client.lifecycle() == SyncLifecycle::Live {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(client.lifecycle(), SyncLifecycle::Live);

    // Send ResyncRequest from client
    client.request_resync().expect("send resync");

    // Client transitions back to ReceivingBaseline then Live once retransmitted baseline is committed
    let resync_wait = Instant::now();
    let mut resync_live = false;
    while resync_wait.elapsed() < Duration::from_secs(5) {
        if client.lifecycle() == SyncLifecycle::Live {
            resync_live = true;
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }

    assert!(
        resync_live,
        "Client must re-commit baseline and return to Live after ResyncRequest"
    );
    assert_eq!(
        client.get_storage().get_block(5, 64, 5),
        "minecraft:diamond_block"
    );

    client.stop();
    yefira.server_manager.stop();
}
