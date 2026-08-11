//! Home Assistant MQTT discovery types and connection trait.

use crate::config::HaConfig;
use rethink_util::sync::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub identifiers: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manufacturer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sw_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OriginInfo {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub support_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sw_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvailabilityInfo {
    pub topic: String,
}

/// MQTT Device Trigger definition (HA device_automation discovery).
///
/// Published to `{discovery_prefix}/device_automation/{device_id}/{object_id}/config`
/// and fired on `{rethink_prefix}/{device_id}/{topic_suffix}`.
#[derive(Debug, Clone)]
pub struct DeviceTriggerDef {
    /// Discovery object_id path segment (e.g. `bucket_full`).
    pub object_id: String,
    /// HA trigger type (e.g. `problem`, `turned_on`, `button_short_press`).
    pub type_: String,
    /// HA trigger subtype (e.g. `bucket_full`) — unique with type per device.
    pub subtype: String,
    /// Payload HA matches on the event topic.
    pub payload: String,
    /// Event topic under the device (`triggers/bucket_full` → `rethink/{id}/triggers/bucket_full`).
    pub topic_suffix: String,
}

impl DeviceTriggerDef {
    /// Problem-class notification trigger (bucket full, filter, etc.).
    pub fn problem(object_id: &str, subtype: &str, payload: &str) -> Self {
        Self {
            object_id: object_id.into(),
            type_: "problem".into(),
            subtype: subtype.into(),
            payload: payload.into(),
            topic_suffix: format!("triggers/{object_id}"),
        }
    }

    /// Generic named trigger (cycle complete, door open, …).
    pub fn custom(object_id: &str, type_: &str, subtype: &str, payload: &str) -> Self {
        Self {
            object_id: object_id.into(),
            type_: type_.into(),
            subtype: subtype.into(),
            payload: payload.into(),
            topic_suffix: format!("triggers/{object_id}"),
        }
    }
}

/// Device discovery document (HA MQTT discovery).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceDiscovery {
    pub device: DeviceInfo,
    pub origin: OriginInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub availability: Option<Vec<AvailabilityInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub availability_mode: Option<String>,
    pub components: HashMap<String, Value>,
    /// Extra MQTT device triggers (not part of HA device discovery JSON).
    #[serde(skip)]
    pub device_triggers: Vec<DeviceTriggerDef>,
}

/// Trait for publishing to HA MQTT (real connection or mock).
pub trait HaConnection: Send + Sync {
    fn publish_config(&self, id: &str, config: &DeviceDiscovery);
    fn publish_property(&self, id: &str, property: &str, value: PropertyValue);
    /// Fire a one-shot event (e.g. device trigger payload). Not retained.
    fn publish_event(&self, id: &str, topic_suffix: &str, payload: &str);
    fn is_connected(&self) -> bool;
}

#[derive(Debug, Clone, PartialEq)]
pub enum PropertyValue {
    Str(String),
    Num(f64),
    Int(i64),
}

impl PropertyValue {
    pub fn as_string(&self) -> String {
        match self {
            PropertyValue::Str(s) => s.clone(),
            PropertyValue::Num(n) => {
                if *n == (*n as i64) as f64 {
                    format!("{}", *n as i64)
                } else {
                    n.to_string()
                }
            }
            PropertyValue::Int(i) => i.to_string(),
        }
    }

    pub fn from_num(n: impl Into<f64>) -> Self {
        let v = n.into();
        if v == (v as i64) as f64 && v.abs() < 1e15 {
            PropertyValue::Int(v as i64)
        } else {
            PropertyValue::Num(v)
        }
    }
}

impl From<&str> for PropertyValue {
    fn from(s: &str) -> Self {
        PropertyValue::Str(s.into())
    }
}

impl From<String> for PropertyValue {
    fn from(s: String) -> Self {
        PropertyValue::Str(s)
    }
}

impl From<i32> for PropertyValue {
    fn from(n: i32) -> Self {
        PropertyValue::Int(n as i64)
    }
}

