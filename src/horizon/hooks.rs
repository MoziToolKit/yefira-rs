//! Horizon Trait Mod Hooks implementation.

use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::OnceLock;

use horizon_client::mod_api::{
    BlockChangedEvent, CameraPoseEvent, CommandEvent, CommandsEvent, Flow, GuiLayerPreEvent, Hooks,
    Host, HudEvent, KeyEvent, LevelDimension, LevelHost, NativeCommand, PerformCommandEvent,
    RegisterCommandsEvent, RenderHandEvent, RenderHooks, ScreenClosingEvent,
    ScreenKeyPressedPreEvent, ScreenMouseButtonPressedPreEvent, ScreenMouseButtonReleasedPreEvent,
    ScreenMouseDraggedPreEvent, ScreenMouseScrolledPreEvent, ScreenRenderPostEvent,
    ServerAboutToStartEvent, ServerStoppingEvent, ServerTickEvent, ServerTickPhase, Text,
};

use crate::ghost::*;
use crate::horizon::YefiraMod;
use crate::selection::SelectionBox;

static YEFIRA: OnceLock<YefiraMod> = OnceLock::new();

pub fn yefira() -> &'static YefiraMod {
    YEFIRA.get_or_init(YefiraMod::default)
}

/// The entry point invoked by the launcher/game to register the mod's hooks.
pub fn hooks() -> Box<dyn Hooks> {
    Box::new(YefiraModBridge::new())
}

pub struct YefiraModBridge {
    pending_screen_close: Mutex<Option<&'static str>>,
    pending_screen_open: Mutex<Option<&'static str>>,
    pending_block_changes: Mutex<Vec<(String, [i32; 3])>>,
}

impl YefiraModBridge {
    pub fn new() -> Self {
        Self {
            pending_screen_close: Mutex::new(None),
            pending_screen_open: Mutex::new(None),
            pending_block_changes: Mutex::new(Vec::new()),
        }
    }
}

impl Default for YefiraModBridge {
    fn default() -> Self {
        Self::new()
    }
}

/// Reads all block states within the active selection box from Horizon's world,
/// and populates 3D quart biomes into the live sync provider directly from runtime memory.
fn scan_selection_blocks(
    host: &dyn Host,
    sel: &SelectionBox,
    dim: &str,
) -> HashMap<[i32; 3], String> {
    // 1. Populate 3D biomes directly from runtime memory
    crate::scanner::biome_reader::populate_selection_biomes(
        host,
        dim,
        sel,
        &yefira().world_provider,
    );

    let mut blocks = HashMap::new();
    let level = host
        .level(dim)
        .or_else(|| host.level("minecraft:overworld"));
    let Some(level) = level else {
        return blocks;
    };

    let min = sel.min();
    let max = sel.max();
    for x in min[0]..=max[0] {
        for y in min[1]..=max[1] {
            for z in min[2]..=max[2] {
                if let Some(state) = host.level_block_state(level, [x, y, z])
                    && !state.ends_with(":air")
                    && !state.contains("air")
                {
                    blocks.insert([x, y, z], state);
                }
            }
        }
    }
    blocks
}

impl Hooks for YefiraModBridge {
    /// Connects the 3D Selection Box and Gizmo Renderer.
    fn render(&self) -> Option<&dyn RenderHooks> {
        Some(yefira().renderer.as_ref())
    }

    /// Server starting: initialize WebSocket live sync server.
    fn server_about_to_start(&self, _host: &mut dyn Host, _event: &mut ServerAboutToStartEvent) {
        log::info!("Yefira: Horizon server starting, initializing live sync server");
        yefira().on_server_started();
    }

    /// Server stopping: shut down live sync server and clean up selections.
    fn server_stopping(&self, _host: &mut dyn Host, _event: &mut ServerStoppingEvent) {
        log::info!("Yefira: Horizon server stopping, shutting down live sync server");
        yefira().on_server_stopping();
        yefira().ghost_anchor.clear();
    }

