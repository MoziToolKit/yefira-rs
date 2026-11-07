//! Horizon client and server mod integration adapter.

use std::sync::Arc;

use crate::command::{execute_command, CommandOutput};
use crate::config::YefiraConfig;
use crate::scanner::WorldBlockProvider;
use crate::selection::{SelectionBox, SelectionManager};
use crate::server::{DeltaQueue, WebSocketServerManager};

/// Central controller managing Yefira state within Horizon.
pub struct YefiraMod {
    pub config: YefiraConfig,
    pub selection_manager: Arc<SelectionManager>,
    pub delta_queue: Arc<DeltaQueue>,
    pub server_manager: Arc<WebSocketServerManager>,
}

impl Default for YefiraMod {
    fn default() -> Self {
        Self::new(YefiraConfig::default())
    }
}

impl YefiraMod {
    /// Creates a new Yefira instance configured for Horizon.
    pub fn new(config: YefiraConfig) -> Self {
        let selection_manager = Arc::new(SelectionManager::new());
        let delta_queue = Arc::new(DeltaQueue::new());
        let server_manager = Arc::new(WebSocketServerManager::new(
            config.clone(),
            selection_manager.clone(),
            delta_queue.clone(),
        ));

        Self {
            config,
            selection_manager,
            delta_queue,
            server_manager,
        }
    }

    /// Attaches an authoritative Horizon world block provider.
    pub fn attach_world_provider(&self, provider: Arc<dyn WorldBlockProvider>) {
        self.server_manager.set_provider(provider);
    }

    /// Lifecycle hook: Server started / world loaded.
    pub fn on_server_started(&self) {
        log::info!("Yefira: Horizon server started, initializing Live Sync");
        if self.config.auto_start_on_world_load {
            let host = self.config.host.clone();
            let port = self.config.port;
            if let Err(e) = self.server_manager.start(&host, port) {
                log::error!(
                    "Yefira: Failed to auto-start WebSocket server on {}:{}: {}",
                    host,
                    port,
                    e
                );
            }
        }
    }

    /// Lifecycle hook: Server stopping / world unloading.
    pub fn on_server_stopping(&self) {
        log::info!("Yefira: Horizon server stopping, shutting down Live Sync");
        self.server_manager.stop();
        self.selection_manager.clear();
        self.delta_queue.clear();
    }

    /// Lifecycle hook: Server tick (pre/post phase). Flushes queued block deltas.
    pub fn on_server_tick(&self) {
        self.server_manager.flush_queued_deltas();
    }

    /// Hook: Authoritative block changed event in world.
    /// `pos`: world coordinates `[x, y, z]`
    /// `new_state`: string representation of the block state (e.g. "minecraft:stone")
    pub fn on_block_changed(
        &self,
        dimension: &str,
        pos: [i32; 3],
        _old_state: &str,
        new_state: &str,
    ) {
        if self.selection_manager.dimension() == dimension {
            self.server_manager.queue_delta_update(pos, new_state);
        }
    }

    /// Hook: Command dispatch (`/yefira ...`).
    pub fn on_command(
        &self,
        args: &[&str],
        player_pos: Option<[i32; 3]>,
        dimension: &str,
    ) -> CommandOutput {
        execute_command(
            args,
            player_pos,
            dimension,
            &self.selection_manager,
            &self.server_manager,
        )
    }

    /// Handles a raw in-game command string (e.g. from Horizon's `native_command`).
    /// Returns `Some(CommandOutput)` if the command is handled by Yefira, or `None` otherwise.
    pub fn handle_chat_command(
        &self,
        raw_cmd: &str,
        player_pos: Option<[i32; 3]>,
        dimension: &str,
    ) -> Option<CommandOutput> {
        let trimmed = raw_cmd.trim().trim_start_matches('/');
        let mut tokens = trimmed.split_whitespace();
        let cmd = tokens.next()?;
        if cmd.eq_ignore_ascii_case("yefira") {
            let args: Vec<&str> = tokens.collect();
            Some(self.on_command(&args, player_pos, dimension))
        } else {
            None
        }
    }

    /// Returns the currently active selection box, if any.
    pub fn current_selection(&self) -> Option<SelectionBox> {
        self.selection_manager.current_selection()
    }
}
