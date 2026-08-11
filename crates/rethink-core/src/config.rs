//! Config loading and normalization (JSONC-compatible).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HaConfig {
    pub mqtt_url: String,
    pub discovery_prefix: String,
    pub rethink_prefix: String,
    #[serde(default)]
    pub mqtt_user: String,
    #[serde(default)]
    pub mqtt_pass: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PortSpec {
    Number(u16),
    Full {
        bind: u16,
        advertise: u16,
        #[serde(default)]
        address: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Port {
    pub bind: u16,
    pub advertise: u16,
    pub address: Option<String>,
}

impl From<PortSpec> for Port {
    fn from(p: PortSpec) -> Self {
        match p {
            PortSpec::Number(n) => Port {
                bind: n,
                advertise: n,
                address: None,
            },
            PortSpec::Full {
                bind,
                advertise,
                address,
            } => Port {
                bind,
                advertise,
                address,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeConfig {
    pub storage_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawConfig {
    pub hostname: String,
    pub homeassistant: HaConfig,
    pub ca_key_file: String,
    pub ca_cert_file: String,
    pub https_port: PortSpec,
    pub mqtts_port: PortSpec,
    pub mqtt_port: PortSpec,
    #[serde(default)]
    pub management_port: Option<PortSpec>,
    #[serde(default)]
    pub thinq1_https_port: Option<PortSpec>,
    #[serde(default)]
    pub thinq1_port: Option<PortSpec>,
    #[serde(default)]
    pub mqtt: Option<bool>,
    #[serde(default)]
    pub bridge: Option<BridgeConfig>,
    #[serde(default)]
    pub log: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub hostname: String,
    pub homeassistant: HaConfig,
    pub ca_key_file: String,
    pub ca_cert_file: String,
    pub https_port: Port,
    pub mqtts_port: Port,
    pub mqtt_port: Port,
    pub management_port: Option<Port>,
    pub thinq1_https_port: Port,
    pub thinq1_port: Port,
    pub mqtt: bool,
    pub bridge: Option<BridgeConfig>,
    pub log: Vec<String>,
}

#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    Json(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "IO error: {e}"),
            Self::Json(s) => write!(f, "JSON parse error: {s}"),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Json(_) => None,
        }
    }
}

impl From<std::io::Error> for ConfigError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

pub fn normalize(raw: RawConfig) -> Config {
    Config {
        hostname: raw.hostname,
        homeassistant: raw.homeassistant,
        ca_key_file: raw.ca_key_file,
        ca_cert_file: raw.ca_cert_file,
        https_port: raw.https_port.into(),
        mqtts_port: raw.mqtts_port.into(),
        mqtt_port: raw.mqtt_port.into(),
        management_port: raw.management_port.map(Into::into),
        thinq1_https_port: raw
            .thinq1_https_port
            .map(Into::into)
            .unwrap_or(Port {
                bind: 46030,
                advertise: 46030,
                address: None,
            }),
        thinq1_port: raw.thinq1_port.map(Into::into).unwrap_or(Port {
            bind: 47878,
            advertise: 47878,
            address: None,
        }),
        mqtt: raw.mqtt.unwrap_or(true),
        bridge: raw.bridge,
        log: raw
            .log
            .unwrap_or_else(|| vec!["status".into(), "incoming".into(), "HTTPS".into()]),
    }
}

/// Parse JSON or JSONC text into a normalized Config.
pub fn parse_config_text(text: &str) -> Result<Config, ConfigError> {
    // Strip // and /* */ comments for JSONC support
    let stripped = strip_jsonc_comments(text);
    let raw: RawConfig =
        serde_json::from_str(&stripped).map_err(|e| ConfigError::Json(e.to_string()))?;
    Ok(normalize(raw))
}

pub fn load_config(path: &std::path::Path) -> Result<Config, ConfigError> {
    let text = std::fs::read_to_string(path)?;
    parse_config_text(&text)
}

/// Minimal JSONC comment stripper (handles // line and /* */ block comments outside strings).
fn strip_jsonc_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    let mut in_string = false;
    let mut escape = false;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if in_string {
            out.push(c);
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if c == '"' {
            in_string = true;
            out.push(c);
            i += 1;
            continue;
        }
        if c == '/' && i + 1 < bytes.len() {
            if bytes[i + 1] == b'/' {
                // line comment
                i += 2;
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            if bytes[i + 1] == b'*' {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i = (i + 2).min(bytes.len());
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_jsonc_with_comments() {
        let text = r#"{
  "hostname": "rethink.lan", // comment
  "homeassistant": {
    "mqtt_url": "mqtt://localhost:1883",
    "discovery_prefix": "homeassistant",
    "rethink_prefix": "rethink",
    "mqtt_user": "",
    "mqtt_pass": ""
  },
  "ca_key_file": "ca.key",
  "ca_cert_file": "ca.cert",
  "https_port": 443,
  "mqtts_port": 8883,
  "mqtt_port": 1884
}"#;
        let cfg = parse_config_text(text).unwrap();
        assert_eq!(cfg.hostname, "rethink.lan");
        assert_eq!(cfg.https_port.bind, 443);
        assert_eq!(cfg.thinq1_https_port.bind, 46030);
        assert!(cfg.mqtt);
    }

    #[test]
    fn parse_port_object() {
        let text = r#"{
  "hostname": "x",
  "homeassistant": {
    "mqtt_url": "mqtt://localhost:1883",
    "discovery_prefix": "homeassistant",
    "rethink_prefix": "rethink",
    "mqtt_user": "",
    "mqtt_pass": ""
  },
  "ca_key_file": "k",
  "ca_cert_file": "c",
  "https_port": { "bind": 4433, "advertise": 443 },
  "mqtts_port": 8883,
  "mqtt_port": 1884
}"#;
        let cfg = parse_config_text(text).unwrap();
        assert_eq!(cfg.https_port.bind, 4433);
        assert_eq!(cfg.https_port.advertise, 443);
    }
}
