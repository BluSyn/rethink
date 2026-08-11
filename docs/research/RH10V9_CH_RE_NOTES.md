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
| 0–1 | Programmed H:M | Often **frozen** during auto sensor dry (stayed 25 min) |
| 2 | Phase | `00` Off, `01` Initial, **`02` Drying** (heat-pump), `03` Pause, `04` End; also `0x32`/`0x33` on other firmwares |
| 4 | **Live remaining minutes** | Decrements **1:1 with wall-clock minutes** while drying (27→16 over 11 min) |
| 6 | Course | e.g. `0x37` → mapped “Auto / Sensor” |
| 7 | Dry level | 1–5 style (4 = More in capture) |
| 10 | Temp code | 1–5 style (3 = Medium) |
| 17 | Flags | bit0 child lock, bit3 damp-dry (same convention as other laundry) |
| 20 | Tick | +1 about every **6 s** while active |
| 25 | `0x75` | constant in all captures |

**HA mapping:** `remaining_time` prefers `rec[4]` when non-zero, else H:M. `initial_time` = programmed H:M. `progress%` from those two.

## Still open

- Full course name table for Chinese heat-pump SKUs
- Meaning of `0x3e` telemetry bytes (`0092031b06` repeated)
- Confirm dry-level / temp codes against UI labels on a second cycle
- Control TX (start/stop/course) — none in this RX-only capture
