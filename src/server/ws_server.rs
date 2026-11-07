//! Multi-client WebSocket Live Sync server conforming to RFC v3.1.

use base64::Engine;
use parking_lot::{Mutex, RwLock};
use sha1::{Digest, Sha1};
use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, sync_channel};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tungstenite::Message;
use tungstenite::protocol::{Role, WebSocket};

use super::delta_queue::DeltaQueue;
use super::session::ClientSession;
use super::streamer::BaselineStreamer;
use crate::config::YefiraConfig;
use crate::protocol::*;
use crate::scanner::{CanonicalVoxelScanner, WorldBlockProvider};
use crate::selection::SelectionManager;

fn generate_random_uuid() -> [u8; 16] {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let c = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut hasher = Sha1::new();
    hasher.update(now.as_nanos().to_le_bytes());
    hasher.update(c.to_le_bytes());
    let digest = hasher.finalize();
    let mut uuid = [0u8; 16];
    uuid.copy_from_slice(&digest[..16]);
    uuid
}

struct SharedServerState {
    server_uuid: RwLock<[u8; 16]>,
    epoch: AtomicU64,
    global_seq_id: AtomicU64,
    cached_state_hash: AtomicU64,

    palette_lock: Mutex<PaletteState>,
    clients: Mutex<HashMap<usize, Arc<ClientSession>>>,
    next_client_id: AtomicUsize,
}

struct PaletteState {
    palette_lookup: HashMap<String, u16>,
    palette_list: Vec<String>,
    palette_revision: u32,
}

/// Multi-client WebSocket Live Sync Server.
pub struct WebSocketServerManager {
    config: RwLock<YefiraConfig>,
    selection_manager: Arc<SelectionManager>,
    delta_queue: Arc<DeltaQueue>,
    provider: RwLock<Option<Arc<dyn WorldBlockProvider>>>,

    running: Arc<AtomicBool>,
    shared: Arc<SharedServerState>,
    listener_thread: Mutex<Option<JoinHandle<()>>>,
}

impl WebSocketServerManager {
    pub fn new(
        config: YefiraConfig,
        selection_manager: Arc<SelectionManager>,
        delta_queue: Arc<DeltaQueue>,
    ) -> Self {
        let uuid = generate_random_uuid();

        let shared = Arc::new(SharedServerState {
            server_uuid: RwLock::new(uuid),
            epoch: AtomicU64::new(1),
            global_seq_id: AtomicU64::new(1),
            cached_state_hash: AtomicU64::new(0),
            palette_lock: Mutex::new(PaletteState {
                palette_lookup: HashMap::new(),
                palette_list: Vec::new(),
                palette_revision: 1,
            }),
            clients: Mutex::new(HashMap::new()),
            next_client_id: AtomicUsize::new(1),
        });

        Self {
            config: RwLock::new(config),
            selection_manager,
            delta_queue,
            provider: RwLock::new(None),
            running: Arc::new(AtomicBool::new(false)),
            shared,
            listener_thread: Mutex::new(None),
        }
    }

    /// Sets the world block provider for baseline scanning.
    pub fn set_provider(&self, provider: Arc<dyn WorldBlockProvider>) {
        *self.provider.write() = Some(provider);
    }

    /// Updates the server configuration.
    pub fn update_config(&self, config: YefiraConfig) {
        *self.config.write() = config;
    }

    pub fn config(&self) -> YefiraConfig {
        self.config.read().clone()
    }

