//! Error definitions for RFC v3.1 binary packet codecs.

use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    #[error("Packet too short: expected at least {expected} bytes, got {actual}")]
    PacketTooShort { expected: usize, actual: usize },

    #[error("Invalid magic header: expected [0x4D, 0x54] ('MT'), got [0x{0:02X}, 0x{1:02X}]")]
    InvalidMagic(u8, u8),

    #[error("Unsupported protocol version: 0x{0:02X} (expected 0x03)")]
    UnsupportedVersion(u8),

    #[error("Unknown opcode: 0x{0:02X}")]
    UnknownOpcode(u8),

    #[error("UTF-8 decoding error for field '{field}': {detail}")]
    Utf8Error { field: &'static str, detail: String },

    #[error("Payload truncated while parsing {field}")]
    TruncatedPayload { field: &'static str },

    #[error("Resource limit exceeded for '{resource}': limit is {limit}, actual was {actual}")]
    ResourceLimitExceeded {
        resource: &'static str,
        limit: usize,
        actual: usize,
    },

    #[error("Invalid selection dimensions: {0}x{1}x{2} (negative or overflowing volume)")]
    InvalidDimensions(i32, i32, i32),

    #[error("Checksum mismatch for {field}: expected 0x{expected:016X}, got 0x{actual:016X}")]
    ChecksumMismatch {
        field: &'static str,
        expected: u64,
        actual: u64,
    },

    #[error("Invalid enum discriminant for {field}: value was {val}")]
    InvalidEnum { field: &'static str, val: u32 },
}
