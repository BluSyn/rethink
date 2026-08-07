//! Bridge state storage (JSON files under a base path).

use serde::{de::DeserializeOwned, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Environment {
    pub country_code: String,
    #[serde(default)]
    pub language_code: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Credentials {
    pub refresh_token: String,
    pub env: Environment,
}

pub trait BridgeState: Send + Sync {
    fn get_credentials(&self) -> Option<Credentials>;
    fn set_credentials(&self, credentials: Option<Credentials>);
    fn get_device_state_json(&self, id: &str) -> Option<serde_json::Value>;
    fn set_device_state_json(&self, id: &str, state: Option<serde_json::Value>);
}

/// File-backed storage matching TypeScript `JSONStorage`.
pub struct JsonStorage {
    base_path: PathBuf,
}

impl JsonStorage {
    pub fn new(base_path: impl Into<PathBuf>) -> Self {
        Self {
            base_path: base_path.into(),
        }
    }

    fn oauth2_path(&self) -> PathBuf {
        self.base_path.join("oauth2.json")
    }

    fn device_path(&self, id: &str) -> PathBuf {
        self.base_path.join(format!("device_{id}.json"))
    }

    fn read_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
        let data = fs::read_to_string(path).ok()?;
        serde_json::from_str(&data).ok()
    }

    fn write_json<T: Serialize>(path: &Path, value: &T) {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(s) = serde_json::to_string(value) {
            let _ = fs::write(path, s);
        }
    }
}

impl BridgeState for JsonStorage {
    fn get_credentials(&self) -> Option<Credentials> {
        Self::read_json(&self.oauth2_path())
    }

    fn set_credentials(&self, credentials: Option<Credentials>) {
        let path = self.oauth2_path();
        match credentials {
            Some(c) => Self::write_json(&path, &c),
            None => {
                let _ = fs::remove_file(path);
            }
        }
    }

    fn get_device_state_json(&self, id: &str) -> Option<serde_json::Value> {
        Self::read_json(&self.device_path(id))
    }

    fn set_device_state_json(&self, id: &str, state: Option<serde_json::Value>) {
        let path = self.device_path(id);
        match state {
            Some(s) => Self::write_json(&path, &s),
            None => {
                let _ = fs::remove_file(path);
            }
        }
    }
}