    #[inline]
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }

    /// Returns the number of connected clients.
    pub fn client_count(&self) -> usize {
        self.shared.clients.lock().len()
    }

    /// Starts the WebSocket server on the configured or specified host/port.
    /// Returns the actual port bound.
    pub fn start(&self, host: &str, port: u16) -> io::Result<u16> {
        if self.running.load(Ordering::Acquire) {
            log::info!("WebSocket server is already running");
            return Ok(self.config.read().port);
        }

        let bind_addr = format!("{}:{}", host, port);
        let listener = TcpListener::bind(&bind_addr)?;
        let bound_port = listener.local_addr()?.port();
        listener.set_nonblocking(true)?;

        self.config.write().port = bound_port;
        let actual_bind_addr = format!("{}:{}", host, bound_port);

        log::info!(
            "Yefira Live Sync WebSocket server bound to ws://{}",
            actual_bind_addr
        );
        self.running.store(true, Ordering::Release);

        let shared = self.shared.clone();
        let selection_mgr = self.selection_manager.clone();
        let config = self.config.read().clone();
        let provider_lock = Arc::new(parking_lot::RwLock::new(self.provider.read().clone()));
        let running_clone = self.running.clone();

        let handle = thread::Builder::new()
            .name("yefira-ws-listener".to_string())
            .spawn(move || {
                log::info!("WebSocket accept loop started on {}", actual_bind_addr);
                while running_clone.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok((stream, client_addr)) => {
                            let client_ip = client_addr.ip().to_string();
                            log::info!("New TCP connection from {}", client_addr);

                            if !config.is_authorized(&client_ip, None)
                                && !config.allow_loopback_without_token
                            {
                                log::warn!(
                                    "Connection from {} rejected: unauthorized",
                                    client_addr
                                );
                                continue;
                            }

                            let client_id = shared.next_client_id.fetch_add(1, Ordering::Relaxed);
                            let (tx, rx) = sync_channel::<Vec<u8>>(1024);
                            let session = Arc::new(ClientSession::new(client_id, client_addr, tx));

                            shared.clients.lock().insert(client_id, session.clone());

                            let shared_worker = shared.clone();
                            let sel_worker = selection_mgr.clone();
                            let cfg_worker = config.clone();
                            let prov_worker = provider_lock.clone();

                            thread::Builder::new()
                                .name(format!("yefira-client-{}", client_id))
                                .spawn(move || {
                                    handle_client_connection(
                                        stream,
                                        session,
                                        rx,
                                        shared_worker,
                                        sel_worker,
                                        cfg_worker,
                                        prov_worker,
                                    );
                                })
                                .ok();
                        }
                        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(50));
                        }
                        Err(e) => {
                            log::error!("Error accepting TCP connection: {}", e);
                            thread::sleep(Duration::from_millis(100));
                        }
                    }
                }
                log::info!("WebSocket accept loop stopped");
            })?;

        *self.listener_thread.lock() = Some(handle);
        Ok(bound_port)
    }

    /// Stops the WebSocket server.
    pub fn stop(&self) {
        if !self.running.swap(false, Ordering::AcqRel) {
            return;
        }

        log::info!("Stopping Yefira WebSocket server...");
        let clients = self.shared.clients.lock().clone();
        for session in clients.values() {
            session.stream_gen.store(0, Ordering::Release);
        }
        self.shared.clients.lock().clear();

        if let Some(handle) = self.listener_thread.lock().take() {
            let _ = handle.join();
        }
        log::info!("Yefira WebSocket server stopped");
    }

    /// Queues a block update from world events.
    pub fn queue_delta_update(&self, pos: [i32; 3], new_state: &str) {
        if let Some(sel) = self.selection_manager.current_selection()
            && sel.contains(pos)
        {
            self.delta_queue.queue_change(pos, new_state);
        }
    }

    /// Flushes queued delta updates and broadcasts them to verified clients on server tick.
    pub fn flush_queued_deltas(&self) {
        if self.delta_queue.is_empty() {
            return;
        }

        let Some(sel) = self.selection_manager.current_selection() else {
            self.delta_queue.clear();
            return;
        };

        // If delta queue overflowed during a long baseline, trigger full resync
        if self.delta_queue.take_overflow() {
            log::warn!("Delta queue overflowed; broadcasting full resync notice");
            self.broadcast_packet(&Packet::Error {
                epoch: self.shared.epoch.load(Ordering::Acquire),
                error_code: ErrorCode::ResourceLimitExceeded,
                message: "Deferred delta queue overflowed; baseline resync required".to_string(),
            });
            return;
        }

        let epoch = self.shared.epoch.load(Ordering::Acquire);
        let mut seq = self.shared.global_seq_id.load(Ordering::Acquire);

        let packets = {
            let mut pal_guard = self.shared.palette_lock.lock();
            let pal = &mut *pal_guard;
            let pkts = self.delta_queue.flush_deltas(
                &sel,
                epoch,
                &mut seq,
                &mut pal.palette_lookup,
                &mut pal.palette_list,
                &mut pal.palette_revision,
            );
            self.shared.global_seq_id.store(seq, Ordering::Release);
            pkts
        };

        for pkt in &packets {
            self.broadcast_packet(pkt);
        }
    }

    /// Broadcasts a packet to all connected and authenticated DCC clients.
    pub fn broadcast_packet(&self, packet: &Packet) {
        let raw = encode_packet(packet);
        let clients = self.shared.clients.lock().clone();
        for session in clients.values() {
            if session.is_authenticated() {
                session.send_raw(raw.clone());
            }
        }
    }

    /// Returns the current server lifetime epoch.
    #[inline]
    pub fn epoch(&self) -> u64 {
        self.shared.epoch.load(Ordering::Acquire)
    }

    /// Returns the active server session UUID (16 bytes).
    #[inline]
    pub fn server_uuid(&self) -> [u8; 16] {
        *self.shared.server_uuid.read()
    }

    /// Returns the authoritative cached state hash.
    #[inline]
    pub fn cached_state_hash(&self) -> u64 {
        self.shared.cached_state_hash.load(Ordering::Acquire)
    }

    /// Sets the authoritative cached state hash.
    #[inline]
    pub fn set_cached_state_hash(&self, hash: u64) {
        self.shared.cached_state_hash.store(hash, Ordering::Release);
    }

    /// Rotates the session identity for a new or reloaded world (RFC v3.1 / Java parity).
    /// Generates a new random UUID, increments epoch, and invalidates cached state hash.
    pub fn reset_session_for_new_world(&self) {
        let new_uuid = generate_random_uuid();
        *self.shared.server_uuid.write() = new_uuid;
        self.shared.epoch.fetch_add(1, Ordering::SeqCst);
        self.shared.global_seq_id.store(1, Ordering::SeqCst);
        self.shared.cached_state_hash.store(0, Ordering::Release);
        self.delta_queue.clear();
        log::info!("Yefira: Rotated live sync session UUID and bumped epoch");
    }

    /// Performs a manual baseline scan and streams it to all connected clients.
    pub fn refresh_all_clients(&self) {
        let clients = self.shared.clients.lock().clone();
        let Some(sel) = self.selection_manager.current_selection() else {
            return;
        };
        let dim = self.selection_manager.dimension();

        let Some(provider_arc) = self.provider.read().clone() else {
            log::warn!("Cannot refresh baseline: no WorldBlockProvider registered");
            return;
        };

        match CanonicalVoxelScanner::scan_baseline(&*provider_arc, &sel, &dim, || false) {
            Ok(scan) => {
                // Keep the constant server epoch (never increment epoch during an active session)
                let epoch = self.shared.epoch.load(Ordering::Acquire);
                let seq = self.shared.global_seq_id.fetch_add(1, Ordering::SeqCst) + 1;
                let mut snap_uuid = [0u8; 16];
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos();
                snap_uuid[..8].copy_from_slice(&(ts as u64).to_le_bytes());
                snap_uuid[8..16].copy_from_slice(&seq.to_le_bytes());

                self.shared
                    .cached_state_hash
                    .store(scan.state_hash, Ordering::Release);

                // Synchronize shared palette with new baseline palette
                {
                    let mut pal = self.shared.palette_lock.lock();
                    pal.palette_lookup.clear();
                    for (id, name) in scan.block_palette.iter().enumerate() {
                        pal.palette_lookup.insert(name.clone(), id as u16);
                    }
                    pal.palette_list = scan.block_palette.clone();
                    pal.palette_revision = 1;
                }

                // Clear in-flight deltas as we are establishing a new baseline barrier
                self.delta_queue.clear();

                let packets =
                    BaselineStreamer::build_baseline_packets(&scan, epoch, snap_uuid, seq, 1);

                for pkt in &packets {
                    let raw = encode_packet(pkt);
                    for s in clients.values() {
                        if s.is_authenticated() {
                            s.send_raw(raw.clone());
                        }
                    }
                }
            }
            Err(e) => {
                log::error!("Failed to scan baseline for refresh: {}", e);
            }
        }
    }

    /// Broadcasts a CANCEL packet (reason: SelectionChanged) to all clients and resets snapshot hash.
    pub fn broadcast_cancel_selection(&self) {
        let clients = self.shared.clients.lock().clone();
        let epoch = self.shared.epoch.load(Ordering::Acquire);
        self.delta_queue.clear();
        self.shared.cached_state_hash.store(0, Ordering::Release);

        let cancel = Packet::Cancel {
            epoch,
            snapshot_uuid: [0u8; 16],
            reason: CancelReason::SelectionChanged,
        };
        let raw = encode_packet(&cancel);
        for s in clients.values() {
            if s.is_authenticated() {
                s.send_raw(raw.clone());
            }
        }
    }
}

