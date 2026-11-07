//! Authoritative 3D continuous voxel and biome scanner conforming to RFC v3.1.

use glam::IVec3;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use thiserror::Error;
use xxhash_rust::xxh64::Xxh64;

use super::provider::WorldBlockProvider;
use crate::protocol::{SPARSE_CANONICAL_VERSION, normalize_canonical_blockstate};
use crate::selection::SelectionBox;

#[derive(Debug, Error)]
pub enum ScanError {
    #[error("Scan timed out after {0:?}")]
    Timeout(Duration),

    #[error("Scan was cancelled by newer selection")]
    Cancelled,

    #[error("Selection volume exceeds hard limit: {0} voxels")]
    VolumeExceeded(u64),
}

/// The result of an authoritative baseline scan.
#[derive(Debug, Clone, PartialEq)]
pub struct BaselineScanResult {
    pub min_pos: IVec3,
    pub size: IVec3,
    pub total_voxels: u64,
    pub dimension: String,
    pub block_palette: Vec<String>,
    pub biome_palette: Vec<String>,
    pub payload: Vec<u8>,
    pub palette_bits_per_block: u8,
    pub biome_bits_per_quart: u8,
    pub state_hash: u64,
    pub entities: Vec<(IVec3, String)>,
}

/// Authoritative 3D continuous voxel scanner.
pub struct CanonicalVoxelScanner;

impl CanonicalVoxelScanner {
    /// Default maximum scan duration before aborting (120 seconds).
    pub const DEFAULT_MAX_SCAN_DURATION: Duration = Duration::from_secs(120);

    /// Scans the bounding volume of `box_3d`, generating canonical palette, packed payload,
    /// and authoritative xxHash64 fingerprint.
    pub fn scan_baseline<P, F>(
        provider: &P,
        box_3d: &SelectionBox,
        dimension: &str,
        is_cancelled: F,
    ) -> Result<BaselineScanResult, ScanError>
    where
        P: WorldBlockProvider + ?Sized,
        F: Fn() -> bool,
    {
        Self::scan_baseline_with_timeout(
            provider,
            box_3d,
            dimension,
            is_cancelled,
            Self::DEFAULT_MAX_SCAN_DURATION,
        )
    }

