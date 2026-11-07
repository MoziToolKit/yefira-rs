//! Binary codec for RFC v3.1 Live Sync packets.

use glam::IVec3;

use super::constants::*;
use super::error::ProtocolError;
use super::packet::*;

/// Helper to read a little-endian u16-length prefixed UTF-8 string.
fn read_str<'a>(
    buf: &'a [u8],
    offset: &mut usize,
    field: &'static str,
) -> Result<&'a str, ProtocolError> {
    if *offset + 2 > buf.len() {
        return Err(ProtocolError::TruncatedPayload { field });
    }
    let len = u16::from_le_bytes([buf[*offset], buf[*offset + 1]]) as usize;
    *offset += 2;
    if *offset + len > buf.len() {
        return Err(ProtocolError::TruncatedPayload { field });
    }
    let s = std::str::from_utf8(&buf[*offset..*offset + len]).map_err(|e| {
        ProtocolError::Utf8Error {
            field,
            detail: e.to_string(),
        }
    })?;
    *offset += len;
    Ok(s)
}

/// Helper to write a little-endian u16-length prefixed string.
fn write_str(buf: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    buf.extend_from_slice(&(bytes.len() as u16).to_le_bytes());
    buf.extend_from_slice(bytes);
}

/// Encodes any `Packet` into a complete WebSocket binary payload with standard 4-byte header.
pub fn encode_packet(packet: &Packet) -> Vec<u8> {
    let mut buf = Vec::with_capacity(64);
    buf.extend_from_slice(&PROTOCOL_MAGIC);
    buf.push(PROTOCOL_VERSION);

    match packet {
        Packet::Hello {
            min_version,
            max_version,
            capabilities,
            flags,
            cached_world_uuid,
            cached_epoch,
            cached_seq,
            cached_min,
            cached_size,
            cached_state_hash,
            auth_token,
            cached_dimension,
        } => {
            buf.push(Opcode::Hello as u8);
            buf.extend_from_slice(&min_version.to_le_bytes());
            buf.extend_from_slice(&max_version.to_le_bytes());
            buf.extend_from_slice(&capabilities.to_le_bytes());
            buf.extend_from_slice(&flags.to_le_bytes());
            buf.extend_from_slice(cached_world_uuid);
            buf.extend_from_slice(&cached_epoch.to_le_bytes());
            buf.extend_from_slice(&cached_seq.to_le_bytes());
            buf.extend_from_slice(&cached_min.x.to_le_bytes());
            buf.extend_from_slice(&cached_min.y.to_le_bytes());
            buf.extend_from_slice(&cached_min.z.to_le_bytes());
            buf.extend_from_slice(&cached_size.x.to_le_bytes());
            buf.extend_from_slice(&cached_size.y.to_le_bytes());
            buf.extend_from_slice(&cached_size.z.to_le_bytes());
            buf.extend_from_slice(&cached_state_hash.to_le_bytes());
            write_str(&mut buf, auth_token);
            write_str(&mut buf, cached_dimension);
        }

        Packet::Welcome {
            negotiated_version,
            server_uuid,
            epoch,
            server_seq,
            current_min,
            current_size,
            current_state_hash,
            decision,
            message_or_dimension,
        } => {
            buf.push(Opcode::Welcome as u8);
            buf.extend_from_slice(&negotiated_version.to_le_bytes());
            buf.extend_from_slice(server_uuid);
            buf.extend_from_slice(&epoch.to_le_bytes());
            buf.extend_from_slice(&server_seq.to_le_bytes());
            buf.extend_from_slice(&current_min.x.to_le_bytes());
            buf.extend_from_slice(&current_min.y.to_le_bytes());
            buf.extend_from_slice(&current_min.z.to_le_bytes());
            buf.extend_from_slice(&current_size.x.to_le_bytes());
            buf.extend_from_slice(&current_size.y.to_le_bytes());
            buf.extend_from_slice(&current_size.z.to_le_bytes());
            buf.extend_from_slice(&current_state_hash.to_le_bytes());
            buf.push(*decision as u8);
            write_str(&mut buf, message_or_dimension);
        }

        Packet::BaselineBegin {
            epoch,
            snapshot_uuid,
            snapshot_seq,
            min_pos,
            size,
            state_hash,
            palette_revision,
            total_chunks,
            total_encoded_bytes,
            total_decoded_bytes,
            compression_mode,
            palette_bits_per_block,
            biome_bits_per_quart,
            dimension,
            block_palette,
            biome_palette,
        } => {
            buf.push(Opcode::BaselineBegin as u8);
            buf.extend_from_slice(&epoch.to_le_bytes());
            buf.extend_from_slice(snapshot_uuid);
            buf.extend_from_slice(&snapshot_seq.to_le_bytes());
            buf.extend_from_slice(&min_pos.x.to_le_bytes());
            buf.extend_from_slice(&min_pos.y.to_le_bytes());
            buf.extend_from_slice(&min_pos.z.to_le_bytes());
            buf.extend_from_slice(&size.x.to_le_bytes());
            buf.extend_from_slice(&size.y.to_le_bytes());
            buf.extend_from_slice(&size.z.to_le_bytes());
            buf.extend_from_slice(&state_hash.to_le_bytes());
            buf.extend_from_slice(&palette_revision.to_le_bytes());
            buf.extend_from_slice(&total_chunks.to_le_bytes());
            buf.extend_from_slice(&total_encoded_bytes.to_le_bytes());
            buf.extend_from_slice(&total_decoded_bytes.to_le_bytes());
            buf.push(*compression_mode);
            buf.push(*palette_bits_per_block);
            buf.push(*biome_bits_per_quart);
            write_str(&mut buf, dimension);

            buf.extend_from_slice(&(block_palette.len() as u32).to_le_bytes());
            for p in block_palette {
                write_str(&mut buf, p);
            }

            buf.extend_from_slice(&(biome_palette.len() as u16).to_le_bytes());
            for b in biome_palette {
                write_str(&mut buf, b);
            }
        }

        Packet::BaselineChunk {
            epoch,
            snapshot_uuid,
            chunk_index,
            total_chunks,
            first_voxel_index,
            voxel_count,
            payload_len: _,
            decoded_target_bytes,
            chunk_xxhash64,
            payload,
        } => {
            buf.push(Opcode::BaselineChunk as u8);
            buf.extend_from_slice(&epoch.to_le_bytes());
            buf.extend_from_slice(snapshot_uuid);
            buf.extend_from_slice(&chunk_index.to_le_bytes());
            buf.extend_from_slice(&total_chunks.to_le_bytes());
            buf.extend_from_slice(&first_voxel_index.to_le_bytes());
            buf.extend_from_slice(&voxel_count.to_le_bytes());
            buf.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            buf.extend_from_slice(&decoded_target_bytes.to_le_bytes());
            buf.extend_from_slice(&chunk_xxhash64.to_le_bytes());
            buf.extend_from_slice(payload);
        }

        Packet::BaselineEnd {
            epoch,
            snapshot_uuid,
            snapshot_seq,
            total_chunks_sent,
            final_state_hash,
            status,
        } => {
            buf.push(Opcode::BaselineEnd as u8);
            buf.extend_from_slice(&epoch.to_le_bytes());
            buf.extend_from_slice(snapshot_uuid);
            buf.extend_from_slice(&snapshot_seq.to_le_bytes());
            buf.extend_from_slice(&total_chunks_sent.to_le_bytes());
            buf.extend_from_slice(&final_state_hash.to_le_bytes());
            buf.push(*status);
        }

        Packet::BaselineAck {
            epoch,
            snapshot_uuid,
            committed_seq,
            result_code,
        } => {
            buf.push(Opcode::BaselineAck as u8);
            buf.extend_from_slice(&epoch.to_le_bytes());
            buf.extend_from_slice(snapshot_uuid);
            buf.extend_from_slice(&committed_seq.to_le_bytes());
            buf.push(*result_code as u8);
        }

        Packet::DeltaOp {
            epoch,
            seq_id,
            palette_revision,
            count: _,
            batch_xxhash64,
            changes,
        } => {
            buf.push(Opcode::DeltaOp as u8);
            buf.extend_from_slice(&epoch.to_le_bytes());
            buf.extend_from_slice(&seq_id.to_le_bytes());
            buf.extend_from_slice(&palette_revision.to_le_bytes());
            buf.extend_from_slice(&(changes.len() as u16).to_le_bytes());
            buf.extend_from_slice(&batch_xxhash64.to_le_bytes());
            for c in changes {
                buf.extend_from_slice(&c.rel_pos.x.to_le_bytes());
                buf.extend_from_slice(&c.rel_pos.y.to_le_bytes());
                buf.extend_from_slice(&c.rel_pos.z.to_le_bytes());
                buf.extend_from_slice(&c.state_id.to_le_bytes());
            }
        }

        Packet::PaletteUpdate {
            epoch,
            seq_id,
            base_revision,
            new_revision,
            entries,
        } => {
            buf.push(Opcode::PaletteUpdate as u8);
            buf.extend_from_slice(&epoch.to_le_bytes());
            buf.extend_from_slice(&seq_id.to_le_bytes());
            buf.extend_from_slice(&base_revision.to_le_bytes());
            buf.extend_from_slice(&new_revision.to_le_bytes());
            buf.extend_from_slice(&(entries.len() as u16).to_le_bytes());
            for (id, s) in entries {
                buf.extend_from_slice(&id.to_le_bytes());
                write_str(&mut buf, s);
            }
        }

        Packet::Cancel {
            epoch,
            snapshot_uuid,
            reason,
        } => {
            buf.push(Opcode::Cancel as u8);
            buf.extend_from_slice(&epoch.to_le_bytes());
            buf.extend_from_slice(snapshot_uuid);
            buf.push(*reason as u8);
        }

        Packet::ResyncRequest {
            epoch,
            missing_from_seq,
            missing_to_seq,
            reason,
        } => {
            buf.push(Opcode::ResyncRequest as u8);
            buf.extend_from_slice(&epoch.to_le_bytes());
            buf.extend_from_slice(&missing_from_seq.to_le_bytes());
            buf.extend_from_slice(&missing_to_seq.to_le_bytes());
            buf.push(*reason as u8);
        }

        Packet::Ping { timestamp } => {
            buf.push(Opcode::Ping as u8);
            buf.extend_from_slice(&timestamp.to_le_bytes());
        }

        Packet::Pong { timestamp } => {
            buf.push(Opcode::Pong as u8);
            buf.extend_from_slice(&timestamp.to_le_bytes());
        }

        Packet::Error {
            epoch,
            error_code,
            message,
        } => {
            buf.push(Opcode::Error as u8);
            buf.extend_from_slice(&epoch.to_le_bytes());
            buf.extend_from_slice(&(*error_code as u16).to_le_bytes());
            write_str(&mut buf, message);
        }

        Packet::BaselineEntities {
            epoch,
            snapshot_uuid,
            entities,
        } => {
            buf.push(Opcode::BaselineEntities as u8);
            buf.extend_from_slice(&epoch.to_le_bytes());
            buf.extend_from_slice(snapshot_uuid);
            buf.extend_from_slice(&(entities.len() as u32).to_le_bytes());
            for (pos, json_str) in entities {
                buf.extend_from_slice(&(pos.x as u32).to_le_bytes());
                buf.extend_from_slice(&(pos.y as u32).to_le_bytes());
                buf.extend_from_slice(&(pos.z as u32).to_le_bytes());
                let ed_bytes = json_str.as_bytes();
                buf.extend_from_slice(&(ed_bytes.len() as u32).to_le_bytes());
                buf.extend_from_slice(ed_bytes);
            }
        }

        Packet::DeltaEntity {
            epoch,
            seq_id,
            rel_pos,
            entity_json,
        } => {
            buf.push(Opcode::DeltaEntity as u8);
            buf.extend_from_slice(&epoch.to_le_bytes());
            buf.extend_from_slice(&seq_id.to_le_bytes());
            buf.extend_from_slice(&(rel_pos.x as u32).to_le_bytes());
            buf.extend_from_slice(&(rel_pos.y as u32).to_le_bytes());
            buf.extend_from_slice(&(rel_pos.z as u32).to_le_bytes());
            if let Some(json_str) = entity_json {
                let ed_bytes = json_str.as_bytes();
                buf.extend_from_slice(&(ed_bytes.len() as u32).to_le_bytes());
                buf.extend_from_slice(ed_bytes);
            } else {
                buf.extend_from_slice(&0u32.to_le_bytes());
            }
        }
    }

    buf
}