impl Drop for WebSocketServerManager {
    fn drop(&mut self) {
        self.stop();
    }
}

fn perform_ws_handshake(mut stream: TcpStream) -> io::Result<WebSocket<TcpStream>> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut key = None;

    loop {
        let mut line = String::new();
        let bytes_read = reader.read_line(&mut line)?;
        if bytes_read == 0 || line == "\r\n" || line == "\n" {
            break;
        }
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("sec-websocket-key:")
            && let Some((_, val)) = line.split_once(':')
        {
            key = Some(val.trim().to_string());
        }
    }

    let Some(key_str) = key else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Missing Sec-WebSocket-Key in WebSocket handshake",
        ));
    };

    let mut hasher = Sha1::new();
    hasher.update(key_str.as_bytes());
    hasher.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    let accept_hash = hasher.finalize();
    let accept_b64 = base64::engine::general_purpose::STANDARD.encode(accept_hash);

    let response = format!(
        "HTTP/1.1 101 Switching Protocols\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Accept: {}\r\n\r\n",
        accept_b64
    );

    stream.write_all(response.as_bytes())?;
    stream.flush()?;

    Ok(WebSocket::from_raw_socket(stream, Role::Server, None))
}

fn handle_client_connection(
    stream: TcpStream,
    session: Arc<ClientSession>,
    outbound_rx: Receiver<Vec<u8>>,
    shared: Arc<SharedServerState>,
    selection_mgr: Arc<SelectionManager>,
    config: YefiraConfig,
    provider_lock: Arc<parking_lot::RwLock<Option<Arc<dyn WorldBlockProvider>>>>,
) {
    let client_addr = session.addr;
    stream.set_nonblocking(false).ok();

    let mut ws = match perform_ws_handshake(stream) {
        Ok(ws) => ws,
        Err(e) => {
            log::error!("WebSocket handshake failed for {}: {}", client_addr, e);
            shared.clients.lock().remove(&session.id);
            return;
        }
    };

    log::info!(
        "WebSocket handshake completed successfully for {}",
        client_addr
    );

    // Set socket timeouts and nodelay so read/write poll cleanly
    let tcp = ws.get_ref();
    tcp.set_read_timeout(Some(Duration::from_millis(20))).ok();
    tcp.set_write_timeout(Some(Duration::from_millis(1000)))
        .ok();
    tcp.set_nodelay(true).ok();

    while session.stream_gen.load(Ordering::Acquire) > 0 {
        // 1. Drain outbound packets (e.g. deltas broadcast from server tick)
        let mut sent_any = false;
        while let Ok(msg_bytes) = outbound_rx.try_recv() {
            if let Err(e) = ws.send(Message::Binary(msg_bytes.into())) {
                log::warn!("Failed to send packet to client {}: {}", client_addr, e);
                break;
            }
            sent_any = true;
        }
        if sent_any {
            let _ = ws.flush();
        }

        // 2. Poll inbound WebSocket message
        match ws.read() {
            Ok(Message::Binary(bytes)) => {
                session.touch();
                match decode_packet(&bytes) {
                    Ok(packet) => {
                        handle_inbound_packet(
                            &mut ws,
                            &session,
                            packet,
                            &shared,
                            &selection_mgr,
                            &config,
                            &provider_lock,
                        );
                    }
                    Err(e) => {
                        log::error!("Error decoding packet from client {}: {}", client_addr, e);
                    }
                }
            }
            Ok(Message::Ping(p)) => {
                session.touch();
                let _ = ws.send(Message::Pong(p));
                let _ = ws.flush();
            }
            Ok(Message::Close(_)) => {
                log::info!("Client {} closed WebSocket connection", client_addr);
                break;
            }
            Err(tungstenite::Error::Io(ref e))
                if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut =>
            {
                // Timeout is expected on poll
                thread::yield_now();
            }
            Err(e) => {
                log::warn!("WebSocket error for client {}: {}", client_addr, e);
                break;
            }
            _ => {}
        }
    }

    shared.clients.lock().remove(&session.id);
    log::info!("Client session {} ({}) terminated", session.id, client_addr);
}

