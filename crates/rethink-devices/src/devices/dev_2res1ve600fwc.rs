//! 2RES1VE600FWC fridge — STATUS_LENGTH = 12, custom temp encoding.

use crate::device_trait::DeviceHandler;
use crate::fridge_common::{pack_status, status_get, unpack_status, Status, TemperatureUnit};
use rethink_util::sync::Mutex;
use rethink_core::device_base::{default_config, AabbDeviceCore};
use rethink_core::ha::{HaConnection, PropertyValue};
use rethink_core::metadata::Metadata;
use rethink_core::thinq::Thinq2Device;
use serde_json::json;
use std::sync::Arc;

const STATUS_LENGTH: usize = 12;

fn fridge_range(unit: TemperatureUnit) -> (String, i32, i32) {
    match unit {
        TemperatureUnit::F => ("°F".into(), 33, 46),
        TemperatureUnit::C => ("°C".into(), 1, 7),
    }
}

fn freezer_range(unit: TemperatureUnit) -> (String, i32, i32) {
    match unit {
        TemperatureUnit::F => ("°F".into(), -17, 7),
        TemperatureUnit::C => ("°C".into(), -24, -14),
    }
}

fn convert_fridge_temperature(unit: TemperatureUnit, input: i32) -> i32 {
    match unit {
        TemperatureUnit::F => 47 - input,
        TemperatureUnit::C => 8 - input,
    }
}

fn convert_to_freezer_temperature(unit: TemperatureUnit, input: i32) -> i32 {
    match unit {
        TemperatureUnit::F => {
            if input < -15 {
                15
            } else if input < -12 {
                14
            } else if input < -7 {
                13
            } else if input < -3 {
                12
            } else {
                8 - input
            }
        }
        TemperatureUnit::C => -13 - input,
    }
}

fn convert_from_freezer_temperature(unit: TemperatureUnit, input: u8) -> i32 {
    match unit {
        TemperatureUnit::F => {
            let table: [i32; 16] = [7, 7, 6, 5, 4, 3, 2, 1, 0, -1, -2, -3, -7, -12, -15, -17];
            table.get(input as usize).copied().unwrap_or(7)
        }
        TemperatureUnit::C => -13 - input as i32,
    }
}

pub struct Device {
    core: Arc<AabbDeviceCore>,
    temperature_unit: Mutex<Option<TemperatureUnit>>,
    meta: Metadata,
}

impl Device {
    pub fn new(ha: Arc<dyn HaConnection>, thinq: Arc<dyn Thinq2Device>, meta: Metadata) -> Arc<Self> {
        let core = AabbDeviceCore::new(ha, thinq.clone());
        let this = Arc::new(Self {
            core,
            temperature_unit: Mutex::new(None),
            meta,
        });
        let t = this.clone();
        thinq.on_data(Box::new(move |data| {
            if let Some(inner) = t.core.process_data_envelope(data) {
                t.process_aabb(&inner);
            }
        }));
        this
    }

    fn set_temperature_unit(&self, unit: TemperatureUnit) {
        if *self.temperature_unit.lock() == Some(unit) {
            return;
        }
        *self.temperature_unit.lock() = Some(unit);
        let (fu, fmin, fmax) = fridge_range(unit);
        let (zu, zmin, zmax) = freezer_range(unit);
        let mut config = default_config(&self.meta, Some(json!({"name": "LG Fridge"})));
        config.components = [
            ("fridge_setpoint".into(), json!({"platform":"number","device_class":"temperature","unique_id":"$deviceid-fridge_setpoint","state_topic":"$this/fridge_setpoint","command_topic":"$this/fridge_setpoint/set","name":"Fridge temperature","unit_of_measurement":fu,"min":fmin,"max":fmax})),
            ("freezer_setpoint".into(), json!({"platform":"number","device_class":"temperature","unique_id":"$deviceid-freezer_setpoint","state_topic":"$this/freezer_setpoint","command_topic":"$this/freezer_setpoint/set","name":"Freezer temperature","unit_of_measurement":zu,"min":zmin,"max":zmax})),
            ("express_freeze".into(), json!({"platform":"switch","unique_id":"$deviceid-express_freeze","state_topic":"$this/express_freeze","command_topic":"$this/express_freeze/set","icon":"mdi:snowflake","name":"Express Freeze"})),
            ("door".into(), json!({"platform":"binary_sensor","device_class":"door","unique_id":"$deviceid-door","state_topic":"$this/door","name":"Door"})),
        ].into_iter().collect();
        self.core.set_config(config);
    }

