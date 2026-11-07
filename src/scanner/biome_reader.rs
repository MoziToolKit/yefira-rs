//! 3D Biome Reader for Horizon runtime environment.
//!
//! Directly extracts 3D quart biomes from Horizon's in-memory chunks (RFC v3.1 / v3.2).
//! No disk I/O or save file dependencies.

use std::collections::HashMap;

use horizon_client::mod_api::Host;

use crate::scanner::provider::TrackingWorldProvider;
use crate::selection::SelectionBox;

/// Populates 3D quart biomes into `provider` directly from Horizon's runtime memory.
pub fn populate_selection_biomes(
    host: &dyn Host,
    dim: &str,
    sel: &SelectionBox,
    provider: &TrackingWorldProvider,
) {
    // 1. Set dimension default biome and clear existing biome overrides
    if dim.contains("nether") {
        provider.set_default_biome("minecraft:nether_wastes");
    } else if dim.contains("end") {
        provider.set_default_biome("minecraft:the_end");
    } else {
        provider.set_default_biome("minecraft:plains");
    }
    provider.clear_biomes();

    let min = sel.min();
    let max = sel.max();

    let min_qx = min[0] >> 2;
    let max_qx = max[0] >> 2;
    let min_qy = min[1] >> 2;
    let max_qy = max[1] >> 2;
    let min_qz = min[2] >> 2;
    let max_qz = max[2] >> 2;

    let min_cx = min[0] >> 4;
    let max_cx = max[0] >> 4;
    let min_cz = min[2] >> 4;
    let max_cz = max[2] >> 4;

    let mut populated_count = 0usize;
    let mut biome_name_cache: HashMap<u16, String> = HashMap::new();

    for cz in min_cz..=max_cz {
        for cx in min_cx..=max_cx {
            let chunk_qx_start = (cx << 2).max(min_qx);
            let chunk_qx_end = ((cx << 2) + 3).min(max_qx);
            let chunk_qz_start = (cz << 2).max(min_qz);
            let chunk_qz_end = ((cz << 2) + 3).min(max_qz);

            // Primary path: in-memory ChunkSnapshot (authoritative server / integrated server thread)
            if let Some(snapshot) = host.chunk_snapshot(dim, [cx, cz]) {
                for qy in min_qy..=max_qy {
                    for qz in chunk_qz_start..=chunk_qz_end {
                        for qx in chunk_qx_start..=chunk_qx_end {
                            if let Some(biome_id) = snapshot.noise_biome(qx, qy, qz) {
                                let name_opt = if let Some(cached) = biome_name_cache.get(&biome_id)
                                {
                                    Some(cached.as_str())
                                } else if let Some(resolved) = host.biome_name(biome_id) {
                                    biome_name_cache.insert(biome_id, resolved.to_string());
                                    Some(resolved)
                                } else {
                                    None
                                };

                                if let Some(name) = name_opt {
                                    provider.set_biome([qx, qy, qz], name);
                                    populated_count += 1;
                                }
                            }
                        }
                    }
                }
            } else {
                // Secondary in-memory path: host.biome([x, y, z])
                for qy in min_qy..=max_qy {
                    let y = qy << 2;
                    for qz in chunk_qz_start..=chunk_qz_end {
                        let z = qz << 2;
                        for qx in chunk_qx_start..=chunk_qx_end {
                            let x = qx << 2;
                            if let Some(biome_name) = host.biome([x, y, z]) {
                                provider.set_biome([qx, qy, qz], &biome_name);
                                populated_count += 1;
                            }
                        }
                    }
                }
            }
        }
    }

    log::info!(
        "Yefira: populated {populated_count} 3D quart biomes directly from runtime memory for selection"
    );
}
