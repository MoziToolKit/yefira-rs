//! Voxel scanner module for extracting continuous voxel volumes and biomes.

mod canonical;
mod provider;

pub use canonical::{BaselineScanResult, CanonicalVoxelScanner, ScanError};
pub use provider::{InMemoryWorld, WorldBlockProvider};
