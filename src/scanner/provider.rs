//! Abstract world block provider interface for voxel scanning.

use std::collections::HashMap;

/// Trait abstracting world voxel access for canonical scanning.
/// This allows scanning across Horizon world data, client render data, or test fixtures.
pub trait WorldBlockProvider: Send + Sync {
    /// Returns the canonical block state string at world coordinates `[x, y, z]`.
    /// E.g. "minecraft:stone", "minecraft:oak_stairs[facing=north,half=bottom]".
    fn get_block_state(&self, pos: [i32; 3]) -> &str;

    /// Returns the biome identifier at 4x4x4 quart coordinates `[qx, qy, qz]`.
    /// E.g. "minecraft:plains", "minecraft:desert".
    fn get_quart_biome(&self, quart_pos: [i32; 3]) -> &str;

    /// Returns the serialized block entity JSON data if present at `[x, y, z]`.
    fn get_block_entity_json(&self, pos: [i32; 3]) -> Option<String> {
        let _ = pos;
        None
    }

    /// Checks if the chunk containing `(cx, cz)` is loaded. Default: true.
    fn is_chunk_loaded(&self, chunk_x: i32, chunk_z: i32) -> bool {
        let _ = (chunk_x, chunk_z);
        true
    }
}

/// Simple in-memory world provider for tests and validation.
#[derive(Debug, Default, Clone)]
pub struct InMemoryWorld {
    pub blocks: HashMap<[i32; 3], String>,
    pub biomes: HashMap<[i32; 3], String>,
    pub entities: HashMap<[i32; 3], String>,
    pub default_biome: String,
}

impl InMemoryWorld {
    pub fn new() -> Self {
        Self {
            blocks: HashMap::new(),
            biomes: HashMap::new(),
            entities: HashMap::new(),
            default_biome: "minecraft:plains".to_string(),
        }
    }

    pub fn set_block(&mut self, pos: [i32; 3], state: impl Into<String>) {
        self.blocks.insert(pos, state.into());
    }

    pub fn set_biome(&mut self, quart_pos: [i32; 3], biome: impl Into<String>) {
        self.biomes.insert(quart_pos, biome.into());
    }

    pub fn set_entity(&mut self, pos: [i32; 3], json: impl Into<String>) {
        self.entities.insert(pos, json.into());
    }
}

impl WorldBlockProvider for InMemoryWorld {
    fn get_block_state(&self, pos: [i32; 3]) -> &str {
        self.blocks
            .get(&pos)
            .map(|s| s.as_str())
            .unwrap_or("minecraft:air")
    }

    fn get_quart_biome(&self, quart_pos: [i32; 3]) -> &str {
        self.biomes
            .get(&quart_pos)
            .map(|s| s.as_str())
            .unwrap_or(&self.default_biome)
    }

    fn get_block_entity_json(&self, pos: [i32; 3]) -> Option<String> {
        self.entities.get(&pos).cloned()
    }
}

/// Thread-safe tracking world provider that dynamically updates as blocks change in the game.
#[derive(Debug)]
pub struct TrackingWorldProvider {
    pub blocks: parking_lot::RwLock<HashMap<[i32; 3], &'static str>>,
    pub biomes: parking_lot::RwLock<HashMap<[i32; 3], &'static str>>,
    pub default_biome: String,
}

impl Default for TrackingWorldProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl TrackingWorldProvider {
    pub fn new() -> Self {
        Self {
            blocks: parking_lot::RwLock::new(HashMap::new()),
            biomes: parking_lot::RwLock::new(HashMap::new()),
            default_biome: "minecraft:plains".to_string(),
        }
    }

    pub fn set_block(&self, pos: [i32; 3], state: &str) {
        let leaked: &'static str = Box::leak(state.to_string().into_boxed_str());
        self.blocks.write().insert(pos, leaked);
    }

    pub fn set_biome(&self, quart_pos: [i32; 3], biome: &str) {
        let leaked: &'static str = Box::leak(biome.to_string().into_boxed_str());
        self.biomes.write().insert(quart_pos, leaked);
    }
}

impl WorldBlockProvider for TrackingWorldProvider {
    fn get_block_state(&self, pos: [i32; 3]) -> &str {
        self.blocks
            .read()
            .get(&pos)
            .copied()
            .unwrap_or("minecraft:air")
    }

    fn get_quart_biome(&self, quart_pos: [i32; 3]) -> &str {
        self.biomes
            .read()
            .get(&quart_pos)
            .copied()
            .unwrap_or(&self.default_biome)
    }
}
