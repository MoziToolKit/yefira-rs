//! Strongly typed Live Sync binary packets (RFC v3.1 / RFC v3.2).

use glam::IVec3;
use serde::{Deserialize, Serialize};

use super::constants::{BaselineAckResult, CancelReason, ErrorCode, ResyncReason, SyncDecision};

/// Single block modification entry in a `DELTA_OP` packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeltaChangeEntry {
    /// Relative position within selection box: 0 <= rel_pos < size.
    pub rel_pos: IVec3,
    /// Direct index into current palette revision.
    pub state_id: u16,
}

/// Strongly-typed Live Sync Packet enum (RFC v3.1 Section 4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Packet {
    /// 0x01: Client Handshake Initiation (C -> S)
    Hello {
        min_version: u16,
        max_version: u16,
        capabilities: u32,
        flags: u32,
        cached_world_uuid: [u8; 16],
        cached_epoch: u64,
        cached_seq: u64,
        cached_min: IVec3,
        cached_size: IVec3,
        cached_state_hash: u64,
        auth_token: String,
        cached_dimension: String,
    },

    /// 0x02: Server Handshake Response (S -> C)
    Welcome {
        negotiated_version: u16,
        server_uuid: [u8; 16],
        epoch: u64,
        server_seq: u64,
        current_min: IVec3,
        current_size: IVec3,
        current_state_hash: u64,
        decision: SyncDecision,
        message_or_dimension: String,
    },

    /// 0x03: Baseline Metadata Beginning (S -> C)
    BaselineBegin {
        epoch: u64,
        snapshot_uuid: [u8; 16],
        snapshot_seq: u64,
        min_pos: IVec3,
        size: IVec3,
        state_hash: u64,
        palette_revision: u32,
        total_chunks: u32,
        total_encoded_bytes: u64,
        total_decoded_bytes: u64,
        compression_mode: u8,
        palette_bits_per_block: u8,
        biome_bits_per_quart: u8,
        dimension: String,
        block_palette: Vec<String>,
        biome_palette: Vec<String>,
    },

    /// 0x04: Baseline Voxel Chunk Stream (S -> C)
    BaselineChunk {
        epoch: u64,
        snapshot_uuid: [u8; 16],
        chunk_index: u32,
        total_chunks: u32,
        first_voxel_index: u64,
        voxel_count: u32,
        payload_len: u32,
        decoded_target_bytes: u32,
        chunk_xxhash64: u64,
        payload: Vec<u8>,
    },

    /// 0x05: Baseline Finished Notification (S -> C)
    BaselineEnd {
        epoch: u64,
        snapshot_uuid: [u8; 16],
        snapshot_seq: u64,
        total_chunks_sent: u32,
        final_state_hash: u64,
        status: u8,
    },

    /// 0x06: Baseline Client Commit Acknowledgment (C -> S)
    BaselineAck {
        epoch: u64,
        snapshot_uuid: [u8; 16],
        committed_seq: u64,
        result_code: BaselineAckResult,
    },

    /// 0x07: Incremental Block Delta Batch (S -> C)
    DeltaOp {
        epoch: u64,
        seq_id: u64,
        palette_revision: u32,
        count: u16,
        batch_xxhash64: u64,
        changes: Vec<DeltaChangeEntry>,
    },

    /// 0x08: Dynamic Palette Append (S -> C)
    PaletteUpdate {
        epoch: u64,
        seq_id: u64,
        base_revision: u32,
        new_revision: u32,
        entries: Vec<(u16, String)>,
    },

    /// 0x0B: Task / Session Cancellation (Bidirectional)
    Cancel {
        epoch: u64,
        snapshot_uuid: [u8; 16],
        reason: CancelReason,
    },

    /// 0x0D: Resync and Gap Recovery Request (C -> S)
    ResyncRequest {
        epoch: u64,
        missing_from_seq: u64,
        missing_to_seq: u64,
        reason: ResyncReason,
    },

    /// 0x0E: Keepalive Ping (Bidirectional)
    Ping { timestamp: u64 },

    /// 0x0F: Keepalive Pong (Bidirectional)
    Pong { timestamp: u64 },

    /// 0x10: Diagnostic Error Response (Bidirectional)
    Error {
        epoch: u64,
        error_code: ErrorCode,
        message: String,
    },

    /// 0x11: Sparse BlockEntity Stream (RFC v3.2, S -> C)
    BaselineEntities {
        epoch: u64,
        snapshot_uuid: [u8; 16],
        entities: Vec<(IVec3, String)>,
    },

    /// 0x12: Sparse BlockEntity Delta Update (RFC v3.2, S -> C)
    DeltaEntity {
        epoch: u64,
        seq_id: u64,
        rel_pos: IVec3,
        entity_json: Option<String>,
    },
}
