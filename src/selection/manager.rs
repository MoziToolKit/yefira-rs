//! Global thread-safe selection manager and change dispatch.

use parking_lot::RwLock;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use super::box_3d::SelectionBox;
use super::storage;

type SelectionCallback = Box<dyn Fn(Option<&SelectionBox>, &str) + Send + Sync>;

struct InnerState {
    pos1: Option<[i32; 3]>,
    pos2: Option<[i32; 3]>,
    dimension: String,
    current_selection: Option<SelectionBox>,
    active_storage_path: Option<PathBuf>,
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
                active_storage_path: None,
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

    /// Returns the active storage path on disk (if associated with a world or server).
    pub fn active_storage_path(&self) -> Option<PathBuf> {
        self.inner.read().active_storage_path.clone()
    }

    /// Sets the active storage path for subsequent auto-saves and deletes.
    pub fn set_active_storage_path(&self, path: Option<PathBuf>) {
        self.inner.write().active_storage_path = path;
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
    ///
    /// If dimension changed, resets pos2 to prevent cross-dimensional bounding boxes.
    pub fn set_pos1(&self, pos: [i32; 3], dimension: &str) -> Option<SelectionBox> {
        let (sel, listeners, dim, storage_path) = {
            let mut state = self.inner.write();
            if state.dimension != dimension {
                state.pos2 = None;
            }
            state.pos1 = Some(pos);
            state.dimension = dimension.to_string();
            let sel = state.pos2.map(|p2| SelectionBox::new(pos, p2));
            state.current_selection = sel;
            self.generation.fetch_add(1, Ordering::Release);
            (
                sel,
                state.listeners.clone(),
                state.dimension.clone(),
                state.active_storage_path.clone(),
            )
        };

        if let (Some(sel_box), Some(path)) = (sel, storage_path) {
            let _ = storage::save_to_path(&path, sel_box.pos1(), sel_box.pos2(), &dim);
        }

        for listener in listeners {
            listener(sel.as_ref(), &dim);
        }
        sel
    }

    /// Sets `pos2` and updates the selection if `pos1` is set.
    ///
    /// If dimension changed, resets pos1 to prevent cross-dimensional bounding boxes.
    pub fn set_pos2(&self, pos: [i32; 3], dimension: &str) -> Option<SelectionBox> {
        let (sel, listeners, dim, storage_path) = {
            let mut state = self.inner.write();
            if state.dimension != dimension {
                state.pos1 = None;
            }
            state.pos2 = Some(pos);
            state.dimension = dimension.to_string();
            let sel = state.pos1.map(|p1| SelectionBox::new(p1, pos));
            state.current_selection = sel;
            self.generation.fetch_add(1, Ordering::Release);
            (
                sel,
                state.listeners.clone(),
                state.dimension.clone(),
                state.active_storage_path.clone(),
            )
        };

        if let (Some(sel_box), Some(path)) = (sel, storage_path) {
            let _ = storage::save_to_path(&path, sel_box.pos1(), sel_box.pos2(), &dim);
        }

        for listener in listeners {
            listener(sel.as_ref(), &dim);
        }
        sel
    }

    /// Sets both corners simultaneously.
    pub fn set_selection(&self, pos1: [i32; 3], pos2: [i32; 3], dimension: &str) -> SelectionBox {
        let sel = SelectionBox::new(pos1, pos2);
        let (listeners, dim, storage_path) = {
            let mut state = self.inner.write();
            state.pos1 = Some(pos1);
            state.pos2 = Some(pos2);
            state.dimension = dimension.to_string();
            state.current_selection = Some(sel);
            self.generation.fetch_add(1, Ordering::Release);
            (
                state.listeners.clone(),
                state.dimension.clone(),
                state.active_storage_path.clone(),
            )
        };

        if let Some(path) = storage_path {
            let _ = storage::save_to_path(&path, pos1, pos2, &dim);
        }

        for listener in listeners {
            listener(Some(&sel), &dim);
        }
        sel
    }

    /// Explicitly clears the current selection and deletes the storage file if active.
    pub fn clear(&self) {
        let (listeners, dim, storage_path) = {
            let mut state = self.inner.write();
            state.pos1 = None;
            state.pos2 = None;
            state.current_selection = None;
            self.generation.fetch_add(1, Ordering::Release);
            (
                state.listeners.clone(),
                state.dimension.clone(),
                state.active_storage_path.clone(),
            )
        };

        if let Some(path) = storage_path {
            storage::delete_storage_file(&path);
        }

        for listener in listeners {
            listener(None, &dim);
        }
    }

    /// Resets the in-memory selection when unloading/leaving a world.
    ///
    /// DOES NOT delete the storage file on disk, preserving selection for the next session.
    pub fn reset_on_world_unload(&self) {
        let (listeners, dim) = {
            let mut state = self.inner.write();
            state.pos1 = None;
            state.pos2 = None;
            state.current_selection = None;
            state.active_storage_path = None;
            self.generation.fetch_add(1, Ordering::Release);
            (state.listeners.clone(), state.dimension.clone())
        };

        for listener in listeners {
            listener(None, &dim);
        }
        log::info!("Yefira: Selection manager reset on world unload (disk storage preserved).");
    }

    /// Loads saved selection from disk and associates `storage_path` as the active storage.
    ///
    /// If `expected_dimension` is provided and does not match the saved selection,
    /// ignores the saved selection to avoid cross-dimensional anomalies.
    pub fn load_saved_selection(
        &self,
        storage_path: &Path,
        expected_dimension: Option<&str>,
    ) -> bool {
        let data_opt = storage::load_from_path(storage_path);
        let mut state = self.inner.write();
        state.active_storage_path = Some(storage_path.to_path_buf());

        if let Some(data) = data_opt {
            if let Some(exp_dim) = expected_dimension
                && !exp_dim.is_empty()
                && data.dimension != exp_dim
            {
                log::warn!(
                    "Yefira: Ignoring saved selection from dimension {} because active dimension is {}",
                    data.dimension,
                    exp_dim
                );
                state.pos1 = None;
                state.pos2 = None;
                state.current_selection = None;
                self.generation.fetch_add(1, Ordering::Release);
                let listeners = state.listeners.clone();
                let dim = state.dimension.clone();
                drop(state);
                for listener in listeners {
                    listener(None, &dim);
                }
                return false;
            }

            let p1 = data.pos1.to_array();
            let p2 = data.pos2.to_array();
            let sel = SelectionBox::new(p1, p2);
            state.pos1 = Some(p1);
            state.pos2 = Some(p2);
            state.dimension = data.dimension.clone();
            state.current_selection = Some(sel);
            self.generation.fetch_add(1, Ordering::Release);
            let listeners = state.listeners.clone();
            let dim = state.dimension.clone();
            drop(state);

            for listener in listeners {
                listener(Some(&sel), &dim);
            }
            log::info!(
                "Yefira: Loaded saved selection from {} (dim: {}, pos1: {:?}, pos2: {:?})",
                storage_path.display(),
                dim,
                p1,
                p2
            );
            true
        } else {
            state.pos1 = None;
            state.pos2 = None;
            state.current_selection = None;
            self.generation.fetch_add(1, Ordering::Release);
            let listeners = state.listeners.clone();
            let dim = state.dimension.clone();
            drop(state);

            for listener in listeners {
                listener(None, &dim);
            }
            false
        }
    }

    /// Saves the current selection to the given path explicitly.
    pub fn save_to_path(&self, path: &Path) -> io::Result<bool> {
        let state = self.inner.read();
        let (Some(p1), Some(p2)) = (state.pos1, state.pos2) else {
            return Ok(false);
        };
        let dim = state.dimension.clone();
        drop(state);
        storage::save_to_path(path, p1, p2, &dim)?;
        Ok(true)
    }

    /// Loads saved selection from disk without setting the active storage path permanently.
    pub fn load_from_path(&self, path: &Path) -> io::Result<Option<SelectionBox>> {
        if self.load_saved_selection(path, None) {
            Ok(self.current_selection())
        } else {
            Ok(None)
        }
    }
}
