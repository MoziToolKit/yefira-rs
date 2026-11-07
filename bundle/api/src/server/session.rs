//! Per-client connection session tracking and state machine.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::SyncSender;
use std::time::Instant;

/// Active state of a connected DCC client (Blender / MoziToolKit / libmtk).
pub struct ClientSession {
    pub id: usize,
    pub addr: SocketAddr,
    pub sender: SyncSender<Vec<u8>>,
    pub is_authenticated: AtomicBool,
    pub is_verified: AtomicBool,
    pub stream_gen: AtomicU64,
    pub active_epoch: AtomicU64,
    pub last_activity: parking_lot::Mutex<Instant>,
}

impl ClientSession {
    pub fn new(id: usize, addr: SocketAddr, sender: SyncSender<Vec<u8>>) -> Self {
        Self {
            id,
            addr,
            sender,
            is_authenticated: AtomicBool::new(false),
            is_verified: AtomicBool::new(false),
            stream_gen: AtomicU64::new(1),
            active_epoch: AtomicU64::new(1),
            last_activity: parking_lot::Mutex::new(Instant::now()),
        }
    }

    #[inline]
    pub fn mark_authenticated(&self) {
        self.is_authenticated.store(true, Ordering::Release);
    }

    #[inline]
    pub fn is_authenticated(&self) -> bool {
        self.is_authenticated.load(Ordering::Acquire)
    }

    #[inline]
    pub fn mark_verified(&self) {
        self.is_verified.store(true, Ordering::Release);
    }

    #[inline]
    pub fn is_verified(&self) -> bool {
        self.is_verified.load(Ordering::Acquire)
    }

    #[inline]
    pub fn touch(&self) {
        *self.last_activity.lock() = Instant::now();
    }

    #[inline]
    pub fn send_raw(&self, data: Vec<u8>) -> bool {
        self.sender.try_send(data).is_ok()
    }
}