impl From<i64> for PropertyValue {
    fn from(n: i64) -> Self {
        PropertyValue::Int(n)
    }
}

impl From<u32> for PropertyValue {
    fn from(n: u32) -> Self {
        PropertyValue::Int(n as i64)
    }
}

impl From<f64> for PropertyValue {
    fn from(n: f64) -> Self {
        PropertyValue::from_num(n)
    }
}

/// Mock HA connection for unit tests.
#[derive(Default)]
pub struct MockHaConnection {
    inner: Mutex<MockHaInner>,
    set_handlers: Mutex<Vec<Box<dyn Fn(&str, &str, &str) + Send + Sync>>>,
}

#[derive(Default)]
struct MockHaInner {
    devices: HashMap<String, MockDeviceInfo>,
}

#[derive(Debug, Clone, Default)]
pub struct MockDeviceInfo {
    pub config: Option<DeviceDiscovery>,
    pub availability: Option<String>,
    pub properties: HashMap<String, PropertyValue>,
    /// (topic_suffix, payload) non-retained events
    pub events: Vec<(String, String)>,
    /// Published device-trigger discovery object_ids
    pub device_triggers: Vec<String>,
}

impl MockHaConnection {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn on_set_property<F>(&self, f: F)
    where
        F: Fn(&str, &str, &str) + Send + Sync + 'static,
    {
        self.set_handlers.lock().push(Box::new(f));
    }

    pub fn devices(&self) -> HashMap<String, MockDeviceInfo> {
        self.inner.lock().devices.clone()
    }

    pub fn device(&self, id: &str) -> Option<MockDeviceInfo> {
        self.inner.lock().devices.get(id).cloned()
    }

    pub fn get_property(&self, id: &str, component: &str, topic_id: &str) -> Option<PropertyValue> {
        let dev = self.device(id)?;
        let topic = self.lookup_topic(&dev, component, topic_id)?;
        dev.properties.get(&topic).cloned()
    }

    pub fn lookup_topic(&self, dev: &MockDeviceInfo, component: &str, topic_id: &str) -> Option<String> {
        let comp = dev.config.as_ref()?.components.get(component)?;
        let key = format!("{topic_id}_topic");
        let topic = comp.get(&key)?.as_str()?;
        Some(topic.strip_prefix("$this/").unwrap_or(topic).to_string())
    }

    pub fn set_property(&self, id: &str, component: &str, topic_id: &str, value: &str) {
        let dev = match self.device(id) {
            Some(d) => d,
            None => return,
        };
        let topic = match self.lookup_topic(&dev, component, topic_id) {
            Some(t) => t,
            None => return,
        };
        let prop = topic.strip_suffix("/set").unwrap_or(&topic);
        for h in self.set_handlers.lock().iter() {
            h(id, prop, value);
        }
    }

    pub fn emit_set_property(&self, id: &str, prop: &str, value: &str) {
        for h in self.set_handlers.lock().iter() {
            h(id, prop, value);
        }
    }
}

impl HaConnection for MockHaConnection {
    fn publish_config(&self, id: &str, config: &DeviceDiscovery) {
        let mut inner = self.inner.lock();
        let entry = inner.devices.entry(id.to_string()).or_default();
        entry.config = Some(config.clone());
        entry.device_triggers = config
            .device_triggers
            .iter()
            .map(|t| t.object_id.clone())
            .collect();
    }

    fn publish_property(&self, id: &str, property: &str, value: PropertyValue) {
        let mut inner = self.inner.lock();
        let entry = inner.devices.entry(id.to_string()).or_default();
        if property == "availability" {
            entry.availability = Some(value.as_string());
        } else {
            entry.properties.insert(property.to_string(), value);
        }
    }

    fn publish_event(&self, id: &str, topic_suffix: &str, payload: &str) {
        let mut inner = self.inner.lock();
        let entry = inner.devices.entry(id.to_string()).or_default();
        entry
            .events
            .push((topic_suffix.to_string(), payload.to_string()));
    }