    fn process_status(&self, cur: &[u8]) {
        let s = unpack_status(cur);
        let unit = if status_get(&s, "tempUnit") != 0 {
            TemperatureUnit::C
        } else {
            TemperatureUnit::F
        };
        self.set_temperature_unit(unit);
        self.core.publish_property(
            "door",
            if status_get(&s, "anyDoorOpen") == 1 {
                "ON"
            } else {
                "OFF"
            }
            .into(),
        );
        self.core.publish_property(
            "fridge_setpoint",
            PropertyValue::Int(
                convert_fridge_temperature(unit, status_get(&s, "fridgeSetpoint") as i32) as i64,
            ),
        );
        self.core.publish_property(
            "freezer_setpoint",
            PropertyValue::Int(convert_from_freezer_temperature(
                unit,
                status_get(&s, "freezerSetpoint"),
            ) as i64),
        );
        self.core.publish_property(
            "express_freeze",
            if status_get(&s, "expressFreeze") == 2 {
                "ON"
            } else {
                "OFF"
            }
            .into(),
        );
    }

    fn process_aabb(&self, buf: &[u8]) {
        if buf.len() == 2 + STATUS_LENGTH * 2 && buf[0] == 0x10 && buf[1] == 0xec {
            self.process_status(&buf[2 + STATUS_LENGTH..2 + STATUS_LENGTH * 2]);
        }
        if buf.len() == 2 + STATUS_LENGTH && buf[0] == 0x10 && buf[1] == 0xeb {
            self.process_status(&buf[2..2 + STATUS_LENGTH]);
        }
    }

    fn send_setting(&self, setting: &Status) {
        let mut payload = rethink_util::hex::decode("F017").unwrap();
        payload.extend_from_slice(&pack_status(setting, STATUS_LENGTH));
        self.core.send(&payload);
    }
}

impl DeviceHandler for Device {
    fn id(&self) -> &str {
        &self.core.id
    }
    fn start(&self) {
        self.core
            .send(&rethink_util::hex::decode("F0ED1211010000010400").unwrap());
    }
    fn drop_device(&self) {
        self.core.drop_device();
    }
    fn set_property(&self, prop: &str, mqtt_value: &str) {
        let unit = self.temperature_unit.lock().unwrap_or(TemperatureUnit::C);
        let mut setting = Status::new();
        setting.insert("tempUnit", if unit == TemperatureUnit::C { 1 } else { 0 });
        match prop {
            "fridge_setpoint" => {
                let n: i32 = mqtt_value.parse().unwrap_or(0);
                setting.insert("fridgeSetpoint", convert_fridge_temperature(unit, n) as u8);
                self.send_setting(&setting);
            }
            "freezer_setpoint" => {
                let n: i32 = mqtt_value.parse().unwrap_or(0);
                setting.insert(
                    "freezerSetpoint",
                    convert_to_freezer_temperature(unit, n) as u8,
                );
                self.send_setting(&setting);
            }
            "express_freeze" => {
                setting.insert("expressFreeze", if mqtt_value == "ON" { 2 } else { 1 });
                self.send_setting(&setting);
            }
            _ => {}
        }
    }
    fn publish_config(&self) {
        if let Some(cfg) = self.core.config.lock().clone() {
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
    use rethink_core::{hex_decode, MockHaConnection, MockThinq2Device};

    const DEVICE_ID: &str = "test-id";
    const SAMPLE_DELTA_DOOR_OPEN: &str = "AA1E10EC02030501FFFFFF0001FF01FF02030501FFFFFF0101FF01FF80BB";

    fn make() -> (Arc<MockHaConnection>, Arc<MockThinq2Device>, Arc<Device>) {
        let ha = MockHaConnection::new();
        let meta = Metadata::new("2RES1VE600FWC", "2RES1VE600FWC", "1.0");
        let thinq = MockThinq2Device::new(DEVICE_ID, meta.clone());
        (ha.clone(), thinq.clone(), Device::new(ha, thinq, meta))
    }

    #[test]
    fn first_delta_publishes_celsius() {
        let (ha, thinq, _) = make();
        thinq.emit_data(&hex_decode(SAMPLE_DELTA_DOOR_OPEN));
        let dev = ha.device(DEVICE_ID).unwrap();
        assert_eq!(
            dev.config.as_ref().unwrap().components["fridge_setpoint"]["unit_of_measurement"],
            json!("°C")
        );
        assert_eq!(
            dev.properties.get("fridge_setpoint").map(|p| p.as_string()),
            Some("5".into())
        );
        assert_eq!(
            dev.properties
                .get("freezer_setpoint")
                .map(|p| p.as_string()),
            Some("-18".into())
        );
        assert_eq!(
            dev.properties.get("door").map(|p| p.as_string()),
            Some("ON".into())
        );
    }
}
