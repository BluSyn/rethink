//! HA device base classes (HADevice, AABBDevice, TLVDevice).

use crate::ha::{DeviceDiscovery, DeviceInfo, HaConnection, OriginInfo, PropertyValue};
use crate::metadata::Metadata;
use crate::thinq::Thinq2Device;
use rethink_util::sync::Mutex;
use rethink_util::crc16::crc16;
use rethink_util::tlv::{self, Tlv};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::Arc;

/// Base HA device.
pub struct HaDeviceState {
    pub id: String,
    pub config: Option<DeviceDiscovery>,
}

pub fn default_config(meta: &Metadata, device_info: Option<Value>) -> DeviceDiscovery {
    let mut device = DeviceInfo {
        // Array form matches HA MQTT discovery examples (Zigbee2MQTT, docs)
        // and ensures device_automation triggers share the same registry identity.
        identifiers: json!(["$deviceid"]),
        manufacturer: Some("LG".into()),
        model: Some(meta.model_name.clone()),
        sw_version: meta.sw_version.clone(),
        name: None,
    };
    if let Some(Value::Object(extra)) = device_info {
        if let Some(Value::String(n)) = extra.get("name") {
            device.name = Some(n.clone());
        }
    }
    DeviceDiscovery {
        device,
        origin: OriginInfo {
            name: "rethink".into(),
            support_url: Some("https://github.com/anszom/rethink".into()),
            sw_version: None,
        },
        availability: Some(vec![
            crate::ha::AvailabilityInfo {
                topic: "$this/availability".into(),
            },
            crate::ha::AvailabilityInfo {
                topic: "$rethink/availability".into(),
            },
        ]),
        availability_mode: Some("all".into()),
        components: HashMap::new(),
    }
}

// ── AABB device ────────────────────────────────────────────────────────────

pub struct AabbDeviceCore {
    pub id: String,
    pub ha: Arc<dyn HaConnection>,
    pub thinq: Arc<dyn Thinq2Device>,
    pub publish_cache: Mutex<HashMap<String, String>>,
    pub config: Mutex<Option<DeviceDiscovery>>,
}

impl AabbDeviceCore {
    pub fn new(ha: Arc<dyn HaConnection>, thinq: Arc<dyn Thinq2Device>) -> Arc<Self> {
        Arc::new(Self {
            id: thinq.id().to_string(),
            ha,
            thinq,
            publish_cache: Mutex::new(HashMap::new()),
            config: Mutex::new(None),
        })
    }

    pub fn send(&self, inner: &[u8]) {
        let mut packet = Vec::with_capacity(inner.len() + 4);
        packet.push(0xaa);
        packet.push((inner.len() + 4) as u8);
        packet.extend_from_slice(inner);
        packet.push(0x00);
        packet.push(0x00);
        let sum: u32 = packet.iter().map(|&b| u32::from(b)).sum();
        let last = packet.len() - 2;
        packet[last] = ((sum & 0xff) as u8) ^ 0x55;
        packet[last + 1] = 0xbb;
        self.thinq.send_packet(&packet);
    }

    pub fn process_data_envelope(&self, buf: &[u8]) -> Option<Vec<u8>> {
        if buf.len() >= 4 && buf[0] == 0xaa && buf[buf.len() - 1] == 0xbb {
            Some(buf[2..buf.len() - 2].to_vec())
        } else {
            None
        }
    }

    pub fn publish_property(&self, prop: &str, value: PropertyValue) {
        let s = value.as_string();
        {
            let mut cache = self.publish_cache.lock();
            if cache.get(prop) == Some(&s) {
                return;
            }
            cache.insert(prop.to_string(), s);
        }
        self.ha.publish_property(&self.id, prop, value);
    }

    /// Publish fridge/freezer door binary state (automate on binary_sensor state).
    pub fn publish_door_with_trigger(&self, open: bool) {
        let val = if open { "ON" } else { "OFF" };
        let prev = self.publish_cache.lock().get("door").cloned();
        if prev.as_deref() == Some(val) {
            return;
        }
        self.publish_property("door", val.into());
    }

    pub fn set_config(&self, config: DeviceDiscovery) {
        *self.config.lock() = Some(config.clone());
        self.ha
            .publish_property(&self.id, "availability", "online".into());
        self.ha.publish_config(&self.id, &config);
    }

    pub fn drop_device(&self) {
        self.ha
            .publish_property(&self.id, "availability", "offline".into());
    }
}

// ── TLV device ─────────────────────────────────────────────────────────────