/// Decodes binary payload into a strongly-typed `Packet`.
pub fn decode_packet(buf: &[u8]) -> Result<Packet, ProtocolError> {
    if buf.len() < HEADER_SIZE {
        return Err(ProtocolError::PacketTooShort {
            expected: HEADER_SIZE,
            actual: buf.len(),
        });
    }

    if buf[0] != PROTOCOL_MAGIC[0] || buf[1] != PROTOCOL_MAGIC[1] {
        return Err(ProtocolError::InvalidMagic(buf[0], buf[1]));
    }

    let version = buf[2];
    if version != PROTOCOL_VERSION {
        return Err(ProtocolError::UnsupportedVersion(version));
    }

    let opcode_byte = buf[3];
    let opcode = Opcode::from_u8(opcode_byte).ok_or(ProtocolError::UnknownOpcode(opcode_byte))?;
    let mut offset = HEADER_SIZE;

    match opcode {
        Opcode::Hello => {
            if offset + 92 > buf.len() {
                return Err(ProtocolError::TruncatedPayload {
                    field: "Hello fixed fields",
                });
            }
            let min_version = u16::from_le_bytes(buf[offset..offset + 2].try_into().unwrap());
            let max_version = u16::from_le_bytes(buf[offset + 2..offset + 4].try_into().unwrap());
            let capabilities = u32::from_le_bytes(buf[offset + 4..offset + 8].try_into().unwrap());
            let flags = u32::from_le_bytes(buf[offset + 8..offset + 12].try_into().unwrap());
            let mut cached_world_uuid = [0u8; 16];
            cached_world_uuid.copy_from_slice(&buf[offset + 12..offset + 28]);
            let cached_epoch =
                u64::from_le_bytes(buf[offset + 28..offset + 36].try_into().unwrap());
            let cached_seq = u64::from_le_bytes(buf[offset + 36..offset + 44].try_into().unwrap());
            let cached_min = IVec3::new(
                i32::from_le_bytes(buf[offset + 44..offset + 48].try_into().unwrap()),
                i32::from_le_bytes(buf[offset + 48..offset + 52].try_into().unwrap()),
                i32::from_le_bytes(buf[offset + 52..offset + 56].try_into().unwrap()),
            );
            let cached_size = IVec3::new(
                i32::from_le_bytes(buf[offset + 56..offset + 60].try_into().unwrap()),
                i32::from_le_bytes(buf[offset + 60..offset + 64].try_into().unwrap()),
                i32::from_le_bytes(buf[offset + 64..offset + 68].try_into().unwrap()),
            );
            let cached_state_hash =
                u64::from_le_bytes(buf[offset + 68..offset + 76].try_into().unwrap());
            offset += 76;

            let auth_token = read_str(buf, &mut offset, "Hello auth_token")?.to_string();
            let cached_dimension =
                read_str(buf, &mut offset, "Hello cached_dimension")?.to_string();

            Ok(Packet::Hello {
                min_version,
                max_version,
                capabilities,
                flags,
                cached_world_uuid,
                cached_epoch,
                cached_seq,
                cached_min,
                cached_size,
                cached_state_hash,
                auth_token,
                cached_dimension,
            })
        }

        Opcode::Welcome => {
            if offset + 67 > buf.len() {
                return Err(ProtocolError::TruncatedPayload {
                    field: "Welcome fixed fields",
                });
            }
            let negotiated_version =
                u16::from_le_bytes(buf[offset..offset + 2].try_into().unwrap());
            let mut server_uuid = [0u8; 16];
            server_uuid.copy_from_slice(&buf[offset + 2..offset + 18]);
            let epoch = u64::from_le_bytes(buf[offset + 18..offset + 26].try_into().unwrap());
            let server_seq = u64::from_le_bytes(buf[offset + 26..offset + 34].try_into().unwrap());
            let current_min = IVec3::new(
                i32::from_le_bytes(buf[offset + 34..offset + 38].try_into().unwrap()),
                i32::from_le_bytes(buf[offset + 38..offset + 42].try_into().unwrap()),
                i32::from_le_bytes(buf[offset + 42..offset + 46].try_into().unwrap()),
            );
            let current_size = IVec3::new(
                i32::from_le_bytes(buf[offset + 46..offset + 50].try_into().unwrap()),
                i32::from_le_bytes(buf[offset + 50..offset + 54].try_into().unwrap()),
                i32::from_le_bytes(buf[offset + 54..offset + 58].try_into().unwrap()),
            );
            let current_state_hash =
                u64::from_le_bytes(buf[offset + 58..offset + 66].try_into().unwrap());
            let decision_byte = buf[offset + 66];
            let decision =
                SyncDecision::from_u8(decision_byte).ok_or(ProtocolError::InvalidEnum {
                    field: "SyncDecision",
                    val: decision_byte as u32,
                })?;
            offset += 67;

            let message_or_dimension =
                read_str(buf, &mut offset, "Welcome message_or_dimension")?.to_string();

            Ok(Packet::Welcome {
                negotiated_version,
                server_uuid,
                epoch,
                server_seq,
                current_min,
                current_size,
                current_state_hash,
                decision,
                message_or_dimension,
            })
        }

        Opcode::BaselineBegin => {
            if offset + 91 > buf.len() {
                return Err(ProtocolError::TruncatedPayload {
                    field: "BaselineBegin fixed fields",
                });
            }
            let epoch = u64::from_le_bytes(buf[offset..offset + 8].try_into().unwrap());
            let mut snapshot_uuid = [0u8; 16];
            snapshot_uuid.copy_from_slice(&buf[offset + 8..offset + 24]);
            let snapshot_seq =
                u64::from_le_bytes(buf[offset + 24..offset + 32].try_into().unwrap());
            let min_pos = IVec3::new(
                i32::from_le_bytes(buf[offset + 32..offset + 36].try_into().unwrap()),
                i32::from_le_bytes(buf[offset + 36..offset + 40].try_into().unwrap()),
                i32::from_le_bytes(buf[offset + 40..offset + 44].try_into().unwrap()),
            );
            let size = IVec3::new(
                i32::from_le_bytes(buf[offset + 44..offset + 48].try_into().unwrap()),
                i32::from_le_bytes(buf[offset + 48..offset + 52].try_into().unwrap()),
                i32::from_le_bytes(buf[offset + 52..offset + 56].try_into().unwrap()),
            );
            let state_hash = u64::from_le_bytes(buf[offset + 56..offset + 64].try_into().unwrap());
            let palette_revision =
                u32::from_le_bytes(buf[offset + 64..offset + 68].try_into().unwrap());
            let total_chunks =
                u32::from_le_bytes(buf[offset + 68..offset + 72].try_into().unwrap());
            let total_encoded_bytes =
                u64::from_le_bytes(buf[offset + 72..offset + 80].try_into().unwrap());
            let total_decoded_bytes =
                u64::from_le_bytes(buf[offset + 80..offset + 88].try_into().unwrap());
            let compression_mode = buf[offset + 88];
            let palette_bits_per_block = buf[offset + 89];
            let biome_bits_per_quart = buf[offset + 90];
            offset += 91;

            let dimension = read_str(buf, &mut offset, "BaselineBegin dimension")?.to_string();

            if offset + 4 > buf.len() {
                return Err(ProtocolError::TruncatedPayload {
                    field: "block_palette length",
                });
            }
            let block_palette_len =
                u32::from_le_bytes(buf[offset..offset + 4].try_into().unwrap()) as usize;
            offset += 4;
            if block_palette_len > MAX_PALETTE_ENTRIES {
                return Err(ProtocolError::ResourceLimitExceeded {
                    resource: "Block Palette Entries",
                    limit: MAX_PALETTE_ENTRIES,
                    actual: block_palette_len,
                });
            }

            let mut block_palette = Vec::with_capacity(block_palette_len);
            for _ in 0..block_palette_len {
                block_palette.push(read_str(buf, &mut offset, "block_palette entry")?.to_string());
            }

            if offset + 2 > buf.len() {
                return Err(ProtocolError::TruncatedPayload {
                    field: "biome_palette length",
                });
            }
            let biome_palette_len =
                u16::from_le_bytes(buf[offset..offset + 2].try_into().unwrap()) as usize;
            offset += 2;

            let mut biome_palette = Vec::with_capacity(biome_palette_len);
            for _ in 0..biome_palette_len {
                biome_palette.push(read_str(buf, &mut offset, "biome_palette entry")?.to_string());
            }

            Ok(Packet::BaselineBegin {
                epoch,
                snapshot_uuid,
                snapshot_seq,
                min_pos,
                size,
                state_hash,
                palette_revision,
                total_chunks,
                total_encoded_bytes,
                total_decoded_bytes,
                compression_mode,
                palette_bits_per_block,
                biome_bits_per_quart,
                dimension,
                block_palette,
                biome_palette,
            })
        }

        Opcode::BaselineChunk => {
            if offset + 56 > buf.len() {
                return Err(ProtocolError::TruncatedPayload {
                    field: "BaselineChunk header",
                });
            }
            let epoch = u64::from_le_bytes(buf[offset..offset + 8].try_into().unwrap());
            let mut snapshot_uuid = [0u8; 16];
            snapshot_uuid.copy_from_slice(&buf[offset + 8..offset + 24]);
            let chunk_index = u32::from_le_bytes(buf[offset + 24..offset + 28].try_into().unwrap());
            let total_chunks =
                u32::from_le_bytes(buf[offset + 28..offset + 32].try_into().unwrap());
            let first_voxel_index =
                u64::from_le_bytes(buf[offset + 32..offset + 40].try_into().unwrap());
            let voxel_count = u32::from_le_bytes(buf[offset + 40..offset + 44].try_into().unwrap());
            let payload_len =
                u32::from_le_bytes(buf[offset + 44..offset + 48].try_into().unwrap()) as usize;
            let decoded_target_bytes =
                u32::from_le_bytes(buf[offset + 48..offset + 52].try_into().unwrap());
            let chunk_xxhash64 =
                u64::from_le_bytes(buf[offset + 52..offset + 60].try_into().unwrap());
            offset += 60;

            if offset + payload_len > buf.len() {
                return Err(ProtocolError::TruncatedPayload {
                    field: "BaselineChunk payload",
                });
            }
            let payload = buf[offset..offset + payload_len].to_vec();

            Ok(Packet::BaselineChunk {
                epoch,
                snapshot_uuid,
                chunk_index,
                total_chunks,
                first_voxel_index,
                voxel_count,
                payload_len: payload_len as u32,
                decoded_target_bytes,
                chunk_xxhash64,
                payload,
            })
        }

        Opcode::BaselineEnd => {
            if offset + 41 > buf.len() {
                return Err(ProtocolError::TruncatedPayload {
                    field: "BaselineEnd",
                });
            }
            let epoch = u64::from_le_bytes(buf[offset..offset + 8].try_into().unwrap());
            let mut snapshot_uuid = [0u8; 16];
            snapshot_uuid.copy_from_slice(&buf[offset + 8..offset + 24]);
            let snapshot_seq =
                u64::from_le_bytes(buf[offset + 24..offset + 32].try_into().unwrap());
            let total_chunks_sent =
                u32::from_le_bytes(buf[offset + 32..offset + 36].try_into().unwrap());
            let final_state_hash =
                u64::from_le_bytes(buf[offset + 36..offset + 44].try_into().unwrap());
            let status = buf[offset + 44];

            Ok(Packet::BaselineEnd {
                epoch,
                snapshot_uuid,
                snapshot_seq,
                total_chunks_sent,
                final_state_hash,
                status,
            })
        }

        Opcode::BaselineAck => {
            if offset + 33 > buf.len() {
                return Err(ProtocolError::TruncatedPayload {
                    field: "BaselineAck",
                });
            }
            let epoch = u64::from_le_bytes(buf[offset..offset + 8].try_into().unwrap());
            let mut snapshot_uuid = [0u8; 16];
            snapshot_uuid.copy_from_slice(&buf[offset + 8..offset + 24]);
            let committed_seq =
                u64::from_le_bytes(buf[offset + 24..offset + 32].try_into().unwrap());
            let result_byte = buf[offset + 32];
            let result_code =
                BaselineAckResult::from_u8(result_byte).ok_or(ProtocolError::InvalidEnum {
                    field: "BaselineAckResult",
                    val: result_byte as u32,
                })?;

            Ok(Packet::BaselineAck {
                epoch,
                snapshot_uuid,
                committed_seq,
                result_code,
            })
        }

        Opcode::DeltaOp => {
            if offset + 30 > buf.len() {
                return Err(ProtocolError::TruncatedPayload {
                    field: "DeltaOp header",
                });
            }
            let epoch = u64::from_le_bytes(buf[offset..offset + 8].try_into().unwrap());
            let seq_id = u64::from_le_bytes(buf[offset + 8..offset + 16].try_into().unwrap());
            let palette_revision =
                u32::from_le_bytes(buf[offset + 16..offset + 20].try_into().unwrap());
            let count =
                u16::from_le_bytes(buf[offset + 20..offset + 22].try_into().unwrap()) as usize;
            let batch_xxhash64 =
                u64::from_le_bytes(buf[offset + 22..offset + 30].try_into().unwrap());
            offset += 30;

            if count > MAX_DELTA_BATCH_CHANGES {
                return Err(ProtocolError::ResourceLimitExceeded {
                    resource: "DeltaOp changes count",
                    limit: MAX_DELTA_BATCH_CHANGES,
                    actual: count,
                });
            }

            if offset + count * 14 > buf.len() {
                return Err(ProtocolError::TruncatedPayload {
                    field: "DeltaOp changes data",
                });
            }

            let mut changes = Vec::with_capacity(count);
            for _ in 0..count {
                let rx = i32::from_le_bytes(buf[offset..offset + 4].try_into().unwrap());
                let ry = i32::from_le_bytes(buf[offset + 4..offset + 8].try_into().unwrap());
                let rz = i32::from_le_bytes(buf[offset + 8..offset + 12].try_into().unwrap());
                let state_id =
                    u16::from_le_bytes(buf[offset + 12..offset + 14].try_into().unwrap());
                offset += 14;
                changes.push(DeltaChangeEntry {
                    rel_pos: IVec3::new(rx, ry, rz),
                    state_id,
                });
            }

            Ok(Packet::DeltaOp {
                epoch,
                seq_id,
                palette_revision,
                count: count as u16,
                batch_xxhash64,
                changes,
            })
        }

        Opcode::PaletteUpdate => {
            if offset + 26 > buf.len() {
                return Err(ProtocolError::TruncatedPayload {
                    field: "PaletteUpdate header",
                });
            }
            let epoch = u64::from_le_bytes(buf[offset..offset + 8].try_into().unwrap());
            let seq_id = u64::from_le_bytes(buf[offset + 8..offset + 16].try_into().unwrap());
            let base_revision =
                u32::from_le_bytes(buf[offset + 16..offset + 20].try_into().unwrap());
            let new_revision =
                u32::from_le_bytes(buf[offset + 20..offset + 24].try_into().unwrap());
            let count =
                u16::from_le_bytes(buf[offset + 24..offset + 26].try_into().unwrap()) as usize;
            offset += 26;

            let mut entries = Vec::with_capacity(count);
            for _ in 0..count {
                if offset + 2 > buf.len() {
                    return Err(ProtocolError::TruncatedPayload {
                        field: "PaletteUpdate entry id",
                    });
                }
                let id = u16::from_le_bytes(buf[offset..offset + 2].try_into().unwrap());
                offset += 2;
                let s = read_str(buf, &mut offset, "PaletteUpdate entry string")?.to_string();
                entries.push((id, s));
            }

            Ok(Packet::PaletteUpdate {
                epoch,
                seq_id,
                base_revision,
                new_revision,
                entries,
            })
        }

        Opcode::Cancel => {
            if offset + 25 > buf.len() {
                return Err(ProtocolError::TruncatedPayload {
                    field: "Cancel packet",
                });
            }
            let epoch = u64::from_le_bytes(buf[offset..offset + 8].try_into().unwrap());
            let mut snapshot_uuid = [0u8; 16];
            snapshot_uuid.copy_from_slice(&buf[offset + 8..offset + 24]);
            let reason_byte = buf[offset + 24];
            let reason = CancelReason::from_u8(reason_byte).ok_or(ProtocolError::InvalidEnum {
                field: "CancelReason",
                val: reason_byte as u32,
            })?;

            Ok(Packet::Cancel {
                epoch,
                snapshot_uuid,
                reason,
            })
        }

        Opcode::ResyncRequest => {
            if offset + 25 > buf.len() {
                return Err(ProtocolError::TruncatedPayload {
                    field: "ResyncRequest",
                });
            }
            let epoch = u64::from_le_bytes(buf[offset..offset + 8].try_into().unwrap());
            let missing_from_seq =
                u64::from_le_bytes(buf[offset + 8..offset + 16].try_into().unwrap());
            let missing_to_seq =
                u64::from_le_bytes(buf[offset + 16..offset + 24].try_into().unwrap());
            let reason_byte = buf[offset + 24];
            let reason = ResyncReason::from_u8(reason_byte).ok_or(ProtocolError::InvalidEnum {
                field: "ResyncReason",
                val: reason_byte as u32,
            })?;

            Ok(Packet::ResyncRequest {
                epoch,
                missing_from_seq,
                missing_to_seq,
                reason,
            })
        }

        Opcode::Ping => {
            if offset + 8 > buf.len() {
                return Err(ProtocolError::TruncatedPayload {
                    field: "Ping timestamp",
                });
            }
            let timestamp = u64::from_le_bytes(buf[offset..offset + 8].try_into().unwrap());
            Ok(Packet::Ping { timestamp })
        }

        Opcode::Pong => {
            if offset + 8 > buf.len() {
                return Err(ProtocolError::TruncatedPayload {
                    field: "Pong timestamp",
                });
            }
            let timestamp = u64::from_le_bytes(buf[offset..offset + 8].try_into().unwrap());
            Ok(Packet::Pong { timestamp })
        }

        Opcode::Error => {
            if offset + 10 > buf.len() {
                return Err(ProtocolError::TruncatedPayload {
                    field: "Error header",
                });
            }
            let epoch = u64::from_le_bytes(buf[offset..offset + 8].try_into().unwrap());
            let code_val = u16::from_le_bytes(buf[offset + 8..offset + 10].try_into().unwrap());
            let error_code = ErrorCode::from_u16(code_val).ok_or(ProtocolError::InvalidEnum {
                field: "ErrorCode",
                val: code_val as u32,
            })?;
            offset += 10;
            let message = read_str(buf, &mut offset, "Error message")?.to_string();

            Ok(Packet::Error {
                epoch,
                error_code,
                message,
            })
        }

        Opcode::BaselineEntities => {
            if offset + 28 > buf.len() {
                return Err(ProtocolError::TruncatedPayload {
                    field: "BaselineEntities header",
                });
            }
            let epoch = u64::from_le_bytes(buf[offset..offset + 8].try_into().unwrap());
            let mut snapshot_uuid = [0u8; 16];
            snapshot_uuid.copy_from_slice(&buf[offset + 8..offset + 24]);
            let count =
                u32::from_le_bytes(buf[offset + 24..offset + 28].try_into().unwrap()) as usize;
            offset += 28;

            let mut entities = Vec::with_capacity(count);
            for _ in 0..count {
                if offset + 16 > buf.len() {
                    return Err(ProtocolError::TruncatedPayload {
                        field: "BaselineEntities item",
                    });
                }
                let rx = i32::from_le_bytes(buf[offset..offset + 4].try_into().unwrap());
                let ry = i32::from_le_bytes(buf[offset + 4..offset + 8].try_into().unwrap());
                let rz = i32::from_le_bytes(buf[offset + 8..offset + 12].try_into().unwrap());
                let ed_len =
                    u32::from_le_bytes(buf[offset + 12..offset + 16].try_into().unwrap()) as usize;
                offset += 16;
                if offset + ed_len > buf.len() {
                    return Err(ProtocolError::TruncatedPayload {
                        field: "BaselineEntities JSON",
                    });
                }
                let json_str = std::str::from_utf8(&buf[offset..offset + ed_len])
                    .map_err(|e| ProtocolError::Utf8Error {
                        field: "BaselineEntities JSON",
                        detail: e.to_string(),
                    })?
                    .to_string();
                offset += ed_len;
                entities.push((IVec3::new(rx, ry, rz), json_str));
            }

            Ok(Packet::BaselineEntities {
                epoch,
                snapshot_uuid,
                entities,
            })
        }

        Opcode::DeltaEntity => {
            if offset + 32 > buf.len() {
                return Err(ProtocolError::TruncatedPayload {
                    field: "DeltaEntity header",
                });
            }
            let epoch = u64::from_le_bytes(buf[offset..offset + 8].try_into().unwrap());
            let seq_id = u64::from_le_bytes(buf[offset + 8..offset + 16].try_into().unwrap());
            let rel_pos = IVec3::new(
                i32::from_le_bytes(buf[offset + 16..offset + 20].try_into().unwrap()),
                i32::from_le_bytes(buf[offset + 20..offset + 24].try_into().unwrap()),
                i32::from_le_bytes(buf[offset + 24..offset + 28].try_into().unwrap()),
            );
            let ed_len =
                u32::from_le_bytes(buf[offset + 28..offset + 32].try_into().unwrap()) as usize;
            offset += 32;

            let entity_json = if ed_len > 0 {
                if offset + ed_len > buf.len() {
                    return Err(ProtocolError::TruncatedPayload {
                        field: "DeltaEntity JSON",
                    });
                }
                let s = std::str::from_utf8(&buf[offset..offset + ed_len])
                    .map_err(|e| ProtocolError::Utf8Error {
                        field: "DeltaEntity JSON",
                        detail: e.to_string(),
                    })?
                    .to_string();
                Some(s)
            } else {
                None
            };

            Ok(Packet::DeltaEntity {
                epoch,
                seq_id,
                rel_pos,
                entity_json,
            })
        }
    }
}
