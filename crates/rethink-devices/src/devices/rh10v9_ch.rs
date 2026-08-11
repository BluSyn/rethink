//! RH10V9_CH heat-pump dryer (AABB, kind 0x30).
//!
//! Status record is 27 bytes (no 0x1b marker used by some NA RV13 models).
//! Live 2026-08-11 capture while running refined the field map:
//!
//! | Offset | Meaning |
//! |--------|---------|
//! | 0–1    | Programmed / frozen H:M estimate (often static during auto dry) |
//! | 2      | Phase (0 Off, 1 Initial, **2 Drying**, 3 Pause, 4 End, 0x32/0x33 alt) |
//! | 3      | (seen 0x01 while running) |
//! | 4      | **Live remaining minutes** (decrements ~1/min wall-clock while drying) |
//! | 6      | Course code |
//! | 7      | Dry level (1–5 style) |
//! | 10–11  | Temperature / option codes |
//! | 17     | Flags bitfield |
//! | 20     | ~6 s tick counter while active |
//! | 21     | Diagnostic |
//! | 25     | Constant 0x75 in captures |
//!
//! Dual EC frames carry prev+cur; we publish **cur**. Subtype 0x3e is a short
//! telemetry burst (energy-ish) published as diagnostics.

use crate::device_trait::DeviceHandler;
use rethink_core::device_base::{default_config, AabbDeviceCore};
use rethink_core::hex_decode;
use rethink_core::{HaConnection, Metadata, Thinq2Device};
use serde_json::{json, Map};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use rethink_util::sync::Mutex;

const RECORD_LEN: usize = 27;

/// Interval for monitor-enable retries (ms). Tests may lower this.
pub static MONITOR_INTERVAL_MS: AtomicU64 = AtomicU64::new(15_000);

fn status_map() -> HashMap<u8, &'static str> {
    HashMap::from([
        (0x00, "Off"),
        (0x01, "Initial"),
        // Heat-pump RH10 uses 0x02 for active drying in live captures (not 0x32).
        (0x02, "Drying"),
        (0x03, "Pause"),
        (0x04, "End"),
        (0x32, "Drying"),
        (0x33, "Cooling"),
    ])
}

fn course_map() -> HashMap<u8, &'static str> {
    // Sparse map; unknown codes published as "Course 0xNN".
    HashMap::from([
        (0x01, "Cotton"),
        (0x02, "Mixed"),
        (0x03, "Easy Care"),
        (0x04, "Delicates"),
        (0x05, "Wool"),
        (0x06, "Sportswear"),
        (0x07, "Duvet"),
        (0x08, "Quick"),
        (0x09, "Time Dry"),
        (0x0a, "Air Dry"),
        (0x0b, "Eco"),
        (0x10, "Speed Dry"),
        (0x12, "Time Dry"),
        (0x37, "Auto / Sensor"), // seen in heat-pump run capture
    ])
}

fn dry_level_map() -> HashMap<u8, &'static str> {
    HashMap::from([
        (0, "—"),
        (1, "Damp"),
        (2, "Less"),
        (3, "Normal"),
        (4, "More"),
        (5, "Very"),
    ])
}

fn temp_map() -> HashMap<u8, &'static str> {
    HashMap::from([
        (0, "—"),
        (1, "Low"),
        (2, "Medium Low"),
        (3, "Medium"),
        (4, "Medium High"),
        (5, "High"),
    ])
}

pub struct Device {
    core: Arc<AabbDeviceCore>,
    stop: Arc<AtomicBool>,
    monitor_thread: Mutex<Option<thread::JoinHandle<()>>>,
    /// Last status phase (for cycle-complete edge).
    last_phase: Mutex<Option<u8>>,
}

