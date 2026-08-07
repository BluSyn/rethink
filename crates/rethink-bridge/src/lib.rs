//! Bridge mode: optional forwarding to the real LG ThinQ cloud.

pub mod oauth2;
pub mod pair;
pub mod state;
pub mod thinq1_conn;
pub mod thinq2_conn;
pub mod thinq_api;
pub mod util;

pub use state::{BridgeState, Credentials, Environment, JsonStorage};
pub use util::{subprocess, SubprocessError, SubprocessOptions};

use parking_lot::Mutex;
use pair::{Thinq1DeviceState, Thinq2DeviceState};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use thinq1_conn::{connect_thinq1, Thinq1Handle};
use thinq2_conn::{connect_thinq2, Thinq2Handle};

/// Minimal local device view for bridge enable/disable (avoids cyclic deps on cloud).
pub trait LocalDevice: Send + Sync {
    fn id(&self) -> &str;
    fn platform(&self) -> &str;
    fn model_id(&self) -> &str;
    fn model_name(&self) -> &str;
    fn device_type(&self) -> Option<&str>;
    fn on_data(&self, handler: Box<dyn Fn(&[u8]) + Send + Sync>);
    fn on_close(&self, handler: Box<dyn Fn() + Send + Sync>);
    fn send_to_local(&self, buf: &[u8]);
    fn send_json_to_local(&self, body: serde_json::Value);
}

enum UpstreamHandle {
    T2(Thinq2Handle),
    T1(Thinq1Handle),
}

struct BridgedSession {
    device_id: String,
    lg_state: serde_json::Value,
    dropped: Arc<AtomicBool>,
    upstream: Mutex<Option<UpstreamHandle>>,
}

pub struct Bridge {
    storage: Arc<dyn BridgeState>,
    sessions: Mutex<HashMap<String, Arc<BridgedSession>>>,
    want_enabled: Mutex<HashSet<String>>,
    logged_in: Mutex<bool>,
}

impl Bridge {
    pub fn new(storage: Arc<dyn BridgeState>) -> Arc<Self> {
        let logged_in = storage.get_credentials().is_some();
        Arc::new(Self {
            storage,
            sessions: Mutex::new(HashMap::new()),
            want_enabled: Mutex::new(HashSet::new()),
            logged_in: Mutex::new(logged_in),
        })
    }

    pub fn is_logged_in(&self) -> bool {
        *self.logged_in.lock() || self.storage.get_credentials().is_some()
    }

    pub fn status_for(&self, id: &str) -> bool {
        self.sessions.lock().contains_key(id)
    }

    pub fn storage(&self) -> &Arc<dyn BridgeState> {
        &self.storage
    }

    pub async fn begin_login(&self, country_code: &str) -> anyhow::Result<String> {
        let mut client = thinq_api::Client::new(Environment {
            country_code: country_code.into(),
            language_code: None,
        });
        let (web, _auth) = client.get_urls().await?;
        Ok(thinq_api::sign_in_url(&web, country_code))
    }

    pub async fn complete_login(
        &self,
        country_code: &str,
        callback_url: &str,
    ) -> anyhow::Result<bool> {
        let mut client = thinq_api::Client::new(Environment {
            country_code: country_code.into(),
            language_code: None,
        });
        let (_web, auth) = client.get_urls().await?;
        let url = url::Url::parse(callback_url)?;
        let code = url
            .query_pairs()
            .find(|(k, _)| k == "code")
            .map(|(_, v)| v.to_string());
        let Some(code) = code else {
            return Ok(false);
        };
        let token = oauth2::from_code(&auth, &code).await?;
        self.storage.set_credentials(Some(Credentials {
            refresh_token: token.refresh_token,
            env: Environment {
                country_code: country_code.into(),
                language_code: None,
            },
        }));
        *self.logged_in.lock() = true;
        Ok(true)
    }

    pub async fn logout(&self) -> anyhow::Result<()> {
        self.storage.set_credentials(None);
        *self.logged_in.lock() = false;
        let sessions: Vec<_> = self.sessions.lock().drain().map(|(_, s)| s).collect();
        for s in sessions {
            s.dropped.store(true, Ordering::SeqCst);
            if let Some(up) = s.upstream.lock().take() {
                match up {
                    UpstreamHandle::T2(h) => h.stop(),
                    UpstreamHandle::T1(h) => h.stop(),
                }
            }
        }
        self.want_enabled.lock().clear();
        Ok(())
    }