    /// Scans with an explicit timeout.
    pub fn scan_baseline_with_timeout<P, F>(
        provider: &P,
        box_3d: &SelectionBox,
        dimension: &str,
        is_cancelled: F,
        timeout: Duration,
    ) -> Result<BaselineScanResult, ScanError>
    where
        P: WorldBlockProvider + ?Sized,
        F: Fn() -> bool,
    {
        let start_time = Instant::now();
        let min = box_3d.min();
        let min_x = min[0];
        let min_y = min[1];
        let min_z = min[2];
        let size_x = box_3d.size_x() as i32;
        let size_y = box_3d.size_y() as i32;
        let size_z = box_3d.size_z() as i32;
        let total_voxels = box_3d.volume();

        if total_voxels > crate::protocol::MAX_SELECTION_VOLUME {
            return Err(ScanError::VolumeExceeded(total_voxels));
        }

        let mut hasher = Xxh64::new(0);

        // 1. Header Hash (Sparse canonical encoding: 0x02)
        hasher.update(&[SPARSE_CANONICAL_VERSION]);
        let dim_bytes = dimension.as_bytes();
        hasher.update(&(dim_bytes.len() as u16).to_le_bytes());
        hasher.update(dim_bytes);
        hasher.update(&min_x.to_le_bytes());
        hasher.update(&min_y.to_le_bytes());
        hasher.update(&min_z.to_le_bytes());
        hasher.update(&(size_x as u32).to_le_bytes());
        hasher.update(&(size_y as u32).to_le_bytes());
        hasher.update(&(size_z as u32).to_le_bytes());

        let mut block_palette: Vec<String> = Vec::with_capacity(64);
        let mut block_palette_lookup: HashMap<String, u16> = HashMap::with_capacity(64);

        // Air is always entry 0
        block_palette.push("minecraft:air".to_string());
        block_palette_lookup.insert("minecraft:air".to_string(), 0);

        let mut voxel_indices = Vec::with_capacity(total_voxels as usize);
        let mut entities = Vec::new();

        let max_x = min_x + size_x;
        let max_y = min_y + size_y;
        let max_z = min_z + size_z;

        // 2. Voxel Scan (Y bottom-up, Z next, X fastest)
        for y in min_y..max_y {
            if is_cancelled() {
                return Err(ScanError::Cancelled);
            }
            if start_time.elapsed() >= timeout {
                return Err(ScanError::Timeout(timeout));
            }

            for z in min_z..max_z {
                for x in min_x..max_x {
                    let raw_state = provider.get_block_state([x, y, z]);
                    let norm_state = normalize_canonical_blockstate(raw_state);

                    let rx = (x - min_x) as u32;
                    let ry = (y - min_y) as u32;
                    let rz = (z - min_z) as u32;

                    let qx = x >> 2;
                    let qy = y >> 2;
                    let qz = z >> 2;
                    let biome_str = provider.get_quart_biome([qx, qy, qz]).trim();
                    let biome = if biome_str.is_empty() {
                        "minecraft:plains"
                    } else {
                        biome_str
                    };

                    let entity_json = provider.get_block_entity_json([x, y, z]);

                    // Hash non-air blocks in canonical order
                    if norm_state != "minecraft:air" {
                        hasher.update(&rx.to_le_bytes());
                        hasher.update(&ry.to_le_bytes());
                        hasher.update(&rz.to_le_bytes());

                        let state_bytes = norm_state.as_bytes();
                        hasher.update(&(state_bytes.len() as u16).to_le_bytes());
                        hasher.update(state_bytes);

                        let biome_bytes = biome.as_bytes();
                        hasher.update(&(biome_bytes.len() as u16).to_le_bytes());
                        hasher.update(biome_bytes);

                        if let Some(ref json) = entity_json {
                            let json_bytes = json.as_bytes();
                            hasher.update(&(json_bytes.len() as u32).to_le_bytes());
                            hasher.update(json_bytes);
                        } else {
                            hasher.update(&0u32.to_le_bytes());
                        }
                    }

                    if let Some(json) = entity_json {
                        entities.push((IVec3::new(rx as i32, ry as i32, rz as i32), json));
                    }

                    // Palette lookup
                    let palette_idx = if norm_state == "minecraft:air" {
                        0
                    } else if let Some(&id) = block_palette_lookup.get(norm_state) {
                        id
                    } else {
                        let id = block_palette.len() as u16;
                        block_palette.push(norm_state.to_string());
                        block_palette_lookup.insert(norm_state.to_string(), id);
                        id
                    };

                    voxel_indices.push(palette_idx);
                }
            }
        }

        // 3. Biome Scan (4x4x4 quart grid)
        let min_qx = min_x >> 2;
        let max_qx = (max_x - 1) >> 2;
        let qw = (max_qx - min_qx + 1).max(0);

        let min_qy = min_y >> 2;
        let max_qy = (max_y - 1) >> 2;
        let qh = (max_qy - min_qy + 1).max(0);

        let min_qz = min_z >> 2;
        let max_qz = (max_z - 1) >> 2;
        let qd = (max_qz - min_qz + 1).max(0);
        let total_quarts = (qw * qh * qd) as usize;

        let mut biome_palette: Vec<String> = Vec::with_capacity(8);
        let mut biome_palette_lookup: HashMap<String, u8> = HashMap::with_capacity(8);
        let mut biome_indices: Vec<u8> = Vec::with_capacity(total_quarts);

        for qy in 0..qh {
            if is_cancelled() {
                return Err(ScanError::Cancelled);
            }
            let world_qy = min_qy + qy;

            for qz in 0..qd {
                let world_qz = min_qz + qz;

                for qx in 0..qw {
                    let world_qx = min_qx + qx;
                    let biome_str = provider
                        .get_quart_biome([world_qx, world_qy, world_qz])
                        .trim();
                    let b = if biome_str.is_empty() {
                        "minecraft:plains"
                    } else {
                        biome_str
                    };

                    let b_id = if let Some(&id) = biome_palette_lookup.get(b) {
                        id
                    } else {
                        let id = biome_palette.len() as u8;
                        biome_palette.push(b.to_string());
                        biome_palette_lookup.insert(b.to_string(), id);
                        id
                    };
                    biome_indices.push(b_id);
                }
            }
        }

        if biome_palette.is_empty() {
            biome_palette.push("minecraft:plains".to_string());
        }

        let state_hash = hasher.digest();

        // 4. Pack payload (LSB packed indices)
        let bits_per_block = if block_palette.len() <= 256 { 8 } else { 16 };
        let voxel_bytes_len = total_voxels as usize * (bits_per_block / 8);

        let has_multiple_biomes = biome_palette.len() > 1;
        let biome_bits = if has_multiple_biomes { 8 } else { 0 };
        let biome_bytes_len = if has_multiple_biomes { total_quarts } else { 0 };

        let mut payload = Vec::with_capacity(voxel_bytes_len + biome_bytes_len);

        if bits_per_block == 8 {
            for &idx in &voxel_indices {
                payload.push(idx as u8);
            }
        } else {
            for &idx in &voxel_indices {
                payload.extend_from_slice(&idx.to_le_bytes());
            }
        }

        if has_multiple_biomes {
            payload.extend_from_slice(&biome_indices);
        }

        Ok(BaselineScanResult {
            min_pos: IVec3::new(min_x, min_y, min_z),
            size: IVec3::new(size_x, size_y, size_z),
            total_voxels,
            dimension: dimension.to_string(),
            block_palette,
            biome_palette,
            payload,
            palette_bits_per_block: bits_per_block as u8,
            biome_bits_per_quart: biome_bits,
            state_hash,
            entities,
        })
    }
}