impl Device {
    pub fn new(ha: Arc<dyn HaConnection>, thinq: Arc<dyn Thinq2Device>, meta: Metadata) -> Arc<Self> {
        let core = AabbDeviceCore::new(ha, thinq.clone());
        let this = Arc::new(Self {
            core: core.clone(),
            stop: Arc::new(AtomicBool::new(false)),
            monitor_thread: Mutex::new(None),
            last_phase: Mutex::new(None),
        });

        let mut base = default_config(&meta, Some(json!({"name": "LG Dryer"})));
        let mut components = Map::new();
        let comps = [
            (
                "power",
                json!({
                    "platform": "binary_sensor",
                    "unique_id": "$deviceid-power",
                    "state_topic": "$this/power",
                    "name": "Running",
                    "icon": "mdi:tumble-dryer",
                    "device_class": "running",
                }),
            ),
            (
                "status",
                json!({
                    "platform": "sensor",
                    "unique_id": "$deviceid-status",
                    "state_topic": "$this/status",
                    "name": "Status",
                    "icon": "mdi:state-machine",
                }),
            ),
            (
                "course",
                json!({
                    "platform": "sensor",
                    "unique_id": "$deviceid-course",
                    "state_topic": "$this/course",
                    "name": "Course",
                    "icon": "mdi:pin-outline",
                }),
            ),
            (
                "remaining_time",
                json!({
                    "platform": "sensor",
                    "unique_id": "$deviceid-remaining_time",
                    "state_topic": "$this/remaining_time",
                    "name": "Remaining time",
                    "icon": "mdi:timer-outline",
                    "device_class": "duration",
                    "unit_of_measurement": "min",
                }),
            ),
            (
                "initial_time",
                json!({
                    "platform": "sensor",
                    "unique_id": "$deviceid-initial_time",
                    "state_topic": "$this/initial_time",
                    "name": "Programmed time",
                    "icon": "mdi:clock-outline",
                    "device_class": "duration",
                    "unit_of_measurement": "min",
                    "entity_category": "diagnostic",
                }),
            ),
            (
                "progress",
                json!({
                    "platform": "sensor",
                    "unique_id": "$deviceid-progress",
                    "state_topic": "$this/progress",
                    "name": "Progress",
                    "icon": "mdi:progress-clock",
                    "unit_of_measurement": "%",
                    "state_class": "measurement",
                }),
            ),
            (
                "dry_level",
                json!({
                    "platform": "sensor",
                    "unique_id": "$deviceid-dry_level",
                    "state_topic": "$this/dry_level",
                    "name": "Dry level",
                    "icon": "mdi:water-percent",
                }),
            ),
            (
                "temp",
                json!({
                    "platform": "sensor",
                    "unique_id": "$deviceid-temp",
                    "state_topic": "$this/temp",
                    "name": "Temperature",
                    "icon": "mdi:thermometer",
                }),
            ),
            (
                "child_lock",
                json!({
                    "platform": "binary_sensor",
                    "unique_id": "$deviceid-child_lock",
                    "state_topic": "$this/child_lock",
                    "name": "Child lock",
                    "icon": "mdi:lock",
                    "entity_category": "diagnostic",
                    "payload_on": "ON",
                    "payload_off": "OFF",
                }),
            ),
            (
                "damp_dry_signal",
                json!({
                    "platform": "binary_sensor",
                    "unique_id": "$deviceid-damp_dry_signal",
                    "state_topic": "$this/damp_dry_signal",
                    "name": "Damp dry signal",
                    "icon": "mdi:water-alert-outline",
                    "payload_on": "ON",
                    "payload_off": "OFF",
                }),
            ),
            (
                "flags",
                json!({
                    "platform": "sensor",
                    "unique_id": "$deviceid-flags",
                    "state_topic": "$this/flags",
                    "name": "Flags (raw)",
                    "icon": "mdi:flag",
                    "entity_category": "diagnostic",
                }),
            ),
            (
                "tick",
                json!({
                    "platform": "sensor",
                    "unique_id": "$deviceid-tick",
                    "state_topic": "$this/tick",
                    "name": "Activity tick",
                    "icon": "mdi:counter",
                    "entity_category": "diagnostic",
                    "state_class": "total_increasing",
                }),
            ),
            (
                "telemetry",
                json!({
                    "platform": "sensor",
                    "unique_id": "$deviceid-telemetry",
                    "state_topic": "$this/telemetry",
                    "name": "Telemetry (0x3e)",
                    "icon": "mdi:sine-wave",
                    "entity_category": "diagnostic",
                }),
            ),
        ];
        for (k, v) in comps {
            components.insert(k.into(), v);
        }
        base.components = components.into_iter().collect();
        let (k, v) = rethink_core::notification_event(
            "cycle_complete",
            "Cycle complete",
            &["cycle_complete"],
            None,
        );
        base.components.insert(k, v);
        core.set_config(base);

        let t = this.clone();
        thinq.on_data(Box::new(move |data| {
            if let Some(inner) = t.core.process_data_envelope(data) {
                t.process_aabb(&inner);
            }
        }));
        this
    }

    fn send_monitor_enable(core: &AabbDeviceCore) {
        tracing::debug!("RH10V9: monitor enable");
        core.send(&hex_decode("F0ED1121010000001800"));
    }

