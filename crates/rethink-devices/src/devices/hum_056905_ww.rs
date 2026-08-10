//! HUM_056905_WW — LG PuriCare humidifying air purifier (deviceType 404, PR #114).
//!
//! Wire map from modelJSON TLV labels + cloud-confirmed enums.

use crate::device_trait::DeviceHandler;
use rethink_core::device_base::{default_config, FieldDefinition, TlvDeviceCore};
use rethink_core::ha::{DeviceDiscovery, HaConnection, PropertyValue};
use rethink_core::metadata::Metadata;
use rethink_core::thinq::Thinq2Device;
use serde_json::json;
use std::sync::Arc;

const HALF_DEGREE: f64 = 2.0;
const AUTO_DRY_ON: u32 = 252;

const OP_MODES: &[(&str, u32)] = &[
    ("Air Clean", 5),
    ("Humidify+Clean", 12),
    ("Humidify", 24),
];

const FAN_LEVELS: &[(&str, u32)] = &[
    ("Auto", 8),
    ("Low", 2),
    ("Mid", 4),
    ("High", 6),
    ("Turbo", 7),
];

const HYGIENE_DRY: &[(&str, u32)] = &[
    ("Off", 0),
    ("Gentle", 1),
    ("Quiet", 2),
    ("Quick", 3),
    ("Focus", 4),
    ("Default", 5),
];

const STANDBY_STERILIZE: &[(&str, u32)] =
    &[("Rapid", 0), ("Normal", 1), ("Silent", 2), ("Power", 3)];

const DISPLAY_BRIGHTNESS: &[(&str, u32)] =
    &[("Off", 0), ("1Level", 8), ("2Level", 9), ("3Level", 10)];

const SENSOR_MON: &[(&str, u32)] = &[("While running only", 0), ("Always", 1)];

const MOOD_COLORS: &[(&str, u32)] = &[
    ("Blue", 1),
    ("Green", 2),
    ("Red", 3),
    ("Purple", 4),
    ("Yellow", 6),
    ("Pink", 7),
    ("Pure White", 8),
    ("Aquarium", 9),
    ("Candlelight", 10),
    ("Sunlight", 11),
    ("Rose", 12),
    ("Mint", 13),
    ("Lime", 14),
    ("Very Peri", 15),
    ("Warm White", 16),
    ("Cool White", 17),
    ("Sapphire", 18),
    ("Rainbow", 19),
];

fn product_status(raw: u32) -> String {
    match raw {
        0 => "Normal".into(),
        1 => "No tank".into(),
        2 => "Low water".into(),
        3 => "Sterilizing".into(),
        4 => "Drying".into(),
        5 => "Boiling".into(),
        6 => "Boiling (low)".into(),
        n => format!("State{n}"),
    }
}

fn label_for(levels: &[(&str, u32)], raw: u32) -> Option<String> {
    levels
        .iter()
        .find(|(_, w)| *w == raw)
        .map(|(l, _)| (*l).to_string())
}

fn wire_for(levels: &[(&str, u32)], label: &str) -> Option<u32> {
    levels
        .iter()
        .find(|(l, _)| *l == label)
        .map(|(_, w)| *w)
}

pub struct Device {
    pub core: Arc<TlvDeviceCore>,
}

impl Device {
    pub fn new(ha: Arc<dyn HaConnection>, thinq: Arc<dyn Thinq2Device>, meta: Metadata) -> Arc<Self> {
        let core = TlvDeviceCore::new(ha.clone(), thinq.clone());
        let this = Arc::new(Self { core: core.clone() });

        *core.is_caps_response.lock() = Some(Box::new(|tlv| tlv.iter().any(|e| e.t == 0x3e8)));
        *core.is_values_response.lock() = Some(Box::new(|tlv| {
            tlv.iter()
                .any(|e| matches!(e.t, 0x1f7 | 0x1f9 | 0x1fa))
        }));

        let t_extra = this.clone();
        *core.on_key_value_extra.lock() = Some(Box::new(move |k, v| {
            if k == 0x1e3 {
                t_extra.core.ha.publish_property(
                    &t_extra.core.id,
                    "water_lack-",
                    if v == 2 { "ON".into() } else { "OFF".into() },
                );
            }
        }));

        let mut config = default_config(
            &meta,
            Some(json!({"name": "LG Humidifying Air Purifier"})),
        );
        config.components.insert(
            "humidifier".into(),
            json!({
                "platform": "humidifier",
                "unique_id": "$deviceid-humidifier",
                "name": null,
                "device_class": "humidifier",
                "modes": OP_MODES.iter().map(|(l, _)| *l).collect::<Vec<_>>(),
                "min_humidity": 0,
                "max_humidity": 100,
            }),
        );
        config.components.insert(
            "water_lack".into(),
            json!({
                "platform": "binary_sensor",
                "unique_id": "$deviceid-water_lack",
                "name": "Low water",
                "device_class": "problem",
                "icon": "mdi:water-off",
                "payload_on": "ON",
                "payload_off": "OFF",
                "state_topic": "$this/water_lack-",
            }),
        );

        this.add_fields(&mut config);

        if let Some(serde_json::Value::Object(hum)) = config.components.get_mut("humidifier") {
            hum.insert("state_topic".into(), json!("$this/humidifier-power"));
            hum.insert(
                "command_topic".into(),
                json!("$this/humidifier-power/set"),
            );
            hum.insert(
                "current_humidity_topic".into(),
                json!("$this/current_humidity-"),
            );
        }

        core.set_config(config);
        this
    }

