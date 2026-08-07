//! ThinQ2 device acceptor — listens on the internal MQTT broker for CLIP traffic.

use super::provisioning::generate_deploy_response;
use crate::devmgr::{ConnectedDevice, DeviceManager, Platform, SendToDevice};
use crate::mqtt_broker::{Broker, PublishPacket};
use base64::Engine;
use chrono::{Datelike, Timelike};
use parking_lot::Mutex;
use rethink_core::metadata::Metadata;
use std::collections::HashMap;
use std::sync::Arc;

pub struct DeviceAcceptor {
    broker: Arc<Broker>,
    manager: Arc<DeviceManager>,
    clients_by_id: Mutex<HashMap<String, u64>>,
    devices: Mutex<HashMap<u64, Arc<ConnectedDevice>>>,
}

impl DeviceAcceptor {
    pub fn new(broker: Arc<Broker>, manager: Arc<DeviceManager>) -> Arc<Self> {
        let acceptor = Arc::new(Self {
            broker: broker.clone(),
            manager,
            clients_by_id: Mutex::new(HashMap::new()),
            devices: Mutex::new(HashMap::new()),
        });

        let acc = acceptor.clone();
        broker.on_publish(Arc::new(move |packet, client| {
            if client.is_none() {
                rethink_core::logging::log(
                    "outgoing",
                    &[
                        &packet.topic,
                        &String::from_utf8_lossy(&packet.payload),
                        "retain:",
                        &packet.retain.to_string(),
                    ],
                );
                return;
            }
            let client_id = client.unwrap();
            rethink_core::logging::log(
                "incoming",
                &[
                    &packet.topic,
                    &String::from_utf8_lossy(&packet.payload),
                    "retain:",
                    &packet.retain.to_string(),
                ],
            );
            if !packet.topic.contains("clip/") {
                return;
            }
            let mut payload_bytes = packet.payload.clone();
            if payload_bytes.last() == Some(&0) {
                payload_bytes.pop();
            }
            let text = match String::from_utf8(payload_bytes) {
                Ok(t) => t,
                Err(_) => return,
            };
            let payload: serde_json::Value = match serde_json::from_str(&text) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("clip parse error: {e}");
                    return;
                }
            };
            acc.handle_mqtt(&packet.topic, &payload, client_id);
        }));

        let acc2 = acceptor.clone();
        broker.on_disconnect(Arc::new(move |client_id| {
            acc2.disconnected(client_id);
        }));

        acceptor
    }

    fn handle_mqtt(&self, topic: &str, payload: &serde_json::Value, client_id: u64) {
        let topic = if let Some(idx) = topic.find("/clip") {
            format!("clip{}", &topic[idx + 5..])
        } else if let Some(idx) = topic.find("clip/") {
            topic[idx..].to_string()
        } else {
            topic.to_string()
        };

        let did = payload
            .get("did")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let cmd = payload
            .get("cmd")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if topic == format!("clip/message/devices/{did}") {
            if cmd == "completeProvisioning_ack" {
                self.complete_provisioning(&did, payload, client_id);
            }
            if cmd == "device_packet" {
                let meta = self.broker.client_meta(client_id);
                if meta
                    .deploy_msg
                    .as_ref()
                    .and_then(|d| d.get("did"))
                    .and_then(|v| v.as_str())
                    == Some(did.as_str())
                {
                    if let Some(dev) = self.devices.lock().get(&client_id).cloned() {
                        if let Some(data) = payload.get("data").and_then(|d| d.as_str()) {
                            if let Ok(buf) = hex::decode(data) {
                                dev.notify_data(&buf);
                            }
                        }
                    }
                }
            }
            if cmd == "req_timesync" {
                self.time_sync_request(client_id);
            }
        }

        if topic == format!("clip/provisioning/devices/{did}")
            && (cmd == "preDeploy" || cmd == "deploy")
        {
            let mut meta = self.broker.client_meta(client_id);
            meta.deploy_msg = Some(payload.clone());
            self.broker.set_client_meta(client_id, meta);
            let resp = generate_deploy_response(payload);
            self.broker.publish(
                PublishPacket {
                    topic: format!("lime/devices/{did}"),
                    payload: serde_json::to_vec(&resp).unwrap_or_default(),
                    retain: false,
                    qos: 0,
                    dup: false,
                },
                None,
            );
        }
    }

    fn complete_provisioning(&self, device_id: &str, _payload: &serde_json::Value, client_id: u64) {
        let meta_c = self.broker.client_meta(client_id);
        let Some(deploy) = meta_c.deploy_msg.clone() else {
            eprintln!("completeProvisioning_ack received without deploy/preDeploy");
            return;
        };
        if meta_c.has_device {
            eprintln!("completeProvisioning_ack received twice?");
            return;
        }

        if let Some(old) = self.clients_by_id.lock().get(device_id).copied() {
            eprintln!("device {device_id} already connected, dropping the old one");
            self.broker.destroy_client(old);
        }
        self.clients_by_id
            .lock()
            .insert(device_id.to_string(), client_id);

        let model_id = deploy
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let model_name = deploy
            .pointer("/data/appInfo/modelName")
            .and_then(|v| v.as_str())
            .unwrap_or(&model_id)
            .to_string();
        let sw_version = deploy
            .pointer("/data/appInfo/softVer")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let device_type = deploy
            .pointer("/data/appInfo/DeviceType")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let meta = Metadata {
            model_id,
            model_name,
            device_type,
            sw_version,
        };

        let device_slot: Arc<Mutex<Option<Arc<ConnectedDevice>>>> = Arc::new(Mutex::new(None));
        let slot_emit = device_slot.clone();
        let emit = Arc::new(move |buf: Vec<u8>| {
            if let Some(dev) = slot_emit.lock().as_ref() {
                dev.notify_data(&buf);
            }
        });

        let broker = self.broker.clone();
        let did = device_id.to_string();
        let slot_send = device_slot.clone();
        let send_to = Arc::new(move |msg: SendToDevice| {
            if let Some(d) = slot_send.lock().as_ref() {
                d.notify_send(msg.clone());
            }
            let mid = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            let messagestr = match msg {
                SendToDevice::T2Packet(buf) => serde_json::json!({
                    "did": did,
                    "mid": mid,
                    "cmd": "packet",
                    "type": 1,
                    "data": hex::encode(&buf),
                }),
                SendToDevice::T2Clip {
                    cmd,
                    msg_type,
                    data,
                } => serde_json::json!({
                    "did": did,
                    "mid": mid,
                    "cmd": cmd,
                    "type": msg_type,
                    "data": data,
                }),
                SendToDevice::T1Json(_) => return,
            };
            broker.publish(
                PublishPacket {
                    topic: format!("lime/devices/{did}"),
                    payload: serde_json::to_vec(&messagestr).unwrap_or_default(),
                    retain: false,
                    qos: 0,
                    dup: false,
                },
                None,
            );
        });

        let dev = ConnectedDevice::new(
            device_id.to_string(),
            Platform::Thinq2,
            meta,
            emit,
            send_to,
        );
        *device_slot.lock() = Some(dev.clone());

        let mut cm = self.broker.client_meta(client_id);
        cm.has_device = true;
        cm.device_id = Some(device_id.to_string());
        self.broker.set_client_meta(client_id, cm);

        self.devices.lock().insert(client_id, dev.clone());
        self.manager.accept(dev);
    }

    fn time_sync_request(&self, client_id: u64) {
        let meta = self.broker.client_meta(client_id);
        let Some(device_id) = meta
            .deploy_msg
            .as_ref()
            .and_then(|d| d.get("did"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
        else {
            return;
        };
        let now = chrono::Utc::now();
        let mut buf = [0u8; 7];
        buf[0] = (now.year() % 100) as u8;
        buf[1] = now.month0() as u8;
        buf[2] = now.day() as u8;
        buf[3] = now.hour() as u8;
        buf[4] = now.minute() as u8;
        buf[5] = now.second() as u8;
        buf[6] = now.weekday().num_days_from_sunday() as u8;
        let mid = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let payload = serde_json::json!({
            "did": device_id,
            "mid": mid,
            "cmd": "resp_timesync",
            "type": 1,
            "data": base64::engine::general_purpose::STANDARD.encode(buf),
        });
        self.broker.publish(
            PublishPacket {
                topic: format!("lime/devices/{device_id}"),
                payload: serde_json::to_vec(&payload).unwrap_or_default(),
                retain: false,
                qos: 0,
                dup: false,
            },
            None,
        );
    }

    fn disconnected(&self, client_id: u64) {
        if let Some(dev) = self.devices.lock().remove(&client_id) {
            self.clients_by_id.lock().remove(&dev.id);
            dev.notify_close();
        }
    }
}
