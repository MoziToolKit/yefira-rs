//! 3D selection box representation and operations for live sync.

use glam::IVec3;
use serde::{Deserialize, Serialize};

/// Cardinal 3D direction for selection resizing/shifting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    Down,
    Up,
    North,
    South,
    West,
    East,
}

impl Direction {
    pub fn offset(self, amount: i32) -> [i32; 3] {
        match self {
            Direction::Down => [0, -amount, 0],
            Direction::Up => [0, amount, 0],
            Direction::North => [0, 0, -amount],
            Direction::South => [0, 0, amount],
            Direction::West => [-amount, 0, 0],
            Direction::East => [amount, 0, 0],
        }
    }

    pub fn from_str_name(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "down" | "d" | "y-" => Some(Direction::Down),
            "up" | "u" | "y+" => Some(Direction::Up),
            "north" | "n" | "z-" => Some(Direction::North),
            "south" | "s" | "z+" => Some(Direction::South),
            "west" | "w" | "x-" => Some(Direction::West),
            "east" | "e" | "x+" => Some(Direction::East),
            _ => None,
        }
    }
}

/// Represents an authoritative 3D bounding volume defined by two corners.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectionBox {
    pos1: [i32; 3],
    pos2: [i32; 3],
    min: [i32; 3],
    max: [i32; 3],
}

impl SelectionBox {
    /// Creates a new `SelectionBox` from two corner positions.
    pub fn new(pos1: [i32; 3], pos2: [i32; 3]) -> Self {
        let min = [
            pos1[0].min(pos2[0]),
            pos1[1].min(pos2[1]),
            pos1[2].min(pos2[2]),
        ];
        let max = [
            pos1[0].max(pos2[0]),
            pos1[1].max(pos2[1]),
            pos1[2].max(pos2[2]),
        ];
        Self {
            pos1,
            pos2,
            min,
            max,
        }
    }

    /// Builds a cube-shaped preset anchored at `center` and extending upward.
    /// Odd sizes stay centered on the cursor; size 1 yields a single block.
    pub fn preset_above(center: [i32; 3], size: u32) -> Result<Self, &'static str> {
        if size == 0 {
            return Err("size must be >= 1");
        }
        let half = ((size - 1) / 2) as i32;
        let p1 = [center[0] - half, center[1], center[2] - half];
        let s = (size - 1) as i32;
        let p2 = [p1[0] + s, p1[1] + s, p1[2] + s];
        Ok(Self::new(p1, p2))
    }

    #[inline]
    pub fn pos1(&self) -> [i32; 3] {
        self.pos1
    }

    #[inline]
    pub fn pos2(&self) -> [i32; 3] {
        self.pos2
    }

    #[inline]
    pub fn min(&self) -> [i32; 3] {
        self.min
    }

    #[inline]
    pub fn max(&self) -> [i32; 3] {
        self.max
    }

    #[inline]
    pub fn min_ivec3(&self) -> IVec3 {
        IVec3::new(self.min[0], self.min[1], self.min[2])
    }

    #[inline]
    pub fn size_x(&self) -> u32 {
        (self.max[0] - self.min[0] + 1) as u32
    }

    #[inline]
    pub fn size_y(&self) -> u32 {
        (self.max[1] - self.min[1] + 1) as u32
    }

    #[inline]
    pub fn size_z(&self) -> u32 {
        (self.max[2] - self.min[2] + 1) as u32
    }

    #[inline]
    pub fn size_ivec3(&self) -> IVec3 {
        IVec3::new(
            self.size_x() as i32,
            self.size_y() as i32,
            self.size_z() as i32,
        )
    }

    #[inline]
    pub fn volume(&self) -> u64 {
        (self.size_x() as u64) * (self.size_y() as u64) * (self.size_z() as u64)
    }

    #[inline]
    pub fn contains(&self, pos: [i32; 3]) -> bool {
        pos[0] >= self.min[0]
            && pos[0] <= self.max[0]
            && pos[1] >= self.min[1]
            && pos[1] <= self.max[1]
            && pos[2] >= self.min[2]
            && pos[2] <= self.max[2]
    }

    #[inline]
    pub fn contains_ivec3(&self, pos: IVec3) -> bool {
        self.contains([pos.x, pos.y, pos.z])
    }

    pub fn center(&self) -> [f64; 3] {
        [
            (self.min[0] as f64 + self.max[0] as f64 + 1.0) / 2.0,
            (self.min[1] as f64 + self.max[1] as f64 + 1.0) / 2.0,
            (self.min[2] as f64 + self.max[2] as f64 + 1.0) / 2.0,
        ]
    }

    /// Checks if the selection box exceeds soft bounds limits.
    pub fn is_oversized(&self, max_volume: u64, max_side: u32) -> bool {
        if self.volume() > max_volume {
            return true;
        }
        self.size_x() > max_side || self.size_y() > max_side || self.size_z() > max_side
    }

    /// Checks if the selection box exceeds standard default limits (1M voxels, 256 side).
    #[inline]
    pub fn is_oversized_default(&self) -> bool {
        self.is_oversized(1_000_000, 256)
    }

    /// Expands or contracts the box in a given direction.
    pub fn expand(&self, dir: Direction, amount: i32) -> Self {
        let mut min = self.min;
        let mut max = self.max;

        match dir {
            Direction::Down => min[1] -= amount,
            Direction::Up => max[1] += amount,
            Direction::North => min[2] -= amount,
            Direction::South => max[2] += amount,
            Direction::West => min[0] -= amount,
            Direction::East => max[0] += amount,
        }

        Self::new(min, max)
    }

    /// Shifts the entire box in a given direction without changing size.
    pub fn shift(&self, dir: Direction, amount: i32) -> Self {
        let delta = dir.offset(amount);
        let p1 = [
            self.pos1[0] + delta[0],
            self.pos1[1] + delta[1],
            self.pos1[2] + delta[2],
        ];
        let p2 = [
            self.pos2[0] + delta[0],
            self.pos2[1] + delta[1],
            self.pos2[2] + delta[2],
        ];
        Self::new(p1, p2)
    }
}
