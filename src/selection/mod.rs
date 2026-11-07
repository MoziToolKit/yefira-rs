//! Selection module for 3D bounding boxes and live sync selection management.

mod box_3d;
mod manager;
pub mod style;

pub use box_3d::{Direction, SelectionBox};
pub use manager::{SelectionManager, SelectionStorageData};
pub use style::*;
