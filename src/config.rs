//! Configuration for Yefira Live Sync server.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::Path;

/// Configuration options for Yefira Live Sync server.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct YefiraConfig {
    /// Host interface to bind WebSocket server. Default: "127.0.0.1".
    #[serde(default = "default_host")]
    pub host: String,

    /// Port to listen on. Default: 8765.
    #[serde(default = "default_port")]
    pub port: u16,

    /// Whether to automatically start server on world load. Default: true.
    #[serde(default = "default_auto_start")]
    pub auto_start_on_world_load: bool,

    /// Soft limit for maximum selection volume in voxels. Default: 2,097,152 (128^3).
    #[serde(default = "default_max_volume_soft_limit")]
    pub max_volume_soft_limit: u64,

    /// Soft limit for maximum side length of selection box. Default: 512.
    #[serde(default = "default_max_side_soft_limit")]
    pub max_side_soft_limit: u32,

    /// Optional authentication token for remote client connections.
    #[serde(default)]
    pub auth_token: Option<String>,

    /// Allow connections from 127.0.0.1 / localhost without token. Default: true.
    #[serde(default = "default_allow_loopback")]
    pub allow_loopback_without_token: bool,
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}

fn default_port() -> u16 {
    8765
}

fn default_auto_start() -> bool {
    true
}

fn default_max_volume_soft_limit() -> u64 {
    2_097_152
}

fn default_max_side_soft_limit() -> u32 {
    512
}

fn default_allow_loopback() -> bool {
    true
}

impl Default for YefiraConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            auto_start_on_world_load: default_auto_start(),
            max_volume_soft_limit: default_max_volume_soft_limit(),
            max_side_soft_limit: default_max_side_soft_limit(),
            auth_token: None,
            allow_loopback_without_token: default_allow_loopback(),
        }
    }
}

impl YefiraConfig {
    /// Loads configuration from a JSON file, creating default if not exists.
    pub fn load_or_default(path: &Path) -> Self {
        if !path.exists() {
            let default_cfg = Self::default();
            let _ = default_cfg.save_to_path(path);
            return default_cfg;
        }

        match fs::read_to_string(path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(e) => {
                log::warn!(
                    "Failed to read config file at {:?}: {}, using defaults",
                    path,
                    e
                );
                Self::default()
            }
        }
    }

    /// Saves configuration to a JSON file.
    pub fn save_to_path(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json_str = serde_json::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        fs::write(path, json_str)
    }

    /// Checks if a client connection is authorized.
    pub fn is_authorized(&self, client_ip: &str, token: Option<&str>) -> bool {
        let is_loopback =
            client_ip == "127.0.0.1" || client_ip == "localhost" || client_ip == "::1";
        if is_loopback && self.allow_loopback_without_token {
            return true;
        }

        match (&self.auth_token, token) {
            (None, _) => true,
            (Some(expected), Some(provided)) => expected == provided,
            (Some(_), None) => false,
        }
    }
}