    /// Server tick: flush pending voxel deltas and perform requested world baseline scans.
    fn server_tick(&self, host: &mut dyn Host, event: &mut ServerTickEvent<'_>) {
        if matches!(event.phase, ServerTickPhase::ServerTickEnd { .. }) {
            // 1. Process pending block changes (place, break, update)
            let dirty_blocks: Vec<(String, [i32; 3])> = {
                let mut lock = self.pending_block_changes.lock();
                std::mem::take(&mut *lock)
            };

            for (dim, pos) in dirty_blocks {
                let level_opt = host.level(&dim);
                let state_str = if let Some(lvl) = level_opt {
                    host.level_block_state(lvl, pos)
                        .unwrap_or_else(|| "minecraft:air".to_string())
                } else {
                    "minecraft:air".to_string()
                };

                let norm_state = if state_str.ends_with(":air") || state_str.contains("air") {
                    "minecraft:air".to_string()
                } else {
                    state_str
                };

                yefira().on_block_changed(&dim, pos, "", &norm_state);
            }

            // 2. Check if world rescan was requested (e.g. after selection was set or on refresh)
            if yefira()
                .needs_world_rescan
                .swap(false, std::sync::atomic::Ordering::AcqRel)
                && let Some(sel) = yefira().selection_manager.current_selection()
            {
                let dim = yefira().selection_manager.dimension();
                let blocks = scan_selection_blocks(host, &sel, &dim);
                yefira().world_provider.clear_blocks();
                yefira().world_provider.bulk_set_blocks(blocks);
                log::info!(
                    "Yefira: Scanned {} world blocks into live sync provider",
                    yefira().world_provider.blocks.read().len()
                );
                yefira().server_manager.refresh_all_clients();
            }

            // 3. Flush pending deltas to connected DCC clients
            yefira().on_server_tick();
        }
    }

    /// Block changed in world: record changes within active selection.
    fn block_changed(&self, _level: &dyn LevelHost, event: &mut BlockChangedEvent) {
        let dim_str = match event.dimension {
            LevelDimension::Overworld => "minecraft:overworld",
            LevelDimension::Nether => "minecraft:the_nether",
            LevelDimension::End => "minecraft:the_end",
        };

        let pos = [event.pos.x, event.pos.y, event.pos.z];
        if let Some(sel) = yefira().selection_manager.current_selection()
            && yefira().selection_manager.dimension() == dim_str
            && sel.contains(pos)
        {
            let mut lock = self.pending_block_changes.lock();
            if !lock.iter().any(|(d, p)| d == dim_str && *p == pos) {
                lock.push((dim_str.to_string(), pos));
            }
        }
    }

    /// Server-side command execution (`Commands.performCommand`).
    fn perform_command(&self, host: &mut dyn Host, event: &mut PerformCommandEvent<'_>) -> Flow {
        let trimmed = event.command.trim();
        if trimmed.starts_with("yefira") || trimmed.starts_with("/yefira") {
            let player_pos = [
                event.player.position[0].floor() as i32,
                event.player.position[1].floor() as i32,
                event.player.position[2].floor() as i32,
            ];
            if let Some(output) =
                yefira().handle_chat_command(trimmed, Some(player_pos), &event.player.dimension)
            {
                let msg = output.message();
                // Send feedback through command capture and direct tell packet
                host.feedback(Text::literal(msg.to_string()));
                host.tell(event.player.uuid, Text::literal(msg.to_string()));

                // If command modified or created selection, scan world blocks immediately
                if let Some(sel) = yefira().selection_manager.current_selection() {
                    let dim = yefira().selection_manager.dimension();
                    let blocks = scan_selection_blocks(host, &sel, &dim);
                    yefira().world_provider.clear_blocks();
                    yefira().world_provider.bulk_set_blocks(blocks);
                    yefira().server_manager.refresh_all_clients();
                }

                return Flow::Cancel;
            }
        }
        Flow::Continue
    }

    /// Client-side command intercept.
    fn command(&self, host: &mut dyn Host, event: &mut CommandEvent<'_>) -> Flow {
        let trimmed = event.command.trim();
        let cmd = trimmed.trim_start_matches('/');

        // Open Yefira GUI Settings dialog
        if cmd.eq_ignore_ascii_case("yefira gui") {
            host.mod_screen_open("yefira_gui");
            return Flow::Cancel;
        }

        // Ghost Mode toggle
        if cmd.eq_ignore_ascii_case("yefira ghost")
            || cmd.eq_ignore_ascii_case("yefira ghost on")
            || cmd.eq_ignore_ascii_case("yefira ghost toggle")
        {
            if host.mod_screen() == Some("yefira_ghost") {
                host.mod_screen_close("yefira_ghost");
                yefira().ghost_mode.set_active(false);
            } else {
                if let Some(cam) = host.camera() {
                    let eye_pos = [
                        cam.position[0],
                        cam.position[1] + cam.eye_height as f64,
                        cam.position[2],
                    ];
                    let hit = host.raycast(128.0);
                    let pivot = hit.map(|h| [h.x, h.y, h.z]);
                    yefira()
                        .ghost_mode
                        .init_from_camera(eye_pos, cam.yaw, cam.pitch, pivot);
                } else {
                    yefira().ghost_mode.set_active(true);
                }
                host.mod_screen_open("yefira_ghost");
            }
            return Flow::Cancel;
        }

        if cmd.eq_ignore_ascii_case("yefira ghost off") {
            host.mod_screen_close("yefira_ghost");
            yefira().ghost_mode.set_active(false);
            return Flow::Cancel;
        }

        // All other commands pass through to server-side perform_command
        Flow::Continue
    }

