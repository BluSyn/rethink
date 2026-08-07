//! 2REB1GLVB1__2 fridge (AABB, STATUS_LENGTH=17).

use crate::device_trait::DeviceHandler;
use crate::fridge_common::{
    convert_freezer_temperature, convert_fridge_temperature, freezer_range, fridge_range,
    pack_status, status_get, unpack_status, Status, TemperatureUnit,
};
use parking_lot::Mutex;
use rethink_core::device_base::{default_config, AabbDeviceCore};
use rethink_core::hex_decode;
use rethink_core::{HaConnection, Metadata, Thinq2Device};
use serde_json::{json, Map};
use std::sync::Arc;

const STATUS_LENGTH: usize = 17;

pub struct Device {
    core: Arc<AabbDeviceCore>,
    temperature_unit: Mutex<Option<TemperatureUnit>>,
    meta: Metadata,
}

impl Device {
    pub fn new(ha: Arc<dyn HaConnection>, thinq: Arc<dyn Thinq2Device>, meta: Metadata) -> Arc<Self> {
        let core = AabbDeviceCore::new(ha, thinq.clone());
        let this = Arc::new(Self {
            core: core.clone(),
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
        {
            let mut cur = self.temperature_unit.lock();
            if *cur == Some(unit) {
                return;
            }
            *cur = Some(unit);
        }
        let (fu, fmin, fmax) = fridge_range(unit);
        let (zu, zmin, zmax) = freezer_range(unit);
        let mut base = default_config(&self.meta, Some(json!({"name": "LG Fridge"})));
        let mut components = Map::new();
        components.insert(
            "fridge_setpoint".into(),
            json!({
                "platform": "number",
                "device_class": "temperature",
                "unique_id": "$deviceid-fridge_setpoint",
                "state_topic": "$this/fridge_setpoint",
                "command_topic": "$this/fridge_setpoint/set",
                "name": "Fridge temperature",
                "unit_of_measurement": fu,
                "min": fmin,
                "max": fmax,
            }),
        );
        components.insert(
            "express_cool".into(),
            json!({
                "platform": "switch",
                "unique_id": "$deviceid-express_cool",
                "state_topic": "$this/express_cool",
                "command_topic": "$this/express_cool/set",
                "icon": "mdi:snowflake-variant",
                "name": "Express Cool",
            }),
        );
        components.insert(
            "freezer_setpoint".into(),
            json!({
                "platform": "number",
                "device_class": "temperature",
                "unique_id": "$deviceid-freezer_setpoint",
                "state_topic": "$this/freezer_setpoint",
                "command_topic": "$this/freezer_setpoint/set",
                "name": "Freezer temperature",
                "unit_of_measurement": zu,
                "min": zmin,
                "max": zmax,
            }),
        );
        components.insert(
            "express_freeze".into(),
            json!({
                "platform": "switch",
                "unique_id": "$deviceid-express_freeze",
                "state_topic": "$this/express_freeze",
                "command_topic": "$this/express_freeze/set",
                "icon": "mdi:snowflake",
                "name": "Express Freeze",
            }),
        );
        components.insert(
            "door".into(),
            json!({
                "platform": "binary_sensor",
                "device_class": "door",
                "unique_id": "$deviceid-door",
                "state_topic": "$this/door",
                "name": "Door",
            }),
        );
        base.components = components.into_iter().collect();
        self.core.set_config(base);
    }

    fn process_aabb(&self, buf: &[u8]) {
        if buf.len() == 2 + STATUS_LENGTH * 2 && buf[0] == 0x10 && buf[1] == 0xec {
            self.process_status(&buf[2 + STATUS_LENGTH..2 + STATUS_LENGTH * 2]);
        }
        if buf.len() == 2 + STATUS_LENGTH && buf[0] == 0x10 && buf[1] == 0xeb {
            self.process_status(&buf[2..2 + STATUS_LENGTH]);
        }
    }

    fn process_status(&self, cur_status: &[u8]) {
        let s = unpack_status(cur_status);
        let unit = if status_get(&s, "tempUnit") != 0 {
            TemperatureUnit::C
        } else {
            TemperatureUnit::F
        };
        self.set_temperature_unit(unit);
        let unit = self.temperature_unit.lock().unwrap_or(TemperatureUnit::C);
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
            convert_fridge_temperature(unit, status_get(&s, "fridgeSetpoint") as i32).into(),
        );
        self.core.publish_property(
            "freezer_setpoint",
            convert_freezer_temperature(unit, status_get(&s, "freezerSetpoint") as i32).into(),
        );
        self.core.publish_property(
            "express_cool",
            if status_get(&s, "expressCool") == 1 {
                "ON"
            } else {
                "OFF"
            }
            .into(),
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

    fn send_setting(&self, setting: &Status) {
        let mut pkt = hex_decode("F017");
        pkt.extend(pack_status(setting, STATUS_LENGTH));
        self.core.send(&pkt);
    }

    pub fn set_property(&self, prop: &str, mqtt_value: &str) {
        let unit = self.temperature_unit.lock().unwrap_or(TemperatureUnit::C);
        let mut setting = Status::new();
        setting.insert("tempUnit", if unit == TemperatureUnit::C { 1 } else { 0 });

        if prop == "fridge_setpoint" {
            let v: i32 = mqtt_value.parse().unwrap_or(0);
            setting.insert(
                "fridgeSetpoint",
                convert_fridge_temperature(unit, v) as u8,
            );
            self.send_setting(&setting);
        } else if prop == "freezer_setpoint" {
            let v: i32 = mqtt_value.parse().unwrap_or(0);
            setting.insert(
                "freezerSetpoint",
                convert_freezer_temperature(unit, v) as u8,
            );
            self.send_setting(&setting);
        } else if prop == "express_cool" {
            setting.insert("expressCool", if mqtt_value == "ON" { 1 } else { 0 });
            self.send_setting(&setting);
        } else if prop == "express_freeze" {
            setting.insert("expressFreeze", if mqtt_value == "ON" { 2 } else { 1 });
            self.send_setting(&setting);
        }
    }
}

impl DeviceHandler for Device {
    fn id(&self) -> &str {
        &self.core.id
    }
    fn start(&self) {
        self.core.send(&hex_decode("F0ED1211010000010400"));
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