pub type ReadXform = Box<dyn Fn(u32) -> Option<PropertyValue> + Send + Sync>;
pub type WriteXform = Box<dyn Fn(&str) -> Option<PropertyValue> + Send + Sync>;
pub type WriteAttach = Box<dyn Fn(u32) -> Vec<u16> + Send + Sync>;
pub type ReadCallback = Box<dyn Fn(&PropertyValue) -> bool + Send + Sync>;
pub type WriteCallback = Box<dyn Fn(u32) -> bool + Send + Sync>;

pub struct FieldDefinition {
    pub id: Option<u16>,
    pub name: String,
    pub comp: String,
    pub state_topic: Option<String>,
    pub readable: bool,
    pub writable: bool,
    pub write_xform: Option<WriteXform>,
    pub write_attach: Option<WriteAttach>,
    pub write_attach_static: Option<Vec<u16>>,
    pub read_xform: Option<ReadXform>,
    pub read_callback: Option<ReadCallback>,
    pub write_callback: Option<WriteCallback>,
}

impl FieldDefinition {
    pub fn new(comp: &str, name: &str) -> Self {
        Self {
            id: None,
            name: name.into(),
            comp: comp.into(),
            state_topic: None,
            readable: true,
            writable: true,
            write_xform: None,
            write_attach: None,
            write_attach_static: None,
            read_xform: None,
            read_callback: None,
            write_callback: None,
        }
    }

    pub fn with_id(mut self, id: u16) -> Self {
        self.id = Some(id);
        self
    }

    pub fn read_only(mut self) -> Self {
        self.writable = false;
        self
    }

    pub fn write_only(mut self) -> Self {
        self.readable = false;
        self
    }
}

pub struct TlvDeviceCore {
    pub id: String,
    pub ha: Arc<dyn HaConnection>,
    pub thinq: Arc<dyn Thinq2Device>,
    pub config: Mutex<Option<DeviceDiscovery>>,
    pub fields_by_id: Mutex<HashMap<u16, Arc<FieldDefinition>>>,
    pub fields_by_ha: Mutex<HashMap<String, Arc<FieldDefinition>>>,
    pub raw_clip_state: Mutex<HashMap<u16, u32>>,
    /// true while waiting for caps
    pub waiting_caps: Mutex<bool>,
    /// true while waiting for initial values
    pub waiting_values: Mutex<bool>,
    /// Hooks for subclasses
    pub on_caps: Mutex<Option<Box<dyn Fn() + Send + Sync>>>,
    pub on_values: Mutex<Option<Box<dyn Fn() + Send + Sync>>>,
    pub is_caps_response: Mutex<Option<Box<dyn Fn(&[Tlv]) -> bool + Send + Sync>>>,
    pub is_values_response: Mutex<Option<Box<dyn Fn(&[Tlv]) -> bool + Send + Sync>>>,
    pub on_priv_data: Mutex<Option<Box<dyn Fn(u8, u8, &[u8]) + Send + Sync>>>,
    pub on_priv_cmd_resp: Mutex<Option<Box<dyn Fn(bool, u8, u8, &[u8]) + Send + Sync>>>,
    pub on_key_value_extra: Mutex<Option<Box<dyn Fn(u16, u32) + Send + Sync>>>,
    /// If set and returns true, skip default process_key_value handling (including storage).
    pub key_value_intercept: Mutex<Option<Box<dyn Fn(u16, u32) -> bool + Send + Sync>>>,
}

impl TlvDeviceCore {
    pub fn new(ha: Arc<dyn HaConnection>, thinq: Arc<dyn Thinq2Device>) -> Arc<Self> {
        let core = Arc::new(Self {
            id: thinq.id().to_string(),
            ha,
            thinq: thinq.clone(),
            config: Mutex::new(None),
            fields_by_id: Mutex::new(HashMap::new()),
            fields_by_ha: Mutex::new(HashMap::new()),
            raw_clip_state: Mutex::new(HashMap::new()),
            waiting_caps: Mutex::new(true),
            waiting_values: Mutex::new(false),
            on_caps: Mutex::new(None),
            on_values: Mutex::new(None),
            is_caps_response: Mutex::new(None),
            is_values_response: Mutex::new(None),
            on_priv_data: Mutex::new(None),
            on_priv_cmd_resp: Mutex::new(None),
            on_key_value_extra: Mutex::new(None),
            key_value_intercept: Mutex::new(None),
        });
        let c = core.clone();
        thinq.on_data(Box::new(move |data| c.process_data(data)));
        // initial caps query
        core.query_caps();
        core
    }

