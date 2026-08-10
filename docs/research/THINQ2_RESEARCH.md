# ThinQ2 reverse-engineering research (Rust rewrite era)

Last updated: 2026-08-10. Sources are named with the capability or model they contribute.

## Wire protocols (ThinQ2)

### CLIP / MQTT (`Thinq2CloudProtocol` wiki)
- After SoftAP Wi-Fi join, devices use HTTPS + MQTTS; no inbound device listeners.
- Provisioning: `https://common.lgthinq.com/route` → `apiServer` / `mqttServer` (any TLS cert accepted on route).
- Device cert: CSR to `apiServer/device/{uuid}/certificate`; MQTT client uses mutual TLS (device cert + AWS IoT CA).
- Topics: subscribe `lime/devices/{did}`; provision on `clip/provisioning/devices/{did}` (`preDeploy` / `deploy`); traffic on `clip/message/devices/{did}`.
- CLIP JSON fields: `mid`, `did`, `kind` (modelId), `cmd`, `type`, `data` (hex for UART payloads).
- **Cmds of interest:** `preDeploy`, `deploy`, `completeProvisioning` / `_ack`, `device_packet` (device→cloud UART), `packet` (cloud→device), `req_timesync`.

### UART TLV framing (`TLVProtocol` wiki + `rethink-util`)
- MQTT wraps UART frames: prefix `a`/`s` reliability bytes (toDevice) or `0000` (fromDevice).
- UART: `04 00 00 00` + kind (`65` toDevice, `87`/`a7` fromDevice) + byte5/6/7 + len + TLV + CRC16-XMODEM over UART body.
- Kind **`0xa7`**: CST cassette / some DHUM platforms (same TLV body; codec must accept it).
- TLV: 10-bit type, 2-bit length 0–3, nibble or multi-byte value (`rethink_util::tlv`).
- **Query tags:** `0x1f5=1` capabilities, `0x1f5=2` values (clip “values query”).

### Common TLV tags (AC / climate / humidity family)
| Tag | Meaning | Sources |
|-----|---------|---------|
| 0x1f5 | Query type (1=caps, 2=values) | wiki, RAC/DHUM/HUM |
| 0x1f7 | Power | RAC, DHUM, HUM, WIN, POT |
| 0x1f9 | Operating mode | RAC, DHUM, HUM |
| 0x1fa | Fan / wind strength | RAC, DHUM, HUM |
| 0x1fd | Current temp (½°C) | RAC, HUM |
| 0x1fe | Setpoint temp | RAC, WIN, POT |
| 0x253 | Target humidity % | DHUM, HUM |
| 0x336 | Current humidity | CST PR#129, DHUM, HUM |
| 0x2da | EEPROM checksum (caps marker) | tlv_device / RAC |
| 0x2cc–0x2d3 | Feature bits (jet, energy save, timers, swing) | RAC |
| 0x20e/0x20f/0x20d | Autodry / airclean / related | RAC CST unlock from state tags |
| 0x1e3 | Product status (HUM) | PR#114 modelJSON |
| 0x1e4 / 0x1e6 / 0x109 | Sleep / auto / night (HUM) | PR#114 |

### AABB framing (`AABBProtocol` wiki + laundry/fridge/hood)
- Envelope: `AA` + length + body + checksum `((sum)&0xff)^0x55` + `BB`.
- Laundry status often kind `0x20` EB/EC; dryers `0x30`; fridges `0x10` EB/EC; hood `0x43` EB/EC.
- Status query / monitor enable: `F0ED…` patterns (model-specific).
- Set commands: `F017…` (fridge), `F0xx…` (washer/dryer/hood).