fn handle_inbound_packet(
    ws: &mut WebSocket<TcpStream>,
    session: &Arc<ClientSession>,
    packet: Packet,
    shared: &Arc<SharedServerState>,
    selection_mgr: &Arc<SelectionManager>,
    config: &YefiraConfig,
    provider_lock: &Arc<parking_lot::RwLock<Option<Arc<dyn WorldBlockProvider>>>>,
) {
    match packet {
        Packet::Hello {
            auth_token,
            cached_world_uuid,
            cached_epoch,
            cached_seq,
            cached_min,
            cached_size,
            cached_state_hash,
            cached_dimension,
            ..
        } => {
            let client_ip = session.addr.ip().to_string();
            if !config.is_authorized(&client_ip, Some(&auth_token)) {
                log::warn!(
                    "Rejecting unauthorized client {}: token mismatch",
                    session.addr
                );
                let err = Packet::Error {
                    epoch: shared.epoch.load(Ordering::Acquire),
                    error_code: ErrorCode::PermissionDenied,
                    message: "Authentication token invalid".to_string(),
                };
                let _ = ws.send(Message::Binary(encode_packet(&err).into()));
                return;
            }

            let sel = selection_mgr.current_selection();
            let dim = selection_mgr.dimension();

            let Some(box_3d) = sel else {
                log::info!("No active selection box; returning Rejected decision");
                let cur_uuid = *shared.server_uuid.read();
                let welcome = Packet::Welcome {
                    negotiated_version: PROTOCOL_VERSION as u16,
                    server_uuid: cur_uuid,
                    epoch: shared.epoch.load(Ordering::Acquire),
                    server_seq: shared.global_seq_id.load(Ordering::Acquire),
                    current_min: glam::IVec3::ZERO,
                    current_size: glam::IVec3::ZERO,
                    current_state_hash: 0,
                    decision: SyncDecision::Rejected,
                    message_or_dimension: "No active selection in Minecraft".to_string(),
                };
                let _ = ws.send(Message::Binary(encode_packet(&welcome).into()));
                return;
            };

            let cur_uuid = *shared.server_uuid.read();
            let cur_min = box_3d.min_ivec3();
            let cur_size = box_3d.size_ivec3();
            let cur_epoch = shared.epoch.load(Ordering::Acquire);
            let cur_seq = shared.global_seq_id.load(Ordering::Acquire);
            let cur_hash = shared.cached_state_hash.load(Ordering::Acquire);

            let has_client_cache = cached_state_hash != 0 && cached_world_uuid != [0u8; 16];
            let matches_uuid = cached_world_uuid == cur_uuid;
            let matches_epoch = cached_epoch == cur_epoch;
            let matches_seq = cached_seq == cur_seq;
            let matches_bounds = cached_min == cur_min && cached_size == cur_size;
            let matches_dimension = !cached_dimension.is_empty() && cached_dimension == dim;
            let matches_hash = cur_hash != 0 && cached_state_hash == cur_hash;

            // Fast-path: Check if client's cached baseline matches authoritative state (RFC v3.1 / Java parity)
            if has_client_cache
                && matches_uuid
                && matches_epoch
                && matches_seq
                && matches_bounds
                && matches_dimension
                && matches_hash
            {
                log::info!(
                    "Client {} matches current state hash 0x{:016X}; VerifiedSkipRebuild",
                    session.addr,
                    cur_hash
                );
                let welcome = Packet::Welcome {
                    negotiated_version: PROTOCOL_VERSION as u16,
                    server_uuid: cur_uuid,
                    epoch: cur_epoch,
                    server_seq: cur_seq,
                    current_min: cur_min,
                    current_size: cur_size,
                    current_state_hash: cur_hash,
                    decision: SyncDecision::VerifiedSkipRebuild,
                    message_or_dimension: dim,
                };
                let _ = ws.send(Message::Binary(encode_packet(&welcome).into()));
                session.mark_authenticated();
                session.mark_verified();
                return;
            }

            // Otherwise, full baseline required
            log::info!(
                "Client {} requires FullBaselineRequired (cached: 0x{:016X}, server: 0x{:016X})",
                session.addr,
                cached_state_hash,
                cur_hash
            );

            let provider_opt = provider_lock.read().clone();
            let Some(provider) = provider_opt else {
                log::warn!("No WorldBlockProvider registered to scan baseline");
                let err = Packet::Error {
                    epoch: cur_epoch,
                    error_code: ErrorCode::UnknownOpcode,
                    message: "WorldBlockProvider not ready".to_string(),
                };
                let _ = ws.send(Message::Binary(encode_packet(&err).into()));
                return;
            };

            let scan =
                match CanonicalVoxelScanner::scan_baseline(&*provider, &box_3d, &dim, || false) {
                    Ok(res) => res,
                    Err(e) => {
                        log::error!("Baseline scan failed for client {}: {}", session.addr, e);
                        let err = Packet::Error {
                            epoch: cur_epoch,
                            error_code: ErrorCode::ResourceLimitExceeded,
                            message: format!("Baseline scan failed: {}", e),
                        };
                        let _ = ws.send(Message::Binary(encode_packet(&err).into()));
                        return;
                    }
                };

            shared
                .cached_state_hash
                .store(scan.state_hash, Ordering::Release);

            let welcome = Packet::Welcome {
                negotiated_version: PROTOCOL_VERSION as u16,
                server_uuid: cur_uuid,
                epoch: cur_epoch,
                server_seq: shared.global_seq_id.load(Ordering::Acquire),
                current_min: cur_min,
                current_size: cur_size,
                current_state_hash: scan.state_hash,
                decision: SyncDecision::FullBaselineRequired,
                message_or_dimension: dim,
            };
            let _ = ws.send(Message::Binary(encode_packet(&welcome).into()));

            // Initialize palette state from scan
            {
                let mut pal = shared.palette_lock.lock();
                pal.palette_lookup.clear();
                for (id, name) in scan.block_palette.iter().enumerate() {
                    pal.palette_lookup.insert(name.clone(), id as u16);
                }
                pal.palette_list = scan.block_palette.clone();
                pal.palette_revision = 1;
            }

            let mut snap_uuid = [0u8; 16];
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let seq = shared.global_seq_id.fetch_add(1, Ordering::SeqCst) + 1;
            snap_uuid[..8].copy_from_slice(&(ts as u64).to_le_bytes());
            snap_uuid[8..16].copy_from_slice(&seq.to_le_bytes());

            let packets =
                BaselineStreamer::build_baseline_packets(&scan, cur_epoch, snap_uuid, seq, 1);

            log::info!(
                "Streaming {} baseline packets to client {}",
                packets.len(),
                session.addr
            );
            for p in &packets {
                let _ = ws.send(Message::Binary(encode_packet(p).into()));
            }
            let _ = ws.flush();
            session.mark_authenticated();
        }

        Packet::BaselineAck {
            result_code,
            committed_seq,
            ..
        } => {
            if result_code == BaselineAckResult::SuccessCommitted {
                log::info!(
                    "Client {} committed baseline successfully at seq {}",
                    session.addr,
                    committed_seq
                );
                session.mark_verified();
            } else {
                log::warn!(
                    "Client {} reported baseline ack error: {:?}",
                    session.addr,
                    result_code
                );
            }
        }

        Packet::Ping { timestamp } => {
            session.send_raw(encode_packet(&Packet::Pong { timestamp }));
        }

        Packet::Cancel { reason, .. } => {
            log::info!(
                "Client {} cancelled active stream: {:?}",
                session.addr,
                reason
            );
            session.stream_gen.fetch_add(1, Ordering::SeqCst);
        }

        Packet::ResyncRequest { reason, .. } => {
            log::info!("Client {} requested resync: {:?}", session.addr, reason);
            session.stream_gen.fetch_add(1, Ordering::SeqCst);
            if let Some(box_3d) = selection_mgr.current_selection() {
                let dim = selection_mgr.dimension();
                if let Some(provider) = provider_lock.read().clone() {
                    match CanonicalVoxelScanner::scan_baseline(&*provider, &box_3d, &dim, || false)
                    {
                        Ok(scan) => {
                            shared
                                .cached_state_hash
                                .store(scan.state_hash, Ordering::Release);
                            let cur_epoch = shared.epoch.load(Ordering::Acquire);
                            let seq = shared.global_seq_id.fetch_add(1, Ordering::SeqCst) + 1;
                            let mut snap_uuid = [0u8; 16];
                            let ts = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_nanos();
                            snap_uuid[..8].copy_from_slice(&(ts as u64).to_le_bytes());
                            snap_uuid[8..16].copy_from_slice(&seq.to_le_bytes());

                            let packets = BaselineStreamer::build_baseline_packets(
                                &scan, cur_epoch, snap_uuid, seq, 1,
                            );
                            log::info!(
                                "Streaming {} baseline packets on resync to client {}",
                                packets.len(),
                                session.addr
                            );
                            for p in &packets {
                                let _ = ws.send(Message::Binary(encode_packet(p).into()));
                            }
                            let _ = ws.flush();
                        }
                        Err(e) => {
                            log::error!(
                                "Resync baseline scan failed for client {}: {}",
                                session.addr,
                                e
                            );
                        }
                    }
                }
            }
        }

        _ => {}
    }
}