    /// Register Brigadier syntax tree for in-game Tab autocompletion and syntax coloring.
    fn register_commands(&self, _host: &mut dyn Host, event: &mut RegisterCommandsEvent<'_>) {
        let usages = [
            "yefira",
            "yefira pos1",
            "yefira pos1 <pos:block_pos>",
            "yefira pos2",
            "yefira pos2 <pos:block_pos>",
            "yefira box",
            "yefira box preset",
            "yefira box preset <size:integer>",
            "yefira box <from:block_pos> <to:block_pos>",
            "yefira expand <amount:integer>",
            "yefira expand <amount:integer> <direction:word>",
            "yefira shift <amount:integer>",
            "yefira shift <amount:integer> <direction:word>",
            "yefira clear",
            "yefira refresh",
            "yefira status",
            "yefira gui",
            "yefira ghost",
            "yefira ghost on",
            "yefira ghost off",
            "yefira ghost toggle",
            "yefira ghost status",
            "yefira server",
            "yefira server start",
            "yefira server stop",
            "yefira server restart",
            "yefira server status",
        ];

        for u in usages {
            event.commands.push(NativeCommand::new(u));
        }
    }

    /// Informs Horizon that this mod owns the "yefira" command root.
    fn commands(&self, _host: &mut dyn Host, event: &mut CommandsEvent<'_>) {
        event.commands.push("yefira");
    }

    /// Global keypress shortcuts (Hotkey 'G' for Ghost Mode, 'O' for Settings GUI).
    fn key(&self, host: &mut dyn Host, event: &mut KeyEvent<'_>) {
        if !event.pressed {
            return;
        }
        let k = event.key.to_lowercase();

        if k == "g" || k == "key.keyboard.g" {
            if host.mod_screen() == Some("yefira_ghost") {
                host.mod_screen_close("yefira_ghost");
                yefira().ghost_mode.set_active(false);
            } else if host.mod_screen().is_none() {
                if let Some(cam) = host.camera() {
                    let eye_pos = [
                        cam.position[0],
                        cam.position[1] + cam.eye_height as f64,
                        cam.position[2],
                    ];
                    let hit = host.raycast(128.0);
                    let pivot = hit.map(|h| [h.x, h.y, h.z]);
                    yefira()
                        .ghost_mode
                        .init_from_camera(eye_pos, cam.yaw, cam.pitch, pivot);
                } else {
                    yefira().ghost_mode.set_active(true);
                }
                host.mod_screen_open("yefira_ghost");
            }
        }

        if k == "o" || k == "key.keyboard.o" {
            if host.mod_screen() == Some("yefira_gui") {
                host.mod_screen_close("yefira_gui");
            } else if host.mod_screen().is_none() {
                host.mod_screen_open("yefira_gui");
            }
        }
    }

    /// Free camera pose detachment in Ghost Mode.
    fn camera_pose(&self, _host: &dyn Host, event: &mut CameraPoseEvent<'_>) {
        if yefira().ghost_mode.is_active() {
            let state = yefira().ghost_mode.state();
            if state.camera_pos[0].is_finite()
                && state.camera_pos[1].is_finite()
                && state.camera_pos[2].is_finite()
            {
                *event.position = state.camera_pos;
            }
            if state.yaw.is_finite() {
                event.yaw = state.yaw;
            }
            if state.pitch.is_finite() {
                event.pitch = state.pitch;
            }
        }
    }

    /// Cancels first-person hand and held item rendering when Ghost Mode is active.
    fn render_hand(&self, _host: &dyn Host, _event: &mut RenderHandEvent<'_>) -> Flow {
        if yefira().ghost_mode.is_active() {
            return Flow::Cancel;
        }
        Flow::Continue
    }

    /// Hides center crosshair, hotbar, health, and status HUD layers when Ghost Mode is active.
    fn gui_layer_pre(&self, _host: &mut dyn Host, event: &mut GuiLayerPreEvent<'_>) -> Flow {
        if yefira().ghost_mode.is_active() {
            match event.layer {
                "minecraft:crosshair"
                | "minecraft:hotbar"
                | "minecraft:player_health"
                | "minecraft:contextual_info_bar"
                | "minecraft:selected_item_name"
                | "minecraft:vehicle_health" => return Flow::Cancel,
                _ => {}
            }
        }
        Flow::Continue
    }

    /// Screen closing event: clean up active Ghost Mode or GUI state.
    fn screen_closing(&self, _host: &dyn Host, event: &mut ScreenClosingEvent<'_>) {
        if event.screen.id == "yefira_ghost" {
            yefira().ghost_mode.set_active(false);
            yefira().ghost_anchor.unpin_player("player");
        }
    }

