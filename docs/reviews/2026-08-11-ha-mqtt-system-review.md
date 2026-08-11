# Deep code review — HA MQTT / device triggers / cloud bridge

**Scope:** Recent rust-rewrite work (device triggers, dual-publish fix, reconnect race, HA client) plus adjacent system risk.  
**Bar:** Structural quality, bugs, quick optimizations — not rubber-stamp “it works.”  
**Date:** 2026-08-11  
**HEAD:** `3f0171b` (and parents through `d359975` / `9c4a11c`)

---

## Executive summary

The dual-publish fix (`3f0171b`) is correct and necessary. Several **real bugs and design smells** remain around HA MQTT I/O, trigger API redundancy, discovery cleanup, and oversized device modules. Highest-impact work is small and local; file splits of `rac_056905_ww.rs` (1481) / `management.rs` (1067) are medium-term, not blocking.

---

## P0 — Bugs / correctness

### 1. `ha_client`: TLS is a no-op (broken `mqtts://`)

**File:** `crates/rethink-cloud/src/ha_client.rs`

```rust
if use_tls {
    // rumqttc 0.24 uses Transport — leave default TCP for mqtt://
}
```

`parse_mqtt_url` detects `mqtts`/`ssl` and picks port 8883, but **never sets `Transport::tls`**. Compare working pattern in `crates/rethink-bridge/src/thinq2_conn.rs` (`Transport::tls_with_config`).

**Fix:** Wire TLS the same way as the bridge (or at least fail loudly if TLS requested and not configured). Silent TCP to 8883 is a production footgun.

### 2. Nested trigger cleanup may leave live HA stuck

**File:** `crates/rethink-core/src/ha.rs` (`publish_config`)

Omitting `trigger_*` from the retained device document stops **re-registration on restart**, but HA device-discovery docs require a **platform-only stub** to remove an already-active component while HA is running:

```json
"trigger_filter_needs_change": { "platform": "device_automation" }
```

then a second publish with the key omitted.

If any install registered the nested path first (order of retained messages is not guaranteed forever), classic discovery keeps conflict-failing until that nested component is explicitly removed.

**Fix:** Two-step remove for known `trigger_{object_id}` keys when publishing config that has `device_triggers`, then classic-only discovery. Or publish empty retain to clear (only works for classic topics).

### 3. MQTT publish: fire-and-forget + swallowed errors

**File:** `crates/rethink-cloud/src/ha_client.rs`

```rust
tokio::spawn(async move {
    let _ = client.publish(...).await;
});
```

- Publish failures are invisible (no log).
- Ordering across concurrent spawns is not guaranteed (discovery burst + many properties).
- Backpressure: unbounded spawn under reconnect/republish storms.

**Fix (minimal):** log publish errors. **Better:** single mpsc publisher task that serializes publishes (keeps retain order for discovery).

### 4. `do_publish` silent no-op if `publish_fn` unset

**File:** `crates/rethink-core/src/ha.rs`

Early `publish_config` / property publishes before `set_publish_fn` are dropped with no log. Race at startup if a device connects before the HA client task installs the callback.

**Fix:** `tracing::warn` once (or count) when publish_fn is None; optionally queue.

### 5. Availability wipe race (subtle)

**File:** `ha_client` ConnAck + `handle_message` availability wipe

On reconnect: clear `published_availability`, subscribe `+/availability`, then `emit_discovery`. Retained `online` from devices is processed on later polls; discovery marks devices as published **if** they are in `ha_devices`. Devices that reconnect later are fine; devices that published online before clear can briefly get force-offline if retained messages interleave before their re-publish. Worth hardening: only wipe retained online if id not currently in bridge / skip wipe for `online` that we just re-asserted.

---

## P1 — Design / code-judo (high value, small surface)

### 6. `DeviceTriggerDef` API forces triple string duplication

**Everywhere:**

```rust
DeviceTriggerDef::problem("bucket_full", "bucket_full", "bucket_full")
// publish_event(..., "triggers/bucket_full", "bucket_full")
```

Almost all call sites use **identical** object_id / subtype / payload. Constructors should be:

```rust
DeviceTriggerDef::problem(name)           // type=problem, rest=name
DeviceTriggerDef::event(type_, name)      // subtype=payload=object_id=name
```

And a single fire helper:

```rust
// on HaConnection or core:
fn fire_trigger(&self, id: &str, object_id: &str); // topic triggers/{id}, payload = object_id
// or pass &DeviceTriggerDef
```

**Delete** the triple-arg forms at call sites (dhum ×2, rac, rh10, fridges).

