# CST_570004_WW (cassette AC, deviceType 401) RE notes

Alias of `RAC_056905_WW` handler. Values frames use kind **`0xa7`** (same TLV body as wall RAC).

## Your delta capture (2026-08-10)

A and B are **byte-identical** (`hex` equal), Δt **268 ms**. Empty delta is correct — two deliveries of the same values snapshot (keep-alive / re-publish), not a state change. For RE, treat as **one** frame.

### Decoded operating state (wire → meaning)

| Tag | Wire | HA / physical |
|-----|------|----------------|
| 0x1f7 | 1 | Power **ON** |
| 0x1f9 | 1 | Mode **dry** (RAC: 0=cool, 1=dry, 2=fan_only, 4=heat) |
| 0x1fa | 8 | Fan **auto** (common encoding; 2/4/6 = low/med/high) |
| 0x1fd | 50 | Room **25.0 °C** (half-degrees) |
| 0x1fe | 46 | Setpoint **23.0 °C** |
| 0x20e | 1 | **Autodry ON** |
| 0x20f | 0 | Airclean off |
| 0x20d | 0 | Energy save off |
| 0x21a | 0 | Sleep timer off |
| 0x321 | 0 | **Vertical swing off** (handler id 0x321; catalog was mislabeled “filter”) |
| 0x336 | 900 | Humidity **90.0 %** (RAC/CST: wire = RH×10) |
| 0x355 / 0x356 | 1603 / 2400 | Filter used / life (**~66.8 %** used) |
| 0x333–0x335 | 0 | PM1/2.5/10 sensors present, idle/zero |
| 0x21f | 12 | Display brightness level |
| 0x221 | 0 | No error |

### Unknowns on this dump

| Tag | Wire | Reading | Confidence |
|-----|------|---------|------------|
| **0x1fc** | 0 | Next to 0x1fd/0x1fe temp block — often “feel”/outdoor/pipe related on other LG models; **idle 0** here | Low–med |
| **0x205 / 0x206** | 0 | Cluster with wind/vane family; cassette may use later | Low |
| **0x23f** | 0 | Near AQ/PM cluster | Low |
| **0x325** | 0 | Near humidity control | Low |
| **0x353 / 0x354** | 0 | Flags beside PM tags | Low |
| **0x2f2** | **137** | Sits with filter block (0x355/356). Not remaining hours (797). Possible status/code or residual metric — **needs filter-reset / long-run diff** | Med |
| **0x357** | **60** | Filter/maintenance param (hours/days scale?) | Med |
| **0x358** | **32** | Paired with 0x357 | Med |
| **0x271** | 0 | Power-limit / eco flag candidate | Low |
| **0x2b1** | 0 | Event-style channel (DHUM: bucket emptied); AC usually 0 | Med (as “event”) |
| **0x3a7** | **1** | Sticky feature bit **ON** (like a capability echo); toggle experiments needed | Med |

### How to attack remaining unknowns

1. **Same-kind pairs only** (`a70204` ↔ `a70204`) with Δt of a few seconds after **one** LG-app action.
2. For **0x2f2 / 0x357 / 0x358**: capture before/after filter reset and after ~1 h runtime.
3. For **0x1fc**: heat/cool setpoints and pipe/outdoor if the unit exposes them in the app.
4. For **0x3a7**: note which app toggle (display, beep, AI, etc.) flips 1↔0.

### Fixture vs this live frame

Unit-test `CST_VALUES_HEX` is the same **layout** (len 0x4b, same tag set). Live differs in live temps/humidity/mode/fan/filter counters — good sign the map is stable for CST.
