//! Global thread-safe selection manager and change dispatch.

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use super::box_3d::SelectionBox;

/// Persistent JSON format for saved selections (matching Yefira Java format).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectionStorageData {
    pub pos1: [i32; 3],
    pub pos2: [i32; 3],
    pub dimension: String,
}

type SelectionCallback = Box<dyn Fn(Option<&SelectionBox>, &str) + Send + Sync>;

struct InnerState {
    pos1: Option<[i32; 3]>,
    pos2: Option<[i32; 3]>,
    dimension: String,
    current_selection: Option<SelectionBox>,
    listeners: Vec<Arc<SelectionCallback>>,
}

/// Thread-safe manager for current live sync selection.
pub struct SelectionManager {
    inner: RwLock<InnerState>,
    generation: AtomicU64,
}

impl Default for SelectionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SelectionManager {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(InnerState {
                pos1: None,
                pos2: None,
                dimension: "minecraft:overworld".to_string(),
                current_selection: None,
                listeners: Vec::new(),
            }),
            generation: AtomicU64::new(1),
        }
    }

    /// Monotonically increasing generation number, incremented on every selection change.
    #[inline]
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// Returns the current selection if both pos1 and pos2 are defined.
    pub fn current_selection(&self) -> Option<SelectionBox> {
        self.inner.read().current_selection
    }

    /// Returns the current dimension ID (e.g. "minecraft:overworld").
    pub fn dimension(&self) -> String {
        self.inner.read().dimension.clone()
    }

    /// Checks if a selection is currently defined.
    pub fn has_selection(&self) -> bool {
        self.inner.read().current_selection.is_some()
    }

    /// Returns pos1 if defined.
    pub fn pos1(&self) -> Option<[i32; 3]> {
        self.inner.read().pos1
    }

    /// Returns pos2 if defined.
    pub fn pos2(&self) -> Option<[i32; 3]> {
        self.inner.read().pos2
    }

    /// Registers a selection change listener.
    pub fn add_listener<F>(&self, callback: F)
    where
        F: Fn(Option<&SelectionBox>, &str) + Send + Sync + 'static,
    {
        self.inner
            .write()
            .listeners
            .push(Arc::new(Box::new(callback)));
    }

    /// Sets `pos1` and updates the selection if `pos2` is set.
    pub fn set_pos1(&self, pos: [i32; 3], dimension: &str) -> Option<SelectionBox> {
        let (sel, listeners, dim) = {
            let mut state = self.inner.write();
            state.pos1 = Some(pos);
            state.dimension = dimension.to_string();
            let sel = state.pos2.map(|p2| SelectionBox::new(pos, p2));
            state.current_selection = sel;
            self.generation.fetch_add(1, Ordering::Release);
            (sel, state.listeners.clone(), state.dimension.clone())
        };

        for listener in listeners {
            listener(sel.as_ref(), &dim);
        }
        sel
    }

    /// Sets `pos2` and updates the selection if `pos1` is set.
    pub fn set_pos2(&self, pos: [i32; 3], dimension: &str) -> Option<SelectionBox> {
        let (sel, listeners, dim) = {
            let mut state = self.inner.write();
            state.pos2 = Some(pos);
            state.dimension = dimension.to_string();
            let sel = state.pos1.map(|p1| SelectionBox::new(p1, pos));
            state.current_selection = sel;
            self.generation.fetch_add(1, Ordering::Release);
            (sel, state.listeners.clone(), state.dimension.clone())
        };

        for listener in listeners {
            listener(sel.as_ref(), &dim);
        }
        sel
    }

    /// Sets both corners simultaneously.
    pub fn set_selection(&self, pos1: [i32; 3], pos2: [i32; 3], dimension: &str) -> SelectionBox {
        let sel = SelectionBox::new(pos1, pos2);
        let (listeners, dim) = {
            let mut state = self.inner.write();
            state.pos1 = Some(pos1);
            state.pos2 = Some(pos2);
            state.dimension = dimension.to_string();
            state.current_selection = Some(sel);
            self.generation.fetch_add(1, Ordering::Release);
            (state.listeners.clone(), state.dimension.clone())
        };

        for listener in listeners {
            listener(Some(&sel), &dim);
        }
        sel
    }

    /// Clears the current selection.
    pub fn clear(&self) {
        let (listeners, dim) = {
            let mut state = self.inner.write();
            state.pos1 = None;
            state.pos2 = None;
            state.current_selection = None;
            self.generation.fetch_add(1, Ordering::Release);
            (state.listeners.clone(), state.dimension.clone())
        };

        for listener in listeners {
            listener(None, &dim);
        }
    }

    /// Saves the current selection to disk.
    pub fn save_to_path(&self, path: &Path) -> io::Result<bool> {
        let state = self.inner.read();
        let (Some(p1), Some(p2)) = (state.pos1, state.pos2) else {
            return Ok(false);
        };
        let data = SelectionStorageData {
            pos1: p1,
            pos2: p2,
            dimension: state.dimension.clone(),
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json_str = serde_json::to_string_pretty(&data)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        fs::write(path, json_str)?;
        Ok(true)
    }

    /// Loads saved selection from disk.
    pub fn load_from_path(&self, path: &Path) -> io::Result<Option<SelectionBox>> {
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(path)?;
        let data: SelectionStorageData = serde_json::from_str(&content)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Ok(Some(self.set_selection(
            data.pos1,
            data.pos2,
            &data.dimension,
        )))
    }
}
