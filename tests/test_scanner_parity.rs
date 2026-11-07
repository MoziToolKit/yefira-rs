use glam::IVec3;
use yefira_rs::scanner::{CanonicalVoxelScanner, InMemoryWorld, WorldBlockProvider};
use yefira_rs::selection::SelectionBox;

#[test]
fn test_scanner_state_hash_parity_with_libmozitoolkit() {
    let mut world = InMemoryWorld::new();

    // Box bounds: [0, 60, 0] to [3, 63, 3] (size 4x4x4 = 64 voxels)
    let min_pos = [0, 60, 0];
    let max_pos = [3, 63, 3];
    let box_3d = SelectionBox::new(min_pos, max_pos);
    let dimension = "minecraft:overworld";

    // Place non-air blocks at specific positions
    world.set_block([0, 60, 0], "minecraft:bedrock");
    world.set_block([1, 60, 0], "minecraft:stone");
    world.set_block([2, 60, 1], "minecraft:dirt");
    world.set_block([3, 61, 2], "minecraft:oak_planks");
    world.set_block([0, 62, 3], "minecraft:glass");
    world.set_block([1, 63, 1], "minecraft:torch");

    // Ignored/air variants (should normalize to air and not contribute to sparse hash)
    world.set_block([2, 62, 2], "minecraft:cave_air");
    world.set_block([3, 62, 3], "void_air");

    // Set biomes
    world.set_biome([0, 15, 0], "minecraft:plains");

    // 1. Scan baseline with yefira-rs CanonicalVoxelScanner
    let scan_result = CanonicalVoxelScanner::scan_baseline(&world, &box_3d, dimension, || false)
        .expect("Scan should succeed");

    assert_eq!(scan_result.total_voxels, 64);
    assert_eq!(scan_result.size, IVec3::new(4, 4, 4));
    assert_eq!(scan_result.min_pos, IVec3::new(0, 60, 0));

    // Verify block palette: Air is index 0
    assert_eq!(scan_result.block_palette[0], "minecraft:air");
    assert!(scan_result
        .block_palette
        .contains(&"minecraft:bedrock".to_string()));
    assert!(scan_result
        .block_palette
        .contains(&"minecraft:stone".to_string()));
    assert!(scan_result
        .block_palette
        .contains(&"minecraft:dirt".to_string()));
    assert!(scan_result
        .block_palette
        .contains(&"minecraft:oak_planks".to_string()));
    assert!(scan_result
        .block_palette
        .contains(&"minecraft:glass".to_string()));
    assert!(scan_result
        .block_palette
        .contains(&"minecraft:torch".to_string()));

    // 2. Compute authoritative hash using libmozitoolkit's compute_sparse_state_hash
    // We collect the exact non-air entries in (Y, Z, X) order
    let mut mtk_entries = Vec::new();
    for y in 60..=63 {
        for z in 0..=3 {
            for x in 0..=3 {
                let state = world.get_block_state([x, y, z]);
                let norm = yefira_rs::normalize_canonical_blockstate(state);
                if norm != "minecraft:air" {
                    let rx = x as u32;
                    let ry = (y - 60) as u32;
                    let rz = z as u32;
                    mtk_entries.push(mtk_sync::SparseBlockEntry {
                        rel_x: rx,
                        rel_y: ry,
                        rel_z: rz,
                        state: norm,
                        biome: "minecraft:plains",
                        entity_json: None,
                    });
                }
            }
        }
    }

    let mtk_expected_hash = mtk_sync::compute_sparse_state_hash(
        glam_mtk::IVec3::new(0, 60, 0),
        glam_mtk::IVec3::new(4, 4, 4),
        dimension,
        mtk_entries,
    );

    // 3. Cryptographic parity assertion:
    assert_ne!(scan_result.state_hash, 0);
    assert_eq!(
        scan_result.state_hash, mtk_expected_hash,
        "Authoritative xxHash64 must match libmozitoolkit down to the exact bit!"
    );
}