    /// Renders overlays and mod screens (Ghost HUD banner and Yefira GUI Settings dialog).
    fn screen_render_post(&self, host: &dyn Host, event: &mut ScreenRenderPostEvent<'_>) {
        let mod_screen = host.mod_screen();

        if mod_screen == Some("yefira_ghost") {
            let width = event.screen.rect[2].max(320.0);
            let height = event.screen.rect[3].max(240.0);
            let mouse = event.screen.mouse;

            // 1. Top Status Bar (height 24)
            host.fill(event.batch, [0.0, 0.0, width, 24.0], 0xD0181825);
            host.fill(event.batch, [0.0, 23.0, width, 1.5], 0xFF00E5FF);

            host.text(
                event.batch,
                "👻 [Yefira 3D Viewport Mode]",
                [10.0, 7.0],
                0x00FFFF,
                true,
            );

            let hints = "MMB: Orbit | Shift+MMB: Pan | Scroll: Zoom | LMB Drag: Box / Gizmo | V: Focus | X: Clear | Esc/G: Exit";
            let hw = host.text_width(hints);
            host.text(
                event.batch,
                hints,
                [(width - hw) * 0.5, 7.0],
                0xE0E0E0,
                false,
            );

            let ws_running = yefira().server_manager.is_running();
            let ws_clients = yefira().server_manager.client_count();
            let status_text = if ws_running {
                format!(
                    "Live Sync: 8765 ({} client{})",
                    ws_clients,
                    if ws_clients == 1 { "" } else { "s" }
                )
            } else {
                "Live Sync: Stopped".to_string()
            };
            let status_color = if ws_running { 0x55FF55 } else { 0xFF7777 };
            let sw = host.text_width(&status_text);
            host.text(
                event.batch,
                &status_text,
                [width - sw - 10.0, 7.0],
                status_color,
                true,
            );

            // 2. Selection Info Box (bottom left)
            let bottom_y = height - 25.0;
            if let Some(sel) = yefira().selection_manager.current_selection() {
                let info = format!(
                    "Selection: {}x{}x{} ({} voxels)",
                    sel.size_x(),
                    sel.size_y(),
                    sel.size_z(),
                    sel.volume()
                );
                let box_w = host.text_width(&info) + 12.0;
                host.fill(event.batch, [6.0, bottom_y - 3.0, box_w, 16.0], 0x88000000);
                host.text(event.batch, &info, [8.0, bottom_y], 0x55FFFF, true);
            }

            // 3. Hover Hit-Test for Gizmos and World Blocks (aligned 1:1 with Java)
            let fov = host.camera().map(|c| c.fov_degrees).unwrap_or(70.0);
            let mut ghost_guard = yefira().ghost_mode.write_state();
            let state = &mut *ghost_guard;

            update_gizmo_hover(
                state,
                &yefira().selection_manager,
                mouse,
                [width, height],
                fov,
                |pos| host.block(pos),
            );
        } else if mod_screen == Some("yefira_gui") {
            // Render Yefira Settings & Control Panel GUI dialog
            let width = event.screen.rect[2].max(320.0);
            let height = event.screen.rect[3].max(240.0);
            let cx = width * 0.5;
            let cy = height * 0.5;

            let panel_w = 280.0;
            let panel_h = 240.0;
            let px = cx - panel_w * 0.5;
            let py = cy - panel_h * 0.5;

            // Card backdrop
            host.fill(event.batch, [px, py, panel_w, panel_h], 0xFA1E1E2E);
            host.fill(event.batch, [px, py, panel_w, 2.0], 0xFF00E5FF);
            host.fill(
                event.batch,
                [px, py + panel_h - 2.0, panel_w, 2.0],
                0xFF00E5FF,
            );
            host.fill(event.batch, [px, py, 2.0, panel_h], 0xFF00E5FF);
            host.fill(
                event.batch,
                [px + panel_w - 2.0, py, 2.0, panel_h],
                0xFF00E5FF,
            );

            // Title
            let title = "⚡ Yefira Live Sync Settings";
            let tw = host.text_width(title);
            host.text(
                event.batch,
                title,
                [cx - tw * 0.5, py + 12.0],
                0x00FFFF,
                true,
            );

            // Status
            let is_running = yefira().server_manager.is_running();
            let clients = yefira().server_manager.client_count();
            let status_str = if is_running {
                format!(
                    "● Server: Running (Port 8765, {} client{})",
                    clients,
                    if clients == 1 { "" } else { "s" }
                )
            } else {
                "○ Server: Stopped (Port 8765)".to_string()
            };
            let status_color = if is_running { 0x55FF55 } else { 0xFF6666 };
            host.text(
                event.batch,
                &status_str,
                [px + 16.0, py + 38.0],
                status_color,
                true,
            );

            // Button 1: Start / Stop Server
            let b1_rect = [px + 16.0, py + 58.0, 118.0, 24.0];
            let b1_bg = if is_running { 0xFFBA3F3F } else { 0xFF2E8B57 };
            host.fill(event.batch, b1_rect, b1_bg);
            let b1_txt = if is_running {
                "Stop Server"
            } else {
                "Start Server"
            };
            let b1_w = host.text_width(b1_txt);
            host.text(
                event.batch,
                b1_txt,
                [b1_rect[0] + (b1_rect[2] - b1_w) * 0.5, py + 65.0],
                0xFFFFFF,
                true,
            );

            // Button 2: Restart Server
            let b2_rect = [px + 144.0, py + 58.0, 120.0, 24.0];
            host.fill(event.batch, b2_rect, 0xFF45475A);
            let b2_txt = "Restart Server";
            let b2_w = host.text_width(b2_txt);
            host.text(
                event.batch,
                b2_txt,
                [b2_rect[0] + (b2_rect[2] - b2_w) * 0.5, py + 65.0],
                0xFFFFFF,
                true,
            );

            // Button 3: Toggle Ghost Mode
            let b3_rect = [px + 16.0, py + 88.0, 248.0, 24.0];
            let ghost_active = yefira().ghost_mode.is_active();
            let b3_bg = if ghost_active { 0xFF007A99 } else { 0xFF313244 };
            host.fill(event.batch, b3_rect, b3_bg);
            let b3_txt = if ghost_active {
                "Ghost Mode: Active (Click to Exit)"
            } else {
                "Enter 3D Ghost Mode (Hotkey: G)"
            };
            let b3_w = host.text_width(b3_txt);
            host.text(
                event.batch,
                b3_txt,
                [b3_rect[0] + (b3_rect[2] - b3_w) * 0.5, py + 95.0],
                0xFFFFFF,
                true,
            );

            // Button 4: Clear Selection
            let b4_rect = [px + 16.0, py + 118.0, 248.0, 24.0];
            host.fill(event.batch, b4_rect, 0xFF313244);
            let b4_txt = "Clear Selection";
            let b4_w = host.text_width(b4_txt);
            host.text(
                event.batch,
                b4_txt,
                [b4_rect[0] + (b4_rect[2] - b4_w) * 0.5, py + 125.0],
                0xFFDDAA,
                true,
            );

            // Selection Stats
            let sel_opt = yefira().selection_manager.current_selection();
            let sel_str = match sel_opt {
                Some(s) => format!(
                    "Active Selection: {}x{}x{} ({} voxels)",
                    s.size_x(),
                    s.size_y(),
                    s.size_z(),
                    s.volume()
                ),
                None => "Active Selection: None".to_string(),
            };
            host.text(
                event.batch,
                &sel_str,
                [px + 16.0, py + 154.0],
                0x55FFFF,
                false,
            );

            // Button 5: Close / Done
            let b5_rect = [px + 16.0, py + 185.0, 248.0, 24.0];
            host.fill(event.batch, b5_rect, 0xFF585B70);
            let b5_txt = "Done / Close";
            let b5_w = host.text_width(b5_txt);
            host.text(
                event.batch,
                b5_txt,
                [b5_rect[0] + (b5_rect[2] - b5_w) * 0.5, py + 192.0],
                0xFFFFFF,
                true,
            );
        }
    }

