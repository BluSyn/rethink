//! F_V__F___W.B_1QEUK washer (AABB, 80-byte status).

use crate::device_trait::DeviceHandler;
use crate::devices::washer_ctrl::{pub_error_status, pub_temp_spin, set_power_start_pause};
use crate::washer_common::{course_name, drying_mode, error_options, state_options};
use rethink_core::device_base::{default_config, AabbDeviceCore};
use rethink_core::hex_decode;
use rethink_core::{HaConnection, Metadata, Thinq2Device};
use serde_json::{json, Map};
use std::sync::Arc;

pub struct Device {
    core: Arc<AabbDeviceCore>,
}

impl Device {
    pub fn new(ha: Arc<dyn HaConnection>, thinq: Arc<dyn Thinq2Device>, meta: Metadata) -> Arc<Self> {
        let core = AabbDeviceCore::new(ha, thinq.clone());
        let this = Arc::new(Self { core: core.clone() });

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
            ("drying_mode", json!({"platform":"sensor","unique_id":"$deviceid-drying-mode","state_topic":"$this/drying_mode","name":"Drying mode","icon":"mdi:tumble-dryer"})),
            ("cycles", json!({"platform":"sensor","unique_id":"$deviceid-cycles","state_topic":"$this/cycles","name":"Cycle count","icon":"mdi:counter"})),
            ("remote_start", json!({"platform":"binary_sensor","unique_id":"$deviceid-remote_start","state_topic":"$this/remote_start","name":"Remote start","icon":"mdi:play-circle-outline"})),
            ("door_lock", json!({"platform":"binary_sensor","unique_id":"$deviceid-door_lock","state_topic":"$this/door_lock","name":"Door lock","device_class":"lock"})),
            ("energy", json!({"platform":"sensor","unique_id":"$deviceid-energy","state_topic":"$this/energy","name":"Energy","icon":"mdi:lightning-bolt","device_class":"energy","state_class":"total_increasing","unit_of_measurement":"Wh"})),
            ("initial_time", json!({"platform":"sensor","unique_id":"$deviceid-initial_time","state_topic":"$this/initial_time","device_class":"duration","unit_of_measurement":"min","name":"Initial time"})),
            ("remaining_time", json!({"platform":"sensor","unique_id":"$deviceid-remaining_time","state_topic":"$this/remaining_time","device_class":"duration","unit_of_measurement":"min","name":"Remaining time"})),
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
        if buf.len() == 80 && buf[0] == 0x20 {
            let status = buf[43];
            let time_remain = buf[44] as i64 * 60 + buf[45] as i64;
            let time_initial = buf[46] as i64 * 60 + buf[47] as i64;
            let course = buf[48];
            let error = buf[49];
            let temp = buf[52];
            let spin = buf[51];
            let drying_mode_b = buf[54];
            let lock_status = buf[58];
            let cycles = buf[64] as i64;
            let energy = buf[71] as i64 * 256 + buf[72] as i64;

            self.core
                .publish_property("power", if status > 0 { "ON" } else { "OFF" }.into());
            pub_error_status(&self.core, error, status);
            self.core.publish_property(
                "course",
                course_name(course as u32).unwrap_or("unknown").into(),
            );
            pub_temp_spin(&self.core, temp, spin);
            self.core.publish_property(
                "drying_mode",
                drying_mode(drying_mode_b as u32)
                    .unwrap_or("unknown")
                    .into(),
            );
            self.core.publish_property("cycles", cycles.into());
            self.core.publish_property(
                "remote_start",
                if lock_status & 2 != 0 { "ON" } else { "OFF" }.into(),
            );
            self.core.publish_property(
                "door_lock",
                if lock_status & 0x40 == 0 { "ON" } else { "OFF" }.into(),
            );
            self.core
                .publish_property("initial_time", time_initial.into());
            self.core
                .publish_property("remaining_time", time_remain.into());
            self.core.publish_property("energy", energy.into());
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

#[cfg(test)]
mod tests {
    use super::*;
    use rethink_core::{Thinq2Device, hex_decode, hex_encode, MockHaConnection, MockThinq2Device, Metadata};
    use crate::device_trait::DeviceHandler;

    const DEVICE_ID: &str = "test-id";
    fn meta() -> Metadata { Metadata::new("F_V__F___W.B_1QEUK", "F_V__F___W.B_1QEUK", "2.10.123") }
    fn make() -> (std::sync::Arc<MockHaConnection>, std::sync::Arc<MockThinq2Device>, std::sync::Arc<Device>) {
        let ha = MockHaConnection::new();
        let thinq = MockThinq2Device::new(DEVICE_ID, meta());
        let dev = Device::new(ha.clone(), thinq.clone(), meta());
        (ha, thinq, dev)
    }
    fn prop(ha: &MockHaConnection, n: &str) -> Option<String> {
        ha.device(DEVICE_ID)?.properties.get(n).map(|p| p.as_string())
    }

    const SAMPLE_INITIAL: &str = "AA5420EC002500000000000000000000000000000000000000010036006400000000000000000000000000002501000000000000000000000000000000000000000036006400000000000000000000000000DFBB";
    const SAMPLE_WASH: &str = "AA5420EC00250101000100180000000000020000000000000600001000640000000000000000000000000000250104380438130003090401020000000000000400001000640000040000000000000000000053BB";
    const SAMPLE_RUNNING: &str = "AA5420EC002506001300140C00030204010000000142200001010036006400000100000E00000000000000002506001300140C00030204010000000142200001010036006400000100000F00000000000000A2BB";

    #[test]
    fn config_and_decode() {
        let (ha, thinq, dev) = make();
        let devinfo = ha.device(DEVICE_ID).unwrap();
        let comps = &devinfo.config.as_ref().unwrap().components;
        for c in ["power","start","pause","status","drying_mode","energy","remaining_time"] {
            assert!(comps.contains_key(c));
        }
        thinq.emit_data(&hex_decode(SAMPLE_INITIAL));
        assert_eq!(prop(&ha, "power").as_deref(), Some("ON"));
        assert_eq!(prop(&ha, "status").as_deref(), Some("Ready"));
        assert_eq!(prop(&ha, "error").as_deref(), Some("OFF"));
        assert_eq!(prop(&ha, "error_message").as_deref(), Some("OK"));
        assert_eq!(prop(&ha, "drying_mode").as_deref(), Some("Off"));
        assert_eq!(prop(&ha, "cycles").as_deref(), Some("54"));
        assert_eq!(prop(&ha, "remote_start").as_deref(), Some("OFF"));
        assert_eq!(prop(&ha, "door_lock").as_deref(), Some("ON"));

        thinq.emit_data(&hex_decode(SAMPLE_WASH));
        assert_eq!(prop(&ha, "course").as_deref(), Some("Wash + Dry"));
        assert_eq!(prop(&ha, "spin").as_deref(), Some("1200"));
        assert_eq!(prop(&ha, "temp").as_deref(), Some("40"));
        assert_eq!(prop(&ha, "drying_mode").as_deref(), Some("Auto"));
        assert_eq!(prop(&ha, "remaining_time").as_deref(), Some("296")); // 4*60+56

        thinq.emit_data(&hex_decode(SAMPLE_RUNNING));
        assert_eq!(prop(&ha, "status").as_deref(), Some("Washing"));

        thinq.reset_recorder();
        dev.start();
        assert_eq!(hex_encode(&thinq.outbox()[0]), "AA0EF0ED1121010000001800B5BB");
        thinq.reset_recorder();
        dev.set_property("power", "ON");
        assert_eq!(hex_encode(&thinq.outbox()[0]), "AA08F02A010098BB");
        thinq.reset_recorder();
        dev.set_property("power", "OFF");
        assert_eq!(hex_encode(&thinq.outbox()[0]), "AA09F0240101009CBB");
        thinq.reset_recorder();
        dev.set_property("pause", "");
        assert_eq!(hex_encode(&thinq.outbox()[0]), "AA09F02404010099BB");
        thinq.reset_recorder();
        dev.set_property("start", "");
        assert_eq!(hex_encode(&thinq.outbox()[0]), "AA09F02405010098BB");
    }
}
