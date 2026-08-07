//! ThinQ1 upstream RTI TLS connection (port of bridge/thinq1connection.ts).

use crate::pair::Thinq1DeviceState;
use rethink_util::length_prefixed_frame::{self, Splitter};
use rustls::pki_types::{CertificateDer, ServerName};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_rustls::rustls::ClientConfig;
use tokio_rustls::TlsConnector;

/// Cloneable send handle for local→LG status.
#[derive(Clone)]
pub struct Thinq1Handle {
    write_tx: mpsc::UnboundedSender<Vec<u8>>,
    is_live: Arc<AtomicBool>,
    device_id: String,
    last_state: Arc<parking_lot::Mutex<Option<Vec<u8>>>>,
    stopped: Arc<AtomicBool>,
}

impl Thinq1Handle {
    pub fn send_from_local(&self, data: &[u8]) {
        if self.stopped.load(Ordering::SeqCst) {
            return;
        }
        *self.last_state.lock() = Some(data.to_vec());
        eprintln!("[bridge] {} -> {}", self.device_id, hex::encode(data));
        if !self.is_live.load(Ordering::SeqCst) {
            return;
        }
        let body = format_status_body(&self.device_id, data);
        let frame = length_prefixed_frame::make(body.to_string().as_bytes());
        let _ = self.write_tx.send(frame);
    }

    pub fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
    }
}

pub async fn connect_thinq1(
    state: &Thinq1DeviceState,
    device_id: &str,
    model_name: &str,
    device_type: Option<&str>,
) -> anyhow::Result<(Thinq1Handle, mpsc::UnboundedReceiver<serde_json::Value>)> {
    let _ = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()?
        .post(format!(
            "{}/lgehadm/api/Device/TotalDeviceInfoSvc",
            state.http_server.trim_end_matches('/')
        ))
        .header("Accept", "text/xml")
        .header("content-type", "text/xml;charset=utf-8")
        .header("x-lgedm-userid", "lgehadmUser")
        .header(
            "x-lgedm-password",
            "bxLoLAZ+rp3oJDbEzRuIfAG4YumeqwWM9l6uUH6TupQ=",
        )
        .header("x-lgedm-deviceid", device_id)
        .header("x-lgedm-devicetype", device_type.unwrap_or("201"))
        .body(format!(
            "<lgedmRoot><countryCode>WW</countryCode><modelName>{model_name}</modelName>\
             <itemList><item>THINQ_TIME_SYNC_URI</item>\
             <elementList><elementCode>pushDetailYn</elementCode>\
             <elementValue>Y</elementValue></elementList></itemList></lgedmRoot>"
        ))
        .send()
        .await;

    eprintln!("[bridge] {device_id} connecting to {}", state.rti_server);
    let (host, port) = parse_host_port(&state.rti_server)?;

    let config = ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(NoVerifier))
        .with_no_client_auth();
    let connector = TlsConnector::from(Arc::new(config));
    let tcp = TcpStream::connect((host.as_str(), port)).await?;
    let server_name = ServerName::try_from(host.clone())
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .to_owned();
    let tls = connector.connect(server_name, tcp).await?;
    let (mut reader, mut writer) = tokio::io::split(tls);

    let (write_tx, mut write_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let (from_lg_tx, from_lg_rx) = mpsc::unbounded_channel();
    let is_live = Arc::new(AtomicBool::new(false));
    let stopped = Arc::new(AtomicBool::new(false));
    let last_state = Arc::new(parking_lot::Mutex::new(None::<Vec<u8>>));

    let did = device_id.to_string();
    tokio::spawn(async move {
        while let Some(frame) = write_rx.recv().await {
            if writer.write_all(&frame).await.is_err() {
                break;
            }
        }
    });

    // Alive
    let _ = write_tx.send(length_prefixed_frame::make(format_alive(&did).as_bytes()));
    let write_tx_alive = write_tx.clone();
    let did_alive = did.clone();
    let stopped_a = stopped.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            if stopped_a.load(Ordering::SeqCst) {
                break;
            }
            if write_tx_alive
                .send(length_prefixed_frame::make(format_alive(&did_alive).as_bytes()))
                .is_err()
            {
                break;
            }
        }
    });

    let is_live_r = is_live.clone();
    let last_state_r = last_state.clone();
    let write_tx_r = write_tx.clone();
    let stopped_r = stopped.clone();
    let did_r = did.clone();
    tokio::spawn(async move {
        let mut splitter = Splitter::new(1_000_000);
        let mut buf = [0u8; 8192];
        loop {
            if stopped_r.load(Ordering::SeqCst) {
                break;
            }
            match reader.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let Ok(frames) = splitter.feed(&buf[..n]) else {
                        break;
                    };
                    for payload in frames {
                        if let Ok(j) = serde_json::from_slice::<serde_json::Value>(&payload) {
                            handle_lg_json(
                                &j,
                                &did_r,
                                &is_live_r,
                                &last_state_r,
                                &write_tx_r,
                                &from_lg_tx,
                            );
                        }
                    }
                }
            }
        }
        eprintln!("[bridge] {did_r} disconnected");
    });

    let handle = Thinq1Handle {
        write_tx,
        is_live,
        device_id: device_id.to_string(),
        last_state,
        stopped,
    };
    Ok((handle, from_lg_rx))
}