    /// Mouse button pressed pre-handler: consumes DCC and GUI clicks.
    fn screen_mouse_button_pressed_pre(
        &self,
        host: &dyn Host,
        event: &mut ScreenMouseButtonPressedPreEvent<'_>,
    ) -> Flow {
        let mod_screen = host.mod_screen();

        if mod_screen == Some("yefira_ghost") {
            let button = event.button;
            let modifiers = event.modifiers;

            // Middle Click: Navigation (Orbit, Pan, Zoom)
            if button == 2 {
                let mut state = yefira().ghost_mode.write_state();
                if (modifiers & 2) != 0 {
                    state.is_zooming = true;
                } else if (modifiers & 1) != 0 {
                    state.is_panning = true;
                } else {
                    state.is_orbiting = true;
                }
                return Flow::Cancel;
            }

            // Left Click: Gizmo Dragging or 3D Box Drag Creation
            if button == 0 {
                let width = event.screen.rect[2].max(320.0);
                let height = event.screen.rect[3].max(240.0);
                let mouse = event.screen.mouse;
                let fov = host.camera().map(|c| c.fov_degrees).unwrap_or(70.0);

                let mut state = yefira().ghost_mode.write_state();

                // Fresh hover hit test on press
                update_gizmo_hover(
                    &mut state,
                    &yefira().selection_manager,
                    mouse,
                    [width, height],
                    fov,
                    |pos| host.block(pos),
                );

                let corner = state.hovered_corner;
                let axis = state.hovered_axis;
                if corner != CORNER_NONE && axis != AXIS_NONE {
                    let (ray_origin, ray_dir) = screen_mouse_ray(
                        mouse,
                        [width, height],
                        state.camera_pos,
                        state.yaw,
                        state.pitch,
                        fov,
                    );
                    let cam_look = glam::DVec3::from(forward_vector(state.yaw, state.pitch));
                    state.start_gizmo_drag(
                        corner,
                        axis,
                        yefira().selection_manager.pos1(),
                        yefira().selection_manager.pos2(),
                        glam::DVec3::from(ray_origin),
                        glam::DVec3::from(ray_dir),
                        cam_look,
                    );
                    return Flow::Cancel;
                } else if !yefira().selection_manager.has_selection()
                    && let Some(hb) = state.hovered_block
                {
                    state.start_box_create(hb);
                    return Flow::Cancel;
                }
                return Flow::Cancel;
            }

            // Right Click: Cancel active drag
            if button == 1 {
                let mut state = yefira().ghost_mode.write_state();
                if state.is_box_creating {
                    state.cancel_box_create();
                }
                if state.dragging_corner != CORNER_NONE {
                    state.cancel_gizmo_drag();
                }
                return Flow::Cancel;
            }

            return Flow::Cancel;
        }

        if mod_screen == Some("yefira_gui") && event.button == 0 {
            let width = event.screen.rect[2].max(320.0);
            let height = event.screen.rect[3].max(240.0);
            let cx = width * 0.5;
            let cy = height * 0.5;

            let panel_w = 280.0;
            let panel_h = 240.0;
            let px = cx - panel_w * 0.5;
            let py = cy - panel_h * 0.5;

            let mouse = event.screen.mouse;

            let in_rect = |r: [f32; 4]| -> bool {
                mouse[0] >= r[0]
                    && mouse[0] <= r[0] + r[2]
                    && mouse[1] >= r[1]
                    && mouse[1] <= r[1] + r[3]
            };

            // Button 1: Start / Stop Server
            if in_rect([px + 16.0, py + 58.0, 118.0, 24.0]) {
                if yefira().server_manager.is_running() {
                    yefira().server_manager.stop();
                } else {
                    let cfg = yefira().server_manager.config();
                    let _ = yefira().server_manager.start(&cfg.host, cfg.port);
                }
                return Flow::Cancel;
            }

            // Button 2: Restart Server
            if in_rect([px + 144.0, py + 58.0, 120.0, 24.0]) {
                let cfg = yefira().server_manager.config();
                yefira().server_manager.stop();
                let _ = yefira().server_manager.start(&cfg.host, cfg.port);
                return Flow::Cancel;
            }

            // Button 3: Toggle Ghost Mode
            if in_rect([px + 16.0, py + 88.0, 248.0, 24.0]) {
                *self.pending_screen_close.lock() = Some("yefira_gui");
                *self.pending_screen_open.lock() = Some("yefira_ghost");
                return Flow::Cancel;
            }

            // Button 4: Clear Selection
            if in_rect([px + 16.0, py + 118.0, 248.0, 24.0]) {
                yefira().selection_manager.clear();
                return Flow::Cancel;
            }

            // Button 5: Close
            if in_rect([px + 16.0, py + 185.0, 248.0, 24.0]) {
                *self.pending_screen_close.lock() = Some("yefira_gui");
                return Flow::Cancel;
            }

            return Flow::Cancel;
        }

        Flow::Continue
    }

