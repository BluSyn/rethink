//! Device manager — tracks connected ThinQ1/2 appliances.

use parking_lot::Mutex;
use rethink_core::metadata::Metadata;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Thinq1,
    Thinq2,
}

/// Handle to a connected appliance (platform-agnostic view for management/HA).
pub struct ConnectedDevice {
    pub id: String,
    pub platform: Platform,
    pub meta: Metadata,
    /// Inject data as if from appliance (T2: raw packet bytes hex path).
    pub emit_data: Arc<dyn Fn(Vec<u8>) + Send + Sync>,
    /// Send to appliance.
    pub send_to_device: Arc<dyn Fn(SendToDevice) + Send + Sync>,
    pub on_close: Mutex<Vec<Box<dyn Fn() + Send + Sync>>>,
    pub on_data: Mutex<Vec<Box<dyn Fn(&[u8]) + Send + Sync>>>,
    pub on_send: Mutex<Vec<Box<dyn Fn(SendToDevice) + Send + Sync>>>,
}

#[derive(Debug, Clone)]
pub enum SendToDevice {
    T2Packet(Vec<u8>),
    T1Json(serde_json::Value),
}

impl ConnectedDevice {
    pub fn new(
        id: String,
        platform: Platform,
        meta: Metadata,
        emit_data: Arc<dyn Fn(Vec<u8>) + Send + Sync>,
        send_to_device: Arc<dyn Fn(SendToDevice) + Send + Sync>,
    ) -> Arc<Self> {
        Arc::new(Self {
            id,
            platform,
            meta,
            emit_data,
            send_to_device,
            on_close: Mutex::new(Vec::new()),
            on_data: Mutex::new(Vec::new()),
            on_send: Mutex::new(Vec::new()),
        })
    }

    pub fn notify_data(&self, buf: &[u8]) {
        for h in self.on_data.lock().iter() {
            h(buf);
        }
    }

    pub fn notify_send(&self, msg: SendToDevice) {
        for h in self.on_send.lock().iter() {
            h(msg.clone());
        }
    }

    pub fn notify_close(&self) {
        for h in self.on_close.lock().iter() {
            h();
        }
    }

    pub fn add_close_handler<F: Fn() + Send + Sync + 'static>(&self, f: F) {
        self.on_close.lock().push(Box::new(f));
    }

    pub fn add_data_handler<F: Fn(&[u8]) + Send + Sync + 'static>(&self, f: F) {
        self.on_data.lock().push(Box::new(f));
    }

    pub fn add_send_handler<F: Fn(SendToDevice) + Send + Sync + 'static>(&self, f: F) {
        self.on_send.lock().push(Box::new(f));
    }
}

type NewDeviceHandler = Box<dyn Fn(Arc<ConnectedDevice>) + Send + Sync>;
type DropDeviceHandler = Box<dyn Fn(&str) + Send + Sync>;

pub struct DeviceManager {
    devices: Mutex<HashMap<String, Arc<ConnectedDevice>>>,
    on_new: Mutex<Vec<NewDeviceHandler>>,
    on_drop: Mutex<Vec<DropDeviceHandler>>,
}



impl DeviceManager {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            devices: Mutex::new(HashMap::new()),
            on_new: Mutex::new(Vec::new()),
            on_drop: Mutex::new(Vec::new()),
        })
    }

    pub fn accept(self: &Arc<Self>, device: Arc<ConnectedDevice>) {
        let id = device.id.clone();
        {
            let mut map = self.devices.lock();
            map.insert(id.clone(), device.clone());
        }
        let mgr = self.clone();
        let dev_id = id.clone();
        device.add_close_handler(move || {
            let mut map = mgr.devices.lock();
            if map.get(&dev_id).map(|d| d.id.as_str()) == Some(dev_id.as_str()) {
                map.remove(&dev_id);
                for h in mgr.on_drop.lock().iter() {
                    h(&dev_id);
                }
            }
        });
        for h in self.on_new.lock().iter() {
            h(device.clone());
        }
    }

    pub fn on_new_device<F: Fn(Arc<ConnectedDevice>) + Send + Sync + 'static>(&self, f: F) {
        self.on_new.lock().push(Box::new(f));
    }

    pub fn on_drop_device<F: Fn(&str) + Send + Sync + 'static>(&self, f: F) {
        self.on_drop.lock().push(Box::new(f));
    }

    pub fn get(&self, id: &str) -> Option<Arc<ConnectedDevice>> {
        self.devices.lock().get(id).cloned()
    }

    pub fn all(&self) -> HashMap<String, Arc<ConnectedDevice>> {
        self.devices.lock().clone()
    }

    pub fn list_json(&self) -> serde_json::Value {
        let devices: Vec<_> = self
            .devices
            .lock()
            .values()
            .map(|d| {
                serde_json::json!({
                    "id": d.id,
                    "platform": match d.platform {
                        Platform::Thinq1 => "thinq1",
                        Platform::Thinq2 => "thinq2",
                    },
                    "modelId": d.meta.model_id,
                    "modelName": d.meta.model_name,
                })
            })
            .collect();
        serde_json::json!({ "devices": devices })
    }
}