    pub async fn enable(
        &self,
        device: Arc<dyn LocalDevice>,
        device_type: Option<&str>,
        mut status: Option<Box<dyn FnMut(&str) + Send>>,
    ) -> anyhow::Result<bool> {
        let mut report = |s: &str| {
            if let Some(ref mut cb) = status {
                cb(s);
            }
        };

        if !self.is_logged_in() {
            report("not logged in");
            return Ok(false);
        }
        let id = device.id().to_string();
        if self.sessions.lock().contains_key(&id) {
            return Ok(true);
        }

        let creds = self
            .storage
            .get_credentials()
            .ok_or_else(|| anyhow::anyhow!("Not logged in"))?;

        if let Some(saved) = self.storage.get_device_state_json(&id) {
            report("Restoring saved bridge session");
            self.start_session(device, saved).await?;
            self.want_enabled.lock().insert(id);
            return Ok(true);
        }

        let mut client = thinq_api::Client::new(creds.env.clone());
        client.auth(&creds.refresh_token).await?;

        let dtype = device_type
            .map(|s| s.to_string())
            .or_else(|| device.device_type().map(|s| s.to_string()))
            .ok_or_else(|| anyhow::anyhow!("Device type must be specified"))?;

        report("Removing device from home");
        let _ = client.remove_device(&id).await;

        let lg_state = if device.platform() == "thinq1" {
            report("Adding ThinQ1 device to home");
            let state = client.thinq1_state()?;
            let alias = format!("Rethink {}", &id[..id.len().min(8)]);
            client
                .add_device(
                    &id,
                    &alias,
                    device.model_name(),
                    &dtype,
                    "thinq1",
                    None,
                )
                .await?;
            serde_json::json!({
                "rtiServer": state.get("rtiServer").cloned().unwrap_or(serde_json::Value::Null),
                "httpServer": state.get("httpServer").cloned().unwrap_or(serde_json::Value::Null),
                "platform": "thinq1",
            })
        } else {
            report("Fetching otp key");
            let (otp, pubkey) = client.prepare_new_t2_device().await?;
            report("Pairing ThinQ2 device (route + certificate)");
            let pair = match pair::pair_thinq2(&creds.env, &id, &otp, &pubkey).await {
                Ok(p) => p,
                Err(e) => {
                    report(&format!(
                        "Pairing failed: {e}. Make sure common.lgthinq.com is not redirected"
                    ));
                    return Err(e);
                }
            };
            report("Adding ThinQ2 device to home");
            let alias = format!("Rethink {}", &id[..id.len().min(8)]);
            let ct_b64 = base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD,
                &pair.add_device_ciphertext,
            );
            client
                .add_device(
                    &id,
                    &alias,
                    device.model_name(),
                    &dtype,
                    "thinq2",
                    Some(&ct_b64),
                )
                .await?;
            // Serialize with camelCase for TS-compatible storage
            serde_json::to_value(&pair.state)?
        };