    /// Mouse dragged pre-handler: performs DCC Orbit/Pan/Zoom, Gizmo dragging, and Box creation dragging.
    fn screen_mouse_dragged_pre(
        &self,
        host: &dyn Host,
        event: &mut ScreenMouseDraggedPreEvent<'_>,
    ) -> Flow {
        let mod_screen = host.mod_screen();

        if mod_screen == Some("yefira_ghost") {
            let delta = event.delta;
            let mut state = yefira().ghost_mode.write_state();

            if state.is_orbiting {
                state.yaw += delta[0] * 0.35;
                state.pitch = (state.pitch + delta[1] * 0.35).clamp(-89.9, 89.9);

                let fwd = forward_vector(state.yaw, state.pitch);
                let dist = (((state.camera_pos[0] - state.pivot_pos[0]).powi(2)
                    + (state.camera_pos[1] - state.pivot_pos[1]).powi(2)
                    + (state.camera_pos[2] - state.pivot_pos[2]).powi(2))
                .sqrt())
                .clamp(0.5, 500.0);

                state.camera_pos = [
                    state.pivot_pos[0] - fwd[0] * dist,
                    state.pivot_pos[1] - fwd[1] * dist,
                    state.pivot_pos[2] - fwd[2] * dist,
                ];
                return Flow::Cancel;
            }

            if state.is_panning {
                let right = right_vector(state.yaw);
                let up = up_vector(state.yaw, state.pitch);

                let dist = (((state.camera_pos[0] - state.pivot_pos[0]).powi(2)
                    + (state.camera_pos[1] - state.pivot_pos[1]).powi(2)
                    + (state.camera_pos[2] - state.pivot_pos[2]).powi(2))
                .sqrt())
                .max(2.0);

                let pan_scale = 0.002 * dist;
                let dx = -delta[0] as f64 * pan_scale;
                let dy = delta[1] as f64 * pan_scale;

                let delta_vec = [
                    right[0] * dx + up[0] * dy,
                    right[1] * dx + up[1] * dy,
                    right[2] * dx + up[2] * dy,
                ];

                state.camera_pos[0] += delta_vec[0];
                state.camera_pos[1] += delta_vec[1];
                state.camera_pos[2] += delta_vec[2];
                state.pivot_pos[0] += delta_vec[0];
                state.pivot_pos[1] += delta_vec[1];
                state.pivot_pos[2] += delta_vec[2];
                return Flow::Cancel;
            }

            if state.is_zooming {
                let factor = (delta[1] as f64 * 0.008).exp();
                let fwd = forward_vector(state.yaw, state.pitch);
                let cur_dist = ((state.camera_pos[0] - state.pivot_pos[0]).powi(2)
                    + (state.camera_pos[1] - state.pivot_pos[1]).powi(2)
                    + (state.camera_pos[2] - state.pivot_pos[2]).powi(2))
                .sqrt();
                let new_dist = (cur_dist * factor).clamp(0.5, 500.0);

                state.camera_pos = [
                    state.pivot_pos[0] - fwd[0] * new_dist,
                    state.pivot_pos[1] - fwd[1] * new_dist,
                    state.pivot_pos[2] - fwd[2] * new_dist,
                ];
                return Flow::Cancel;
            }

            // Box creation drag
            if state.is_box_creating {
                let width = event.screen.rect[2].max(320.0);
                let height = event.screen.rect[3].max(240.0);
                let mouse = event.screen.mouse;
                let fov = host.camera().map(|c| c.fov_degrees).unwrap_or(70.0);
                let (ray_origin, ray_dir) = screen_mouse_ray(
                    mouse,
                    [width, height],
                    state.camera_pos,
                    state.yaw,
                    state.pitch,
                    fov,
                );
                if let Some(hit) =
                    raycast_block_from_mouse(ray_origin, ray_dir, 256.0, |pos| host.block(pos))
                {
                    state.update_box_create(hit);
                }
                return Flow::Cancel;
            }

            // Gizmo drag
            if state.dragging_corner != CORNER_NONE && state.dragging_axis != AXIS_NONE {
                let width = event.screen.rect[2].max(320.0);
                let height = event.screen.rect[3].max(240.0);
                let mouse = event.screen.mouse;
                let fov = host.camera().map(|c| c.fov_degrees).unwrap_or(70.0);
                let (ray_origin, ray_dir) = screen_mouse_ray(
                    mouse,
                    [width, height],
                    state.camera_pos,
                    state.yaw,
                    state.pitch,
                    fov,
                );
                let cam_look = glam::DVec3::from(forward_vector(state.yaw, state.pitch));
                state.handle_gizmo_drag(
                    glam::DVec3::from(ray_origin),
                    glam::DVec3::from(ray_dir),
                    cam_look,
                );
                return Flow::Cancel;
            }

            return Flow::Cancel;
        }

        Flow::Continue
    }

