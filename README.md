# rethink

The goal of this project is to de-cloud LG ThinQ-branded appliances, meaning to communicate with them without using the official LG app and cloud service.
The project is developed by reverse engineering various components of the ThinQ ecosystem.

## Status

A working version of `rethink-cloud` is available as a **Rust** binary. This is a service which emulates the cloud part of ThinQ and translates the protocol to HomeAssistant-compatible MQTT.

An optional "bridge" mode is also supported, in which the messages are forwarded to the actual LG ThinQ cloud. This can be used as a reverse-engineering aid, or simply to allow the user to still use the original LG app alongside HomeAssistant.

The following appliances are currently supported in rethink:

- Air Conditioners:
    - 👍 LG DualCool family (Standard 2, Deluxe with and without air purifier, etc.) wall-mounted Air Conditioner IDUs - high level of support. What's missing are mostly some features of higher-end models and more diagnostic coverage,
    - 👍 LW1822HRSM, Smart Window Air Conditioner - mostly working,
    - 👍 LP1022FVSM Portable Air Conditioner - mostly working,
- Fridges:
    - 🫤 LF28H8330S, Standard-Depth 4-Door French Door Refrigerator - preliminary support,
    - 🫤 GSJV70PZTE, LG Side by Side Refrigerator - preliminary support,
    - 🫤 GSB470BASZ, American Style Side by Side Refrigerator - preliminary support,
    - 🫤 GA-B509CMUM - preliminary support,
- Washing Machines:
    - 🫤 (model name unknown) Washing Machine - preliminary support
    - 👍 F2J7HG1W, Washing Machine - mostly working,
    - 🫤 F4WV508S2E, Front-Loading Washing Machine - preliminary support
    - 🫤 F4WV709P1E, Front-Loading Washing Machine - preliminary support
    - 🫤 TW4V9RW9W - preliminary support
    - 👍 F4X7511TWS (VCDWL2QEUK), Front-Load Washing Machine - mostly working
    - 🫤 WT7300CW - preliminary support
    - 👍 WM3900HBA (F3L2CYU__), Front-Load Washing Machine - mostly working
- Dryers:
    - 🫤 DLE7300WE - preliminary support
    - 👍 DLEX3900B (RV13B6BSD_D_US_WIFI), Electric Dryer - mostly working
- WashTowers (combined washer+dryer):
    - 👍 WKEX200HBA (WTL_FXU_BDV_NA_01), WashTower - mostly working
- Dehumidifiers / humidifiers / hood:
    - 👍 DHUM_056905_WW dehumidifier
    - 👍 DHUM_231006_WW Korean dehumidifier (mode/fan tables)
    - 👍 HUM_056905_WW PuriCare humidifying air purifier
    - 👍 STUDIO_HOOD range hood (fan + light)
- Fridges (additional):
    - 👍 2REF12EII_P_2 / GML844-class slim fridge (Pure N Fresh)

The supported appliances can be used "out of the box" with HomeAssistant or another compatible MQTT consumer.  
Appliances not listed above can still be used with the bridge mode, but they will not be translated to MQTT. Contributions are welcome!

