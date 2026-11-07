//! Baseline stream serialization and chunk generator conforming to RFC v3.1.

use xxhash_rust::xxh64::Xxh64;

use crate::protocol::{Packet, DEFAULT_CHUNK_SIZE};
use crate::scanner::BaselineScanResult;

/// Generates the sequence of RFC v3.1 packets for transmitting an authoritative baseline.
pub struct BaselineStreamer;

impl BaselineStreamer {
    /// Builds the list of packets needed to stream a baseline snapshot to a client:
    /// 1. `Packet::BaselineBegin`
    /// 2. `Packet::BaselineChunk` (one or more chunks, max DEFAULT_CHUNK_SIZE each)
    /// 3. `Packet::BaselineEnd`
    /// 4. `Packet::BaselineEntities` (if any block entities are present)
    pub fn build_baseline_packets(
        scan: &BaselineScanResult,
        epoch: u64,
        snapshot_uuid: [u8; 16],
        snapshot_seq: u64,
        palette_revision: u32,
    ) -> Vec<Packet> {
        let total_bytes = scan.payload.len();
        let chunk_size = DEFAULT_CHUNK_SIZE;
        let total_chunks = if total_bytes == 0 {
            1
        } else {
            total_bytes.div_ceil(chunk_size) as u32
        };

        let mut packets = Vec::with_capacity((total_chunks + 3) as usize);

        // 1. BaselineBegin
        packets.push(Packet::BaselineBegin {
            epoch,
            snapshot_uuid,
            snapshot_seq,
            min_pos: scan.min_pos,
            size: scan.size,
            state_hash: scan.state_hash,
            palette_revision,
            total_chunks,
            total_encoded_bytes: total_bytes as u64,
            total_decoded_bytes: total_bytes as u64,
            compression_mode: 0, // Uncompressed
            palette_bits_per_block: scan.palette_bits_per_block,
            biome_bits_per_quart: scan.biome_bits_per_quart,
            dimension: scan.dimension.clone(),
            block_palette: scan.block_palette.clone(),
            biome_palette: scan.biome_palette.clone(),
        });

        // 2. BaselineChunk stream
        let bytes_per_voxel = (scan.palette_bits_per_block as usize / 8).max(1);
        let voxels_per_chunk = chunk_size / bytes_per_voxel;

        for chunk_idx in 0..total_chunks {
            let offset = chunk_idx as usize * chunk_size;
            let len = (total_bytes.saturating_sub(offset)).min(chunk_size);
            let chunk_payload = if len > 0 {
                scan.payload[offset..offset + len].to_vec()
            } else {
                Vec::new()
            };

            // Clamp cursor to exact voxel count (handles trailing biome-only chunks)
            let first_voxel_index =
                ((chunk_idx as u64) * (voxels_per_chunk as u64)).min(scan.total_voxels);
            let remaining_voxels = scan.total_voxels.saturating_sub(first_voxel_index);
            let voxel_count = (voxels_per_chunk as u64).min(remaining_voxels) as u32;

            let mut chunk_hasher = Xxh64::new(0);
            chunk_hasher.update(&chunk_payload);
            let chunk_xxhash64 = chunk_hasher.digest();

            packets.push(Packet::BaselineChunk {
                epoch,
                snapshot_uuid,
                chunk_index: chunk_idx,
                total_chunks,
                first_voxel_index,
                voxel_count,
                payload_len: len as u32,
                decoded_target_bytes: len as u32,
                chunk_xxhash64,
                payload: chunk_payload,
            });
        }

        // 3. BaselineEntities (RFC v3.2, sent before BaselineEnd)
        if !scan.entities.is_empty() {
            packets.push(Packet::BaselineEntities {
                epoch,
                snapshot_uuid,
                entities: scan.entities.clone(),
            });
        }

        // 4. BaselineEnd (finalizes snapshot on client)
        packets.push(Packet::BaselineEnd {
            epoch,
            snapshot_uuid,
            snapshot_seq,
            total_chunks_sent: total_chunks,
            final_state_hash: scan.state_hash,
            status: 0, // OK
        });

        packets
    }
}
