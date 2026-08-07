//! ThinQ1 device acceptor over TLS streams.

use super::connection::{run_connection_with_acks, T1ConnectionEvents};
use super::http::MetaStore;
use crate::devmgr::{ConnectedDevice, DeviceManager, Platform, SendToDevice};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use uuid::Uuid;

pub struct DeviceAcceptor {
    meta: MetaStore,
    manager: Arc<DeviceManager>,
    connections: Mutex<HashMap<String, mpsc::UnboundedSender<serde_json::Value>>>,
}

impl DeviceAcceptor {
    pub fn new(meta: MetaStore, manager: Arc<DeviceManager>) -> Arc<Self> {
        Arc::new(Self {
            meta,
            manager,
            connections: Mutex::new(HashMap::new()),
        })
    }

    pub async fn accept<S>(self: &Arc<Self>, stream: S)
    where
        S: AsyncReadExt + AsyncWriteExt + Unpin + Send + 'static,
    {
        let (tx, rx) = mpsc::unbounded_channel();
        let (ack_tx, mut ack_rx) = mpsc::unbounded_channel();

        let tx_for_ack = tx.clone();
        tokio::spawn(async move {
            while let Some(msg) = ack_rx.recv().await {
                let _ = tx_for_ack.send(msg);
            }
        });

        let device_slot: Arc<Mutex<Option<Arc<ConnectedDevice>>>> = Arc::new(Mutex::new(None));
        let device_slot_init = device_slot.clone();
        let device_slot_status = device_slot.clone();
        let device_slot_close = device_slot.clone();
        let acceptor = self.clone();
        let acceptor_close = self.clone();
        let tx_store = tx.clone();

        let events = T1ConnectionEvents {
            on_init: Arc::new(move |device_id: String| {
                let meta = acceptor.meta.lock().get(&device_id).cloned();
                let Some(meta) = meta else {
                    eprintln!("device {device_id} metadata not known, send HTTP POST first!");
                    return;
                };
                {
                    let mut cons = acceptor.connections.lock();
                    if cons.contains_key(&device_id) {
                        eprintln!("device {device_id} already connected, dropping the old one");
                    }
                    cons.insert(device_id.clone(), tx_store.clone());
                }

                let send_tx = tx_store.clone();
                let id_send = device_id.clone();
                let send_to = Arc::new(move |msg: SendToDevice| {
                    if let SendToDevice::T1Json(body) = msg {
                        let mut body_obj = body.as_object().cloned().unwrap_or_default();
                        body_obj.insert(
                            "CmdWId".into(),
                            serde_json::json!(format!("n-{}", Uuid::new_v4())),
                        );
                        let packet = serde_json::json!({
                            "Header": { "x-lgedm-deviceId": id_send },
                            "Body": body_obj,
                        });
                        let _ = send_tx.send(packet);
                    }
                });

                let slot = device_slot_init.clone();
                let emit = Arc::new(move |buf: Vec<u8>| {
                    if let Some(dev) = slot.lock().as_ref() {
                        dev.notify_data(&buf);
                    }
                });

                let dev = ConnectedDevice::new(
                    device_id.clone(),
                    Platform::Thinq1,
                    meta,
                    emit,
                    send_to,
                );
                *device_slot_init.lock() = Some(dev.clone());
                acceptor.manager.accept(dev);
            }),
            on_status: Arc::new(move |buf: Vec<u8>| {
                if let Some(dev) = device_slot_status.lock().as_ref() {
                    dev.notify_data(&buf);
                }
            }),
            on_close: Arc::new(move || {
                if let Some(dev) = device_slot_close.lock().take() {
                    acceptor_close.connections.lock().remove(&dev.id);
                    dev.notify_close();
                }
            }),
        };

        run_connection_with_acks(stream, events, rx, ack_tx).await;
    }
}
