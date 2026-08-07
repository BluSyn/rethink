//! ThinQ1 length-prefixed JSON connection over a TLS/TCP stream.

use rethink_util::length_prefixed_frame::{self, FrameError};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use tracing::warn;

const IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(90);

pub struct T1ConnectionEvents {
    pub on_init: Arc<dyn Fn(String) + Send + Sync>,
    pub on_status: Arc<dyn Fn(Vec<u8>) + Send + Sync>,
    pub on_close: Arc<dyn Fn() + Send + Sync>,
}

pub async fn run_connection_with_acks<S>(
    stream: S,
    events: T1ConnectionEvents,
    mut outbound: mpsc::UnboundedReceiver<serde_json::Value>,
    ack_tx: mpsc::UnboundedSender<serde_json::Value>,
) where
    S: AsyncReadExt + AsyncWriteExt + Unpin + Send + 'static,
{
    let (mut reader, mut writer) = tokio::io::split(stream);

    let write_task = tokio::spawn(async move {
        while let Some(json) = outbound.recv().await {
            let s = serde_json::to_string(&json).unwrap_or_default();
            rethink_core::logging::log("outgoing", &[&s]);
            let frame = length_prefixed_frame::make(s.as_bytes());
            if writer.write_all(&frame).await.is_err() {
                break;
            }
        }
    });

    let mut splitter = length_prefixed_frame::Splitter::new(1_000_000);
    let mut buf = [0u8; 8192];
    let mut device_id: Option<String> = None;
    let mut idle = tokio::time::interval(IDLE_TIMEOUT);
    idle.reset();

    loop {
        tokio::select! {
            _ = idle.tick() => break,
            n = reader.read(&mut buf) => {
                match n {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        idle.reset();
                        match splitter.feed(&buf[..n]) {
                            Ok(frames) => {
                                for payload in frames {
                                    process_one(&payload, &mut device_id, &events, &ack_tx);
                                }
                            }
                            Err(FrameError::PayloadExceeded) => break,
                            Err(e) => {
                                warn!("thinq1 split: {e}");
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
    (events.on_close)();
    write_task.abort();
}

fn process_one(
    payload: &[u8],
    device_id: &mut Option<String>,
    events: &T1ConnectionEvents,
    ack_tx: &mpsc::UnboundedSender<serde_json::Value>,
) {
    let Ok(json) = serde_json::from_slice::<serde_json::Value>(payload) else {
        return;
    };
    rethink_core::logging::log(
        "incoming",
        &[&String::from_utf8_lossy(payload)],
    );

    // Device id from header
    if let Some(id) = json
        .pointer("/Header/x-lgedm-deviceId")
        .and_then(|v| v.as_str())
    {
        if device_id.is_none() {
            *device_id = Some(id.to_string());
            (events.on_init)(id.to_string());
        }
    }

    // ACK empty responses when needed
    if let Some(cmd) = json.pointer("/Body/Cmd").and_then(|v| v.as_str()) {
        if cmd == "DevInfo" || cmd == "Mon" {
            let ack = serde_json::json!({
                "Header": json.get("Header").cloned().unwrap_or(serde_json::json!({})),
                "Body": { "Return": "OK" }
            });
            let _ = ack_tx.send(ack);
        }
    }

    // Status payloads as raw bytes for handler
    (events.on_status)(payload.to_vec());
}