### SoftAP / setup (`SetupProtocol:JSON` wiki)
- SoftAP IP typically `192.168.120.254`; JSON length-prefixed frames and XML variants.
- Newer modules: RTK_RTL8711am / RTL8720cm / BEKEN_BK7234 — need relaxed TLS ciphers and SoftAP setApInfo hardening (PR#131, #81, #92).

## Local branches (this clone)

| Branch | Contribution |
|--------|----------------|
| `custom-ac` / `origin/custom-ac` | AC customizations; F_VA washer map; SoftAP/TLS hardening already on tip |
| `origin/dehumidifier` / `new-device-dehumid` | DHUM_056905_WW HA support (already ported to Rust) |
| `rust-rewrite` | Full Rust port (this work) |

## Open GitHub PRs (anszom/rethink, 2026-08) — device / RE relevance

| PR | Title | Contribution | Rust status |
|----|-------|--------------|-------------|
| #131 | SoftAP + legacy TLS for RTK_RTL8711am | Provisioning + weak ciphers | Partially present in setup/TLS work |
| #130 | RH10V9_CH heat-pump dryer | AABB dryer status | **Registered** |
| #129 | CST_570004_WW cassette AC | RAC alias + 0xa7 + humidity 0x336 | **Registered** (alias + a7 codec) |
| #128 | F_VA_F___W.B__QEUK washer | Alias → F_V__F handler | **Registered** |
| #127 | Home Assistant add-on | Packaging | Docs only (out of runtime) |
| #123 | F_V7_Y___W.B__QEUK washer | Alias → F_V8_Y | **Registered** as F_V7_Y___W.B_2QEUK |
| #122 | ac_common base | Shared AC TLV refactor | Research; RAC still monolithic |
| #121 | Dashboard UI | Names, nicer panel | **This work: management UI** |
| #120 | STUDIO_HOOD | AABB fan+light hood | **Registered** (`studio_hood`) |
| #118 | 1WPU4CIGCR__2 water purifier | AABB capability API | Documented; deferred (large) |
| #117 | Korean WashTower FAKPK21021 / BDH_D39301_KR | Extended EB/EC | Documented; deferred (3k+ LOC) |
| #116 | HWWA9K_F2 CordZero vacuum | AABB stick vacuum | Documented; deferred |
| #115 | ST_B_E4H01Y_APL Styler | AABB garment care | Documented; deferred |
| #114 | HUM_056905_WW humidifier | TLV modelJSON map | **Registered** (`hum_056905_ww`) |
| #113 | DHUM_056905_WW + **DHUM_231006_WW** | Dehumidifier + KR mode tables | **Both registered** |
| #96 | 2REF12EII_P_2 / GML844 fridge | AABB slim fridge | **Registered** (`dev_2ref12eii_p_2`) |
| #92 / #81 | RTL8720cm SoftAP | Provisioning | Setup path |
| #73 / #89 | WIN_056905_WW window AC | TLV climate | **Registered** |
| #64 | DHUM_056905_WW | Dehumidifier | **Registered** |

## External reverse-engineering sources

| Source | What it contributes |
|--------|---------------------|
| [rethink wiki](https://github.com/anszom/rethink/wiki) | TLV/AABB/CLIP SoftAP specs; per-appliance notes (RAC, WIN, washers, fridges, WHT_056905_WW WashTower family) |
| [rvanbaalen/lg-local](https://github.com/rvanbaalen/lg-local) | Independent local control; TLV parser + CRC16; points back to rethink wiki as protocol authority |
| LG modelJSON (`tlv_*` labels) | Authoritative tag→field map for HUM_056905_WW (PR#114); used instead of packet guessing |
| Bridge dual-decode | PR#113 verifies DHUM fields against LG cloud while bridged |

## DeviceType quick reference

Management UI shows **name + code** (e.g. `A/C (401)`). Map lives in `html/panel.js` (`DEVICE_TYPE_NAMES`; wideq + rethink).

| deviceType | Class | Examples |
|------------|-------|----------|
| 101 | Refrigerator | 2REF*, GML844 / 2REF12EII_P_2 |
| 102 | Kimchi Refrigerator | |
| 103 | Water purifier | 1WPU4CIGCR__2 |
| 201 | Washer | F_V*, VCDWL2QEUK, WTDN3 |
| 202 | Dryer | RV13*, RH10V9_CH |
| 203 | Styler | ST_B_E4H01Y_APL |
| 204 | Dishwasher | |
| 221/222/223 | WashTower | FAKPK21021 / BDH_D39301_KR / WHT_* |
| 301 | Oven / Range | |
| 302 | Microwave | |
| 303 | Cooktop | |
| 304 | Range Hood | STUDIO_HOOD |
| 401 | A/C | RAC, WIN, CST, POT |
| 402 | Air Purifier | |
| 403 | Dehumidifier | DHUM_* |
| 404 | Humidifier | HUM_056905_WW |
| 501 | Robot Vacuum | |
| 504 | Vacuum | HWWA9K_F2 |

## Adding support (Rust)

1. Implement handler under `crates/rethink-devices/src/devices/`.
2. One registry line in `registry.rs` (`t2_factory` / `t1_factory`) + `all_t*_model_ids`.
3. Fixture tests from PR captures or live `monitor` LLM export.
4. See wiki *Adding-support-for-a-new-device* and management RE panel.

## Management RE tooling (this work)

- Device list + detail (meta, HA properties, platform).
- Server-side TLV/AABB decode (`/api/decode`).
- Unknown-tag classification vs known catalog (`/api/re/unknowns`).
- One-click LLM-ready export text for unknown traffic.
