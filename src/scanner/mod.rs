//! Voxel scanner module for extracting continuous voxel volumes and biomes.

pub mod biome_reader;
mod canonical;
mod provider;

pub use biome_reader::populate_selection_biomes;
pub use canonical::{BaselineScanResult, CanonicalVoxelScanner, ScanError};
pub use provider::{InMemoryWorld, TrackingWorldProvider, WorldBlockProvider};
