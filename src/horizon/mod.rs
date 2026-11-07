//! Horizon client and server mod integration adapter.

pub mod hooks;

use std::sync::Arc;

use crate::command::{CommandOutput, execute_command};
use crate::config::YefiraConfig;
use crate::ghost::{GhostAnchorManager, GhostMode};
use crate::scanner::WorldBlockProvider;
use crate::selection::{SelectionBox, SelectionManager};
use crate::server::{DeltaQueue, WebSocketServerManager};
use crate::visual::YefiraRenderer;

/// Central controller managing Yefira state within Horizon.
pub struct YefiraMod {
    pub config: YefiraConfig,
    pub selection_manager: Arc<SelectionManager>,
    pub delta_queue: Arc<DeltaQueue>,
    pub server_manager: Arc<WebSocketServerManager>,
    pub world_provider: Arc<crate::scanner::TrackingWorldProvider>,
    pub ghost_mode: Arc<GhostMode>,
    pub ghost_anchor: Arc<GhostAnchorManager>,
    pub renderer: Arc<YefiraRenderer>,
    pub needs_world_rescan: std::sync::atomic::AtomicBool,
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
        let world_provider = Arc::new(crate::scanner::TrackingWorldProvider::new());
        server_manager.set_provider(world_provider.clone());

        let ghost_mode = Arc::new(GhostMode::new());
        let ghost_anchor = Arc::new(GhostAnchorManager::new());
        let renderer = Arc::new(YefiraRenderer::new(
            selection_manager.clone(),
            ghost_mode.clone(),
        ));

        Self {
            config,
            selection_manager,
            delta_queue,
            server_manager,
            world_provider,
            ghost_mode,
            ghost_anchor,
            renderer,
            needs_world_rescan: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Attaches an authoritative Horizon world block provider.
    pub fn attach_world_provider(&self, provider: Arc<dyn WorldBlockProvider>) {
        self.server_manager.set_provider(provider);
    }

    /// Marks the active selection as needing a full world block scan from Horizon.
    pub fn request_world_rescan(&self) {
        self.needs_world_rescan
            .store(true, std::sync::atomic::Ordering::Release);
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
            self.world_provider.set_block(pos, new_state);
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
            Some(&self.ghost_mode),
            Some(&self.ghost_anchor),
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