Most of the findings from the reverse engineering process are available on the [project wiki](https://github.com/anszom/rethink/wiki) and in-repo under [`docs/research/`](docs/research/) (start with [`THINQ2_RESEARCH.md`](docs/research/THINQ2_RESEARCH.md): protocols, open PRs, TLV catalog notes).

## Build & run (Rust)

Requirements: Rust **1.88+** (Docker image `rust:1.88-bookworm`; `time`/`icu` crates need ≥1.86–1.88), OpenSSL CLI (for CA / device CSR signing).

```bash
# Build all runtime crates
cargo build -p rethink-cloud -p rethink-setup -p rethink-bridge -p rethink-tools

# Run unit tests
cargo test --workspace

# Start rethink-cloud (creates CA certs on first run if missing)
cargo run -p rethink-cloud -- ./config.jsonc

# SoftAP Wi-Fi provisioning (device SoftAP is usually 192.168.120.254)
cargo run -p rethink-setup -- 192.168.120.254 'MySSID' 'MyPassword!'

# Packet tools (against the internal plain MQTT port)
cargo run -p rethink-tools --bin packet-parser -- -message HEX
cargo run -p rethink-tools --bin packet-sender -- localhost:1884 DEVICE-UUID 1 1 2 2 1 501 1

# Reverse-engineering helpers
cargo run -p rethink-tools --bin rethink-capture -- localhost:44401 DEVICE-UUID capture.jsonl
cargo run -p rethink-tools --bin rethink-mcp          # MCP stdio server (see .mcp.json)
cargo run -p rethink-tools --bin lgcloud-monitor -- --state ./state
```

Docker (multi-stage Rust build with dependency-layer caching):

```bash
docker build -t rethink .
docker run --rm -p 443:443 -p 8883:8883 -p 44401:44401 -v rethink-data:/app/data rethink
```

The Dockerfile compiles crates.io dependencies from **manifests only** (a cached layer),
then copies `crates/` + `html/` and rebuilds just the workspace packages. Source-only
edits skip redownloading and recompiling third-party crates.

See [installation instructions](https://github.com/anszom/rethink/wiki/Installing-rethink‐cloud) for network / DNS setup (`rethink.lgthinq.com`, etc.).

### Workspace layout

| Crate | Role |
|-------|------|
| `rethink-util` | Pure codecs (TLV, CRC16, packet framing, MTOSP, JSON splitter) |
| `rethink-core` | Config, HA types, device base classes, ThinQ traits |
| `rethink-devices` | Per-model HA handlers + `modelId` registry |
| `rethink-bridge` | Optional LG-cloud bridge helpers (subprocess util, state) |
| `rethink-cloud` | Main server binary |
| `rethink-setup` | SoftAP provisioning CLI |
| `rethink-tools` | `packet-parser` / `packet-sender` / `rethink-capture` / `rethink-mcp` / `lgcloud-monitor` |

### Adding a new device

1. Implement a handler module under `crates/rethink-devices/src/devices/` that constructs an HA device (typically via `TlvDeviceCore` or `AabbDeviceCore`) and returns `Arc<dyn DeviceHandler>`. Put fixture tests in a `#[cfg(test)]` module **in that same file**.
2. Register **one line** in the `register_devices!` table in `crates/rethink-devices/src/devices/mod.rs` (`module => { "MODEL_ID" | "ALIAS" }`).

No other crates need to change for a new model.

## Management

A web interface is available on a user-defined port (default: **44401**). The panel supports:

- compact **connected-device** list (click a row to focus it)
- integrated **live frame monitor** + inject (to/from device)
- **recent frames** shown immediately (server ring buffer, not only live)
- **TLV decode** side-by-side: click a frame to load/decode it; unknown tags highlighted
- **LLM export** for unknowns (`POST /api/re/export`)
- APIs: `GET /api/devices/{id}/frames`, `POST /api/decode`, `GET /api/tlv/catalog`
- bridge mode login / per-device enable

Health and device list also expose `GET /api/health` and `GET /api/devices`. The old standalone `/monitor` page redirects into the main UI.

## Code (Rust entry points)

- `crates/rethink-setup` — initial Wi-Fi setup from a PC, without the LG app
- `crates/rethink-cloud` — server that replaces LG's cloud service; hosts a simplistic MQTT broker for appliances
- `crates/rethink-tools` — packet tools, capture, MCP RE server, cloud credential check
- `html/` — management UI static assets (HTML/JS) embedded by `rethink-cloud`
- `tools/appliance-simulator` — C++ simulator for the Wi-Fi module UART

### TypeScript / Node deprecation

The former TypeScript application (`cloud/`, `bridge/`, `util/`, `management/`, `package.json`, Node tests) has been **removed**. Runtime, tests, Docker, and CI are Rust-only. Browser-side `html/panel.js` and `html/monitor.js` remain as static front-end for the management UI (not a Node server).

## Notice

LG ThinQ is likely a registered trademark, or whatever, I don't care. The name is used here for identification purposes only. I'm not in any way affiliated with LG.

## Warning

    This program is distributed in the hope that it will be useful,
    but WITHOUT ANY WARRANTY; without even the implied warranty of
    MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
    GNU General Public License for more details.

This means that if your device breaks, you get to fix it yourself or keep both pieces.