        report("Device registered successfully");
        self.storage
            .set_device_state_json(&id, Some(lg_state.clone()));
        self.start_session(device, lg_state).await?;
        self.want_enabled.lock().insert(id);
        Ok(true)
    }

    async fn start_session(
        &self,
        device: Arc<dyn LocalDevice>,
        lg_state: serde_json::Value,
    ) -> anyhow::Result<()> {
        let id = device.id().to_string();
        let model_name = device.model_name().to_string();
        let device_type = device.device_type().map(|s| s.to_string());
        let dropped = Arc::new(AtomicBool::new(false));

        let is_t2 = device.platform() == "thinq2"
            || lg_state.get("mqttServer").is_some()
            || lg_state.get("mqtt_server").is_some();

        let upstream = if is_t2 {
            let state: Thinq2DeviceState =
                serde_json::from_value(normalize_t2_state(lg_state.clone()))?;
            if state.mqtt_server.is_empty() {
                anyhow::bail!("ThinQ2 state missing mqttServer — re-enable to re-pair");
            }
            let (handle, mut from_lg) =
                connect_thinq2(&state, &id, &model_name).await?;

            // LG → local
            let dev = device.clone();
            let drop_f = dropped.clone();
            tokio::spawn(async move {
                while let Some(buf) = from_lg.recv().await {
                    if drop_f.load(Ordering::SeqCst) {
                        break;
                    }
                    dev.send_to_local(&buf);
                }
            });

            // Local → LG
            let h = handle.clone();
            let drop_l = dropped.clone();
            device.on_data(Box::new(move |buf| {
                if drop_l.load(Ordering::SeqCst) {
                    return;
                }
                let h = h.clone();
                let data = buf.to_vec();
                tokio::spawn(async move {
                    let _ = h.send_from_local(&data).await;
                });
            }));

            UpstreamHandle::T2(handle)
        } else {
            let t1 = parse_t1_state(&lg_state)?;
            let (handle, mut from_lg) =
                connect_thinq1(&t1, &id, &model_name, device_type.as_deref()).await?;

            let dev = device.clone();
            let drop_f = dropped.clone();
            tokio::spawn(async move {
                while let Some(body) = from_lg.recv().await {
                    if drop_f.load(Ordering::SeqCst) {
                        break;
                    }
                    dev.send_json_to_local(body);
                }
            });

            let h = handle.clone();
            let drop_l = dropped.clone();
            device.on_data(Box::new(move |buf| {
                if drop_l.load(Ordering::SeqCst) {
                    return;
                }
                h.send_from_local(buf);
            }));

            UpstreamHandle::T1(handle)
        };

        let session = Arc::new(BridgedSession {
            device_id: id.clone(),
            lg_state,
            dropped: dropped.clone(),
            upstream: Mutex::new(Some(upstream)),
        });

        device.on_close(Box::new(move || {
            dropped.store(true, Ordering::SeqCst);
        }));

        self.sessions.lock().insert(id, session);
        Ok(())
    }

    pub async fn enable_id(
        &self,
        device_id: &str,
        device_type: Option<&str>,
        mut status: Option<Box<dyn FnMut(&str) + Send>>,
        lookup: &dyn Fn(&str) -> Option<Arc<dyn LocalDevice>>,
    ) -> anyhow::Result<bool> {
        let Some(dev) = lookup(device_id) else {
            if let Some(ref mut cb) = status {
                cb("device not connected");
            }
            return Ok(false);
        };
        self.enable(dev, device_type, status).await
    }

    pub async fn disable(&self, device_id: &str) -> anyhow::Result<()> {
        self.storage.set_device_state_json(device_id, None);
        if let Some(sess) = self.sessions.lock().remove(device_id) {
            sess.dropped.store(true, Ordering::SeqCst);
            if let Some(up) = sess.upstream.lock().take() {
                match up {
                    UpstreamHandle::T2(h) => h.stop(),
                    UpstreamHandle::T1(h) => h.stop(),
                }
            }
        }
        self.want_enabled.lock().remove(device_id);
        Ok(())
    }

    /// When a local device appears, auto-restore bridge if previously enabled.
    pub fn on_local_device(self: &Arc<Self>, device: Arc<dyn LocalDevice>) {
        let id = device.id().to_string();
        if !self.want_enabled.lock().contains(&id)
            && self.storage.get_device_state_json(&id).is_none()
        {
            return;
        }
        if self.sessions.lock().contains_key(&id) {
            return;
        }
        let Some(state) = self.storage.get_device_state_json(&id) else {
            return;
        };
        let this = self.clone();
        tokio::spawn(async move {
            if let Err(e) = this.start_session(device, state).await {
                eprintln!("[bridge] auto-restore failed for {id}: {e}");
            } else {
                this.want_enabled.lock().insert(id);
            }
        });
    }
}

fn normalize_t2_state(v: serde_json::Value) -> serde_json::Value {
    if v.get("mqtt_server").is_some() && v.get("mqttServer").is_none() {
        // already snake_case from our serde
        return serde_json::json!({
            "countryCode": v.get("country_code").cloned().unwrap_or(serde_json::json!("US")),
            "apiServer": v.get("api_server").cloned().unwrap_or(serde_json::json!("")),
            "mqttServer": v.get("mqtt_server").cloned().unwrap_or(serde_json::json!("")),
            "caCertificate": v.get("ca_certificate").cloned().unwrap_or(serde_json::json!("")),
            "privateKey": v.get("private_key").cloned().unwrap_or(serde_json::json!("")),
            "certificate": v.get("certificate").cloned().unwrap_or(serde_json::json!("")),
            "pubTopic": v.get("pub_topic").cloned().unwrap_or(serde_json::json!("")),
            "provTopic": v.get("prov_topic").cloned().unwrap_or(serde_json::json!("")),
            "subTopic": v.get("sub_topic").cloned().unwrap_or(serde_json::json!("")),
        });
    }
    // camelCase (TS storage) — serde rename on Thinq2DeviceState expects camelCase fields
    serde_json::json!({
        "countryCode": v.get("countryCode").or_else(|| v.get("country_code")).cloned().unwrap_or(serde_json::json!("US")),
        "apiServer": v.get("apiServer").or_else(|| v.get("api_server")).cloned().unwrap_or(serde_json::json!("")),
        "mqttServer": v.get("mqttServer").or_else(|| v.get("mqtt_server")).cloned().unwrap_or(serde_json::json!("")),
        "caCertificate": v.get("caCertificate").or_else(|| v.get("ca_certificate")).cloned().unwrap_or(serde_json::json!("")),
        "privateKey": v.get("privateKey").or_else(|| v.get("private_key")).cloned().unwrap_or(serde_json::json!("")),
        "certificate": v.get("certificate").cloned().unwrap_or(serde_json::json!("")),
        "pubTopic": v.get("pubTopic").or_else(|| v.get("pub_topic")).cloned().unwrap_or(serde_json::json!("")),
        "provTopic": v.get("provTopic").or_else(|| v.get("prov_topic")).cloned().unwrap_or(serde_json::json!("")),
        "subTopic": v.get("subTopic").or_else(|| v.get("sub_topic")).cloned().unwrap_or(serde_json::json!("")),
    })
}