    /// Mouse button released pre-handler: commits Gizmo drag or Box creation.
    fn screen_mouse_button_released_pre(
        &self,
        host: &dyn Host,
        event: &mut ScreenMouseButtonReleasedPreEvent<'_>,
    ) -> Flow {
        let mod_screen = host.mod_screen();

        if mod_screen == Some("yefira_ghost") {
            let button = event.button;

            if button == 2 {
                let mut state = yefira().ghost_mode.write_state();
                state.is_orbiting = false;
                state.is_panning = false;
                state.is_zooming = false;
                return Flow::Cancel;
            }

            if button == 0 {
                let mut state = yefira().ghost_mode.write_state();
                let dim = host
                    .camera()
                    .map(|c| c.dimension)
                    .unwrap_or_else(|| "minecraft:overworld".to_string());

                // Finish box creation
                if state.is_box_creating {
                    if let Some((s, c)) = state.finish_box_create() {
                        yefira().selection_manager.set_selection(s, c, &dim);
                        yefira().request_world_rescan();
                    }
                    return Flow::Cancel;
                }

                // Finish gizmo drag
                if state.dragging_corner != CORNER_NONE {
                    if let Some((p1, p2)) = state.finish_gizmo_drag() {
                        yefira().selection_manager.set_selection(p1, p2, &dim);
                        yefira().request_world_rescan();
                    }
                    return Flow::Cancel;
                }
                return Flow::Cancel;
            }
            return Flow::Cancel;
        }

        Flow::Continue
    }

