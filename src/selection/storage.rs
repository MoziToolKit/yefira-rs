//! File storage manager for Yefira selection persistence.
//!
//! Conforms 1:1 with Java Yefira's `SelectionStorageManager` and Blender's `MoziToolKit`.
//! Saves selections into `<world_dir>/yefira_selection.json` with atomic writes,
//! preventing partial corruption on crash or abrupt game exit.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Canonical file name for Yefira selection persistence.
pub const YEFIRA_SELECTION_FILENAME: &str = "yefira_selection.json";

/// 3D coordinate point serialized as `{ "x": ..., "y": ..., "z": ... }`.
///
/// Supports deserializing both object format and array format `[x, y, z]` for backward compatibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct BlockPosJson {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl BlockPosJson {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    pub const fn to_array(self) -> [i32; 3] {
        [self.x, self.y, self.z]
    }

    pub const fn from_array(arr: [i32; 3]) -> Self {
        Self {
            x: arr[0],
            y: arr[1],
            z: arr[2],
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum BlockPosHelper {
    Object { x: i32, y: i32, z: i32 },
    Array([i32; 3]),
}

impl<'de> Deserialize<'de> for BlockPosJson {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let helper = BlockPosHelper::deserialize(deserializer)?;
        match helper {
            BlockPosHelper::Object { x, y, z } => Ok(BlockPosJson { x, y, z }),
            BlockPosHelper::Array([x, y, z]) => Ok(BlockPosJson { x, y, z }),
        }
    }
}

/// JSON payload structure stored in `yefira_selection.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectionStorageData {
    pub pos1: BlockPosJson,
    pub pos2: BlockPosJson,
    #[serde(default = "default_dimension")]
    pub dimension: String,
}

fn default_dimension() -> String {
    "minecraft:overworld".to_string()
}

/// Resolves `<world_dir>/yefira_selection.json`.
pub fn get_world_storage_path(world_dir: &Path) -> PathBuf {
    world_dir.join(YEFIRA_SELECTION_FILENAME)
}

/// Resolves `<config_dir>/yefira/servers/<clean_ip>.json` for remote server fallback.
pub fn get_server_storage_path(config_dir: &Path, server_ip: &str) -> PathBuf {
    let clean_ip = sanitize_file_name(server_ip);
    let dir = config_dir.join("yefira").join("servers");
    let _ = fs::create_dir_all(&dir);
    dir.join(format!("{}.json", clean_ip))
}

/// Sanitizes a server address or name for safe filesystem storage.
fn sanitize_file_name(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return "unknown_server".to_string();
    }
    trimmed
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Saves selection data atomically to disk at `path`.
///
/// Writes to a temporary sibling file first, calls `sync_all` to flush contents to disk,
/// and renames over the destination path. If renaming fails, falls back to copying.
pub fn save_to_path(
    path: &Path,
    pos1: [i32; 3],
    pos2: [i32; 3],
    dimension: &str,
) -> io::Result<()> {
    let target = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;

    let data = SelectionStorageData {
        pos1: BlockPosJson::from_array(pos1),
        pos2: BlockPosJson::from_array(pos2),
        dimension: dimension.to_string(),
    };

    let json_bytes = serde_json::to_vec_pretty(&data)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    let pid = std::process::id();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let temp_name = format!(".yefira-selection-{}-{}.tmp", pid, now);
    let temp_path = parent.join(temp_name);

    let write_res = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&temp_path)?;
        use std::io::Write;
        file.write_all(&json_bytes)?;
        file.sync_all()?;
        Ok(())
    })();

    if let Err(e) = write_res {
        let _ = fs::remove_file(&temp_path);
        return Err(e);
    }

    if let Err(rename_err) = fs::rename(&temp_path, &target) {
        // Fallback for filesystems that reject atomic rename across boundaries
        if let Err(copy_err) = fs::copy(&temp_path, &target) {
            let _ = fs::remove_file(&temp_path);
            log::error!(
                "Yefira: Failed to move or copy temp selection file: rename error: {}, copy error: {}",
                rename_err,
                copy_err
            );
            return Err(copy_err);
        }
        let _ = fs::remove_file(&temp_path);
    }

    log::info!("Yefira: Saved selection to {}", target.display());
    Ok(())
}

/// Loads and parses selection data from `path`.
///
/// Returns `None` if the file does not exist or fails validation.
pub fn load_from_path(path: &Path) -> Option<SelectionStorageData> {
    if !path.is_file() {
        return None;
    }
    match fs::read_to_string(path) {
        Ok(content) => match serde_json::from_str::<SelectionStorageData>(&content) {
            Ok(data) => Some(data),
            Err(e) => {
                log::warn!(
                    "Yefira: Failed to parse selection from {}: {}",
                    path.display(),
                    e
                );
                None
            }
        },
        Err(e) => {
            log::warn!(
                "Yefira: Failed to read selection from {}: {}",
                path.display(),
                e
            );
            None
        }
    }
}

/// Deletes the selection storage file at `path` if it exists.
pub fn delete_storage_file(path: &Path) {
    if path.is_file() {
        if let Err(e) = fs::remove_file(path) {
            log::warn!(
                "Yefira: Failed to delete selection file {}: {}",
                path.display(),
                e
            );
        } else {
            log::info!("Yefira: Deleted selection file at {}", path.display());
        }
    }
}
