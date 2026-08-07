//! T1789EFH_F top-load washer (AABB).

use crate::device_trait::DeviceHandler;
use rethink_core::device_base::{default_config, AabbDeviceCore};
use rethink_core::{HaConnection, Metadata, Thinq2Device};
use serde_json::{json, Map};
use std::collections::HashMap;
use std::sync::Arc;

fn status_map() -> HashMap<u8, &'static str> {
    HashMap::from([
        (0x00, "Off"),
        (0x01, "Fill / Sense"),
        (0x02, "Paused"),
        (0x03, "Wash (initial)"),
        (0x05, "Wash (main)"),
        (0x06, "Rinse / Drain"),
        (0x07, "Rinse / Drain"),
        (0x08, "Spin"),
    ])
}

pub struct Device {
    core: Arc<AabbDeviceCore>,
}

impl Device {
    pub fn new(ha: Arc<dyn HaConnection>, thinq: Arc<dyn Thinq2Device>, meta: Metadata) -> Arc<Self> {
        let core = AabbDeviceCore::new(ha, thinq.clone());
        let this = Arc::new(Self { core: core.clone() });

        let mut options: Vec<&str> = status_map().values().copied().collect();
        options.sort();
        options.dedup();
        // Keep insertion order of unique values as in TS [...new Set(Object.values(STATUS))]
        let opts: Vec<&str> = {
            let m = status_map();
            let order = [0x00u8, 0x01, 0x02, 0x03, 0x05, 0x06, 0x07, 0x08];
            let mut seen = std::collections::HashSet::new();
            let mut out = Vec::new();
            for k in order {
                if let Some(v) = m.get(&k) {
                    if seen.insert(*v) {
                        out.push(*v);
                    }
                }
            }
            out
        };

        let mut base = default_config(&meta, Some(json!({"name": "LG Washer"})));
        let mut components = Map::new();
        components.insert(
            "power".into(),
            json!({
                "platform": "binary_sensor",
                "unique_id": "$deviceid-power",
                "state_topic": "$this/power",
                "name": "Power",
                "icon": "mdi:washing-machine",
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
                "device_class": "enum",
                "options": opts,
            }),
        );
        components.insert(
            "remaining_time".into(),
            json!({
                "platform": "sensor",
                "unique_id": "$deviceid-remaining_time",
                "state_topic": "$this/remaining_time",
                "name": "Remaining time",
                "device_class": "duration",
                "unit_of_measurement": "min",
            }),
        );
        base.components = components.into_iter().collect();
        core.set_config(base);

        let t = this.clone();
        thinq.on_data(Box::new(move |data| {
            if let Some(inner) = t.core.process_data_envelope(data) {
                t.process_aabb(&inner);
            }
        }));
        this
    }

    fn process_record(&self, rec: &[u8]) {
        if rec.len() < 5 {
            return;
        }
        let phase = rec[2];
        let mins = rec[4];
        let status = status_map().get(&phase).copied().unwrap_or("unknown");
        self.core
            .publish_property("power", if phase != 0 { "ON" } else { "OFF" }.into());
        self.core.publish_property("status", status.into());
        self.core
            .publish_property("remaining_time", (mins as i64).into());
    }

    fn process_aabb(&self, buf: &[u8]) {
        if buf.is_empty() || buf[0] != 0x20 {
            return;
        }
        if buf.len() > 1 && buf[1] == 0xec && buf.len() == 56 {
            self.process_record(&buf[2..29]);
        } else if buf.len() > 1 && buf[1] == 0xeb && buf.len() == 29 {
            self.process_record(&buf[2..29]);
        }
    }

    pub fn set_property(&self, _prop: &str, _mqtt_value: &str) {}
}

impl DeviceHandler for Device {
    fn id(&self) -> &str {
        &self.core.id
    }
    fn start(&self) {}
    fn drop_device(&self) {
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
