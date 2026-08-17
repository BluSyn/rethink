//! F_C__Y___W.A__QEUK front-load washer (AABB) — A-generation UK.
//!
//! Port of upstream PR #136. 62-byte 0x20/0xEC dual-section status and 32-byte
//! 0xEB compact status share the first-section field layout. 0xE2 end-of-cycle
//! alerts are ignored. Door lock comes from 0xD8 when idle/ready, else status-derived.

use crate::device_trait::DeviceHandler;
use crate::devices::washer_ctrl::{pub_error_status, pub_temp_spin, set_power_start_pause};
use crate::washer_common::{course_name, error_options, state_name, state_options};
use rethink_core::device_base::{default_config, AabbDeviceCore};
use rethink_core::hex_decode;
use rethink_core::{HaConnection, Metadata, Thinq2Device};
use rethink_util::sync::Mutex;
use serde_json::{json, Map};
use std::sync::Arc;

pub struct Device {
    core: Arc<AabbDeviceCore>,
    last_status: Mutex<i32>,
}

impl Device {
    pub fn new(ha: Arc<dyn HaConnection>, thinq: Arc<dyn Thinq2Device>, meta: Metadata) -> Arc<Self> {
        let core = AabbDeviceCore::new(ha, thinq.clone());
        let this = Arc::new(Self {
            core: core.clone(),
            last_status: Mutex::new(-1),
        });

        let mut base = default_config(&meta, Some(json!({"name": "LG Washer"})));
        let mut components = Map::new();
        let common = [
            ("power", json!({"platform":"switch","unique_id":"$deviceid-power","state_topic":"$this/power","command_topic":"$this/power/set","name":"","icon":"mdi:washing-machine"})),
            ("start", json!({"platform":"button","unique_id":"$deviceid-start","command_topic":"$this/start/set","payload_press":"","name":"Start","icon":"mdi:play-circle-outline"})),
            ("pause", json!({"platform":"button","unique_id":"$deviceid-pause","command_topic":"$this/pause/set","payload_press":"","name":"Pause","icon":"mdi:pause-circle-outline"})),
            ("status", json!({"platform":"sensor","unique_id":"$deviceid-status","state_topic":"$this/status","name":"Status","icon":"mdi:state-machine","device_class":"enum","options":state_options()})),
            ("error", json!({"platform":"binary_sensor","unique_id":"$deviceid-error","state_topic":"$this/error","name":"Error","icon":"mdi:check-circle","device_class":"problem","entity_category":"diagnostic"})),
            ("error_message", json!({"platform":"sensor","unique_id":"$deviceid-error-message","state_topic":"$this/error_message","name":"Error message","icon":"mdi:alert-circle-outline","device_class":"enum","entity_category":"diagnostic","options":error_options()})),
            ("course", json!({"platform":"sensor","unique_id":"$deviceid-course","state_topic":"$this/course","name":"Course","icon":"mdi:pin-outline"})),
            ("temp", json!({"platform":"sensor","unique_id":"$deviceid-temp","state_topic":"$this/temp","name":"Temperature","device_class":"temperature","unit_of_measurement":"°C","suggested_display_precision":0,"value_template":"{{ value if value | is_number else 'None' }}"})),
            ("spin", json!({"platform":"sensor","unique_id":"$deviceid-spin","state_topic":"$this/spin","name":"Spin","icon":"mdi:autorenew","unit_of_measurement":"RPM","value_template":"{{ value if value | is_number else 'None' }}"})),
            ("remote_start", json!({"platform":"binary_sensor","unique_id":"$deviceid-remote_start","state_topic":"$this/remote_start","name":"Remote start","icon":"mdi:play-circle-outline"})),
            ("door_lock", json!({"platform":"binary_sensor","unique_id":"$deviceid-door_lock","state_topic":"$this/door_lock","name":"Door lock","device_class":"lock"})),
            ("steam", json!({"platform":"binary_sensor","unique_id":"$deviceid-steam","state_topic":"$this/steam","name":"Steam","icon":"mdi:weather-fog"})),
            ("wrinkle_care", json!({"platform":"binary_sensor","unique_id":"$deviceid-wrinkle_care","state_topic":"$this/wrinkle_care","name":"Wrinkle care","icon":"mdi:iron-outline"})),
            ("child_lock", json!({"platform":"binary_sensor","unique_id":"$deviceid-child_lock","state_topic":"$this/child_lock","name":"Child lock","icon":"mdi:account-lock","device_class":"lock","entity_category":"diagnostic"})),
            ("active", json!({"platform":"binary_sensor","unique_id":"$deviceid-active","state_topic":"$this/active","name":"Active","icon":"mdi:washing-machine"})),
            ("pre_state", json!({"platform":"sensor","unique_id":"$deviceid-pre_state","state_topic":"$this/pre_state","name":"Pre state","icon":"mdi:state-machine","device_class":"enum","options":state_options()})),
            ("tub_clean", json!({"platform":"sensor","unique_id":"$deviceid-tub-clean","state_topic":"$this/tub_clean","name":"Tub clean counter","icon":"mdi:washing-machine-alert","entity_category":"diagnostic"})),
            ("initial_time", json!({"platform":"sensor","unique_id":"$deviceid-initial_time","state_topic":"$this/initial_time","device_class":"duration","unit_of_measurement":"min","name":"Initial time"})),
            ("remaining_time", json!({"platform":"sensor","unique_id":"$deviceid-remaining_time","state_topic":"$this/remaining_time","device_class":"duration","unit_of_measurement":"min","name":"Remaining time"})),
            ("delay_remaining", json!({"platform":"sensor","unique_id":"$deviceid-delay_remaining","state_topic":"$this/delay_remaining","device_class":"duration","unit_of_measurement":"min","name":"Delay remaining","icon":"mdi:clock-start"})),
        ];
        for (k, v) in common {
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

    fn process_aabb(&self, buf: &[u8]) {
        let is_ec = buf.len() == 62 && buf[0] == 0x20 && buf[1] == 0xec;
        let is_eb = buf.len() == 32 && buf[0] == 0x20 && buf[1] == 0xeb;
        let is_e2 = buf.len() == 32 && buf[0] == 0x20 && buf[1] == 0xe2;
        let is_d8 = buf.len() == 3 && buf[0] == 0x20 && buf[1] == 0xd8;
        if is_e2 {
            return;
        }
        if is_d8 {
            let last = *self.last_status.lock();
            // Only authoritative during Off (0) and Ready (1)
            if last <= 1 {
                // HA lock: OFF=Locked, ON=Unlocked
                self.core.publish_property(
                    "door_lock",
                    if buf[2] != 0 { "OFF" } else { "ON" }.into(),
                );
            }
            return;
        }
        if !(is_ec || is_eb) {
            return;
        }
        if buf.len() < 26 {
            return;
        }
        let status = buf[4];
        *self.last_status.lock() = status as i32;
        let remain_h = buf[5];
        let remain_m = buf[6];
        let initial_h = buf[7];
        let initial_m = buf[8];
        let lock_status = buf[9];
        let error_code = buf[10];
        let spin = buf[12];
        let temp = buf[13];
        let course = buf[14];
        let delay_h = buf[16];
        let delay_m = buf[17];
        let steam = buf[18] & 0x80;
        let wrinkle_care = buf[18] & 0x20;
        let active = buf[19] & 0x40;
        let child_lock = buf[19] & 0x80;
        let pre_state = buf[23];
        let tub_clean = buf[25];

        self.core
            .publish_property("power", if status > 0 { "ON" } else { "OFF" }.into());
        pub_error_status(&self.core, error_code, status);
        self.core.publish_property(
            "course",
            course_name(course as u32).unwrap_or("unknown").into(),
        );
        pub_temp_spin(&self.core, temp, spin);
        self.core.publish_property(
            "remaining_time",
            (remain_h as i64 * 60 + remain_m as i64).into(),
        );
        self.core.publish_property(
            "initial_time",
            (initial_h as i64 * 60 + initial_m as i64).into(),
        );
        self.core.publish_property(
            "delay_remaining",
            (delay_h as i64 * 60 + delay_m as i64).into(),
        );
        self.core.publish_property(
            "remote_start",
            if lock_status & 2 != 0 { "ON" } else { "OFF" }.into(),
        );
        self.core
            .publish_property("steam", if steam != 0 { "ON" } else { "OFF" }.into());
        self.core.publish_property(
            "wrinkle_care",
            if wrinkle_care != 0 { "ON" } else { "OFF" }.into(),
        );
        self.core
            .publish_property("active", if active != 0 { "ON" } else { "OFF" }.into());
        // HA lock device_class: OFF=Locked, ON=Unlocked for child_lock mapping from TS
        self.core.publish_property(
            "child_lock",
            if child_lock != 0 { "OFF" } else { "ON" }.into(),
        );
        self.core.publish_property(
            "pre_state",
            state_name(pre_state as usize).into(),
        );
        self.core
            .publish_property("tub_clean", (tub_clean as i64).into());

        // Off → unlocked ON; Ready → leave to 0xD8; else locked OFF
        if status == 0 {
            self.core.publish_property("door_lock", "ON".into());
        } else if status != 1 {
            self.core.publish_property("door_lock", "OFF".into());
        }
    }

    pub fn set_property(&self, prop: &str, mqtt_value: &str) {
        set_power_start_pause(&self.core, prop, mqtt_value);
    }
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
