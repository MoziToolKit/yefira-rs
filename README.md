# yefira-rs: High-Performance Minecraft Live Sync Mod for Horizon (Rust)

[![Rust](https://img.shields.io/badge/rust-2024%20edition-orange.svg)](https://www.rust-lang.org)
[![CI](https://github.com/MoziToolKit/yefira-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/MoziToolKit/yefira-rs/actions/workflows/ci.yml)
[![Rolling Release](https://img.shields.io/github/v/release/MoziToolKit/yefira-rs?include_prereleases&label=rolling%20release&color=blue)](https://github.com/MoziToolKit/yefira-rs/releases/tag/ci-latest)
[![RFC v3.1](https://img.shields.io/badge/protocol-RFC%20v3.1%20compliant-blue.svg)](../MoziToolKit/docs/live_sync/live_sync_protocol.md)
[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](LICENSE)

`yefira-rs` is the pure-Rust port of the **Yefira Live Sync Mod** designed specifically for **[Horizon](../horizon)** (Minecraft: Java Edition client/server rewritten in Rust & wgpu).

It acts as the authoritative live voxel server in the **Three-Pillar Sync Architecture**, maintaining a seamless, zero-latency bidirectional synchronization closed loop between **Horizon**, **[libmozitoolkit](../libmozitoolkit)** (`mtk-sync`), and **[MoziToolKit](../MoziToolKit)** (Blender 3D Addon).

---

## 1. Multi-Workspace Collaborative Topology

```
┌─────────────────────────────────────────────────────────────┐
│                       Horizon Client                        │
│            (Rust wgpu Minecraft 26.3 Client/Server)         │
│                                                             │
│   ┌─────────────────────────────────────────────────────┐   │
│   │                     yefira-rs                       │   │
│   │   • CanonicalVoxelScanner (authoritative xxHash64)  │   │
│   │   • SelectionManager (Box bounds, expand/shift)    │   │
│   │   • WebSocketServerManager (RFC v3.1 / v3.2 Server) │   │
│   │   • DeltaQueue (Batched real-time block updates)    │   │
│   └──────────────────────────┬──────────────────────────┘   │
└──────────────────────────────┼──────────────────────────────┘
                               │ WebSocket (TCP Loopback / LAN)
                               │ RFC v3.1 Binary Protocol
                               ▼
┌─────────────────────────────────────────────────────────────┐
│                    libmozitoolkit                           │
│   • mtk-sync: LiveSyncSession & Dispatcher                  │
│   • mtk-voxel: VoxelStorage, Sparse Storage, DeltaMesher   │
│   • mtk-material: Biome SSOT Tint & Dual-Atlas Remapping    │
└──────────────────────────────┬──────────────────────────────┘
                               │ Buffer Protocol / Direct C-DNA
                               ▼
┌─────────────────────────────────────────────────────────────┐
│                 MoziToolKit (Blender Addon)                 │
│   • Interactive 3D Viewport Live Sync & Selection Raycast   │
│   • Zero-copy Native DNA Mesh Generation & Shader Assembly │
└─────────────────────────────────────────────────────────────┘
```

---

## 2. Core Protocol & Parity Standards (RFC v3.1 / v3.2)

`yefira-rs` strictly implements the RFC v3.1 protocol specification, guaranteeing 100% wire-compatibility with `mtk-sync`:

### Binary Protocol Framing
- **Header Magic**: `[0x4D, 0x54]` (`"MT"`)
- **Protocol Version**: `0x03`
- **Supported Opcodes**:
  | Opcode | Packet | Direction | Description |
  | :--- | :--- | :--- | :--- |
  | `0x01` | `HELLO` | C $\to$ S | Client handshake, capabilities, auth token, cached baseline state hash |
  | `0x02` | `WELCOME` | S $\to$ C | Server negotiation decision (`VerifiedSkipRebuild`, `FullBaselineRequired`, `Rejected`) |
  | `0x03` | `BASELINE_BEGIN` | S $\to$ C | Header metadata, total chunks, total compressed/raw bytes |
  | `0x04` | `BASELINE_CHUNK` | S $\to$ C | 128 KiB sliced voxel binary streams |
  | `0x05` | `BASELINE_END` | S $\to$ C | Baseline stream delimiter and finalization barrier |
  | `0x06` | `BASELINE_ACK` | C $\to$ S | Client commit confirmation or validation failure |
  | `0x07` | `DELTA_OP` | S $\to$ C | Monotonic real-time block update batches ($\le$ 16,384 changes) |
  | `0x08` | `PALETTE_UPDATE`| S $\to$ C | Dynamic block state palette additions |
  | `0x09` | `DELTA_ENTITY` | S $\to$ C | Block entity NBT / JSON payload updates and deletions |
  | `0x0A` | `BASELINE_ENTITIES`| S $\to$ C | Initial baseline block entity collection |
  | `0x0E` | `PING` / `PONG` | Both | Round-trip latency check and keep-alive heartbeat |
  | `0x10` | `ERROR` | Both | Graceful error notification |
  | `0x11` | `CANCEL` | C $\to$ S | Abort baseline stream |
  | `0x12` | `RESYNC_REQUEST`| C $\to$ S | Request immediate baseline resync |

### Scanning Traversal & xxHash64 Authoritative Fingerprint
- **Authoritative Traversal**: Slices voxels bottom-up: $Y$ (slowest, bottom-to-top), $Z$ (middle, north-to-south), $X$ (fastest, west-to-east).
- **Exact Hash Parity**: Bit-for-bit parity with `libmozitoolkit::compute_sparse_state_hash`:
  $$\text{xxHash64}(\text{Header} \parallel \sum (\text{rel\_x}, \text{rel\_y}, \text{rel\_z}, \text{state\_id}, \text{biome\_id}, \text{entity\_json}), \text{seed}=0)$$
- **Air Normalization**: `minecraft:air`, `cave_air`, `void_air`, and `structure_void` are normalized to canonical `minecraft:air` at palette index `0` and omitted from sparse state hashing.

---

## 3. In-Game & Console Commands (`/yefira`)

`yefira-rs` provides full selection and server control commands matching Java Yefira:

| Command | Arguments | Description |
| :--- | :--- | :--- |
| `/yefira pos1` | `[x y z]` | Sets selection first corner (or uses player position). |
| `/yefira pos2` | `[x y z]` | Sets selection second corner and confirms box. |
| `/yefira box` | `[size_x [size_y size_z]]` | Creates a cuboid preset box centered around/above the player. |
| `/yefira expand` | `<up\|down\|north\|south\|east\|west> [amount]` | Expands the selection boundary outward in the given direction. |
| `/yefira shift` | `<up\|down\|north\|south\|east\|west> [amount]` | Translates the entire selection box by the specified offset. |
| `/yefira clear` | - | Clears the active selection box in Minecraft. |
| `/yefira refresh` | - | Forces a full baseline rescan and streams it to all clients. |
| `/yefira status` | - | Displays active box dimensions, voxel volume, server state, and clients. |
| `/yefira server` | `<start\|stop\|restart>` | Controls the WebSocket server lifecycle. |

---

## 4. Horizon Mod Hooks Integration

`yefira-rs` interfaces seamlessly with Horizon's modding ecosystem (`horizon-mixins` & `horizon_client::mod_api`):

```rust
use yefira_rs::horizon::YefiraMod;
use yefira_rs::config::YefiraConfig;

// Initialize Yefira in Horizon
let yefira = YefiraMod::new(YefiraConfig::default());

// Bind Horizon World Provider
yefira.attach_world_provider(horizon_world_provider);

// Server Lifecycle Hooks
yefira.on_server_started();
yefira.on_server_stopping();

// Block Change Hook (fired on authoritative Level.setBlock)
yefira.on_block_changed(dimension, [x, y, z], old_state, new_state);

// Server Tick Hook (flushes batched DELTA_OPs)
yefira.on_server_tick();

// In-Game Command Dispatch Hook
if let Some(output) = yefira.handle_chat_command("/yefira pos1 0 64 0", player_pos, dimension) {
    println!("{}", output.message());
}
```

---

## 5. Automated Verification & Quality Gates

The test suite covers wire compatibility, exact hash parity, math stability, and full live sync closed-loop execution:

```bash
# Run unit & integration test suites
cargo test

# Run strict clippy analysis (zero warnings)
cargo clippy --all-targets -- -D warnings

# Check code formatting
cargo fmt -- --check
```

### Verified Test Cases:
1. `test_live_sync_closed_loop_with_libmozitoolkit`: Live handshake $\to$ baseline stream $\to$ verified commit $\to$ real-time `DELTA_OP` block update $\to$ verified in `mtk_sync::VoxelStorage`.
2. `test_scanner_state_hash_parity_with_libmozitoolkit`: Exact xxHash64 bit-for-bit parity against `libmozitoolkit::compute_sparse_state_hash`.
3. `test_cross_interoperability_with_libmozitoolkit_mtk_sync`: Binary packet decode/encode cross-compatibility with `mtk-sync`.
4. `test_selection_math`: Bound canonicalization, coordinate safety, preset box, expand/shift operators, and JSON file persistence.
5. `test_horizon_command_dispatch`: Subcommand execution and chat input parsing.

---

## 6. Packaging Pipeline & Client Installation (`.hnmod`)

`yefira-rs` provides an automated packaging pipeline that compiles and bundles the mod into an official, self-contained Horizon Native Mod package (`dist/yefira-<version>.hnmod`).

### Run Packaging Pipeline
```bash
python3 tools/package_hnmod.py
```

This pipeline automatically:
1. Detects the local/target Horizon and mixins Git revisions.
2. Constructs the standard `mixins/yefira/` bundle (including the `api/` Trait Mod bridge and standalone `core/` live sync engine).
3. Generates the top-level `horizon-native.json` manifest conforming to schema 1.
4. Produces a bit-perfect, deterministic Zip archive (`dist/yefira-0.1.0.hnmod`).
5. Invokes Horizon's official mod checker (`cargo horizon-mod check`) to verify 100% compliance.

### Installation into Horizon Game Client
- **Option 1: Drag & Drop (GUI)**:
  Open **Horizon Launcher** $\to$ Select Instance $\to$ **Mods** tab $\to$ Drag and drop `dist/yefira-0.1.0.hnmod` into the window $\to$ Click "Trust & Install".
- **Option 2: CLI Native Build**:
  ```bash
  horizon-launcher native-build bundle/ --instance <InstanceName> --trust-native-code
  ```

