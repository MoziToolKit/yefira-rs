use glam::IVec3;
use yefira_rs::protocol::*;

#[test]
fn test_roundtrip_all_packet_variants() {
    let packets = vec![
        Packet::Hello {
            min_version: 1,
            max_version: 3,
            capabilities: 0x07,
            flags: 0,
            cached_world_uuid: [1u8; 16],
            cached_epoch: 42,
            cached_seq: 100,
            cached_min: IVec3::new(10, 20, 30),
            cached_size: IVec3::new(16, 16, 16),
            cached_state_hash: 0xDEADBEEFCAFE,
            auth_token: "secret_token_123".to_string(),
            cached_dimension: "minecraft:overworld".to_string(),
        },
        Packet::Welcome {
            negotiated_version: 3,
            server_uuid: [2u8; 16],
            epoch: 42,
            server_seq: 101,
            current_min: IVec3::new(10, 20, 30),
            current_size: IVec3::new(16, 16, 16),
            current_state_hash: 0xDEADBEEFCAFE,
            decision: SyncDecision::FullBaselineRequired,
            message_or_dimension: "minecraft:overworld".to_string(),
        },
        Packet::BaselineBegin {
            epoch: 42,
            snapshot_uuid: [3u8; 16],
            snapshot_seq: 101,
            min_pos: IVec3::new(0, 60, 0),
            size: IVec3::new(16, 16, 16),
            state_hash: 0x1234567890ABCDEF,
            palette_revision: 1,
            total_chunks: 2,
            total_encoded_bytes: 4096,
            total_decoded_bytes: 4096,
            compression_mode: 0,
            palette_bits_per_block: 8,
            biome_bits_per_quart: 0,
            dimension: "minecraft:the_nether".to_string(),
            block_palette: vec!["minecraft:air".into(), "minecraft:netherrack".into()],
            biome_palette: vec!["minecraft:nether_wastes".into()],
        },
        Packet::BaselineChunk {
            epoch: 42,
            snapshot_uuid: [3u8; 16],
            chunk_index: 0,
            total_chunks: 2,
            first_voxel_index: 0,
            voxel_count: 2048,
            payload_len: 2048,
            decoded_target_bytes: 2048,
            chunk_xxhash64: 0xCAFEBABE,
            payload: vec![1u8; 2048],
        },
        Packet::BaselineEnd {
            epoch: 42,
            snapshot_uuid: [3u8; 16],
            snapshot_seq: 101,
            total_chunks_sent: 2,
            final_state_hash: 0x1234567890ABCDEF,
            status: 0,
        },
        Packet::BaselineAck {
            epoch: 42,
            snapshot_uuid: [3u8; 16],
            committed_seq: 101,
            result_code: BaselineAckResult::SuccessCommitted,
        },
        Packet::DeltaOp {
            epoch: 42,
            seq_id: 102,
            palette_revision: 1,
            count: 2,
            batch_xxhash64: 0x999,
            changes: vec![
                DeltaChangeEntry {
                    rel_pos: IVec3::new(1, 2, 3),
                    state_id: 5,
                },
                DeltaChangeEntry {
                    rel_pos: IVec3::new(4, 5, 6),
                    state_id: 7,
                },
            ],
        },
        Packet::PaletteUpdate {
            epoch: 42,
            seq_id: 103,
            base_revision: 1,
            new_revision: 2,
            entries: vec![(8, "minecraft:diamond_block".to_string())],
        },
        Packet::Cancel {
            epoch: 42,
            snapshot_uuid: [3u8; 16],
            reason: CancelReason::SelectionChanged,
        },
        Packet::ResyncRequest {
            epoch: 42,
            missing_from_seq: 100,
            missing_to_seq: 105,
            reason: ResyncReason::StateHashMismatch,
        },
        Packet::Ping { timestamp: 9999999 },
        Packet::Pong { timestamp: 9999999 },
        Packet::Error {
            epoch: 42,
            error_code: ErrorCode::PermissionDenied,
            message: "Access Denied".to_string(),
        },
    ];

    for original in packets {
        let encoded = encode_packet(&original);
        assert!(encoded.len() >= 4);
        assert_eq!(&encoded[0..2], &PROTOCOL_MAGIC);
        assert_eq!(encoded[2], PROTOCOL_VERSION);

        let decoded = decode_packet(&encoded).unwrap();
        assert_eq!(original, decoded);
    }
}

