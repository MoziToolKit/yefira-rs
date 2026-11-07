//! Horizon Trait Mod Hooks implementation.

use std::sync::OnceLock;

use horizon_client::mod_api::{
    BlockChangedEvent, CommandEvent, CommandsEvent, Flow, Hooks, Host,
    LevelDimension, LevelHost, ServerAboutToStartEvent, ServerStoppingEvent,
    ServerTickEvent, ServerTickPhase, Text,
};

use crate::horizon::YefiraMod;

static YEFIRA: OnceLock<YefiraMod> = OnceLock::new();

pub fn yefira() -> &'static YefiraMod {
    YEFIRA.get_or_init(YefiraMod::default)
}

/// The entry point invoked by the launcher/game to register the mod's hooks.
pub fn hooks() -> Box<dyn Hooks> {
    Box::new(YefiraModBridge)
}

pub struct YefiraModBridge;

impl Hooks for YefiraModBridge {
    /// Server starting: initialize WebSocket live sync server.
    fn server_about_to_start(&self, _host: &mut dyn Host, _event: &mut ServerAboutToStartEvent) {
        log::info!("Yefira: Horizon server starting, initializing live sync server");
        yefira().on_server_started();
    }

    /// Server stopping: shut down live sync server and clean up selections.
    fn server_stopping(&self, _host: &mut dyn Host, _event: &mut ServerStoppingEvent) {
        log::info!("Yefira: Horizon server stopping, shutting down live sync server");
        yefira().on_server_stopping();
    }

    /// Server tick: flush pending voxel deltas to connected DCC clients (Blender / MoziToolKit).
    fn server_tick(&self, _host: &mut dyn Host, event: &mut ServerTickEvent<'_>) {
        if matches!(event.phase, ServerTickPhase::ServerTickEnd { .. }) {
            yefira().on_server_tick();
        }
    }

    /// Block changed in world: record changes within active selection.
    fn block_changed(&self, level: &dyn LevelHost, event: &mut BlockChangedEvent) {
        let dim_str = match event.dimension {
            LevelDimension::Overworld => "minecraft:overworld",
            LevelDimension::Nether => "minecraft:the_nether",
            LevelDimension::End => "minecraft:the_end",
        };

        let pos = [event.pos.x, event.pos.y, event.pos.z];
        let _ = level.block_state(pos);
        yefira().on_block_changed(dim_str, pos, "", "minecraft:stone");
    }

    /// Intercept in-game `/yefira ...` chat command.
    fn command(&self, host: &mut dyn Host, event: &mut CommandEvent<'_>) -> Flow {
        let trimmed = event.command.trim();
        if trimmed.starts_with("yefira") || trimmed.starts_with("/yefira") {
            if let Some(output) = yefira().handle_chat_command(trimmed, None, "minecraft:overworld") {
                host.broadcast(Text::literal(output.message().to_string()));
                return Flow::Cancel;
            }
        }
        Flow::Continue
    }

    /// Register command names for in-game auto-completion and syntax highlighting.
    fn commands(&self, _host: &mut dyn Host, event: &mut CommandsEvent<'_>) {
        event.commands.push("yefira");
    }
}