    /// Mouse scrolled pre-handler: zooms camera in Ghost Mode.
    fn screen_mouse_scrolled_pre(
        &self,
        host: &dyn Host,
        event: &mut ScreenMouseScrolledPreEvent<'_>,
    ) -> Flow {
        if host.mod_screen() == Some("yefira_ghost") {
            let scroll_y = event.scroll[1];
            let zoom_factor = if scroll_y > 0.0 { 0.85 } else { 1.15 };

            let mut state = yefira().ghost_mode.write_state();
            let fwd = forward_vector(state.yaw, state.pitch);
            let cur_dist = ((state.camera_pos[0] - state.pivot_pos[0]).powi(2)
                + (state.camera_pos[1] - state.pivot_pos[1]).powi(2)
                + (state.camera_pos[2] - state.pivot_pos[2]).powi(2))
            .sqrt();
            let new_dist = (cur_dist * zoom_factor).clamp(0.5, 500.0);

            state.camera_pos = [
                state.pivot_pos[0] - fwd[0] * new_dist,
                state.pivot_pos[1] - fwd[1] * new_dist,
                state.pivot_pos[2] - fwd[2] * new_dist,
            ];
            return Flow::Cancel;
        }

        Flow::Continue
    }

    /// Key pressed pre-handler for active mod screens.
    fn screen_key_pressed_pre(
        &self,
        host: &dyn Host,
        event: &mut ScreenKeyPressedPreEvent<'_>,
    ) -> Flow {
        let mod_screen = host.mod_screen();
        let k = event.key.to_lowercase();

        if mod_screen == Some("yefira_ghost") {
            if k == "escape" || k == "key.keyboard.escape" {
                let mut state = yefira().ghost_mode.write_state();
                if state.is_box_creating {
                    state.is_box_creating = false;
                    state.box_create_start = None;
                    state.box_create_current = None;
                    return Flow::Cancel;
                }
                *self.pending_screen_close.lock() = Some("yefira_ghost");
                return Flow::Cancel;
            }

            if k == "g" || k == "key.keyboard.g" {
                *self.pending_screen_close.lock() = Some("yefira_ghost");
                return Flow::Cancel;
            }

            if k == "v" || k == "key.keyboard.v" {
                if let Some(sel) = yefira().selection_manager.current_selection() {
                    let c = sel.center();
                    let max_side = sel.size_x().max(sel.size_y()).max(sel.size_z()) as f64;
                    yefira()
                        .ghost_mode
                        .focus_selection([c[0], c[1], c[2]], max_side);
                }
                return Flow::Cancel;
            }

            if k == "x" || k == "key.keyboard.x" {
                yefira().selection_manager.clear();
                return Flow::Cancel;
            }

            if k == "c" || k == "key.keyboard.c" || k == "b" || k == "key.keyboard.b" {
                let pivot = yefira().ghost_mode.state().pivot_pos;
                let center = [
                    pivot[0].floor() as i32,
                    pivot[1].floor() as i32,
                    pivot[2].floor() as i32,
                ];
                if let Ok(box_3d) = SelectionBox::preset_above(center, 16) {
                    yefira().selection_manager.set_selection(
                        box_3d.pos1(),
                        box_3d.pos2(),
                        "minecraft:overworld",
                    );
                    yefira().request_world_rescan();
                }
                return Flow::Cancel;
            }

            if k == "o" || k == "key.keyboard.o" {
                *self.pending_screen_close.lock() = Some("yefira_ghost");
                *self.pending_screen_open.lock() = Some("yefira_gui");
                return Flow::Cancel;
            }

            return Flow::Cancel;
        }

        if mod_screen == Some("yefira_gui") {
            if k == "escape" || k == "key.keyboard.escape" || k == "o" || k == "key.keyboard.o" {
                *self.pending_screen_close.lock() = Some("yefira_gui");
                return Flow::Cancel;
            }
            return Flow::Cancel;
        }

        Flow::Continue
    }

    /// HUD callback: handles deferred screen transitions.
    fn hud(&self, host: &mut dyn Host, _event: &mut HudEvent<'_>) {
        if let Some(close_id) = self.pending_screen_close.lock().take() {
            host.mod_screen_close(close_id);
            if close_id == "yefira_ghost" {
                yefira().ghost_mode.set_active(false);
            }
        }
        if let Some(open_id) = self.pending_screen_open.lock().take() {
            if open_id == "yefira_ghost" {
                if let Some(cam) = host.camera() {
                    let eye_pos = [
                        cam.position[0],
                        cam.position[1] + cam.eye_height as f64,
                        cam.position[2],
                    ];
                    let hit = host.raycast(128.0);
                    let pivot = hit.map(|h| [h.x, h.y, h.z]);
                    yefira()
                        .ghost_mode
                        .init_from_camera(eye_pos, cam.yaw, cam.pitch, pivot);
                } else {
                    yefira().ghost_mode.set_active(true);
                }
            }
            host.mod_screen_open(open_id);
        }
    }
}