### 7. `republish_all` holds `ha_devices` lock across all publishes

**File:** `ha_bridge.rs`

```rust
for d in self.ha_devices.lock().values() {
    d.publish_config();
}
```

Holds the map lock for the entire fan-out (and nested MQTT work). `set_property` / `new_device` block.

**Fix:**

```rust
let devices: Vec<_> = self.ha_devices.lock().values().cloned().collect();
for d in devices { d.publish_config(); }
```

### 8. Classic discovery missing `origin` (optional but useful)

HA recommends `origin` on discovery payloads for logging/troubleshooting. Classic trigger JSON only has `device`. Low risk add:

```json
"origin": { "name": "rethink", "support_url": "..." }
```

### 9. `drop_device` only sets availability offline

Does not clear discovery topics. After permanent remove, HA keeps entities/triggers until empty retained payloads. Acceptable for reconnect, but long-term ghost devices if model unregistered.

**Optional fix:** empty retain on device + classic trigger topics on drop (config-gated).

---

## P2 — Structure / file size / spaghetti

### 10. Files already past 1k lines

| File | Lines | Note |
|------|------:|------|
| `rac_056905_ww.rs` | 1481 | Climate + filter + energy + TLV soup |
| `management.rs` | 1067 | HTTP + WS + frame log + API |
| `dhum_056905_ww.rs` | 943 | Approaching limit |

**Do not grow these further without extract.** Code-judo for RAC: pull `filter_*` (query/publish/trigger) into `rac_filter.rs` or a shared “filter life” helper used by RAC variants. Management: split `frame_log` / `ws` / REST routes.

Not required for trigger correctness; flag as debt.

### 11. `HaMqttSink` is a god object

Config + publish_fn + set handlers + discovery handlers + availability set + connected flag. Works, but testing and lifecycle are awkward (`publish_fn` late-bound). Longer-term: separate `MqttPublisher` + `HaDiscovery` + message router. Don’t big-bang refactor now.

### 12. T2Adapter double path on send

```rust
self.dev.notify_send(...);
(self.dev.send_to_device)(...);
```

Intentional for frame log? Document or factor `send_and_log` so every path is consistent (T1 only calls send_to_device for JSON — asymmetric).

---

## P3 — Quick optimizations / nits

- **`recursive_replace`**: full tree clone on every `publish_config`. Fine at device count scale; if republish storms hurt, replace `$this`/`$deviceid` only on known string fields.
- **`DeviceInfo.identifiers: Value`**: untyped. Prefer `Vec<String>` (or enum) so normalize becomes unnecessary.
- **Fixed MQTT client id `"rethink-cloud"`**: two instances fight. Suffix hostname/random.
- **Birth message:** already handled; keep retain + birth (don’t drop either).
- **Trigger type `"problem"`**: valid free-form; UI shows `filter_needs_change problem`. Prefer HA-friendly pairs where possible (`button_short_press` is wrong semantically; `problem` is fine).

---

## What is already good

- Classic-only trigger discovery after dual-publish diagnosis — right fix.
- Birth + ConnAck `emit_discovery` + retain on discovery.
- Reconnect close via `Arc::ptr_eq` (orphan race) — correct model.
- Edge firing for filter / bucket / cycle / door (not level spam).
- Identifiers forced to array form for registry identity.

---

## Implementer checklist (prioritized)

Do in order; keep behavior, prefer small commits:

1. **[P0]** Fix `ha_client` TLS for `mqtts://` / `ssl://` (mirror bridge).
2. **[P0]** Log MQTT publish errors (and ideally single ordered publisher task).
3. **[P0]** Two-step nested `trigger_*` removal on device discovery publish, then classic triggers only.
4. **[P0/P1]** Warn when `publish_fn` is None on publish.
5. **[P1]** `republish_all`: clone Arcs, drop lock, then publish.
6. **[P1]** Simplify `DeviceTriggerDef` constructors + `fire_trigger` helper; update all call sites.
7. **[P1]** Add `origin` to classic trigger discovery JSON (from device origin or rethink defaults).
8. **[P2 optional]** Extract RAC filter helpers if touching that file anyway.
9. Tests: extend `device_trigger_discovery_and_event_are_published`; add TLS parse/transport unit if feasible; republish lock not unit-testable easily.

**Out of scope for this pass:** full `management.rs` / `rac` 1k-line split, HaMqttSink redesign, drop_device discovery tombstones (unless quick).

---

## Approval

**Not approved as final HA MQTT layer** until P0 items 1–3 are addressed. Dual-publish fix is good; remaining TLS + cleanup + silent publish are real production risks.