    pub fn set_config(&self, config: DeviceDiscovery) {
        *self.config.lock() = Some(config.clone());
        self.ha
            .publish_property(&self.id, "availability", "online".into());
        self.ha.publish_config(&self.id, &config);
    }

    pub fn add_field(&self, config: &mut DeviceDiscovery, options: FieldDefinition, autoreg: bool) {
        let full_name = format!("{}-{}", options.comp, options.name);
        let options = Arc::new(options);
        if let Some(id) = options.id {
            self.fields_by_id.lock().insert(id, options.clone());
        }
        self.fields_by_ha
            .lock()
            .insert(full_name.clone(), options.clone());

        if autoreg {
            let topic_prefix = if options.name.is_empty() {
                String::new()
            } else {
                format!("{}_", options.name)
            };
            let entry = config
                .components
                .entry(options.comp.clone())
                .or_insert_with(|| json!({}));
            if let Value::Object(map) = entry {
                if options.readable {
                    let state_topic = options.state_topic.as_deref().unwrap_or("state_topic");
                    map.insert(
                        format!("{topic_prefix}{state_topic}"),
                        json!(format!("$this/{full_name}")),
                    );
                }
                if options.writable {
                    map.insert(
                        format!("{topic_prefix}command_topic"),
                        json!(format!("$this/{full_name}/set")),
                    );
                }
            }
        }
    }

    pub fn query_caps(&self) {
        self.send(&[1, 1, 2, 2, 1], &[Tlv::new(0x1f5, 1)]);
    }

    pub fn query(&self) {
        self.send(&[1, 1, 2, 2, 1], &[Tlv::new(0x1f5, 2)]);
    }

    pub fn send(&self, header: &[u8], tlv_els: &[Tlv]) {
        let b0 = header[0];
        let b1 = header[1];
        let b2 = header.get(2).copied().unwrap_or(2);
        let b3 = header.get(3).copied().unwrap_or(2);
        let b4 = header.get(4).copied().unwrap_or(1);
        let tlv_array = tlv::build(tlv_els);
        let mut buf = vec![0x04, 0x00, 0x00, 0x00, 0x65, b2, b3, b4, tlv_array.len() as u8];
        buf.extend_from_slice(&tlv_array);
        let result = crc16(&buf);
        let mut out = vec![b0, b1];
        out.extend_from_slice(&buf);
        out.push((result >> 8) as u8);
        out.push((result & 0xff) as u8);
        self.thinq.send_packet(&out);
    }

    pub fn send_priv_command(&self, cmd: u8, cmd_sub: u8, data: &[u8]) {
        let cmd_data_len = data.len() + 1;
        let mut buf = vec![
            0x00,
            0xff,
            0x04,
            0x00,
            0x00,
            0x00,
            0x65,
            0xfd,
            cmd_sub,
            (cmd_data_len >> 8) as u8,
            (cmd_data_len & 0xff) as u8,
            cmd,
        ];
        buf.extend_from_slice(data);
        let crc = crc16(&buf[2..]);
        buf.push((crc >> 8) as u8);
        buf.push((crc & 0xff) as u8);
        self.thinq.send_packet(&buf);
    }

    pub fn process_data(&self, buf: &[u8]) {
        if buf.len() < 13 {
            return;
        }
        // Standard TLV fromDevice
        if buf[2] == 0x04
            && buf[3] == 0x00
            && buf[4] == 0x00
            && buf[5] == 0x00
            && (buf[6] == 0x87 || buf[6] == 0xa7)
            && buf[7] == 0x02
            && (buf[8] == 0x01 || buf[8] == 0x04)
            && buf[10] as usize == buf.len() - 13
        {
            let tlv = tlv::parse(&buf[11..buf.len() - 2]);
            self.process_tlv(&tlv);
            return;
        }
        // priv data
        if buf[1] == 0xff
            && buf[2] == 0x04
            && buf[3] == 0x00
            && buf[4] == 0x00
            && buf[5] == 0x00
            && buf[6] == 0x87
            && buf[7] == 0xfd
            && buf[8] == 0x03
            && buf[10] as usize == buf.len() - 13
        {
            if let Some(h) = self.on_priv_data.lock().as_ref() {
                h(buf[0], buf[9], &buf[11..buf.len() - 2]);
            }
            return;
        }
        // priv cmd response
        if (buf[0] == 0x02 || buf[0] == 0x03)
            && buf[2] == 0x04
            && buf[3] == 0x00
            && buf[4] == 0x00
            && buf[5] == 0x00
            && buf[6] == 0x87
            && buf[7] == 0xfd
            && buf[8] == 0x10
            && buf[9] == 0x00
            && buf[10] == 0x05
            && buf[11] == 0xfe
            && buf.len() > 12
        {
            if let Some(h) = self.on_priv_cmd_resp.lock().as_ref() {
                h(buf[0] == 0x02, buf[1], buf[12], &buf[13..buf.len().saturating_sub(2)]);
            }
        }
    }