    fn is_connected(&self) -> bool {
        true
    }
}

/// Shared sink for a real MQTT-backed HA connection (filled by rethink-cloud).
pub struct HaMqttSink {
    pub config: HaConfig,
    pub published_availability: Mutex<HashSet<String>>,
    /// Callback to publish raw MQTT: (topic, payload, retain)
    pub publish_fn: Mutex<Option<Box<dyn Fn(&str, &[u8], bool) + Send + Sync>>>,
    pub set_handlers: Mutex<Vec<Box<dyn Fn(&str, &str, &str) + Send + Sync>>>,
    pub discovery_handlers: Mutex<Vec<Box<dyn Fn() + Send + Sync>>>,
    pub connected: Mutex<bool>,
}

impl HaMqttSink {
    pub fn new(config: HaConfig) -> Arc<Self> {
        Arc::new(Self {
            config,
            published_availability: Mutex::new(HashSet::new()),
            publish_fn: Mutex::new(None),
            set_handlers: Mutex::new(Vec::new()),
            discovery_handlers: Mutex::new(Vec::new()),
            connected: Mutex::new(false),
        })
    }

    pub fn set_publish_fn<F>(&self, f: F)
    where
        F: Fn(&str, &[u8], bool) + Send + Sync + 'static,
    {
        *self.publish_fn.lock() = Some(Box::new(f));
    }

    fn do_publish(&self, topic: &str, payload: &[u8], retain: bool) {
        if let Some(f) = self.publish_fn.lock().as_ref() {
            f(topic, payload, retain);
        }
    }

    pub fn on_set_property<F>(&self, f: F)
    where
        F: Fn(&str, &str, &str) + Send + Sync + 'static,
    {
        self.set_handlers.lock().push(Box::new(f));
    }

    pub fn on_discovery<F>(&self, f: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.discovery_handlers.lock().push(Box::new(f));
    }

    pub fn emit_discovery(&self) {
        for h in self.discovery_handlers.lock().iter() {
            h();
        }
    }

    pub fn handle_message(&self, topic: &str, message: &[u8], retain: bool) {
        let msg = String::from_utf8_lossy(message);
        if topic == format!("{}/status", self.config.discovery_prefix) && msg == "online" {
            self.emit_discovery();
        }
        let prefix = format!("{}/", self.config.rethink_prefix);
        if let Some(rest) = topic.strip_prefix(&prefix) {
            let parts: Vec<&str> = rest.split('/').collect();
            if parts.len() >= 3 && parts[parts.len() - 1] == "set" {
                let id = parts[0];
                let prop = parts[1..parts.len() - 1].join("/");
                for h in self.set_handlers.lock().iter() {
                    h(id, &prop, &msg);
                }
            }
            if parts.len() == 2 && parts[1] == "availability" && msg == "online" && retain {
                if !self.published_availability.lock().contains(parts[0]) {
                    self.do_publish(topic, b"offline", true);
                }
            }
        }
    }
}

#[cfg(test)]
mod ha_mqtt_sink_tests {
    use super::*;
    use crate::config::HaConfig;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn test_cfg() -> HaConfig {
        HaConfig {
            mqtt_url: "mqtt://127.0.0.1:1883".into(),
            discovery_prefix: "homeassistant".into(),
            rethink_prefix: "rethink".into(),
            mqtt_user: String::new(),
            mqtt_pass: String::new(),
        }
    }

