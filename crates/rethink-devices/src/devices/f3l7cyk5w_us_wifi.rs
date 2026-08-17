//! F3L7CYK5W_US_WIFI front-load washer (AABB).
//!
//! Port of upstream PR #135. Shares 25-byte 0x18 record layout with F3L2CYU__ but
//! has a different course table and additional fields (child lock, rinse+spin,
//! live rinse count, initial time, tub clean count, load level). Deliberately
//! ignores 0xE2 (stale post-cycle replay).

use crate::device_trait::DeviceHandler;
use rethink_core::device_base::{default_config, AabbDeviceCore};
use rethink_core::hex_decode;
use rethink_core::{HaConnection, Metadata, Thinq2Device};
use serde_json::{json, Map};
use std::collections::HashMap;
use std::sync::Arc;

const STATUS_FRAME_TYPE: u8 = 0xec;
const STATUS_FRAME_LEN: usize = 54;
const RECORD_B_OFFSET: usize = 29;
const SINGLE_STATUS_FRAME_TYPE: u8 = 0xeb;
const SINGLE_STATUS_FRAME_LEN: usize = 28;
const SINGLE_RECORD_OFFSET: usize = 3;
const RECORD_MARKER: u8 = 0x18;

const PHASE_OFFSET: usize = 1;
const TIME_HOUR_OFFSET: usize = 2;
const TIME_MIN_OFFSET: usize = 3;
const INITIAL_TIME_HOUR_OFFSET: usize = 4;
const INITIAL_TIME_MIN_OFFSET: usize = 5;
const COURSE_OFFSET: usize = 6;
const SOIL_OFFSET: usize = 8;
const SPIN_OFFSET: usize = 9;
const TEMP_OFFSET: usize = 10;
const RINSE_OFFSET: usize = 11;
const RESERVE_HOUR_OFFSET: usize = 13;
const RESERVE_MIN_OFFSET: usize = 14;
const FLAGS_OFFSET: usize = 15;
const FLAG_CHILD_LOCK: u8 = 0x01;
const FLAG_DELAY_ACTIVE: u8 = 0x02;
const FLAG_STEAM: u8 = 0x04;
const FLAG_PRE_WASH: u8 = 0x08;
const FLAG_RINSE_SPIN: u8 = 0x20;
const FLAG_EXTRA_RINSE: u8 = 0x40;
const OPT2_OFFSET: usize = 16;
const OPT2_COLD_WASH: u8 = 0x10;
const OPT2_DOOR_LOCKED: u8 = 0x80;
const TCL_COUNT_OFFSET: usize = 22;
const LOAD_LEVEL_OFFSET: usize = 24;
const PHASE_OFF: u8 = 0x00;
const PHASE_COMPLETE: u8 = 0x3c;

fn status_map() -> HashMap<u8, &'static str> {
    HashMap::from([
        (0x00, "Off"),
        (0x05, "Initial"),
        (0x06, "Pause"),
        (0x0a, "Delay Wash"),
        (0x14, "Sensing"),
        (0x15, "Add Garments"),
        (0x17, "Washing"),
        (0x1e, "Rinsing"),
        (0x28, "Spinning"),
        (0x3c, "Complete"),
    ])
}
fn course_map() -> HashMap<u8, &'static str> {
    HashMap::from([
        (0x01, "Tub Clean"),
        (0x02, "Allergiene"),
        (0x03, "Sanitary"),
        (0x04, "Bedding"),
        (0x05, "Heavy Duty"),
        (0x06, "Normal"),
        (0x07, "Bright Whites"),
        (0x08, "Perm Press"),
        (0x09, "Delicates"),
        (0x0a, "Towels"),
        (0x0b, "Speed Wash"),
        (0x0c, "Downloaded"),
    ])
}
fn soil_map() -> HashMap<u8, &'static str> {
    HashMap::from([
        (1, "Light"),
        (2, "Light-Normal"),
        (3, "Normal"),
        (4, "Normal-Heavy"),
        (5, "Heavy"),
    ])
}
fn spin_map() -> HashMap<u8, &'static str> {
    HashMap::from([
        (1, "No Spin"),
        (2, "Low"),
        (3, "Medium"),
        (4, "High"),
        (5, "Extra High"),
    ])
}
fn temp_map() -> HashMap<u8, &'static str> {
    HashMap::from([
        (1, "Tap Cold"),
        (2, "Cold"),
        (4, "Warm"),
        (6, "Hot"),
        (7, "Extra Hot"),
    ])
}

pub struct Device {
    core: Arc<AabbDeviceCore>,
}