    fn add_select(
        core: &Arc<TlvDeviceCore>,
        config: &mut DeviceDiscovery,
        id: u16,
        comp: &str,
        desc: &str,
        icon: &str,
        levels: &'static [(&'static str, u32)],
    ) {
        config.components.insert(
            comp.into(),
            json!({
                "platform": "select",
                "unique_id": format!("$deviceid-{comp}"),
                "name": desc,
                "icon": icon,
                "options": levels.iter().map(|(l, _)| *l).collect::<Vec<_>>(),
            }),
        );
        let mut f = FieldDefinition::new(comp, "").with_id(id);
        f.read_xform = Some(Box::new(move |raw| {
            label_for(levels, raw).map(|s| s.into())
        }));
        f.write_xform = Some(Box::new(move |val| {
            wire_for(levels, val).map(|w| PropertyValue::Int(w as i64))
        }));
        core.add_field(config, f, true);
    }

    fn add_switch(
        core: &Arc<TlvDeviceCore>,
        config: &mut DeviceDiscovery,
        id: u16,
        comp: &str,
        desc: &str,
        icon: &str,
        on_value: u32,
    ) {
        config.components.insert(
            comp.into(),
            json!({
                "platform": "switch",
                "unique_id": format!("$deviceid-{comp}"),
                "name": desc,
                "icon": icon,
            }),
        );
        let mut f = FieldDefinition::new(comp, "").with_id(id);
        f.read_xform = Some(Box::new(move |raw| {
            Some(if raw == on_value {
                "ON".into()
            } else {
                "OFF".into()
            })
        }));
        f.write_xform = Some(Box::new(move |val| {
            Some(if val == "ON" {
                PropertyValue::Int(on_value as i64)
            } else {
                0i64.into()
            })
        }));
        core.add_field(config, f, true);
    }

    fn add_number(
        core: &Arc<TlvDeviceCore>,
        config: &mut DeviceDiscovery,
        id: u16,
        comp: &str,
        desc: &str,
        icon: &str,
        min: i64,
        max: i64,
        step: i64,
    ) {
        config.components.insert(
            comp.into(),
            json!({
                "platform": "number",
                "unique_id": format!("$deviceid-{comp}"),
                "name": desc,
                "icon": icon,
                "min": min,
                "max": max,
                "step": step,
                "mode": "box",
            }),
        );
        let mut f = FieldDefinition::new(comp, "").with_id(id);
        f.write_xform = Some(Box::new(|val| {
            let n: f64 = val.parse().ok()?;
            Some(PropertyValue::Int(n.round() as i64))
        }));
        core.add_field(config, f, true);
    }

    fn add_sensor(
        core: &Arc<TlvDeviceCore>,
        config: &mut DeviceDiscovery,
        id: u16,
        comp: &str,
        desc: &str,
        attrs: serde_json::Value,
        read_xform: Option<Box<dyn Fn(u32) -> Option<PropertyValue> + Send + Sync>>,
    ) {
        let mut obj = attrs.as_object().cloned().unwrap_or_default();
        obj.insert("platform".into(), json!("sensor"));
        obj.insert("unique_id".into(), json!(format!("$deviceid-{comp}")));
        obj.insert("name".into(), json!(desc));
        config
            .components
            .insert(comp.into(), serde_json::Value::Object(obj));
        let mut f = FieldDefinition::new(comp, "").with_id(id).read_only();
        if let Some(xf) = read_xform {
            f.read_xform = Some(xf);
        }
        core.add_field(config, f, true);
    }

    fn add_fields(self: &Arc<Self>, config: &mut DeviceDiscovery) {
        let core = &self.core;

        // power 0x1f7 — autoreg false; wired to humidifier state topics manually
        {
            let mut power = FieldDefinition::new("humidifier", "power").with_id(0x1f7);
            power.write_xform = Some(Box::new(|val| {
                Some(if val == "ON" {
                    1i64.into()
                } else {
                    0i64.into()
                })
            }));
            power.read_xform = Some(Box::new(|raw| {
                Some(if raw != 0 { "ON".into() } else { "OFF".into() })
            }));
            core.add_field(config, power, false);
        }

        // mode 0x1f9
        {
            let core_w = core.clone();
            let mut mode = FieldDefinition::new("humidifier", "mode").with_id(0x1f9);
            mode.read_xform = Some(Box::new(|raw| {
                Some(
                    label_for(OP_MODES, raw)
                        .unwrap_or_else(|| format!("mode{raw}"))
                        .into(),
                )
            }));
            mode.write_xform = Some(Box::new(move |val| {
                let wire = wire_for(OP_MODES, val)?;
                // Selecting a mode while off is how the LG app turns it on.
                core_w.set_raw(0x1f7, 1);
                Some(PropertyValue::Int(wire as i64))
            }));
            mode.write_attach_static = Some(vec![0x1f7]);
            core.add_field(config, mode, true);
        }

        // target humidity 0x253 — snap to 5%
        {
            let mut f = FieldDefinition::new("humidifier", "target_humidity").with_id(0x253);
            f.read_xform = Some(Box::new(|raw| Some(PropertyValue::Int(raw as i64))));
            f.write_xform = Some(Box::new(|val| {
                let n: f64 = val.parse().ok()?;
                let snapped = ((n / 5.0).round() * 5.0) as i64;
                Some(PropertyValue::Int(snapped))
            }));
            core.add_field(config, f, true);
        }

        // current humidity 0x336 — also as standalone sensor topic used by humidifier
        {
            config.components.insert(
                "current_humidity".into(),
                json!({
                    "platform": "sensor",
                    "unique_id": "$deviceid-current_humidity",
                    "name": "Current humidity",
                    "device_class": "humidity",
                    "unit_of_measurement": "%",
                    "state_class": "measurement",
                    "state_topic": "$this/current_humidity-",
                }),
            );
            let ha = core.ha.clone();
            let id = core.id.clone();
            let mut f = FieldDefinition::new("humidifier", "current_humidity")
                .with_id(0x336)
                .read_only();
            f.read_callback = Some(Box::new(move |val| {
                ha.publish_property(&id, "current_humidity-", val.clone());
                true
            }));
            core.add_field(config, f, true);
        }

        Self::add_select(core, config, 0x1fa, "fan_speed", "Fan speed", "mdi:fan", FAN_LEVELS);
        Self::add_select(
            core,
            config,
            0x1e9,
            "hygiene_dry",
            "Sanitary dry",
            "mdi:hair-dryer",
            HYGIENE_DRY,
        );
        Self::add_select(
            core,
            config,
            0x164,
            "standby_sterilize",
            "Standby sterilize",
            "mdi:shimmer",
            STANDBY_STERILIZE,
        );
        Self::add_select(
            core,
            config,
            0x21f,
            "display",
            "Screen brightness",
            "mdi:brightness-6",
            DISPLAY_BRIGHTNESS,
        );
        Self::add_select(
            core,
            config,
            0x337,
            "sensor_mon",
            "Air quality sensor",
            "mdi:motion-sensor",
            SENSOR_MON,
        );
        Self::add_select(
            core,
            config,
            0x3e0,
            "mood_color",
            "Mood light color",
            "mdi:palette",
            MOOD_COLORS,
        );

        Self::add_switch(core, config, 0x109, "night_mode", "Night mode", "mdi:weather-night", 1);
        Self::add_switch(core, config, 0x1e4, "sleep_mode", "Sleep mode", "mdi:power-sleep", 1);
        Self::add_switch(
            core,
            config,
            0x1e6,
            "auto_strength",
            "Auto operation",
            "mdi:autorenew",
            1,
        );
        Self::add_switch(core, config, 0x1e7, "humidify", "Humidify", "mdi:water", 1);
        Self::add_switch(
            core,
            config,
            0x117,
            "over_prevention",
            "Over-humidification prevention",
            "mdi:water-alert",
            1,
        );
        Self::add_switch(core, config, 0x161, "anti_glare", "Anti-glare", "mdi:eye-off", 1);
        Self::add_switch(
            core,
            config,
            0x1b8,
            "mood_light",
            "Mood light",
            "mdi:lightbulb-on",
            1,
        );
        Self::add_switch(
            core,
            config,
            0x3a0,
            "bell_sound",
            "Notification sound",
            "mdi:bell",
            1,
        );
        Self::add_switch(
            core,
            config,
            0x20e,
            "auto_dry",
            "Auto dry",
            "mdi:fan-auto",
            AUTO_DRY_ON,
        );

        Self::add_number(
            core,
            config,
            0x21e,
            "watertank_light",
            "Tank light brightness",
            "mdi:lightbulb",
            0,
            200,
            1,
        );
        Self::add_number(
            core,
            config,
            0x21b,
            "off_timer",
            "Off timer (min)",
            "mdi:timer-off",
            0,
            720,
            10,
        );
        Self::add_number(
            core,
            config,
            0x35a,
            "start_time",
            "Scheduled on time(HHMM)",
            "mdi:clock-start",
            0,
            2400,
            10,
        );
        Self::add_number(
            core,
            config,
            0x35b,
            "stop_time",
            "Scheduled off time(HHMM)",
            "mdi:clock-end",
            0,
            2400,
            10,
        );

        Self::add_sensor(
            core,
            config,
            0x1fd,
            "temperature",
            "Current temperature",
            json!({
                "device_class": "temperature",
                "unit_of_measurement": "°C",
                "state_class": "measurement",
            }),
            Some(Box::new(|raw| {
                Some(PropertyValue::from(raw as f64 / HALF_DEGREE))
            })),
        );
        Self::add_sensor(
            core,
            config,
            0x333,
            "pm1",
            "PM1.0",
            json!({
                "device_class": "pm1",
                "unit_of_measurement": "µg/m³",
                "state_class": "measurement",
            }),
            None,
        );
        Self::add_sensor(
            core,
            config,
            0x334,
            "pm25",
            "PM2.5",
            json!({
                "device_class": "pm25",
                "unit_of_measurement": "µg/m³",
                "state_class": "measurement",
            }),
            None,
        );
        Self::add_sensor(
            core,
            config,
            0x335,
            "pm10",
            "PM10",
            json!({
                "device_class": "pm10",
                "unit_of_measurement": "µg/m³",
                "state_class": "measurement",
            }),
            None,
        );
        Self::add_sensor(
            core,
            config,
            0x240,
            "air_quality",
            "Overall air quality",
            json!({
                "icon": "mdi:air-filter",
                "state_class": "measurement",
            }),
            None,
        );
        Self::add_sensor(
            core,
            config,
            0x1ee,
            "watertank_remain",
            "Tank level",
            json!({
                "icon": "mdi:cup-water",
                "unit_of_measurement": "%",
                "state_class": "measurement",
            }),
            None,
        );
        Self::add_sensor(
            core,
            config,
            0x2ad,
            "watertank_time",
            "Tank time remaining",
            json!({
                "icon": "mdi:timer-sand",
                "unit_of_measurement": "min",
            }),
            None,
        );
        Self::add_sensor(
            core,
            config,
            0x1ed,
            "water_filter",
            "Water filter level",
            json!({
                "icon": "mdi:filter",
                "unit_of_measurement": "%",
                "state_class": "measurement",
            }),
            None,
        );
        Self::add_sensor(
            core,
            config,
            0x355,
            "filter_used",
            "Filter usage hours",
            json!({
                "icon": "mdi:filter-outline",
                "entity_category": "diagnostic",
            }),
            None,
        );
        Self::add_sensor(
            core,
            config,
            0x356,
            "filter_max",
            "Filter replacement cycle",
            json!({
                "icon": "mdi:filter-cog",
                "entity_category": "diagnostic",
            }),
            None,
        );
        Self::add_sensor(
            core,
            config,
            0x225,
            "auto_dry_remain",
            "Auto dry remaining",
            json!({
                "icon": "mdi:timer-sand",
                "unit_of_measurement": "min",
            }),
            None,
        );
        Self::add_sensor(
            core,
            config,
            0x1e3,
            "status",
            "Operating state",
            json!({
                "icon": "mdi:information-outline",
            }),
            Some(Box::new(|raw| Some(product_status(raw).into()))),
        );
        Self::add_sensor(
            core,
            config,
            0x221,
            "error",
            "Error code",
            json!({
                "icon": "mdi:alert-circle-outline",
                "entity_category": "diagnostic",
            }),
            None,
        );
    }

    pub fn set_property(&self, prop: &str, value: &str) {
        self.core.set_property(prop, value);
    }

    pub fn process_key_value(&self, k: u16, v: u32) {
        self.core.process_key_value(k, v);
    }

    pub fn drop(&self) {
        self.core.drop_device();
    }
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
        self.core.set_property(prop, value);
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
    use rethink_core::{hex_decode, MockHaConnection, MockThinq2Device};
    use rethink_util::tlv::{self, Tlv};

    const DEVICE_ID: &str = "test-id";
    const CAPS_RESPONSE_HEX: &str =
        "000004000000a702012778640f644f6c4cb00eb0601020b480a5c0965020b0a001d4b4e05000b5b020100057c3580f79507bb6a04956b6e04378b700b750feb9501eb99046bc200800bc70020201b3c0bd1031bd60099fbd85d3c0d400dd04f7905af7d014fa01fb0bf870fffc40ef83b5c5b600b642b5ccb600b642b5d018b600b642e059";
    const QUERY_RESPONSE_HEX: &str =
        "000004000000a702042d9d6880698069c06a006a406a806ac06b006b406b806bc06c006cc06e405902fa807dc17e457e827f50374241584179c07a437a807b50647b8fb4501cb7901c86c0870087c08940898094d041d8808790c8cd46cd05ccc49001cd90308840d5503ed59064ab007f00e801e900e94ae989ebc1ed08ed40ee405cf0180c055d00f80b6e0178c179007980ab40b5c5b600b642b5ccb600b642b5d018b600b64290c1";

    fn make() -> (Arc<MockHaConnection>, Arc<MockThinq2Device>, Arc<Device>) {
        let ha = MockHaConnection::new();
        let meta = Metadata::new("HUM_056905_WW", "TEST HUM", "4378");
        let thinq = MockThinq2Device::new(DEVICE_ID, meta.clone());
        let dev = Device::new(ha.clone(), thinq.clone(), meta);
        let d = dev.clone();
        ha.on_set_property(move |id, prop, value| {
            if id == DEVICE_ID {
                d.set_property(prop, value);
            }
        });
        (ha, thinq, dev)
    }

    fn build_ready() -> (Arc<MockHaConnection>, Arc<MockThinq2Device>, Arc<Device>) {
        let (ha, thinq, dev) = make();
        thinq.reset_recorder();
        thinq.emit_data(&hex_decode(CAPS_RESPONSE_HEX));
        thinq.emit_data(&hex_decode(QUERY_RESPONSE_HEX));
        thinq.reset_recorder();
        (ha, thinq, dev)
    }

    fn written_fields(thinq: &MockThinq2Device) -> Vec<(u16, u32)> {
        let packet = thinq.outbox().last().cloned().expect("packet sent");
        // UART TLV starts after 2-byte reliability + 9-byte header, ends before 2-byte CRC
        tlv::parse(&packet[11..packet.len() - 2])
            .into_iter()
            .map(|t| (t.t, t.v))
            .collect()
    }

    #[test]
    fn captured_values_populate_entities() {
        let (ha, _, _) = build_ready();
        let p = ha.device(DEVICE_ID).unwrap().properties;
        assert_eq!(
            p.get("humidifier-power").map(|x| x.as_string()).as_deref(),
            Some("ON")
        );
        assert_eq!(
            p.get("humidifier-mode").map(|x| x.as_string()).as_deref(),
            Some("Air Clean")
        );
        assert_eq!(
            p.get("humidifier-target_humidity")
                .map(|x| x.as_string())
                .as_deref(),
            Some("65")
        );
        assert_eq!(
            p.get("fan_speed-").map(|x| x.as_string()).as_deref(),
            Some("Low")
        );
        assert_eq!(
            p.get("humidifier-current_humidity")
                .map(|x| x.as_string())
                .as_deref(),
            Some("48")
        );
        assert_eq!(
            p.get("temperature-").map(|x| x.as_string()).as_deref(),
            Some("27.5")
        );
        assert_eq!(
            p.get("night_mode-").map(|x| x.as_string()).as_deref(),
            Some("ON")
        );
        assert_eq!(
            p.get("sleep_mode-").map(|x| x.as_string()).as_deref(),
            Some("OFF")
        );
        assert_eq!(
            p.get("status-").map(|x| x.as_string()).as_deref(),
            Some("No tank")
        );
        assert_eq!(
            p.get("water_lack-").map(|x| x.as_string()).as_deref(),
            Some("OFF")
        );
    }

    #[test]
    fn config_offers_mode_and_fan_vocab() {
        let (ha, _, _) = build_ready();
        let comps = ha.device(DEVICE_ID).unwrap().config.unwrap().components;
        assert_eq!(
            comps["humidifier"]["modes"],
            json!(["Air Clean", "Humidify+Clean", "Humidify"])
        );
        assert_eq!(
            comps["fan_speed"]["options"],
            json!(["Auto", "Low", "Mid", "High", "Turbo"])
        );
    }

    #[test]
    fn every_mode_decodes_and_writes_with_power_on() {
        let (ha, thinq, dev) = build_ready();
        for (label, wire) in OP_MODES {
            dev.process_key_value(0x1f9, *wire);
            assert_eq!(
                ha.device(DEVICE_ID)
                    .unwrap()
                    .properties
                    .get("humidifier-mode")
                    .map(|x| x.as_string())
                    .as_deref(),
                Some(*label)
            );
            thinq.reset_recorder();
            dev.set_property("humidifier-mode", label);
            let fields = written_fields(&thinq);
            assert!(fields.contains(&(0x1f9, *wire)), "mode write {label}");
            assert!(fields.contains(&(0x1f7, 1)), "power attach for {label}");
        }
    }

    #[test]
    fn every_fan_decodes_and_writes_1fa_alone() {
        let (ha, thinq, dev) = build_ready();
        for (label, wire) in FAN_LEVELS {
            dev.process_key_value(0x1fa, *wire);
            assert_eq!(
                ha.device(DEVICE_ID)
                    .unwrap()
                    .properties
                    .get("fan_speed-")
                    .map(|x| x.as_string())
                    .as_deref(),
                Some(*label)
            );
            thinq.reset_recorder();
            dev.set_property("fan_speed-", label);
            assert_eq!(written_fields(&thinq), vec![(0x1fa, *wire)]);
        }
    }

    #[test]
    fn target_humidity_snaps_to_5pct() {
        let (_, thinq, dev) = build_ready();
        thinq.reset_recorder();
        dev.set_property("humidifier-target_humidity", "52");
        assert_eq!(written_fields(&thinq), vec![(0x253, 50)]);
    }

    #[test]
    fn auto_dry_uses_252_not_1() {
        let (ha, thinq, dev) = build_ready();
        dev.process_key_value(0x20e, 252);
        assert_eq!(
            ha.device(DEVICE_ID)
                .unwrap()
                .properties
                .get("auto_dry-")
                .map(|x| x.as_string())
                .as_deref(),
            Some("ON")
        );
        dev.process_key_value(0x20e, 0);
        assert_eq!(
            ha.device(DEVICE_ID)
                .unwrap()
                .properties
                .get("auto_dry-")
                .map(|x| x.as_string())
                .as_deref(),
            Some("OFF")
        );
        thinq.reset_recorder();
        dev.set_property("auto_dry-", "ON");
        assert_eq!(written_fields(&thinq), vec![(0x20e, 252)]);
    }

    #[test]
    fn water_lack_from_product_status() {
        let (ha, _, dev) = build_ready();
        dev.process_key_value(0x1e3, 2);
        let p = ha.device(DEVICE_ID).unwrap().properties;
        assert_eq!(
            p.get("water_lack-").map(|x| x.as_string()).as_deref(),
            Some("ON")
        );
        assert_eq!(
            p.get("status-").map(|x| x.as_string()).as_deref(),
            Some("Low water")
        );
        dev.process_key_value(0x1e3, 0);
        let p = ha.device(DEVICE_ID).unwrap().properties;
        assert_eq!(
            p.get("water_lack-").map(|x| x.as_string()).as_deref(),
            Some("OFF")
        );
    }

    #[test]
    fn unused_helper_keeps_tlv_import() {
        // ensure Tlv is available if tests extend fixtures
        let _ = Tlv::new(0x1f7, 1);
    }
}
