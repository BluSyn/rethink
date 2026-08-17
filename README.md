# rethink (Rust rewrite)

**This repository is a full Rust rewrite of [anszom/rethink](https://github.com/anszom/rethink).**

The original project — design, reverse engineering, protocol documentation, device maps, and the TypeScript/`rethink-cloud` implementation — is the work of [Andrzej Szombierski](https://github.com/anszom) and [the upstream contributors](https://github.com/anszom/rethink/graphs/contributors). This tree reimplements that system in Rust. Credit for the project belongs upstream.

## Use upstream instead

**If you want to run rethink, report a bug, request a device, or send a patch, go to the original repo:**

**→ [https://github.com/anszom/rethink](https://github.com/anszom/rethink)**

That is the maintained project. Installation, wiki, issues, and PRs live there.

This Rust fork is **not actively maintained**. It is a snapshot / experiment:

- Do **not** open issues or PRs here expecting review or merge.
- Device support, bugfixes, and new work should target **upstream**.
- The wiki and protocol notes you actually want are on the [upstream wiki](https://github.com/anszom/rethink/wiki).

If you already have this tree checked out, the rest of this file is only enough to build it.

---

## What rethink does

Rethink de-clouds LG ThinQ appliances: it talks to them without the official LG app/cloud and exposes them as Home Assistant MQTT. An optional **bridge** mode can still forward traffic to LG’s cloud (useful for reverse engineering, or to keep the official app working).

This fork is a **Rust-only** runtime. The former TypeScript/Node application was removed; `html/` remains as static assets for the management UI.

## Status of this fork

A working `rethink-cloud` binary exists and can enroll appliances, serve MQTT discovery to Home Assistant, and run the management UI. Appliance coverage is a subset of what upstream documents, plus some models ported from open upstream PRs. Treat the list below as “what this snapshot compiled,” not a support promise.

Supported in this tree (model IDs in `crates/rethink-devices`):

- **Air conditioners:** RAC / CST DualCool family, WIN window AC, POT portable
- **Fridges:** several `2REF*` / `2RES*` / `2REB*` AABB models
- **Washers / dryers / WashTower:** F_/Y_ UK-EU washers, F3L2 / F3L7, T1789 / T17A1, RV13 BSD/ES/U6, RH10 heat-pump dryer, WTL WashTower, WTDN3 (ThinQ1)
- **Other:** DHUM / HUM, STUDIO_HOOD, H11 dishwasher, VCDWL2QEUK

Unlisted appliances can still be used in **bridge mode** (no HA translation). For new devices or gaps, contribute **upstream**, not here.

Research notes in this repo: [`docs/research/`](docs/research/) (start with [`THINQ2_RESEARCH.md`](docs/research/THINQ2_RESEARCH.md)). Canonical docs: [upstream wiki](https://github.com/anszom/rethink/wiki).

## Build & run (this snapshot)

Requirements: Rust **1.88+**, OpenSSL CLI (CA / device CSR signing).

```bash
cargo build -p rethink-cloud -p rethink-setup -p rethink-bridge -p rethink-tools
cargo test --workspace
cargo run -p rethink-cloud -- ./config.jsonc
cargo run -p rethink-setup -- 192.168.120.254 'MySSID' 'MyPassword!'
```

Docker:

```bash
docker build -t rethink .
docker run --rm -p 443:443 -p 8883:8883 -p 44401:44401 -v rethink-data:/app/data rethink
```

Network / DNS (`rethink.lgthinq.com`, etc.): follow **[upstream installation](https://github.com/anszom/rethink/wiki/Installing-rethink‐cloud)**.

### Workspace

| Crate | Role |
|-------|------|
| `rethink-util` | Codecs (TLV, CRC16, framing, MTOSP) |
| `rethink-core` | Config, HA types, device bases, ThinQ traits |
| `rethink-devices` | Per-model HA handlers + `modelId` registry |
| `rethink-bridge` | Optional LG-cloud bridge helpers |
| `rethink-cloud` | Main server |
| `rethink-setup` | SoftAP provisioning CLI |
| `rethink-tools` | packet-parser / packet-sender / capture / MCP / lgcloud-monitor |

Adding a device in *this* tree (for your own fork only): implement `crates/rethink-devices/src/devices/<module>.rs` with tests in the same file, then one line in the `register_devices!` table in `devices/mod.rs`. Prefer sending that work **upstream**.

Management UI defaults to port **44401**.

## Credits

- **Original project:** [anszom/rethink](https://github.com/anszom/rethink) by Andrzej Szombierski and contributors.
- **This rewrite:** a Rust reimplementation of that work. It would not exist without the upstream reverse engineering and Node/TypeScript codebase.

## Notice

LG ThinQ is used here for identification only. This project is not affiliated with LG.

## Warning

    This program is distributed in the hope that it will be useful,
    but WITHOUT ANY WARRANTY; without even the implied warranty of
    MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
    GNU General Public License for more details.

If your device breaks, you get to fix it yourself or keep both pieces.
