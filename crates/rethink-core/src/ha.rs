//! Home Assistant MQTT discovery types and connection trait.

use crate::config::HaConfig;
use parking_lot::Mutex;
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
}

/// Trait for publishing to HA MQTT (real connection or mock).
pub trait HaConnection: Send + Sync {
    fn publish_config(&self, id: &str, config: &DeviceDiscovery);
    fn publish_property(&self, id: &str, property: &str, value: PropertyValue);
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
}

impl HaConnection for HaMqttSink {
    fn publish_config(&self, id: &str, config: &DeviceDiscovery) {
        let discovery_topic = format!(
            "{}/device/rethink/{}/config",
            self.config.discovery_prefix, id
        );
        let device_topic = format!("{}/{}", self.config.rethink_prefix, id);
        let payload = recursive_replace(
            &serde_json::to_value(config).unwrap_or(json!({})),
            &[
                ("$this", device_topic.as_str()),
                ("$rethink", self.config.rethink_prefix.as_str()),
                ("$deviceid", id),
            ],
        );
        let body = serde_json::to_vec(&payload).unwrap_or_default();
        self.do_publish(&discovery_topic, &body, false);
    }

    fn publish_property(&self, id: &str, property: &str, value: PropertyValue) {
        if property == "availability" {
            self.published_availability.lock().insert(id.to_string());
        }
        let topic = format!("{}/{}/{}", self.config.rethink_prefix, id, property);
        let payload = value.as_string();
        self.do_publish(&topic, payload.as_bytes(), true);
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
