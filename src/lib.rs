#![forbid(unsafe_code)]
//! # Yefira-RS: Live Sync for Horizon & libmozitoolkit
//!
//! **yefira-rs** is a pure Rust implementation of the Yefira Live Sync system for the
//! [Horizon](https://github.com/horizon-rust/horizon) Minecraft client and dedicated server.
//!
//! It conforms strictly to **RFC v3.1** (and RFC v3.2 Sparse BlockEntity extensions),
//! hosting an authoritative WebSocket streaming server that synchronizes Minecraft
//! continuous voxel fields and incremental block deltas in real-time with
//! `libmozitoolkit` (`mtk-sync`) and Blender DCC tools (`MoziToolKit`).

pub mod command;
pub mod config;
pub mod horizon;
pub mod protocol;
pub mod scanner;
pub mod selection;
pub mod server;

pub use command::{execute_command, CommandOutput};
pub use config::YefiraConfig;
pub use horizon::YefiraMod;
pub use protocol::{
    compute_canonical_state_hash, compute_delta_batch_hash, decode_packet, encode_packet,
    normalize_canonical_blockstate, BaselineAckResult, CancelReason, DeltaChangeEntry, ErrorCode,
    Opcode, Packet, ProtocolError, ResyncReason, SparseBlockEntry, SyncDecision, PROTOCOL_MAGIC,
    PROTOCOL_VERSION,
};
pub use scanner::{
    BaselineScanResult, CanonicalVoxelScanner, InMemoryWorld, ScanError, TrackingWorldProvider,
    WorldBlockProvider,
};
pub use selection::{Direction, SelectionBox, SelectionManager, SelectionStorageData};
pub use server::{
    BaselineStreamer, BlockChangeRecord, ClientSession, DeltaQueue, WebSocketServerManager,
};

#[cfg(feature = "horizon")]
pub use horizon::hooks::hooks;