#[test]
fn test_cross_interoperability_with_libmozitoolkit_mtk_sync() {
    // 1. Packet created & encoded in yefira-rs -> decoded by libmozitoolkit's mtk_sync
    let yefira_welcome = Packet::Welcome {
        negotiated_version: 3,
        server_uuid: [9u8; 16],
        epoch: 77,
        server_seq: 300,
        current_min: IVec3::new(-10, 60, -10),
        current_size: IVec3::new(32, 32, 32),
        current_state_hash: 0x1122334455667788,
        decision: SyncDecision::VerifiedSkipRebuild,
        message_or_dimension: "minecraft:overworld".to_string(),
    };

    let encoded_by_yefira = encode_packet(&yefira_welcome);

    // Decode with libmozitoolkit mtk-sync
    let decoded_by_mtk =
        mtk_sync::decode_packet(&encoded_by_yefira).expect("mtk_sync decode should succeed");

    if let mtk_sync::Packet::Welcome {
        negotiated_version,
        server_uuid,
        epoch,
        server_seq,
        current_min,
        current_size,
        current_state_hash,
        decision,
        message_or_dimension,
    } = decoded_by_mtk
    {
        assert_eq!(negotiated_version, 3);
        assert_eq!(server_uuid, [9u8; 16]);
        assert_eq!(epoch, 77);
        assert_eq!(server_seq, 300);
        assert_eq!(current_min, IVec3::new(-10, 60, -10));
        assert_eq!(current_size, IVec3::new(32, 32, 32));
        assert_eq!(current_state_hash, 0x1122334455667788);
        assert_eq!(decision, mtk_sync::SyncDecision::VerifiedSkipRebuild);
        assert_eq!(message_or_dimension, "minecraft:overworld");
    } else {
        panic!("Decoded packet was not Welcome");
    }

    // 2. Packet created & encoded in libmozitoolkit's mtk_sync -> decoded by yefira-rs
    let mtk_hello = mtk_sync::Packet::Hello {
        min_version: 1,
        max_version: 3,
        capabilities: 0x01,
        flags: 0,
        cached_world_uuid: [5u8; 16],
        cached_epoch: 10,
        cached_seq: 50,
        cached_min: IVec3::new(0, 64, 0),
        cached_size: IVec3::new(16, 16, 16),
        cached_state_hash: 0x9999AAAA,
        auth_token: "mtk_auth_abc".to_string(),
        cached_dimension: "minecraft:overworld".to_string(),
    };

    let encoded_by_mtk = mtk_sync::encode_packet(&mtk_hello);

    let decoded_by_yefira =
        decode_packet(&encoded_by_mtk).expect("yefira-rs decode should succeed");

    if let Packet::Hello {
        min_version,
        max_version,
        capabilities,
        cached_world_uuid,
        cached_epoch,
        cached_seq,
        cached_min,
        cached_size,
        cached_state_hash,
        auth_token,
        cached_dimension,
        ..
    } = decoded_by_yefira
    {
        assert_eq!(min_version, 1);
        assert_eq!(max_version, 3);
        assert_eq!(capabilities, 0x01);
        assert_eq!(cached_world_uuid, [5u8; 16]);
        assert_eq!(cached_epoch, 10);
        assert_eq!(cached_seq, 50);
        assert_eq!(cached_min, IVec3::new(0, 64, 0));
        assert_eq!(cached_size, IVec3::new(16, 16, 16));
        assert_eq!(cached_state_hash, 0x9999AAAA);
        assert_eq!(auth_token, "mtk_auth_abc");
        assert_eq!(cached_dimension, "minecraft:overworld");
    } else {
        panic!("Decoded packet was not Hello");
    }
}
