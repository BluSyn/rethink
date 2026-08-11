//! 2REF11EIDA__4 fridge — 68-byte status, F/C unit detection, flex drawer.

use crate::device_trait::DeviceHandler;
use crate::fridge_common::{
    convert_freezer_temperature, convert_fridge_temperature, freezer_range, fridge_range,
    TemperatureUnit,
};
use parking_lot::Mutex;
use rethink_core::device_base::{default_config, AabbDeviceCore};
use rethink_core::ha::{HaConnection, PropertyValue};
use rethink_core::metadata::Metadata;
use rethink_core::thinq::Thinq2Device;
use serde_json::json;
use std::sync::Arc;

const FLEX_OPTIONS: &[&str] = &[
    "Chilled Wine",
    "Deli/Snacks",
    "Cold Drink",
    "Meat/Seafood",
    "Freezer",
];

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
            (
                "fridge_setpoint".into(),
                json!({
                    "platform": "number", "device_class": "temperature",
                    "unique_id": "$deviceid-fridge_setpoint",
                    "state_topic": "$this/fridge_setpoint",
                    "command_topic": "$this/fridge_setpoint/set",
                    "name": "Fridge temperature",
                    "unit_of_measurement": fu, "min": fmin, "max": fmax,
                }),
            ),
            (
                "freezer_setpoint".into(),
                json!({
                    "platform": "number", "device_class": "temperature",
                    "unique_id": "$deviceid-freezer_setpoint",
                    "state_topic": "$this/freezer_setpoint",
                    "command_topic": "$this/freezer_setpoint/set",
                    "name": "Freezer temperature",
                    "unit_of_measurement": zu, "min": zmin, "max": zmax,
                }),
            ),
            (
                "flex_setpoint".into(),
                json!({
                    "platform": "select", "icon": "mdi:thermometer",
                    "unique_id": "$deviceid-flex_setpoint",
                    "state_topic": "$this/flex_setpoint",
                    "command_topic": "$this/flex_setpoint/set",
                    "name": "Convertible",
                    "options": FLEX_OPTIONS,
                }),
            ),
            (
                "door".into(),
                json!({
                    "platform": "binary_sensor", "device_class": "door",
                    "unique_id": "$deviceid-door", "state_topic": "$this/door", "name": "Door",
                }),
            ),
        ]
        .into_iter()
        .collect();
        config.device_triggers.push(rethink_core::DeviceTriggerDef::custom(
            "door_open",
            "opened",
            "door",
            "door_open",
        ));
        config.device_triggers.push(rethink_core::DeviceTriggerDef::custom(
            "door_closed",
            "closed",
            "door",
            "door_closed",
        ));
        self.core.set_config(config);
    }

    fn process_status(&self, cur: &[u8]) {
        if cur.len() < 14 {
            return;
        }
        let unit = if cur[8] != 0 {
            TemperatureUnit::C
        } else {
            TemperatureUnit::F
        };
        self.set_temperature_unit(unit);
        let setpoint_fridge = convert_fridge_temperature(unit, cur[1] as i32);
        let setpoint_freezer = convert_freezer_temperature(unit, cur[2] as i32);
        let any_door_open = cur[7];
        let setpoint_flex = cur[13] as usize;
        self.core.publish_door_with_trigger(any_door_open == 1);
        self.core
            .publish_property("fridge_setpoint", PropertyValue::Int(setpoint_fridge as i64));
        self.core
            .publish_property("freezer_setpoint", PropertyValue::Int(setpoint_freezer as i64));
        if setpoint_flex >= 1 && setpoint_flex <= FLEX_OPTIONS.len() {
            self.core
                .publish_property("flex_setpoint", FLEX_OPTIONS[setpoint_flex - 1].into());
        }
    }

    fn process_aabb(&self, buf: &[u8]) {
        if buf.len() == 2 + 68 * 2 && buf[0] == 0x10 && buf[1] == 0xec {
            self.process_status(&buf[2 + 68..2 + 68 + 68]);
        }
        // Note: TS has a bug `subarray(2, 2 + 68 + 68)` but only 68-byte buffer for 10EB;
        // processStatus only reads first 14+ bytes so we take 68 correctly.
        if buf.len() == 2 + 68 && buf[0] == 0x10 && buf[1] == 0xeb {
            self.process_status(&buf[2..2 + 68]);
        }
    }
}

