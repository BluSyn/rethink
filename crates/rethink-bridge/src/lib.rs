//! Bridge mode: optional forwarding to the real LG ThinQ cloud.
//!
//! The subprocess helper is fully ported and unit-tested. Full OAuth2 / ThinQ API
//! cloud bridging is stubbed with a stable API so rethink-cloud and management can wire it.

pub mod state;
pub mod util;

pub use state::{BridgeState, Credentials, Environment, JsonStorage};
pub use util::{subprocess, SubprocessError, SubprocessOptions};

use parking_lot::Mutex;
use std::collections::HashSet;
use std::sync::Arc;

/// Status of the optional LG-cloud bridge for management UI.
#[derive(Debug, Clone, Default)]
pub struct BridgeStatus {
    pub logged_in: bool,
}

/// Minimal bridge controller used by management and cloud entrypoint.
///
/// Full device-proxying (thinqApi / oauth2 / thinq1|2 connections) can be filled in
/// incrementally; enable/disable currently tracks intended state on disk.
pub struct Bridge {
    storage: Arc<dyn BridgeState>,
    enabled: Mutex<HashSet<String>>,
    logged_in: Mutex<bool>,
}

impl Bridge {
    pub fn new(storage: Arc<dyn BridgeState>) -> Arc<Self> {
        let logged_in = storage.get_credentials().is_some();
        Arc::new(Self {
            storage,
            enabled: Mutex::new(HashSet::new()),
            logged_in: Mutex::new(logged_in),
        })
    }

    pub fn is_logged_in(&self) -> bool {
        *self.logged_in.lock()
    }

    pub fn status_for(&self, id: &str) -> bool {
        self.enabled.lock().contains(id)
    }

    pub fn storage(&self) -> &Arc<dyn BridgeState> {
        &self.storage
    }

    /// Begin OAuth login — returns a placeholder URL until full OAuth is ported.
    pub async fn begin_login(&self, country_code: &str) -> anyhow::Result<String> {
        let _ = country_code;
        Ok("https://kr.lgthinq.com/login/signIn?bridge=rethink-stub".into())
    }

    pub async fn complete_login(&self, country_code: &str, _callback_url: &str) -> anyhow::Result<bool> {
        self.storage.set_credentials(Some(Credentials {
            refresh_token: "stub".into(),
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
        self.enabled.lock().clear();
        Ok(())
    }

    pub async fn enable(
        &self,
        device_id: &str,
        _device_type: Option<&str>,
        status: Option<&dyn Fn(&str)>,
    ) -> anyhow::Result<bool> {
        if !self.is_logged_in() {
            if let Some(s) = status {
                s("not logged in");
            }
            return Ok(false);
        }
        self.enabled.lock().insert(device_id.to_string());
        if let Some(s) = status {
            s(&format!("bridge enabled for {device_id} (stub)"));
        }
        Ok(true)
    }

    pub async fn disable(&self, device_id: &str) -> anyhow::Result<()> {
        self.enabled.lock().remove(device_id);
        Ok(())
    }
}
