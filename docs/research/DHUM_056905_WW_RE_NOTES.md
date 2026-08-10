# DHUM_056905_WW frame RE notes

Captured 2026-08-10 via management UI (deviceType **403**).

## Frame A — climate **values** (good TLV)

```
kind=0xa7  b5=0x02  b6=0x04  (fromDevice values dialect)
hex prefix: 000004000000a70204…
```

This is a normal ThinQ2 climate **values** push. Known tags match the shipped handler.

### High-confidence unknowns → meanings

| Tag | Sample | Interpretation | Confidence |
|-----|--------|----------------|------------|
| **0x2d7 / 0x2d8 / 0x2d9** | triples `(17,0,2)…(22,0,2)` | **Per-mode fan memory table**: mode, pad, fan. Same triple the Rust handler *writes* on fan change and *ignores* on read (`key_value_intercept`). Modes 17–22 = Smart/Jet/Silent/Spot/Laundry/+extra. Fan 2 = Low. | **High** (code + wire) |
| **0x21c** | 0 | **Turn-on timer** (hours), 0=off. RAC maps 0x21c=`starttimer` next to 0x21b=`stoptimer`/`off_timer`. | **High** (RAC parity) |
| **0x21b** | 0 | Off/sleep timer (already catalogued) | Known |
| **0x226** | 0 | Timer/schedule related flag in the 0x21b–0x226 block; always 0 here | Medium |
| **0x232** | 7282 | Cumulative **usage counter** (runtime minutes ≈121 h, or Wh-scale energy). Needs before/after run to confirm unit. | Medium |
| **0x233** | 26 | Small aux reading (not ambient: ambient is 0x1fd=54 → 27.0 °C). Could be outdoor/coil/offset. | Low–medium |
| **0x2ac** | 0 | Near 0x2a2 (UVnano); filter/sensor flag | Low–medium |
| **0x324 / 0x33a** | 0 | Beside humidity tags 0x336; feature/echo flags | Low |

### Mode table on the wire (from 0x2d7×6)

| 0x2d7 mode | HA label (056905) | 0x2d9 fan |
|------------|-------------------|-----------|
| 17 | Smart | 2 (Low) |
| 18 | Jet | 2 |
| 19 | Silent | 2 |
| 20 | Spot | 2 |
| 21 | Laundry | 2 (panel would force High=6 on write) |
| 22 | (extra / unused on HA list) | 2 |

### How to confirm remaining counters

1. Note 0x232 before a long run; read again — if Δ ≈ minutes elapsed → runtime; if Δ tracks compressor load → energy.
2. Toggle **on timer** in LG app while bridged → expect 0x21c change (not only 0x21b).
3. Diff two **values** frames (same kind 0xa7) minutes apart — not a values vs binary mix.

---

## Frame B — **not** climate TLV

```
kind=0xa8  b5=0x66  b6=0x10  b7=0x01  body_len=73
CRC valid (XMODEM over UART body).
```

| Field | Meaning |
|-------|---------|
| kind **0xa8** | Not 0x87/0xa7/0x65 climate path |
| b5 **0x66** | In wiki “SUPERSET” band (0x03–0x66), not values b5=0x02 |
| b6 **0x10** | On 0x87 path wiki calls this **ACK-like**; here with 0xa8 it is a **binary/private payload** class |
| Body | Fixed-layout blob (sensor/log/filter?), **not** 10-bit TLV |

The management UI previously fell through to **TlvRaw** and invented tags like `0x000`, `0x004` — **decode noise**. Treat as `UartBinary`.

Body contains byte pairs that *look* like humidity/temp (`0x36 0x50` ≈ half-°C 54 and RH 80) but **without TLV framing**; do not map them as tags until the private layout is documented.

Δt to frame A was **~23 minutes** — not a tight state transition; do not use that delta to claim A’s unknowns “became” B’s garbage tags.

---

## Query exchange (caps then values) — 2026-08-10 capture

Typical thinq2 poll (~1.9 s) observed on device `fb4349b1-…`:

| Step | Dir | Frame | Notes |
|------|-----|-------|--------|
| 1–2 | TX | `0x65` `0x1f5=1` (dup) | **query_type=1** → capability / feature table |
| 3 | RX | `0xa7` b6=`0x01` len=104 | **Caps** reply (`0x3e8=caps_marker_hum`, feature bits, fan table 17–21) |
| 4–5 | TX | `0x65` `0x1f5=2` (dup) | **query_type=2** → values |
| 6–7 | RX | same caps body as #3 | Retries / late caps (b7 sequence advances; TLV tags identical) |
| 8 | RX | `0x87` b6=`0x10` len=0 | Empty **ACK** |
| 9 | RX | `0xa7` b6=`0x04` len=88 | **Values**: power=1, mode=17 (Smart), fan=2, target RH=30, ambient 33.0 °C (`0x1fd=66`), `0x336=60` (6.0% or wrong scale — recheck), `0x232=7974` usage |

Duplicate TX pairs are common (UI/bridge double-send). Caps replies with identical tags but different CRC/b7 are not state changes.

---

## RE workflow tips (this model)

1. Prefer pairs of **same kind** (both `a70204…` values, or two binary frames).
2. When protocol is `UartBinary`, use hex dump / struct layout RE, not tag catalog.
3. For fan table, only one triple should change when you set fan in a given mode (write path); all six appear on full values dumps.
4. `0x1f5=1` → expect caps (`b6=0x01`); `0x1f5=2` → expect values (`b6=0x04`).
