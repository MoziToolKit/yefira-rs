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
    strings: parking_lot::RwLock<Vec<&'static str>>,
    string_lookup: parking_lot::RwLock<HashMap<String, u32>>,
    pub blocks: parking_lot::RwLock<HashMap<[i32; 3], u32>>,
    pub biomes: parking_lot::RwLock<HashMap<[i32; 3], u32>>,
    default_biome: &'static str,
}

impl Default for TrackingWorldProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl TrackingWorldProvider {
    pub fn new() -> Self {
        let mut strings = Vec::new();
        let mut string_lookup = HashMap::new();

        // 0 is always air
        strings.push("minecraft:air");
        string_lookup.insert("minecraft:air".to_string(), 0);

        // 1 is default plains biome
        strings.push("minecraft:plains");
        string_lookup.insert("minecraft:plains".to_string(), 1);

        Self {
            strings: parking_lot::RwLock::new(strings),
            string_lookup: parking_lot::RwLock::new(string_lookup),
            blocks: parking_lot::RwLock::new(HashMap::new()),
            biomes: parking_lot::RwLock::new(HashMap::new()),
            default_biome: "minecraft:plains",
        }
    }

    /// Interns a block state or biome string, returning its unique integer ID.
    fn intern(&self, s: &str) -> u32 {
        if let Some(&id) = self.string_lookup.read().get(s) {
            return id;
        }

        let mut lookup = self.string_lookup.write();
        if let Some(&id) = lookup.get(s) {
            return id;
        }

        let leaked: &'static str = Box::leak(s.to_string().into_boxed_str());
        let mut strings = self.strings.write();
        let id = strings.len() as u32;
        strings.push(leaked);
        lookup.insert(s.to_string(), id);
        id
    }

    pub fn set_block(&self, pos: [i32; 3], state: &str) {
        let id = self.intern(state);
        self.blocks.write().insert(pos, id);
    }

    pub fn bulk_set_blocks(&self, block_map: HashMap<[i32; 3], String>) {
        let mut write_blocks = self.blocks.write();
        for (pos, state) in block_map {
            let id = self.intern(&state);
            write_blocks.insert(pos, id);
        }
    }

    pub fn clear_blocks(&self) {
        self.blocks.write().clear();
    }

    pub fn set_biome(&self, quart_pos: [i32; 3], biome: &str) {
        let id = self.intern(biome);
        self.biomes.write().insert(quart_pos, id);
    }
}

impl WorldBlockProvider for TrackingWorldProvider {
    fn get_block_state(&self, pos: [i32; 3]) -> &str {
        let id_opt = self.blocks.read().get(&pos).copied();
        match id_opt {
            Some(id) => self
                .strings
                .read()
                .get(id as usize)
                .copied()
                .unwrap_or("minecraft:air"),
            None => "minecraft:air",
        }
    }

    fn get_quart_biome(&self, quart_pos: [i32; 3]) -> &str {
        let id_opt = self.biomes.read().get(&quart_pos).copied();
        match id_opt {
            Some(id) => self
                .strings
                .read()
                .get(id as usize)
                .copied()
                .unwrap_or(self.default_biome),
            None => self.default_biome,
        }
    }
}