    pub fn process_tlv(&self, tlv_array: &[Tlv]) {
        for el in tlv_array {
            self.process_key_value(el.t, el.v);
        }

        let is_caps = self
            .is_caps_response
            .lock()
            .as_ref()
            .map(|f| f(tlv_array))
            .unwrap_or(false);
        let is_vals = self
            .is_values_response
            .lock()
            .as_ref()
            .map(|f| f(tlv_array))
            .unwrap_or(false);

        if *self.waiting_caps.lock() && is_caps {
            *self.waiting_caps.lock() = false;
            if let Some(h) = self.on_caps.lock().as_ref() {
                h();
            }
            self.query();
            *self.waiting_values.lock() = true;
        }

        if !*self.waiting_caps.lock() && is_vals {
            if *self.waiting_values.lock() {
                *self.waiting_values.lock() = false;
            }
            if let Some(h) = self.on_values.lock().as_ref() {
                h();
            }
        }
    }

    pub fn process_key_value(&self, k: u16, v: u32) {
        if let Some(h) = self.key_value_intercept.lock().as_ref() {
            if h(k, v) {
                return;
            }
        }
        self.raw_clip_state.lock().insert(k, v);
        if let Some(h) = self.on_key_value_extra.lock().as_ref() {
            h(k, v);
        }
        let def = self.fields_by_id.lock().get(&k).cloned();
        let Some(def) = def else { return };

        let mut processed = PropertyValue::Int(v as i64);
        if let Some(ref xform) = def.read_xform {
            match xform(v) {
                Some(p) => processed = p,
                None => return,
            }
        }
        let do_read = def
            .read_callback
            .as_ref()
            .map(|cb| cb(&processed))
            .unwrap_or(true);
        if do_read && def.readable {
            let full_name = format!("{}-{}", def.comp, def.name);
            self.ha.publish_property(&self.id, &full_name, processed);
        }
    }

    pub fn set_property(&self, prop: &str, mqtt_value: &str) {
        let def = self.fields_by_ha.lock().get(prop).cloned();
        let Some(def) = def else {
            eprintln!("Attempting to set property {prop} which is not writable");
            return;
        };
        if !def.writable {
            eprintln!("Attempting to set property {prop} which is not writable");
            return;
        }

        let value = if let Some(ref xform) = def.write_xform {
            match xform(mqtt_value) {
                Some(v) => v,
                None => return,
            }
        } else {
            PropertyValue::Str(mqtt_value.into())
        };

        let num = match &value {
            PropertyValue::Int(i) => *i as u32,
            PropertyValue::Num(n) => *n as u32,
            PropertyValue::Str(s) => match s.parse::<u32>() {
                Ok(n) => n,
                Err(_) => return,
            },
        };

        let do_write = def
            .write_callback
            .as_ref()
            .map(|cb| cb(num))
            .unwrap_or(true);
        if do_write {
            if let Some(id) = def.id {
                self.raw_clip_state.lock().insert(id, num);
                let mut attach = Vec::new();
                if let Some(ref a) = def.write_attach_static {
                    attach = a.clone();
                }
                if let Some(ref a) = def.write_attach {
                    attach = a(num);
                }
                let mut write_fields = vec![id];
                write_fields.extend(attach);
                let state = self.raw_clip_state.lock();
                let tlv_array: Vec<Tlv> = write_fields
                    .iter()
                    .map(|&fid| Tlv::new(fid, *state.get(&fid).unwrap_or(&0)))
                    .collect();
                drop(state);
                self.send(&[1, 1, 2, 1, 1], &tlv_array);
            }
        }
    }

    pub fn drop_device(&self) {
        self.ha
            .publish_property(&self.id, "availability", "offline".into());
    }

    pub fn get_raw(&self, id: u16) -> Option<u32> {
        self.raw_clip_state.lock().get(&id).copied()
    }

    pub fn set_raw(&self, id: u16, v: u32) {
        self.raw_clip_state.lock().insert(id, v);
    }
}

/// Helper to build a component JSON object.
pub fn component(platform: &str, unique_id: &str) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("platform".into(), json!(platform));
    m.insert("unique_id".into(), json!(unique_id));
    m
}

pub fn insert_component(config: &mut DeviceDiscovery, key: &str, obj: Map<String, Value>) {
    config.components.insert(key.into(), Value::Object(obj));
}
