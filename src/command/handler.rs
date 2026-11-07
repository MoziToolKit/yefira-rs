//! Command handling for `/yefira` subcommands.

use std::sync::Arc;

use crate::selection::{Direction, SelectionBox, SelectionManager};
use crate::server::WebSocketServerManager;

/// Output result of executing a `/yefira` command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandOutput {
    Success(String),
    Error(String),
}

impl CommandOutput {
    #[inline]
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success(_))
    }

    #[inline]
    pub fn message(&self) -> &str {
        match self {
            Self::Success(msg) | Self::Error(msg) => msg,
        }
    }
}

/// Executes a `/yefira` command string.
pub fn execute_command(
    args: &[&str],
    current_pos: Option<[i32; 3]>,
    dimension: &str,
    selection_mgr: &Arc<SelectionManager>,
    server_mgr: &Arc<WebSocketServerManager>,
    ghost_mode: Option<&crate::ghost::GhostMode>,
    ghost_anchor: Option<&crate::ghost::GhostAnchorManager>,
) -> CommandOutput {
    if args.is_empty() {
        return CommandOutput::Success(
            "Yefira Live Sync (Rust for Horizon) v0.1.0\nUse '/yefira status' or '/yefira help'"
                .to_string(),
        );
    }

    match args[0].to_ascii_lowercase().as_str() {
        "pos1" => {
            let target_pos = if args.len() >= 4 {
                let Ok(x) = args[1].parse::<i32>() else {
                    return CommandOutput::Error("Invalid x".into());
                };
                let Ok(y) = args[2].parse::<i32>() else {
                    return CommandOutput::Error("Invalid y".into());
                };
                let Ok(z) = args[3].parse::<i32>() else {
                    return CommandOutput::Error("Invalid z".into());
                };
                [x, y, z]
            } else if let Some(p) = current_pos {
                p
            } else {
                return CommandOutput::Error("Specify x y z or run as player".into());
            };

            let sel = selection_mgr.set_pos1(target_pos, dimension);
            match sel {
                Some(box_3d) => CommandOutput::Success(format!(
                    "Set pos1 to [{}, {}, {}]. Box: {}x{}x{} ({} voxels)",
                    target_pos[0],
                    target_pos[1],
                    target_pos[2],
                    box_3d.size_x(),
                    box_3d.size_y(),
                    box_3d.size_z(),
                    box_3d.volume()
                )),
                None => CommandOutput::Success(format!(
                    "Set pos1 to [{}, {}, {}]. (pos2 not set yet)",
                    target_pos[0], target_pos[1], target_pos[2]
                )),
            }
        }

        "pos2" => {
            let target_pos = if args.len() >= 4 {
                let Ok(x) = args[1].parse::<i32>() else {
                    return CommandOutput::Error("Invalid x".into());
                };
                let Ok(y) = args[2].parse::<i32>() else {
                    return CommandOutput::Error("Invalid y".into());
                };
                let Ok(z) = args[3].parse::<i32>() else {
                    return CommandOutput::Error("Invalid z".into());
                };
                [x, y, z]
            } else if let Some(p) = current_pos {
                p
            } else {
                return CommandOutput::Error("Specify x y z or run as player".into());
            };

            let sel = selection_mgr.set_pos2(target_pos, dimension);
            match sel {
                Some(box_3d) => CommandOutput::Success(format!(
                    "Set pos2 to [{}, {}, {}]. Box: {}x{}x{} ({} voxels)",
                    target_pos[0],
                    target_pos[1],
                    target_pos[2],
                    box_3d.size_x(),
                    box_3d.size_y(),
                    box_3d.size_z(),
                    box_3d.volume()
                )),
                None => CommandOutput::Success(format!(
                    "Set pos2 to [{}, {}, {}]. (pos1 not set yet)",
                    target_pos[0], target_pos[1], target_pos[2]
                )),
            }
        }

        "box" => {
            if args.len() >= 2 && args[1].eq_ignore_ascii_case("preset") {
                let size = if args.len() >= 3 {
                    args[2].parse::<u32>().unwrap_or(16).clamp(1, 256)
                } else {
                    16
                };
                let center = current_pos.unwrap_or([0, 64, 0]);
                match SelectionBox::preset_above(center, size) {
                    Ok(box_3d) => {
                        selection_mgr.set_selection(box_3d.pos1(), box_3d.pos2(), dimension);
                        CommandOutput::Success(format!(
                            "Created {}x{}x{} preset box at [{}, {}, {}]",
                            box_3d.size_x(),
                            box_3d.size_y(),
                            box_3d.size_z(),
                            center[0],
                            center[1],
                            center[2]
                        ))
                    }
                    Err(e) => CommandOutput::Error(format!("Failed to create preset: {}", e)),
                }
            } else if args.len() >= 7 {
                let Ok(x1) = args[1].parse::<i32>() else {
                    return CommandOutput::Error("Invalid x1".into());
                };
                let Ok(y1) = args[2].parse::<i32>() else {
                    return CommandOutput::Error("Invalid y1".into());
                };
                let Ok(z1) = args[3].parse::<i32>() else {
                    return CommandOutput::Error("Invalid z1".into());
                };
                let Ok(x2) = args[4].parse::<i32>() else {
                    return CommandOutput::Error("Invalid x2".into());
                };
                let Ok(y2) = args[5].parse::<i32>() else {
                    return CommandOutput::Error("Invalid y2".into());
                };
                let Ok(z2) = args[6].parse::<i32>() else {
                    return CommandOutput::Error("Invalid z2".into());
                };

                let box_3d = selection_mgr.set_selection([x1, y1, z1], [x2, y2, z2], dimension);
                CommandOutput::Success(format!(
                    "Set box from [{}, {}, {}] to [{}, {}, {}] ({}x{}x{}, {} voxels)",
                    x1,
                    y1,
                    z1,
                    x2,
                    y2,
                    z2,
                    box_3d.size_x(),
                    box_3d.size_y(),
                    box_3d.size_z(),
                    box_3d.volume()
                ))
            } else {
                CommandOutput::Error(
                    "Usage: /yefira box preset [size] OR /yefira box <x1 y1 z1> <x2 y2 z2>".into(),
                )
            }
        }

        "expand" => {
            if args.len() < 3 {
                return CommandOutput::Error("Usage: /yefira expand <direction> <amount>".into());
            }
            let Some(dir) = Direction::from_str_name(args[1]) else {
                return CommandOutput::Error(format!("Unknown direction '{}'", args[1]));
            };
            let Ok(amount) = args[2].parse::<i32>() else {
                return CommandOutput::Error("Invalid amount".into());
            };
            let Some(sel) = selection_mgr.current_selection() else {
                return CommandOutput::Error("No active selection to expand".into());
            };
            let expanded = sel.expand(dir, amount);
            selection_mgr.set_selection(expanded.pos1(), expanded.pos2(), dimension);
            CommandOutput::Success(format!(
                "Expanded {:?} by {}: new size {}x{}x{} ({} voxels)",
                dir,
                amount,
                expanded.size_x(),
                expanded.size_y(),
                expanded.size_z(),
                expanded.volume()
            ))
        }

        "shift" => {
            if args.len() < 3 {
                return CommandOutput::Error("Usage: /yefira shift <direction> <amount>".into());
            }
            let Some(dir) = Direction::from_str_name(args[1]) else {
                return CommandOutput::Error(format!("Unknown direction '{}'", args[1]));
            };
            let Ok(amount) = args[2].parse::<i32>() else {
                return CommandOutput::Error("Invalid amount".into());
            };
            let Some(sel) = selection_mgr.current_selection() else {
                return CommandOutput::Error("No active selection to shift".into());
            };
            let shifted = sel.shift(dir, amount);
            selection_mgr.set_selection(shifted.pos1(), shifted.pos2(), dimension);
            CommandOutput::Success(format!(
                "Shifted {:?} by {}: new min [{}, {}, {}]",
                dir,
                amount,
                shifted.min()[0],
                shifted.min()[1],
                shifted.min()[2]
            ))
        }

        "clear" => {
            selection_mgr.clear();
            CommandOutput::Success("Selection cleared".to_string())
        }

        "refresh" => {
            server_mgr.refresh_all_clients();
            CommandOutput::Success(
                "Broadcasting fresh baseline snapshot to all DCC clients".to_string(),
            )
        }

        "status" => {
            let sel = selection_mgr.current_selection();
            let is_running = server_mgr.is_running();
            let clients = server_mgr.client_count();
            let cfg = server_mgr.config();

            let sel_str = match sel {
                Some(b) => format!(
                    "Active: [{}, {}, {}] -> [{}, {}, {}] ({}x{}x{}, {} voxels)",
                    b.min()[0],
                    b.min()[1],
                    b.min()[2],
                    b.max()[0],
                    b.max()[1],
                    b.max()[2],
                    b.size_x(),
                    b.size_y(),
                    b.size_z(),
                    b.volume()
                ),
                None => "None (unset)".to_string(),
            };

            CommandOutput::Success(format!(
                "=== Yefira Live Sync Status ===\nServer: {} (ws://{}:{})\nClients Connected: {}\nSelection ({}): {}",
                if is_running { "RUNNING" } else { "STOPPED" },
                cfg.host,
                cfg.port,
                clients,
                dimension,
                sel_str
            ))
        }

        "server" => {
            if args.len() < 2 {
                return CommandOutput::Error(
                    "Usage: /yefira server <start|stop|restart|status>".into(),
                );
            }
            match args[1].to_ascii_lowercase().as_str() {
                "start" => {
                    let cfg = server_mgr.config();
                    match server_mgr.start(&cfg.host, cfg.port) {
                        Ok(p) => CommandOutput::Success(format!(
                            "WebSocket server started on ws://{}:{}",
                            cfg.host, p
                        )),
                        Err(e) => CommandOutput::Error(format!("Failed to start server: {}", e)),
                    }
                }
                "stop" => {
                    server_mgr.stop();
                    CommandOutput::Success("WebSocket server stopped".to_string())
                }
                "restart" => {
                    server_mgr.stop();
                    let cfg = server_mgr.config();
                    match server_mgr.start(&cfg.host, cfg.port) {
                        Ok(p) => CommandOutput::Success(format!(
                            "WebSocket server restarted on ws://{}:{}",
                            cfg.host, p
                        )),
                        Err(e) => CommandOutput::Error(format!("Failed to restart server: {}", e)),
                    }
                }
                "status" => {
                    let is_running = server_mgr.is_running();
                    let clients = server_mgr.client_count();
                    let cfg = server_mgr.config();
                    CommandOutput::Success(format!(
                        "Server status: {} on ws://{}:{}, {} client(s) connected",
                        if is_running { "RUNNING" } else { "STOPPED" },
                        cfg.host,
                        cfg.port,
                        clients
                    ))
                }
                _ => {
                    CommandOutput::Error("Usage: /yefira server <start|stop|restart|status>".into())
                }
            }
        }

        "ghost" => {
            if args.len() < 2 {
                return CommandOutput::Success(
                    "Ghost Mode: use '/yefira ghost <on|off|toggle|status>'".into(),
                );
            }
            let Some(ghost) = ghost_mode else {
                return CommandOutput::Error("Ghost mode not available in this context".into());
            };
            match args[1].to_ascii_lowercase().as_str() {
                "on" | "enable" => {
                    if let Some(pos) = current_pos {
                        let eye = [pos[0] as f64, pos[1] as f64 + 1.62, pos[2] as f64];
                        ghost.init_from_camera(eye, 0.0, 0.0, None);
                        if let Some(anchor) = ghost_anchor {
                            anchor.pin_player(
                                "player",
                                dimension,
                                [pos[0] as f64, pos[1] as f64, pos[2] as f64],
                            );
                        }
                    } else {
                        ghost.set_active(true);
                    }
                    CommandOutput::Success(
                        "§b[Yefira] Ghost Mode ENABLED. Player pinned at anchor.".into(),
                    )
                }
                "off" | "disable" => {
                    ghost.set_active(false);
                    if let Some(anchor) = ghost_anchor {
                        anchor.unpin_player("player");
                    }
                    CommandOutput::Success(
                        "§7[Yefira] Ghost Mode DISABLED. Player restored.".into(),
                    )
                }
                "toggle" => {
                    let active = ghost.toggle();
                    if active {
                        if let (Some(pos), Some(anchor)) = (current_pos, ghost_anchor) {
                            anchor.pin_player(
                                "player",
                                dimension,
                                [pos[0] as f64, pos[1] as f64, pos[2] as f64],
                            );
                        }
                        CommandOutput::Success("§b[Yefira] Ghost Mode ENABLED.".into())
                    } else {
                        if let Some(anchor) = ghost_anchor {
                            anchor.unpin_player("player");
                        }
                        CommandOutput::Success("§7[Yefira] Ghost Mode DISABLED.".into())
                    }
                }
                "status" => {
                    let active = ghost.is_active();
                    CommandOutput::Success(format!(
                        "Ghost Mode: {}",
                        if active {
                            "ENABLED (Camera Free)"
                        } else {
                            "DISABLED"
                        }
                    ))
                }
                _ => CommandOutput::Error("Usage: /yefira ghost <on|off|toggle|status>".into()),
            }
        }

        _ => CommandOutput::Error(format!(
            "Unknown subcommand '{}'. Use pos1, pos2, box, clear, refresh, status, server, ghost",
            args[0]
        )),
    }
}
