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

The supported appliances can be used "out of the box" with HomeAssistant or another compatible MQTT consumer.  
Appliances not listed above can still be used with the bridge mode, but they will not be translated to MQTT. Contributions are welcome!

Most of the findings from the reverse engineering process are available on the [project wiki](https://github.com/anszom/rethink/wiki) as well.

## Build & run (Rust)

Requirements: Rust 1.80+ (edition 2021), OpenSSL CLI (for CA / device CSR signing).

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
```

Docker (multi-stage Rust build):

```bash
docker build -t rethink .
docker run --rm -p 443:443 -p 8883:8883 -p 44401:44401 -v rethink-data:/app/data rethink
```

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
| `rethink-tools` | `packet-parser` / `packet-sender` |

### Adding a new device

1. Implement a handler module under `crates/rethink-devices/src/devices/` that constructs an HA device (typically via `TlvDeviceCore` or `AabbDeviceCore`) and returns `Arc<dyn DeviceHandler>`.
2. Register **one line** in `crates/rethink-devices/src/registry.rs` (`t1_factory` or `t2_factory`) mapping the appliance `modelId` to your `create` factory.
3. Add a unit test under the same module (or `tests/`) using `MockHaConnection` + `MockThinq2Device` / `MockThinq1Device`.

No other crates need to change for a new model.

## Management

A simple web interface is available on a user-defined port (default: 44401). The interface supports:

- listing the devices connected to rethink
- monitoring their communications (with packet injection)
- configuring the bridge mode

## Code (Rust entry points)

- `crates/rethink-setup` — initial Wi-Fi setup from a PC, without the LG app
- `crates/rethink-cloud` — server that replaces LG's cloud service; hosts a simplistic MQTT broker for appliances
- `crates/rethink-tools` — `packet-parser` / `packet-sender` utilities
- `tools/appliance-simulator` — C++ simulator for the Wi-Fi module UART (unchanged)

The historical TypeScript sources remain in the tree for reference during the port; **Rust is the supported build/run path**.

## Notice

LG ThinQ is likely a registered trademark, or whatever, I don't care. The name is used here for identification purposes only. I'm not in any way affiliated with LG.

## Warning

    This program is distributed in the hope that it will be useful,
    but WITHOUT ANY WARRANTY; without even the implied warranty of
    MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
    GNU General Public License for more details.

This means that if your device breaks, you get to fix it yourself or keep both pieces.
