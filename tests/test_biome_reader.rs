use glam::IVec3;
use horizon_client::mod_api::Host;
use yefira_rs::scanner::biome_reader::populate_selection_biomes;
use yefira_rs::scanner::{CanonicalVoxelScanner, TrackingWorldProvider, WorldBlockProvider};
use yefira_rs::selection::SelectionBox;

struct MockRuntimeHost;

impl Host for MockRuntimeHost {
    fn chunk_snapshot(
        &self,
        _dimension: &str,
        _chunk: [i32; 2],
    ) -> Option<horizon_client::mod_api::NativeChunkSnapshot> {
        None
    }

    fn biome(&self, pos: [i32; 3]) -> Option<String> {
        // 3D biome distribution:
        // Underground (Y < 16): lush_caves
        // Surface (Y >= 16): forest
        if pos[1] < 16 {
            Some("minecraft:lush_caves".to_string())
        } else {
            Some("minecraft:forest".to_string())
        }
    }
}

#[test]
fn test_runtime_in_memory_biome_extraction() {
    let host = MockRuntimeHost;
    let provider = TrackingWorldProvider::new();
    let sel = SelectionBox::new([0, 0, 0], [15, 31, 15]);

    populate_selection_biomes(&host, "minecraft:overworld", &sel, &provider);

    // Verify quarts at Y < 16 (qy 0..3) are lush_caves
    assert_eq!(provider.get_quart_biome([0, 0, 0]), "minecraft:lush_caves");
    assert_eq!(provider.get_quart_biome([1, 2, 1]), "minecraft:lush_caves");
    assert_eq!(provider.get_quart_biome([3, 3, 3]), "minecraft:lush_caves");

    // Verify quarts at Y >= 16 (qy 4..7) are forest
    assert_eq!(provider.get_quart_biome([0, 4, 0]), "minecraft:forest");
    assert_eq!(provider.get_quart_biome([2, 5, 2]), "minecraft:forest");
    assert_eq!(provider.get_quart_biome([3, 7, 3]), "minecraft:forest");
}

#[test]
fn test_3d_multi_biome_baseline_payload_and_interop() {
    let host = MockRuntimeHost;
    let provider = TrackingWorldProvider::new();

    // Selection 16x32x16 covering Y=0..=31
    let sel = SelectionBox::new([0, 0, 0], [15, 31, 15]);

    // Populate biomes directly from runtime memory
    populate_selection_biomes(&host, "minecraft:overworld", &sel, &provider);

    // Place a block in section 0 and another in section 1
    provider.set_block([0, 5, 0], "minecraft:stone");
    provider.set_block([0, 20, 0], "minecraft:oak_log");

    let scan =
        CanonicalVoxelScanner::scan_baseline(&provider, &sel, "minecraft:overworld", || false)
            .expect("Scan failed");

    // Verify multi-biome properties
    assert_eq!(scan.biome_palette.len(), 2);
    assert!(
        scan.biome_palette
            .contains(&"minecraft:lush_caves".to_string())
    );
    assert!(scan.biome_palette.contains(&"minecraft:forest".to_string()));
    assert_eq!(scan.biome_bits_per_quart, 8);

    // Total quarts: 16x32x16 -> 4x8x4 = 128 quarts
    let expected_quarts = 4 * 8 * 4;
    let expected_voxels = 16 * 32 * 16;

    // Payload should be: 8192 bytes (voxel indices) + 128 bytes (8-bit biome indices)
    assert_eq!(scan.payload.len(), expected_voxels + expected_quarts);

    let biome_slice = &scan.payload[expected_voxels..];
    assert_eq!(biome_slice.len(), expected_quarts);

    let lush_idx = scan
        .biome_palette
        .iter()
        .position(|b| b == "minecraft:lush_caves")
        .unwrap() as u8;
    let forest_idx = scan
        .biome_palette
        .iter()
        .position(|b| b == "minecraft:forest")
        .unwrap() as u8;

    for (i, &b_id) in biome_slice.iter().enumerate() {
        let qy = i / (4 * 4); // each Y level has 4*4 = 16 quarts
        if qy < 4 {
            assert_eq!(b_id, lush_idx, "Expected lush_caves at quart {i}");
        } else {
            assert_eq!(b_id, forest_idx, "Expected forest at quart {i}");
        }
    }

    // Cross-verify with mtk-sync
    let packet = yefira_rs::protocol::Packet::BaselineBegin {
        epoch: 1,
        snapshot_uuid: [1u8; 16],
        snapshot_seq: 1,
        min_pos: IVec3::new(0, 0, 0),
        size: IVec3::new(16, 32, 16),
        state_hash: scan.state_hash,
        palette_revision: 1,
        total_chunks: 1,
        total_encoded_bytes: scan.payload.len() as u64,
        total_decoded_bytes: scan.payload.len() as u64,
        compression_mode: 0,
        palette_bits_per_block: scan.palette_bits_per_block,
        biome_bits_per_quart: scan.biome_bits_per_quart,
        dimension: "minecraft:overworld".to_string(),
        block_palette: scan.block_palette.clone(),
        biome_palette: scan.biome_palette.clone(),
    };

    let encoded = yefira_rs::protocol::encode_packet(&packet);
    let decoded_mtk = mtk_sync::decode_packet(&encoded)
        .expect("mtk-sync failed to decode baseline begin with 3D biomes");

    match decoded_mtk {
        mtk_sync::Packet::BaselineBegin {
            biome_bits_per_quart,
            biome_palette,
            ..
        } => {
            assert_eq!(biome_bits_per_quart, 8);
            assert_eq!(biome_palette.len(), 2);
            assert!(biome_palette.contains(&"minecraft:lush_caves".to_string()));
            assert!(biome_palette.contains(&"minecraft:forest".to_string()));
        }
        _ => panic!("Expected BaselineBegin"),
    }
}