impl Device {
    pub fn new(ha: Arc<dyn HaConnection>, thinq: Arc<dyn Thinq2Device>, meta: Metadata) -> Arc<Self> {
        let core = AabbDeviceCore::new(ha, thinq.clone());
        let this = Arc::new(Self { core: core.clone() });

        let mut base = default_config(&meta, Some(json!({"name": "LG Washer"})));
        let comps = [
            ("power", json!({"platform":"binary_sensor","unique_id":"$deviceid-power","state_topic":"$this/power","name":"Power","icon":"mdi:washing-machine","device_class":"running"})),
            ("status", json!({"platform":"sensor","unique_id":"$deviceid-status","state_topic":"$this/status","name":"Status","icon":"mdi:state-machine"})),
            ("course", json!({"platform":"sensor","unique_id":"$deviceid-course","state_topic":"$this/course","name":"Course","icon":"mdi:pin-outline"})),
            ("remaining_time", json!({"platform":"sensor","unique_id":"$deviceid-remaining_time","state_topic":"$this/remaining_time","name":"Remaining time","icon":"mdi:timer-outline","device_class":"duration","unit_of_measurement":"min"})),
            ("initial_time", json!({"platform":"sensor","unique_id":"$deviceid-initial_time","state_topic":"$this/initial_time","name":"Initial time","icon":"mdi:timer-sand","device_class":"duration","unit_of_measurement":"min"})),
            ("reserve_time", json!({"platform":"sensor","unique_id":"$deviceid-reserve_time","state_topic":"$this/reserve_time","name":"Delay Wash time remaining","icon":"mdi:clock-outline","device_class":"duration","unit_of_measurement":"min"})),
            ("soil", json!({"platform":"sensor","unique_id":"$deviceid-soil","state_topic":"$this/soil","name":"Soil level","icon":"mdi:liquid-spot"})),
            ("spin", json!({"platform":"sensor","unique_id":"$deviceid-spin","state_topic":"$this/spin","name":"Spin","icon":"mdi:autorenew"})),
            ("temp", json!({"platform":"sensor","unique_id":"$deviceid-temp","state_topic":"$this/temp","name":"Temperature","icon":"mdi:thermometer"})),
            ("rinse_count", json!({"platform":"sensor","unique_id":"$deviceid-rinse_count","state_topic":"$this/rinse_count","name":"Rinse count","icon":"mdi:water","state_class":"measurement"})),
            ("extra_rinse", json!({"platform":"binary_sensor","unique_id":"$deviceid-extra_rinse","state_topic":"$this/extra_rinse","name":"Extra rinse","icon":"mdi:water-sync"})),
            ("extra_rinse_count", json!({"platform":"sensor","unique_id":"$deviceid-extra_rinse_count","state_topic":"$this/extra_rinse_count","name":"Extra rinse count","icon":"mdi:water-sync","state_class":"measurement"})),
            ("pre_wash", json!({"platform":"binary_sensor","unique_id":"$deviceid-pre_wash","state_topic":"$this/pre_wash","name":"Pre-wash","icon":"mdi:water-sync"})),
            ("steam", json!({"platform":"binary_sensor","unique_id":"$deviceid-steam","state_topic":"$this/steam","name":"Steam","icon":"mdi:kettle-steam"})),
            ("cold_wash", json!({"platform":"binary_sensor","unique_id":"$deviceid-cold_wash","state_topic":"$this/cold_wash","name":"Cold wash","icon":"mdi:snowflake"})),
            ("rinse_spin", json!({"platform":"binary_sensor","unique_id":"$deviceid-rinse_spin","state_topic":"$this/rinse_spin","name":"Rinse+Spin","icon":"mdi:rotate-right"})),
            ("delay_wash", json!({"platform":"binary_sensor","unique_id":"$deviceid-delay_wash","state_topic":"$this/delay_wash","name":"Delay Wash","icon":"mdi:clock-plus-outline"})),
            ("door_lock", json!({"platform":"binary_sensor","unique_id":"$deviceid-door_lock","state_topic":"$this/door_lock","name":"Door lock","icon":"mdi:lock","entity_category":"diagnostic"})),
            ("child_lock", json!({"platform":"binary_sensor","unique_id":"$deviceid-child_lock","state_topic":"$this/child_lock","name":"Child lock","icon":"mdi:lock","entity_category":"diagnostic"})),
            ("load_level", json!({"platform":"sensor","unique_id":"$deviceid-load_level","state_topic":"$this/load_level","name":"Load level","icon":"mdi:scale","state_class":"measurement"})),
            ("tub_clean_count", json!({"platform":"sensor","unique_id":"$deviceid-tub_clean_count","state_topic":"$this/tub_clean_count","name":"Tub clean count","icon":"mdi:counter","entity_category":"diagnostic","state_class":"measurement"})),
        ];
        let mut components = Map::new();
        for (k, v) in comps {
            components.insert(k.into(), v);
        }
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

    fn process_status(&self, buf: &[u8], record_offset: usize, expected_len: usize) {
        if buf.len() != expected_len {
            return;
        }
        let rec = &buf[record_offset..];
        if rec.len() < LOAD_LEVEL_OFFSET + 1 || rec[0] != RECORD_MARKER {
            return;
        }
        let phase = rec[PHASE_OFFSET];
        let is_off = phase == PHASE_OFF;
        let idle = is_off || phase == PHASE_COMPLETE;

        self.core
            .publish_property("power", if is_off { "OFF" } else { "ON" }.into());
        self.core.publish_property(
            "status",
            status_map().get(&phase).copied().unwrap_or("Running").into(),
        );
        self.core.publish_property(
            "course",
            course_map()
                .get(&rec[COURSE_OFFSET])
                .copied()
                .unwrap_or("unknown")
                .into(),
        );
        self.core.publish_property(
            "remaining_time",
            if idle {
                0i64
            } else {
                rec[TIME_HOUR_OFFSET] as i64 * 60 + rec[TIME_MIN_OFFSET] as i64
            }
            .into(),
        );
        self.core.publish_property(
            "initial_time",
            if idle {
                0i64
            } else {
                rec[INITIAL_TIME_HOUR_OFFSET] as i64 * 60 + rec[INITIAL_TIME_MIN_OFFSET] as i64
            }
            .into(),
        );
        self.core.publish_property(
            "reserve_time",
            if is_off {
                0i64
            } else {
                rec[RESERVE_HOUR_OFFSET] as i64 * 60 + rec[RESERVE_MIN_OFFSET] as i64
            }
            .into(),
        );
        self.core.publish_property(
            "soil",
            soil_map()
                .get(&rec[SOIL_OFFSET])
                .copied()
                .unwrap_or("unknown")
                .into(),
        );
        self.core.publish_property(
            "spin",
            spin_map()
                .get(&rec[SPIN_OFFSET])
                .copied()
                .unwrap_or("unknown")
                .into(),
        );
        self.core.publish_property(
            "temp",
            temp_map()
                .get(&rec[TEMP_OFFSET])
                .copied()
                .unwrap_or("unknown")
                .into(),
        );
        self.core
            .publish_property("rinse_count", ((rec[RINSE_OFFSET] & 0x0f) as i64).into());
        self.core.publish_property(
            "extra_rinse_count",
            ((rec[RINSE_OFFSET] >> 4) as i64).into(),
        );

        let flags = rec[FLAGS_OFFSET];
        self.core.publish_property(
            "child_lock",
            if flags & FLAG_CHILD_LOCK != 0 {
                "ON"
            } else {
                "OFF"
            }
            .into(),
        );
        self.core.publish_property(
            "delay_wash",
            if flags & FLAG_DELAY_ACTIVE != 0 {
                "ON"
            } else {
                "OFF"
            }
            .into(),
        );
        self.core.publish_property(
            "steam",
            if flags & FLAG_STEAM != 0 { "ON" } else { "OFF" }.into(),
        );
        self.core.publish_property(
            "pre_wash",
            if flags & FLAG_PRE_WASH != 0 {
                "ON"
            } else {
                "OFF"
            }
            .into(),
        );
        self.core.publish_property(
            "rinse_spin",
            if flags & FLAG_RINSE_SPIN != 0 {
                "ON"
            } else {
                "OFF"
            }
            .into(),
        );
        self.core.publish_property(
            "extra_rinse",
            if flags & FLAG_EXTRA_RINSE != 0 {
                "ON"
            } else {
                "OFF"
            }
            .into(),
        );

        let opt2 = rec[OPT2_OFFSET];
        self.core.publish_property(
            "cold_wash",
            if opt2 & OPT2_COLD_WASH != 0 {
                "ON"
            } else {
                "OFF"
            }
            .into(),
        );
        self.core.publish_property(
            "door_lock",
            if opt2 & OPT2_DOOR_LOCKED != 0 {
                "ON"
            } else {
                "OFF"
            }
            .into(),
        );
        self.core
            .publish_property("load_level", (rec[LOAD_LEVEL_OFFSET] as i64).into());
        self.core
            .publish_property("tub_clean_count", (rec[TCL_COUNT_OFFSET] as i64).into());
    }

    fn process_aabb(&self, buf: &[u8]) {
        if buf.len() < 2 || buf[0] != 0x20 {
            return;
        }
        // Deliberately ignore 0xE2 (stale post-cycle replay).
        if buf[1] == STATUS_FRAME_TYPE {
            self.process_status(buf, RECORD_B_OFFSET, STATUS_FRAME_LEN);
        } else if buf[1] == SINGLE_STATUS_FRAME_TYPE {
            self.process_status(buf, SINGLE_RECORD_OFFSET, SINGLE_STATUS_FRAME_LEN);
        }
    }

    pub fn set_property(&self, _prop: &str, _mqtt_value: &str) {}
}

impl DeviceHandler for Device {
    fn id(&self) -> &str {
        &self.core.id
    }
    fn start(&self) {
        self.core.send(&hex_decode("F0ED1121010000001800"));
    }
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
