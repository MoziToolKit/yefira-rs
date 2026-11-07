//! Protocol definitions, packet structures, binary codecs, and hasher for RFC v3.1.

mod codec;
mod constants;
mod error;
mod hasher;
mod packet;

pub use codec::{decode_packet, encode_packet};
pub use constants::{
    BaselineAckResult, CancelReason, ErrorCode, Opcode, ResyncReason, SyncDecision,
    DEFAULT_CHUNK_SIZE, HEADER_SIZE, MAX_DELTA_BATCH_CHANGES, MAX_DELTA_STAGING_BATCHES,
    MAX_PALETTE_ENTRIES, MAX_SELECTION_VOLUME, PROTOCOL_MAGIC, PROTOCOL_VERSION,
};
pub use error::ProtocolError;
pub use hasher::{
    compute_canonical_state_hash, compute_delta_batch_hash, normalize_canonical_blockstate,
    SparseBlockEntry, SPARSE_CANONICAL_VERSION,
};
pub use packet::{DeltaChangeEntry, Packet};
