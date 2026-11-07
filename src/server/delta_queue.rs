//! Pending delta block change queue and batch packer.

use glam::IVec3;
use parking_lot::Mutex;
use std::collections::{HashMap, VecDeque};

use crate::protocol::{
    DeltaChangeEntry, MAX_DELTA_BATCH_CHANGES, Packet, compute_delta_batch_hash,
};
use crate::selection::SelectionBox;

/// Record of an authoritative block change in world coordinates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockChangeRecord {
    pub pos: [i32; 3],
    pub new_state: String,
}

/// Thread-safe queue collecting block changes between server ticks.
pub struct DeltaQueue {
    pending: Mutex<HashMap<[i32; 3], String>>,
    overflowed: Mutex<bool>,
}

impl Default for DeltaQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl DeltaQueue {
    pub const MAX_PENDING_CHANGES: usize = 262_144;

    pub fn new() -> Self {
        Self {
            pending: Mutex::new(HashMap::with_capacity(256)),
            overflowed: Mutex::new(false),
        }
    }

    /// Queues a block change. If queue exceeds maximum capacity, sets overflow flag.
    pub fn queue_change(&self, pos: [i32; 3], new_state: impl Into<String>) {
        let mut map = self.pending.lock();
        if map.len() >= Self::MAX_PENDING_CHANGES {
            *self.overflowed.lock() = true;
            return;
        }
        map.insert(pos, new_state.into());
    }

    /// Checks and clears the overflow flag.
    pub fn take_overflow(&self) -> bool {
        let mut flag = self.overflowed.lock();
        let val = *flag;
        *flag = false;
        val
    }

    /// Returns the number of currently queued changes.
    pub fn len(&self) -> usize {
        self.pending.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.pending.lock().is_empty()
    }

    /// Clears the pending queue.
    pub fn clear(&self) {
        self.pending.lock().clear();
        *self.overflowed.lock() = false;
    }

    /// Flushes queued changes inside `selection`, assigning palette IDs and generating
    /// `Packet::PaletteUpdate` (if needed) and `Packet::DeltaOp` packets.
    pub fn flush_deltas(
        &self,
        selection: &SelectionBox,
        epoch: u64,
        next_seq_id: &mut u64,
        palette_lookup: &mut HashMap<String, u16>,
        palette_list: &mut Vec<String>,
        palette_revision: &mut u32,
    ) -> Vec<Packet> {
        let changes_map = {
            let mut map = self.pending.lock();
            if map.is_empty() {
                return Vec::new();
            }
            std::mem::take(&mut *map)
        };

        let min = selection.min();
        let mut new_palette_entries = Vec::new();
        let mut delta_entries = Vec::with_capacity(changes_map.len());

        for (pos, raw_state) in changes_map {
            if !selection.contains(pos) {
                continue;
            }

            let norm_state = crate::protocol::normalize_canonical_blockstate(&raw_state);
            let state_id = if norm_state == "minecraft:air" {
                0
            } else if let Some(&id) = palette_lookup.get(norm_state) {
                id
            } else {
                let id = palette_list.len() as u16;
                palette_list.push(norm_state.to_string());
                palette_lookup.insert(norm_state.to_string(), id);
                new_palette_entries.push((id, norm_state.to_string()));
                id
            };

            let rel_x = pos[0] - min[0];
            let rel_y = pos[1] - min[1];
            let rel_z = pos[2] - min[2];

            delta_entries.push(DeltaChangeEntry {
                rel_pos: IVec3::new(rel_x, rel_y, rel_z),
                state_id,
            });
        }

        if delta_entries.is_empty() && new_palette_entries.is_empty() {
            return Vec::new();
        }

        let mut packets = Vec::new();
        let mut remaining = VecDeque::from(delta_entries);
        let mut first_batch = true;

        while !remaining.is_empty() {
            let batch_len = remaining.len().min(MAX_DELTA_BATCH_CHANGES);
            let batch: Vec<DeltaChangeEntry> = remaining.drain(..batch_len).collect();
            let batch_hash = compute_delta_batch_hash(&batch);

            *next_seq_id += 1;
            let seq = *next_seq_id;

            // 1. Dynamic palette update if new block states appeared in this flush (sent once before delta)
            if first_batch && !new_palette_entries.is_empty() {
                let base_rev = *palette_revision;
                *palette_revision += 1;
                let new_rev = *palette_revision;

                packets.push(Packet::PaletteUpdate {
                    epoch,
                    seq_id: seq,
                    base_revision: base_rev,
                    new_revision: new_rev,
                    entries: new_palette_entries.clone(),
                });
                first_batch = false;
            }

            // 2. DeltaOp carrying the changes for this batch (shares the same sequence ID with PaletteUpdate)
            packets.push(Packet::DeltaOp {
                epoch,
                seq_id: seq,
                palette_revision: *palette_revision,
                count: batch.len() as u16,
                batch_xxhash64: batch_hash,
                changes: batch,
            });
        }

        packets
    }
}