impl DeviceHandler for Device {
    fn id(&self) -> &str {
        &self.core.id
    }
    fn start(&self) {
        self.core
            .send(&hex::decode("F0ED1211010000010400").unwrap());
    }
    fn drop_device(&self) {
        self.core.drop_device();
    }
    fn set_property(&self, prop: &str, mqtt_value: &str) {
        let unit = self.temperature_unit.lock().unwrap_or(TemperatureUnit::C);
        let mut base = hex::decode(
            "F017FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF000000FFFF00FFFFFFFF00FFFFFFFFFFFFFFFFFF00FFFFFF1EFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF0AFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF",
        )
        .unwrap();
        base[2 + 8] = if unit == TemperatureUnit::C { 1 } else { 0 };
        match prop {
            "fridge_setpoint" => {
                let n: i32 = mqtt_value.parse().unwrap_or(0);
                base[2 + 1] = convert_fridge_temperature(unit, n) as u8;
                self.core.send(&base);
            }
            "freezer_setpoint" => {
                let n: i32 = mqtt_value.parse().unwrap_or(0);
                base[2 + 2] = convert_freezer_temperature(unit, n) as u8;
                self.core.send(&base);
            }
            "flex_setpoint" => {
                if let Some(index) = FLEX_OPTIONS.iter().position(|o| *o == mqtt_value) {
                    base[2 + 13] = (1 + index) as u8;
                    self.core.send(&base);
                }
            }
            _ => eprintln!("Unknown property {prop}"),
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
    const SAMPLE_INITIAL: &str = "AA4A10EB0209060202020400000001FFFF0300FFFF00FFFFFFFFFFFFFF020001010100000102FF6161FFFFFF01FF00FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF0078FF000079BB";

    fn make() -> (Arc<MockHaConnection>, Arc<MockThinq2Device>, Arc<Device>) {
        let ha = MockHaConnection::new();
        let meta = Metadata::new("2REF11EIDA__4", "2REF11EIDA__4", "1.0");
        let thinq = MockThinq2Device::new(DEVICE_ID, meta.clone());
        (ha.clone(), thinq.clone(), Device::new(ha, thinq, meta))
    }

    #[test]
    fn no_config_until_status() {
        let (ha, _, _) = make();
        assert!(ha.device(DEVICE_ID).is_none());
    }

    #[test]
    fn initial_status_fahrenheit() {
        let (ha, thinq, _) = make();
        thinq.emit_data(&hex_decode(SAMPLE_INITIAL));
        let dev = ha.device(DEVICE_ID).unwrap();
        let comps = &dev.config.as_ref().unwrap().components;
        assert_eq!(comps["fridge_setpoint"]["unit_of_measurement"], json!("°F"));
        assert_eq!(comps["fridge_setpoint"]["min"], json!(33));
        assert_eq!(comps["fridge_setpoint"]["max"], json!(43));
        assert_eq!(comps["freezer_setpoint"]["min"], json!(-7));
        assert_eq!(comps["freezer_setpoint"]["max"], json!(5));
        assert_eq!(
            dev.properties.get("fridge_setpoint").map(|p| p.as_string()),
            Some("35".into())
        );
        assert_eq!(
            dev.properties
                .get("freezer_setpoint")
                .map(|p| p.as_string()),
            Some("0".into())
        );
        assert_eq!(
            dev.properties.get("flex_setpoint").map(|p| p.as_string()),
            Some("Cold Drink".into())
        );
        assert_eq!(
            dev.properties.get("door").map(|p| p.as_string()),
            Some("OFF".into())
        );
    }
}
