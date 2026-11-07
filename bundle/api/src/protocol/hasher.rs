//! Canonical state hash and delta batch hash calculations using xxHash64 (RFC v3.1).

use glam::IVec3;
use xxhash_rust::xxh64::Xxh64;

use super::packet::DeltaChangeEntry;

/// Sparse canonical encoding format version: 0x02.
pub const SPARSE_CANONICAL_VERSION: u8 = 0x02;

/// Normalizes block state strings according to RFC Section 3.1:
/// - Void air, cave air, structure void, empty string -> "minecraft:air"
/// - All other blocks preserve their authentic state identifier and sorted properties.
pub fn normalize_canonical_blockstate(raw: &str) -> &str {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "minecraft:air";
    }
    let base = trimmed
        .find('[')
        .map_or(trimmed, |bracket| &trimmed[..bracket]);
    match base {
        "minecraft:air"
        | "air"
        | "minecraft:cave_air"
        | "cave_air"
        | "minecraft:void_air"
        | "void_air"
        | "minecraft:structure_void"
        | "structure_void" => "minecraft:air",
        _ => trimmed,
    }
}

/// An entry of a non-air block for canonical state hash fingerprinting.
pub struct SparseBlockEntry<'a> {
    pub rel_x: u32,
    pub rel_y: u32,
    pub rel_z: u32,
    pub state: &'a str,
    pub biome: &'a str,
    pub entity_json: Option<&'a str>,
}

/// Computes the authoritative canonical sparse state_hash across the 3D bounding volume.
///
/// Only non-air blocks are hashed. Air blocks and empty spaces are completely ignored.
/// Non-air blocks MUST be provided in canonical (Y-slowest, Z-next, X-fastest) order.
pub fn compute_canonical_state_hash<'a, I>(
    min_pos: IVec3,
    size: IVec3,
    dimension_str: &str,
    non_air_blocks: I,
) -> u64
where
    I: IntoIterator<Item = SparseBlockEntry<'a>>,
{
    let mut hasher = Xxh64::new(0);

    // 1. Metadata Header
    hasher.update(&[SPARSE_CANONICAL_VERSION]);
    let dim_bytes = dimension_str.as_bytes();
    hasher.update(&(dim_bytes.len() as u16).to_le_bytes());
    hasher.update(dim_bytes);
    hasher.update(&min_pos.x.to_le_bytes());
    hasher.update(&min_pos.y.to_le_bytes());
    hasher.update(&min_pos.z.to_le_bytes());
    hasher.update(&(size.x as u32).to_le_bytes());
    hasher.update(&(size.y as u32).to_le_bytes());
    hasher.update(&(size.z as u32).to_le_bytes());

    // 2. Stream of non-air blocks in (Y, Z, X) order
    for entry in non_air_blocks {
        hasher.update(&entry.rel_x.to_le_bytes());
        hasher.update(&entry.rel_y.to_le_bytes());
        hasher.update(&entry.rel_z.to_le_bytes());

        let norm_state = normalize_canonical_blockstate(entry.state);
        let state_bytes = norm_state.as_bytes();
        hasher.update(&(state_bytes.len() as u16).to_le_bytes());
        hasher.update(state_bytes);

        let trimmed_biome = entry.biome.trim();
        let biome_bytes = if trimmed_biome.is_empty() {
            b"minecraft:plains".as_slice()
        } else {
            trimmed_biome.as_bytes()
        };
        hasher.update(&(biome_bytes.len() as u16).to_le_bytes());
        hasher.update(biome_bytes);

        if let Some(json) = entry.entity_json.filter(|s| !s.trim().is_empty()) {
            let json_bytes = json.as_bytes();
            hasher.update(&(json_bytes.len() as u32).to_le_bytes());
            hasher.update(json_bytes);
        } else {
            hasher.update(&0u32.to_le_bytes());
        }
    }

    hasher.digest()
}

/// Computes xxHash64 checksum across a batch of delta changes (RFC Section 4.7).
pub fn compute_delta_batch_hash(changes: &[DeltaChangeEntry]) -> u64 {
    let mut hasher = Xxh64::new(0);
    for change in changes {
        hasher.update(&change.rel_pos.x.to_le_bytes());
        hasher.update(&change.rel_pos.y.to_le_bytes());
        hasher.update(&change.rel_pos.z.to_le_bytes());
        hasher.update(&change.state_id.to_le_bytes());
    }
    hasher.digest()
}
