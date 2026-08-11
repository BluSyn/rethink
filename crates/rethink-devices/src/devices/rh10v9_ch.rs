//! RH10V9_CH heat-pump dryer (AABB).

use crate::device_trait::DeviceHandler;
use rethink_util::sync::Mutex;
use rethink_core::device_base::{default_config, AabbDeviceCore};
use rethink_core::hex_decode;
use rethink_core::{HaConnection, Metadata, Thinq2Device};
use serde_json::{json, Map};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const RECORD_LEN: usize = 27;

/// Interval for monitor-enable retries (ms). Tests may lower this.
pub static MONITOR_INTERVAL_MS: AtomicU64 = AtomicU64::new(15_000);

fn status_map() -> HashMap<u8, &'static str> {
    HashMap::from([
        (0x00, "Off"),
        (0x01, "Initial"),
        (0x03, "Pause"),
        (0x32, "Drying"),
        (0x33, "Cooling"),
        (0x04, "End"),
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
        components.insert(
            "power".into(),
            json!({
                "platform": "binary_sensor",
                "unique_id": "$deviceid-power",
                "state_topic": "$this/power",
                "name": "Power",
                "icon": "mdi:tumble-dryer",
                "device_class": "running",
            }),
        );
        components.insert(
            "status".into(),
            json!({
                "platform": "sensor",
                "unique_id": "$deviceid-status",
                "state_topic": "$this/status",
                "name": "Status",
                "icon": "mdi:state-machine",
            }),
        );
        components.insert(
            "remaining_time".into(),
            json!({
                "platform": "sensor",
                "unique_id": "$deviceid-remaining_time",
                "state_topic": "$this/remaining_time",
                "name": "Remaining time",
                "icon": "mdi:timer-outline",
                "device_class": "duration",
                "unit_of_measurement": "min",
            }),
        );
        components.insert(
            "flags".into(),
            json!({
                "platform": "sensor",
                "unique_id": "$deviceid-flags",
                "state_topic": "$this/flags",
                "name": "Flags (raw)",
                "icon": "mdi:flag",
                "entity_category": "diagnostic",
            }),
        );
        components.insert(
            "raw_b21".into(),
            json!({
                "platform": "sensor",
                "unique_id": "$deviceid-raw_b21",
                "state_topic": "$this/raw_b21",
                "name": "Raw byte 21",
                "icon": "mdi:numeric",
                "entity_category": "diagnostic",
            }),
        );
        base.components = components.into_iter().collect();
        base.device_triggers
            .push(rethink_core::DeviceTriggerDef::event(
                "turned_off",
                "cycle_complete",
            ));
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
        let remaining = rec[0] as i64 * 60 + rec[1] as i64;
        let flags = rec[17] as i64;
        let b21 = rec[21] as i64;
        let status = status_map()
            .get(&phase)
            .map(|s| (*s).to_string())
            .unwrap_or_else(|| format!("0x{phase:x}"));

        let prev_phase = *self.last_phase.lock();
        *self.last_phase.lock() = Some(phase);

        self.core.publish_property("status", status.into());
        self.core
            .publish_property("remaining_time", remaining.into());
        self.core
            .publish_property("power", if phase != 0 { "ON" } else { "OFF" }.into());
        self.core.publish_property("flags", flags.into());
        self.core.publish_property("raw_b21", b21.into());

        // Phase 0x04 = End — fire once when cycle completes
        if phase == 0x04 && prev_phase != Some(0x04) {
            self.core
                .ha
                .fire_device_trigger(&self.core.id, "cycle_complete");
        }
    }

    fn process_aabb(&self, buf: &[u8]) {
        if buf.is_empty() || buf[0] != 0x30 {
            return;
        }
        if buf.len() > 1 && buf[1] == 0x31 {
            return;
        }
        if buf.len() > 1 && buf[1] == 0xeb && buf.len() == 2 + RECORD_LEN {
            self.process_record(&buf[2..2 + RECORD_LEN]);
        } else if buf.len() > 1 && buf[1] == 0xec && buf.len() == 2 + 2 * RECORD_LEN {
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