fn handle_lg_json(
    j: &serde_json::Value,
    device_id: &str,
    is_live: &AtomicBool,
    last_state: &parking_lot::Mutex<Option<Vec<u8>>>,
    write_tx: &mpsc::UnboundedSender<Vec<u8>>,
    from_lg: &mpsc::UnboundedSender<serde_json::Value>,
) {
    let Some(body) = j.get("Body") else {
        return;
    };
    if body.get("CmdOpt").and_then(|v| v.as_str()) == Some("Start") {
        is_live.store(true, Ordering::SeqCst);
        if let Some(ref st) = *last_state.lock() {
            let msg = format_status_body(device_id, st);
            let _ = write_tx.send(length_prefixed_frame::make(msg.to_string().as_bytes()));
        }
        return;
    }
    if body.get("CmdOpt").and_then(|v| v.as_str()) == Some("Stop") {
        is_live.store(false, Ordering::SeqCst);
        return;
    }
    eprintln!("[bridge] {device_id} <- {body}");
    let _ = from_lg.send(body.clone());
    if body.get("ReturnCode").is_none() {
        if let Some(cmd_w_id) = body.get("CmdWId") {
            let ack = serde_json::json!({
                "Header": { "x-lgedm-deviceId": device_id },
                "Body": { "CmdWId": cmd_w_id, "ReturnCode": "0000" }
            });
            let _ = write_tx.send(length_prefixed_frame::make(ack.to_string().as_bytes()));
        }
    }
}

pub fn format_status_body(device_id: &str, data: &[u8]) -> serde_json::Value {
    serde_json::json!({
        "Header": { "x-lgedm-deviceId": device_id },
        "Body": {
            "CmdWId": format!("n-{device_id}"),
            "ReturnCode": "0000",
            "Format": "B64",
            "Data": base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD,
                data
            ),
        }
    })
}

fn format_alive(device_id: &str) -> String {
    serde_json::json!({
        "Header": { "x-lgedm-deviceId": device_id },
        "Body": {
            "CmdWId": uuid::Uuid::new_v4().to_string(),
            "Cmd": "Alive",
        }
    })
    .to_string()
}

fn parse_host_port(s: &str) -> anyhow::Result<(String, u16)> {
    let mut parts = s.split(':');
    let host = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("bad rtiServer"))?
        .to_string();
    let port: u16 = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("bad rtiServer port"))?
        .parse()?;
    Ok((host, port))
}

#[derive(Debug)]
struct NoVerifier;

impl rustls::client::danger::ServerCertVerifier for NoVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }
    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }
    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rethink_util::length_prefixed_frame;

    #[test]
    fn status_body_is_b64_and_framed() {
        let body = format_status_body("id-1", &[0xAA, 0xBB]);
        assert_eq!(body["Body"]["Format"], "B64");
        let data = body["Body"]["Data"].as_str().unwrap();
        assert_eq!(
            base64::Engine::decode(&base64::engine::general_purpose::STANDARD, data).unwrap(),
            vec![0xAA, 0xBB]
        );
        let frame = length_prefixed_frame::make(body.to_string().as_bytes());
        assert!(frame.len() > 4);
    }
}