    #[test]
    fn set_property_handlers_fire_on_handle_message() {
        let sink = HaMqttSink::new(test_cfg());
        let hits = Arc::new(AtomicUsize::new(0));
        let h = hits.clone();
        sink.on_set_property(move |id, prop, val| {
            assert_eq!(id, "dev1");
            assert_eq!(prop, "climate-mode");
            assert_eq!(val, "heat");
            h.fetch_add(1, Ordering::SeqCst);
        });
        sink.handle_message("rethink/dev1/climate-mode/set", b"heat", false);
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn publish_fn_receives_publish_property() {
        let sink = HaMqttSink::new(test_cfg());
        let hits = Arc::new(AtomicUsize::new(0));
        let h = hits.clone();
        sink.set_publish_fn(move |topic, payload, retain| {
            assert!(topic.contains("rethink/dev1/power"));
            assert_eq!(payload, b"ON");
            assert!(retain);
            h.fetch_add(1, Ordering::SeqCst);
        });
        use crate::ha::HaConnection;
        sink.publish_property("dev1", "power", "ON".into());
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn device_trigger_discovery_and_event_are_published() {
        use crate::ha::{DeviceDiscovery, DeviceInfo, DeviceTriggerDef, HaConnection, OriginInfo};
        use std::sync::Mutex as StdMutex;

        let sink = HaMqttSink::new(test_cfg());
        let pubs: Arc<StdMutex<Vec<(String, String, bool)>>> = Arc::new(StdMutex::new(Vec::new()));
        let p = pubs.clone();
        sink.set_publish_fn(move |topic, payload, retain| {
            p.lock().unwrap().push((
                topic.to_string(),
                String::from_utf8_lossy(payload).into_owned(),
                retain,
            ));
        });

        let config = DeviceDiscovery {
            device: DeviceInfo {
                identifiers: json!(["$deviceid"]),
                manufacturer: Some("LG".into()),
                model: Some("DHUM".into()),
                sw_version: None,
                name: Some("Dehumidifier".into()),
            },
            origin: OriginInfo {
                name: "rethink".into(),
                support_url: None,
                sw_version: None,
            },
            availability: None,
            availability_mode: None,
            components: Default::default(),
            device_triggers: vec![DeviceTriggerDef::problem(
                "bucket_full",
                "bucket_full",
                "bucket_full",
            )],
        };
        sink.publish_config("dev-xyz", &config);

        let logged = pubs.lock().unwrap().clone();
        // Classic discovery topic only (HA unique on device+type+subtype).
        assert!(
            logged.iter().any(|(t, body, retain)| {
                t == "homeassistant/device_automation/dev-xyz/bucket_full/config"
                    && *retain
                    && body.contains("\"automation_type\":\"trigger\"")
                    && body.contains("\"subtype\":\"bucket_full\"")
                    && body.contains("rethink/dev-xyz/triggers/bucket_full")
                    && body.contains("\"identifiers\":[\"dev-xyz\"]")
            }),
            "retained classic device trigger discovery missing: {logged:?}"
        );
        // Device discovery must not nest device_automation (dual-path conflict).
        assert!(
            logged.iter().any(|(t, body, retain)| {
                t == "homeassistant/device/rethink/dev-xyz/config"
                    && *retain
                    && body.contains("\"identifiers\":[\"dev-xyz\"]")
                    && !body.contains("trigger_bucket_full")
                    && !body.contains("\"platform\":\"device_automation\"")
            }),
            "device discovery must omit nested triggers: {logged:?}"
        );

        pubs.lock().unwrap().clear();
        sink.publish_event("dev-xyz", "triggers/bucket_full", "bucket_full");
        let logged = pubs.lock().unwrap().clone();
        assert!(
            logged.iter().any(|(t, body, retain)| {
                t == "rethink/dev-xyz/triggers/bucket_full"
                    && body == "bucket_full"
                    && !*retain
            }),
            "non-retained trigger event missing: {logged:?}"
        );
    }
}

impl HaConnection for HaMqttSink {
    fn publish_config(&self, id: &str, config: &DeviceDiscovery) {
        let discovery_topic = format!(
            "{}/device/rethink/{}/config",
            self.config.discovery_prefix, id
        );
        let device_topic = format!("{}/{}", self.config.rethink_prefix, id);
        let replacements = [
            ("$this", device_topic.as_str()),
            ("$rethink", self.config.rethink_prefix.as_str()),
            ("$deviceid", id),
        ];
        let mut payload = recursive_replace(
            &serde_json::to_value(config).unwrap_or(json!({})),
            &replacements,
        );
        normalize_device_identifiers(&mut payload);

        // HA keys MQTT device triggers uniquely by (device, type, subtype).
        // Do NOT also nest device_automation under modern device discovery —
        // dual publish yields "conflicts with existing device trigger" and the
        // second path never sets up. Entities stay on the device discovery doc;
        // triggers use classic discovery only (device_trigger.mqtt docs).
        //
        // If a previous build nested `trigger_*` components, strip them so the
        // retained device config no longer re-registers them on HA restart.
        if let Some(comps) = payload
            .as_object_mut()
            .and_then(|o| o.get_mut("components"))
            .and_then(|c| c.as_object_mut())
        {
            comps.retain(|k, _| !k.starts_with("trigger_"));
        }

        let body = serde_json::to_vec(&payload).unwrap_or_default();
        // Retain so HA recovers entities after broker/HA restart without waiting
        // for the next birth + republish.
        self.do_publish(&discovery_topic, &body, true);

        // Classic per-trigger discovery only.
        let mut device_info = recursive_replace(
            &serde_json::to_value(&config.device).unwrap_or(json!({})),
            &replacements,
        );
        normalize_device_identifiers_obj(&mut device_info);
        for trig in &config.device_triggers {
            let event_topic = format!(
                "{}/{}/{}",
                self.config.rethink_prefix, id, trig.topic_suffix
            );
            let disc_topic = format!(
                "{}/device_automation/{}/{}/config",
                self.config.discovery_prefix, id, trig.object_id
            );
            let disc = json!({
                "automation_type": "trigger",
                "type": trig.type_,
                "subtype": trig.subtype,
                "payload": trig.payload,
                "topic": event_topic,
                "device": device_info,
            });
            let body = serde_json::to_vec(&disc).unwrap_or_default();
            self.do_publish(&disc_topic, &body, true);
            tracing::debug!(
                target: "rethink_ha",
                %disc_topic,
                event_topic,
                "published MQTT device trigger discovery"
            );
        }
    }

    fn publish_property(&self, id: &str, property: &str, value: PropertyValue) {
        if property == "availability" {
            self.published_availability.lock().insert(id.to_string());
        }
        let topic = format!("{}/{}/{}", self.config.rethink_prefix, id, property);
        let payload = value.as_string();
        self.do_publish(&topic, payload.as_bytes(), true);
    }

    fn publish_event(&self, id: &str, topic_suffix: &str, payload: &str) {
        let topic = format!("{}/{}/{}", self.config.rethink_prefix, id, topic_suffix);
        self.do_publish(&topic, payload.as_bytes(), false);
    }

    fn is_connected(&self) -> bool {
        *self.connected.lock()
    }
}

fn recursive_replace(val: &Value, replacements: &[(&str, &str)]) -> Value {
    match val {
        Value::Array(a) => Value::Array(a.iter().map(|v| recursive_replace(v, replacements)).collect()),
        Value::Object(o) => {
            let mut m = serde_json::Map::new();
            for (k, v) in o {
                m.insert(k.clone(), recursive_replace(v, replacements));
            }
            Value::Object(m)
        }
        Value::String(s) => {
            let mut out = s.clone();
            for (pat, rep) in replacements {
                out = out.replace(pat, rep);
            }
            Value::String(out)
        }
        other => other.clone(),
    }
}

/// HA accepts string or list for identifiers; always emit a list so triggers and
/// entities share a stable multi-value identity in the device registry.
fn normalize_device_identifiers(payload: &mut Value) {
    if let Some(dev) = payload.get_mut("device") {
        normalize_device_identifiers_obj(dev);
    }
}

fn normalize_device_identifiers_obj(device: &mut Value) {
    let Some(obj) = device.as_object_mut() else {
        return;
    };
    match obj.get("identifiers").cloned() {
        Some(Value::String(s)) => {
            obj.insert("identifiers".into(), json!([s]));
        }
        Some(Value::Array(_)) => {}
        Some(other) => {
            obj.insert("identifiers".into(), json!([other]));
        }
        None => {}
    }
}