    fn process_record(&self, rec: &[u8]) {
        if rec.len() < RECORD_LEN {
            return;
        }
        let phase = rec[2];
        let programmed = rec[0] as i64 * 60 + rec[1] as i64;
        // Live countdown lives at rec[4] while drying (1 min wall-clock ≈ 1 unit).
        // When zero (idle/initial), fall back to H:M at rec[0..1].
        let remaining = if rec[4] > 0 {
            rec[4] as i64
        } else {
            programmed
        };
        let course = rec[6];
        let dry_level = rec[7];
        let temp = rec[10];
        let flags = rec[17];
        let tick = rec[20] as i64;
        let status = status_map()
            .get(&phase)
            .map(|s| (*s).to_string())
            .unwrap_or_else(|| format!("0x{phase:02x}"));
        let course_s = course_map()
            .get(&course)
            .map(|s| (*s).to_string())
            .unwrap_or_else(|| format!("Course 0x{course:02x}"));
        let dry_s = dry_level_map()
            .get(&dry_level)
            .map(|s| (*s).to_string())
            .unwrap_or_else(|| format!("0x{dry_level:02x}"));
        let temp_s = temp_map()
            .get(&temp)
            .map(|s| (*s).to_string())
            .unwrap_or_else(|| format!("0x{temp:02x}"));

        let progress = if programmed > 0 && remaining <= programmed {
            (((programmed - remaining) as f64 / programmed as f64) * 100.0).clamp(0.0, 100.0) as i64
        } else if phase == 0x04 {
            100
        } else {
            0
        };

        let prev_phase = *self.last_phase.lock();
        *self.last_phase.lock() = Some(phase);

        let running = phase != 0x00;
        self.core
            .publish_property("power", if running { "ON" } else { "OFF" }.into());
        self.core.publish_property("status", status.into());
        self.core.publish_property("course", course_s.into());
        self.core
            .publish_property("remaining_time", remaining.into());
        self.core
            .publish_property("initial_time", programmed.into());
        self.core.publish_property("progress", progress.into());
        self.core.publish_property("dry_level", dry_s.into());
        self.core.publish_property("temp", temp_s.into());
        self.core
            .publish_property("flags", (flags as i64).into());
        self.core.publish_property("tick", tick.into());
        // Flag bits (aligned with other LG laundry layouts where known).
        self.core.publish_property(
            "child_lock",
            if flags & 0x01 != 0 { "ON" } else { "OFF" }.into(),
        );
        self.core.publish_property(
            "damp_dry_signal",
            if flags & 0x08 != 0 { "ON" } else { "OFF" }.into(),
        );

        if phase == 0x04 && prev_phase != Some(0x04) {
            self.core.ha.fire_notification_event(
                &self.core.id,
                "cycle_complete",
                "cycle_complete",
            );
        }
    }

    /// Short 0x3e telemetry (7-byte body after kind): publish hex for RE / diagnostics.
    fn process_telemetry(&self, body: &[u8]) {
        // body: 30 3e xx xx xx xx xx
        if body.len() < 3 {
            return;
        }
        let payload = &body[2..];
        self.core
            .publish_property("telemetry", rethink_core::hex_encode(payload).into());
    }

    fn process_aabb(&self, buf: &[u8]) {
        if buf.is_empty() || buf[0] != 0x30 {
            return;
        }
        if buf.len() > 1 && buf[1] == 0x31 {
            return; // identity / serial noise
        }
        if buf.len() > 1 && buf[1] == 0x3e {
            self.process_telemetry(buf);
            return;
        }
        if buf.len() > 1 && buf[1] == 0xeb && buf.len() == 2 + RECORD_LEN {
            self.process_record(&buf[2..2 + RECORD_LEN]);
        } else if buf.len() > 1 && buf[1] == 0xec && buf.len() == 2 + 2 * RECORD_LEN {
            // Dual records: previous then current — use current.
            self.process_record(&buf[2 + RECORD_LEN..2 + 2 * RECORD_LEN]);
        } else {
            tracing::debug!("RH10V9 unhandled AABB {:02x?}", buf);
        }
    }

    pub fn set_property(&self, _prop: &str, _mqtt_value: &str) {}
}

impl DeviceHandler for Device {
    fn id(&self) -> &str {
        &self.core.id
    }
    fn start(&self) {
        self.stop.store(false, Ordering::SeqCst);
        Self::send_monitor_enable(&self.core);
        let core = self.core.clone();
        let stop = self.stop.clone();
        let handle = thread::spawn(move || {
            let mut n = 0u32;
            loop {
                let ms = MONITOR_INTERVAL_MS.load(Ordering::SeqCst);
                thread::sleep(Duration::from_millis(ms));
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                Self::send_monitor_enable(&core);
                n += 1;
                if n >= 8 {
                    break;
                }
            }
        });
        *self.monitor_thread.lock() = Some(handle);
    }
    fn drop_device(&self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.monitor_thread.lock().take() {
            let _ = h.join();
        }
        self.core.drop_device();
    }
    fn set_property(&self, prop: &str, value: &str) {
        Device::set_property(self, prop, value);
    }
    fn publish_config(&self) {
        if let Some(cfg) = self.core.config.lock().clone() {
            self.core
                .ha
                .publish_property(&self.core.id, "availability", "online".into());
            self.core.ha.publish_config(&self.core.id, &cfg);
        }
    }
}

pub fn create(
    ha: Arc<dyn HaConnection>,
    thinq: Arc<dyn Thinq2Device>,
    meta: Metadata,
) -> Arc<dyn DeviceHandler> {
    Device::new(ha, thinq, meta)
}
