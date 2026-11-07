//! Selection module for 3D bounding boxes and live sync selection management.

mod box_3d;
mod manager;
pub mod storage;
pub mod style;

pub use box_3d::{Direction, SelectionBox};
pub use manager::SelectionManager;
pub use storage::{
    BlockPosJson, SelectionStorageData, YEFIRA_SELECTION_FILENAME, delete_storage_file,
    get_server_storage_path, get_world_storage_path, load_from_path, save_to_path,
};
pub use style::*;