fn parse_t1_state(v: &serde_json::Value) -> anyhow::Result<Thinq1DeviceState> {
    let rti = v
        .get("rtiServer")
        .or_else(|| v.get("rti_server"))
        .and_then(|x| x.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing rtiServer"))?
        .to_string();
    let http = v
        .get("httpServer")
        .or_else(|| v.get("http_server"))
        .and_then(|x| x.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing httpServer"))?
        .to_string();
    Ok(Thinq1DeviceState {
        rti_server: rti,
        http_server: http,
    })
}

#[cfg(test)]
mod forward_tests {
    use super::*;
    use crate::pair::{format_device_packet, parse_lg_packet_payload};
    use crate::thinq1_conn::format_status_body;

    struct MockLocal {
        id: String,
        platform: String,
        to_local: Mutex<Vec<Vec<u8>>>,
        to_json: Mutex<Vec<serde_json::Value>>,
        data_handlers: Mutex<Vec<Box<dyn Fn(&[u8]) + Send + Sync>>>,
    }

    impl MockLocal {
        fn new(platform: &str) -> Arc<Self> {
            Arc::new(Self {
                id: "mock-dev".into(),
                platform: platform.into(),
                to_local: Mutex::new(Vec::new()),
                to_json: Mutex::new(Vec::new()),
                data_handlers: Mutex::new(Vec::new()),
            })
        }
    }

    impl LocalDevice for MockLocal {
        fn id(&self) -> &str {
            &self.id
        }
        fn platform(&self) -> &str {
            &self.platform
        }
        fn model_id(&self) -> &str {
            "MODEL"
        }
        fn model_name(&self) -> &str {
            "MODEL"
        }
        fn device_type(&self) -> Option<&str> {
            Some("401")
        }
        fn on_data(&self, handler: Box<dyn Fn(&[u8]) + Send + Sync>) {
            self.data_handlers.lock().push(handler);
        }
        fn on_close(&self, _handler: Box<dyn Fn() + Send + Sync>) {}
        fn send_to_local(&self, buf: &[u8]) {
            self.to_local.lock().push(buf.to_vec());
        }
        fn send_json_to_local(&self, body: serde_json::Value) {
            self.to_json.lock().push(body);
        }
    }

    #[test]
    fn local_to_lg_t2_uses_device_packet_cmd() {
        let msg = format_device_packet(10001, "mock-dev", "MODEL", "AABB");
        let v: serde_json::Value = serde_json::from_str(&msg).unwrap();
        assert_eq!(v["cmd"], "device_packet");
        assert_eq!(v["data"], "AABB");
        assert_eq!(v["did"], "mock-dev");
    }

    #[test]
    fn lg_to_local_t2_parse_and_deliver() {
        let local = MockLocal::new("thinq2");
        let buf = parse_lg_packet_payload(&serde_json::json!({"cmd":"packet","data":"0102ff"}))
            .unwrap();
        local.send_to_local(&buf);
        assert_eq!(*local.to_local.lock(), vec![vec![1, 2, 255]]);
    }

    #[test]
    fn local_to_lg_t1_status_b64() {
        let body = format_status_body("id-1", &[0xDE, 0xAD]);
        assert_eq!(body["Body"]["Format"], "B64");
        assert_eq!(body["Body"]["ReturnCode"], "0000");
        let data = body["Body"]["Data"].as_str().unwrap();
        assert_eq!(
            base64::Engine::decode(&base64::engine::general_purpose::STANDARD, data).unwrap(),
            vec![0xDE, 0xAD]
        );
    }

    #[test]
    fn data_handlers_receive_local_emissions() {
        let local = MockLocal::new("thinq2");
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s = seen.clone();
        local.on_data(Box::new(move |b| s.lock().push(b.to_vec())));
        for h in local.data_handlers.lock().iter() {
            h(&[9, 8]);
        }
        assert_eq!(*seen.lock(), vec![vec![9, 8]]);
    }
}
