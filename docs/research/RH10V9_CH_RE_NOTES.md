# RH10V9_CH heat-pump dryer — reverse-engineering notes

Model SoftAP id **RH10V9_CH**, deviceType **202** (Dryer). AABB family kind **0x30**.

## Monitor enable

Host must send laundry-style monitor enable or the module only MQTT-pings:

```
F0ED1121010000001800  (AABB-wrapped)
```

Handler sends on start + up to 8 retries @ 15s.

## Status frames

| Subtype | Layout |
|---------|--------|
| `0xEB` | single 27-byte record |
| `0xEC` | prev 27B + **cur** 27B (publish cur) |
| `0x31` | identity noise — ignore |
| `0x3e` | 5-byte telemetry after `30 3e` — diagnostic only |

## 27-byte record (live 2026-08-11 run)

Capture: ~11 min drying, 119× EC frames. Wall-clock validated:

| Off | Field | Notes |
|-----|--------|--------|
| 0–1 | Programmed H:M | Often **frozen** (stayed 25 min) even when live remaining is higher |
| 2 | Phase | `00` Off, `01` Initial, **`02` Drying** (heat-pump), `03` Pause, `04` End; also `0x32`/`0x33` on other firmwares |
| 3 | Option A | Session-constant (1 vs 0 across two runs) — diagnostic |
| 4 | **Live remaining minutes** | Decrements **1:1 with wall-clock minutes** while drying |
| 6 | Course | e.g. `0x37` → mapped “Auto / Sensor” |
| 7 | Dry level | 1–5 style (4 = More in capture) |
| 10 | Temp code | 1–5 style (3 = Medium) |
| 17 | Flags | bit0 child lock, bit3 damp-dry (same convention as other laundry) |
| 19 | Option B | Session-constant (3 vs 4); **mirrors `0x3e[2]`** — diagnostic |
| 20 | Tick | +1 about every **6 s** while active |
| 25 | `0x75` | constant in all captures |

**HA mapping:**

- `remaining_time` = `rec[4]` when non-zero, else H:M
- `initial_time` = programmed H:M (`rec[0..1]`)
- `cycle_baseline` = max remaining seen this cycle (and programmed)
- `progress%` = `(baseline − remaining) / baseline` (handles extended remaining)
- `option_a` / `option_b` = `rec[3]` / `rec[19]` raw
- `0x3e`: hex + `telemetry_u16` (BE u16) + `telemetry_opt` (= option B echo) — **not** proven Wh/W

## Still open

- Full course name table for Chinese heat-pump SKUs
- Prove or disprove `0x3e` u16 as energy over a full cycle
- Confirm dry-level / temp / option A–B labels against panel UI
- Control TX (start/stop/course) — none in RX-only captures
