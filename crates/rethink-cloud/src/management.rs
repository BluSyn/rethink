//! Management HTTP + WebSocket UI (port of management/index.ts).

use crate::devmgr::{DeviceManager, Platform, SendToDevice};
use crate::ha_bridge::HaBridge;
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Path, Query, State, WebSocketUpgrade};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use include_dir::{include_dir, Dir};
use parking_lot::Mutex;
use rethink_bridge::Bridge;
use rethink_core::ha::HaConnection;
use rethink_util::packet_codec::{decode_packet, Decoded, Direction};
use rethink_util::tlv;
use rethink_util::tlv_catalog::{classify_tlvs, llm_export_text, KNOWN_TAGS};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::mpsc;

static HTML: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../html");

#[derive(Clone)]
pub struct MgmtState {
    pub ha: Arc<dyn HaConnection>,
    pub ha_bridge: Arc<HaBridge>,
    pub manager: Arc<DeviceManager>,
    pub bridge: Option<Arc<Bridge>>,
    pub subscribers: Arc<Mutex<Vec<mpsc::UnboundedSender<String>>>>,
}

pub fn router(state: MgmtState) -> Router {
    Router::new()
        .route("/ws", get(ws_status))
        .route("/device", get(ws_device))
        .route("/thinq_login", get(thinq_login))
        .route("/thinq_login_accept", post(thinq_login_accept))
        .route("/thinq_logout", post(thinq_logout))
        .route("/bridge/{device_id}/enable", post(bridge_enable))
        .route("/bridge/{device_id}/disable", post(bridge_disable))
        // RE / device-detail APIs (used by modern management UI)
        .route("/api/tlv/catalog", get(api_tlv_catalog))
        .route("/api/decode", post(api_decode))
        .route("/api/re/export", post(api_re_export))
        .route("/api/devices/{device_id}", get(api_device_detail))
        .fallback(static_file)
        .with_state(state)
}

fn enum_devices(state: &MgmtState) -> Value {
    let mut all = serde_json::Map::new();
    for (id, dev) in state.manager.all() {
        all.insert(
            id.clone(),
            json!({
                "model": dev.meta.model_id,
                "deviceType": dev.meta.device_type,
                "platform": match dev.platform {
                    Platform::Thinq1 => "thinq1",
                    Platform::Thinq2 => "thinq2",
                },
                "mapped": state.ha_bridge.has_device(&id),
                "bridged": state.bridge.as_ref().map(|b| b.status_for(&id)).unwrap_or(false),
            }),
        );
    }
    Value::Object(all)
}

fn bridge_status(state: &MgmtState) -> Value {
    match &state.bridge {
        Some(b) => json!({ "loggedIn": b.is_logged_in() }),
        None => Value::Null,
    }
}

async fn ws_status(ws: WebSocketUpgrade, State(state): State<MgmtState>) -> Response {
    ws.on_upgrade(move |socket| handle_status_ws(socket, state))
}

async fn handle_status_ws(mut socket: WebSocket, state: MgmtState) {
    let (tx, mut rx) = mpsc::unbounded_channel();
    state.subscribers.lock().push(tx);

    let hello = json!({
        "ha": state.ha.is_connected(),
        "bridge": bridge_status(&state),
        "devices": enum_devices(&state),
    });
    let _ = socket
        .send(Message::Text(hello.to_string().into()))
        .await;

    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Some(s) => {
                        if socket.send(Message::Text(s.into())).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(_)) => break,
                }
            }
        }
    }
    // cleanup dead senders later on next broadcast
}

pub fn broadcast(state: &MgmtState, message: &Value) {
    let str = message.to_string();
    let mut subs = state.subscribers.lock();
    subs.retain(|tx| tx.send(str.clone()).is_ok());
}

#[derive(Deserialize)]
struct DeviceQuery {
    id: Option<String>,
}

async fn ws_device(
    ws: WebSocketUpgrade,
    Query(q): Query<DeviceQuery>,
    State(state): State<MgmtState>,
) -> Response {
    let Some(id) = q.id else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    ws.on_upgrade(move |socket| handle_device_ws(socket, state, id))
}

async fn handle_device_ws(mut socket: WebSocket, state: MgmtState, id: String) {
    // Periodic presence check + packet monitor
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(500));
    let mut online = false;
    let (data_tx, mut data_rx) = mpsc::unbounded_channel::<String>();

    // Register data listeners when device appears
    let mut hooked: HashSet<String> = HashSet::new();

    loop {
        tokio::select! {
            _ = interval.tick() => {
                let dev = state.manager.get(&id);
                let is_on = dev.is_some();
                if is_on != online {
                    online = is_on;
                    if let Some(ref d) = dev {
                        let _ = socket.send(Message::Text(json!({
                            "status": "online",
                            "meta": d.meta,
                        }).to_string().into())).await;
                        if !hooked.contains(&id) {
                            hooked.insert(id.clone());
                            let tx = data_tx.clone();
                            d.add_data_handler(move |buf| {
                                let _ = tx.send(json!({
                                    "rx": hex::encode(buf),
                                    "injected": false,
                                }).to_string());
                            });
                            let tx2 = data_tx.clone();
                            d.add_send_handler(move |msg| {
                                let tx_val = match msg {
                                    SendToDevice::T2Packet(b) => json!({
                                        "tx": hex::encode(b),
                                        "injected": false,
                                    }),
                                    SendToDevice::T2Clip { cmd, msg_type, data } => json!({
                                        "tx": { "cmd": cmd, "type": msg_type, "data": data },
                                        "injected": false,
                                    }),
                                    SendToDevice::T1Json(v) => json!({
                                        "tx": v.to_string(),
                                        "injected": false,
                                    }),
                                };
                                let _ = tx2.send(tx_val.to_string());
                            });
                        }
                    } else {
                        let _ = socket.send(Message::Text(
                            json!({"status":"offline"}).to_string().into()
                        )).await;
                    }
                }
            }
            msg = data_rx.recv() => {
                if let Some(s) = msg {
                    if socket.send(Message::Text(s.into())).await.is_err() {
                        break;
                    }
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Text(t))) => {
                        if let Ok(v) = serde_json::from_str::<Value>(&t) {
                            if let Some(dev) = state.manager.get(&id) {
                                if let Some(s) = v.get("sendToDevice").and_then(|x| x.as_str()) {
                                    if let Ok(buf) = hex::decode(s) {
                                        (dev.send_to_device)(SendToDevice::T2Packet(buf));
                                    }
                                }
                                if let Some(obj) = v.get("sendToDevice").filter(|x| x.is_object()) {
                                    (dev.send_to_device)(SendToDevice::T1Json(obj.clone()));
                                }
                                if let Some(s) = v.get("sendFromDevice").and_then(|x| x.as_str()) {
                                    if let Ok(buf) = hex::decode(s) {
                                        (dev.emit_data)(buf);
                                    }
                                }
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(_)) => break,
                }
            }
        }
    }
}

#[derive(Deserialize)]
struct LoginQuery {
    #[serde(rename = "countryCode")]
    country_code: Option<String>,
}

async fn thinq_login(
    State(state): State<MgmtState>,
    Query(q): Query<LoginQuery>,
) -> Response {
    let Some(bridge) = &state.bridge else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let cc = q.country_code.unwrap_or_else(|| "US".into());
    match bridge.begin_login(&cc).await {
        Ok(url) => Redirect::temporary(&url).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[derive(Deserialize)]
struct LoginAccept {
    url: String,
    #[serde(rename = "countryCode")]
    country_code: String,
}

async fn thinq_login_accept(
    State(state): State<MgmtState>,
    Json(body): Json<LoginAccept>,
) -> StatusCode {
    let Some(bridge) = &state.bridge else {
        return StatusCode::NOT_FOUND;
    };
    match bridge.complete_login(&body.country_code, &body.url).await {
        Ok(true) => {
            broadcast(&state, &json!({ "bridge": bridge_status(&state) }));
            StatusCode::OK
        }
        _ => StatusCode::BAD_REQUEST,
    }
}

async fn thinq_logout(State(state): State<MgmtState>) -> StatusCode {
    if let Some(bridge) = &state.bridge {
        let _ = bridge.logout().await;
        broadcast(&state, &json!({ "bridge": bridge_status(&state) }));
    }
    StatusCode::OK
}

#[derive(Deserialize)]
struct EnableBody {
    #[serde(rename = "deviceType")]
    device_type: Option<String>,
}

async fn bridge_enable(
    State(state): State<MgmtState>,
    Path(device_id): Path<String>,
    body: Result<Json<EnableBody>, axum::extract::rejection::JsonRejection>,
) -> StatusCode {
    let Some(bridge) = state.bridge.clone() else {
        return StatusCode::NOT_FOUND;
    };
    let dt = body.ok().and_then(|b| b.0.device_type);
    let (status_tx, mut status_rx) = mpsc::unbounded_channel::<String>();
    let mgr = state.manager.clone();
    let device = match mgr.get(&device_id) {
        Some(d) => Arc::new(crate::bridge_adapter::ConnectedAsLocal(d))
            as Arc<dyn rethink_bridge::LocalDevice>,
        None => {
            broadcast(&state, &json!({ "status": "device not connected" }));
            return StatusCode::BAD_REQUEST;
        }
    };
    let report: Box<dyn FnMut(&str) + Send> = Box::new(move |s: &str| {
        let _ = status_tx.send(s.to_string());
    });
    let result = bridge.enable(device, dt.as_deref(), Some(report)).await;
    while let Ok(s) = status_rx.try_recv() {
        broadcast(&state, &json!({ "status": s }));
    }
    match result {
        Ok(true) => {
            broadcast(&state, &json!({ "devices": enum_devices(&state) }));
            StatusCode::NO_CONTENT
        }
        Ok(false) => StatusCode::BAD_REQUEST,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

async fn bridge_disable(
    State(state): State<MgmtState>,
    Path(device_id): Path<String>,
) -> StatusCode {
    if let Some(bridge) = &state.bridge {
        let _ = bridge.disable(&device_id).await;
        broadcast(&state, &json!({ "devices": enum_devices(&state) }));
    }
    StatusCode::NO_CONTENT
}

async fn static_file(uri: axum::http::Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    // try with .html extension
    let candidates = [path, &format!("{path}.html")];
    for c in candidates {
        if let Some(file) = HTML.get_file(c) {
            let mime = mime_guess::from_path(c)
                .first_or_octet_stream()
                .to_string();
            return (
                StatusCode::OK,
                [(header::CONTENT_TYPE, mime)],
                file.contents().to_vec(),
            )
                .into_response();
        }
    }
    StatusCode::NOT_FOUND.into_response()
}

// ── Reverse-engineering / device detail APIs ───────────────────────────────

async fn api_tlv_catalog() -> Json<Value> {
    let tags: Vec<Value> = KNOWN_TAGS
        .iter()
        .map(|t| {
            json!({
                "id": t.id,
                "hex": format!("0x{:03x}", t.id),
                "name": t.name,
                "family": t.family,
            })
        })
        .collect();
    Json(json!({ "tags": tags, "count": tags.len() }))
}

#[derive(Deserialize)]
struct DecodeBody {
    hex: String,
    /// Optional: "fromDevice" | "toDevice" (default fromDevice).
    direction: Option<String>,
    model_id: Option<String>,
}

fn strip_hex(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_hexdigit())
        .collect::<String>()
        .to_ascii_lowercase()
}

fn decode_hex_payload(hex_in: &str, direction: Option<&str>) -> Result<Value, String> {
    let hex = strip_hex(hex_in);
    if hex.is_empty() {
        return Err("empty hex".into());
    }
    let bytes = hex::decode(&hex).map_err(|e| format!("hex decode: {e}"))?;
    let requested_dir = match direction.unwrap_or("fromDevice") {
        "toDevice" | "to" | "tx" => "toDevice",
        _ => "fromDevice",
    };

    let mut protocol = String::from("Unknown");
    let mut tlv_items: Vec<(u16, u32)> = Vec::new();
    let mut aabb_body: Option<String> = None;
    let mut notes = Vec::new();
    let mut dir_out = requested_dir.to_string();
    let mut crc_ok: Option<bool> = None;

    match decode_packet(&hex) {
        Decoded::Tlv(t) => {
            protocol = "Tlv".into();
            dir_out = match t.direction {
                Direction::FromDevice => "fromDevice".into(),
                Direction::ToDevice => "toDevice".into(),
            };
            crc_ok = Some(t.crc_ok);
            tlv_items = t.tlv.iter().map(|e| (e.t, e.v)).collect();
            notes.push(format!(
                "kind=0x{:02x} len={}",
                t.frame.kind, t.frame.len
            ));
        }
        Decoded::Aabb(a) => {
            protocol = "Aabb".into();
            aabb_body = Some(a.body.clone());
            crc_ok = Some(a.checksum_ok);
            notes.push("AABB frame; body hex in aabbBody".into());
        }
        Decoded::Unknown(u) => {
            notes.push(format!("packet_codec: {}", u.reason));
            // Raw TLV body (no UART envelope) — common when pasting TLV-only captures
            let candidates: Vec<&[u8]> = if bytes.len() > 2 {
                vec![&bytes[..], &bytes[2..]]
            } else {
                vec![&bytes[..]]
            };
            let mut parsed = false;
            for c in candidates {
                let items = tlv::parse(c);
                if !items.is_empty() {
                    tlv_items = items.into_iter().map(|t| (t.t, t.v)).collect();
                    protocol = "TlvRaw".into();
                    parsed = true;
                    break;
                }
            }
            if !parsed && bytes.len() >= 2 {
                aabb_body = Some(hex.clone());
                protocol = "Raw".into();
            }
        }
    }

    let classified = classify_tlvs(&tlv_items);
    let unknowns: Vec<Value> = classified
        .iter()
        .filter(|c| !c.known)
        .map(|c| json!({"t": c.t, "hex": format!("0x{:03x}", c.t), "v": c.v}))
        .collect();
    let elements: Vec<Value> = classified
        .iter()
        .map(|c| {
            json!({
                "t": c.t,
                "hex": format!("0x{:03x}", c.t),
                "v": c.v,
                "known": c.known,
                "name": c.name,
            })
        })
        .collect();

    Ok(json!({
        "ok": true,
        "protocol": protocol,
        "direction": dir_out,
        "crcOk": crc_ok,
        "elements": elements,
        "unknowns": unknowns,
        "unknownCount": unknowns.len(),
        "aabbBody": aabb_body,
        "notes": notes,
        "hex": hex,
    }))
}

async fn api_decode(Json(body): Json<DecodeBody>) -> Response {
    match decode_hex_payload(&body.hex, body.direction.as_deref()) {
        Ok(v) => Json(v).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "error": e}))).into_response(),
    }
}

#[derive(Deserialize)]
struct ExportBody {
    hex: String,
    direction: Option<String>,
    model_id: Option<String>,
}

async fn api_re_export(Json(body): Json<ExportBody>) -> Response {
    let decoded = match decode_hex_payload(&body.hex, body.direction.as_deref()) {
        Ok(v) => v,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "error": e}))).into_response();
        }
    };
    let items: Vec<(u16, u32)> = decoded
        .get("elements")
        .and_then(|e| e.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|el| {
                    let t = el.get("t")?.as_u64()? as u16;
                    let v = el.get("v")?.as_u64()? as u32;
                    Some((t, v))
                })
                .collect()
        })
        .unwrap_or_default();
    let classified = classify_tlvs(&items);
    let hex = decoded
        .get("hex")
        .and_then(|h| h.as_str())
        .unwrap_or("");
    let text = llm_export_text(
        body.model_id.as_deref(),
        body.direction.as_deref().or(Some("fromDevice")),
        hex,
        &classified,
    );
    Json(json!({
        "ok": true,
        "text": text,
        "unknownCount": classified.iter().filter(|c| !c.known).count(),
        "decode": decoded,
    }))
    .into_response()
}

async fn api_device_detail(
    State(state): State<MgmtState>,
    Path(device_id): Path<String>,
) -> Response {
    let Some(dev) = state.manager.get(&device_id) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"ok": false, "error": "device not connected"})),
        )
            .into_response();
    };
    let mapped = state.ha_bridge.has_device(&device_id);
    let bridged = state
        .bridge
        .as_ref()
        .map(|b| b.status_for(&device_id))
        .unwrap_or(false);

    Json(json!({
        "ok": true,
        "id": dev.id,
        "platform": match dev.platform {
            Platform::Thinq1 => "thinq1",
            Platform::Thinq2 => "thinq2",
        },
        "modelId": dev.meta.model_id,
        "modelName": dev.meta.model_name,
        "deviceType": dev.meta.device_type,
        "swVersion": dev.meta.sw_version,
        "mapped": mapped,
        "bridged": bridged,
        "haConnected": state.ha.is_connected(),
        // Live property stream is via monitor WebSocket (/device?id=…);
        // detail endpoint focuses on identity + bridge/HA mapping status.
        "monitorUrl": format!("monitor?id={device_id}"),
    }))
    .into_response()
}

/// Pure helpers exposed for unit tests (same path as HTTP handlers).
pub fn re_decode_for_test(hex: &str, direction: Option<&str>) -> Result<Value, String> {
    decode_hex_payload(hex, direction)
}

pub fn re_export_for_test(hex: &str, model_id: Option<&str>) -> Result<String, String> {
    let decoded = decode_hex_payload(hex, Some("fromDevice"))?;
    let items: Vec<(u16, u32)> = decoded
        .get("elements")
        .and_then(|e| e.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|el| {
                    let t = el.get("t")?.as_u64()? as u16;
                    let v = el.get("v")?.as_u64()? as u32;
                    Some((t, v))
                })
                .collect()
        })
        .unwrap_or_default();
    let classified = classify_tlvs(&items);
    let h = decoded.get("hex").and_then(|x| x.as_str()).unwrap_or(hex);
    Ok(llm_export_text(model_id, Some("fromDevice"), h, &classified))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_known_tlv_query_style() {
        // Tags are 10-bit; use 0x3fe as a deliberately uncatalogued tag.
        let tlv_bytes = tlv::build(&[tlv::Tlv::new(0x1f7, 1), tlv::Tlv::new(0x3fe, 42)]);
        let hex = hex::encode(&tlv_bytes);
        let v = re_decode_for_test(&hex, Some("fromDevice")).unwrap();
        assert_eq!(v["ok"], true);
        let els = v["elements"].as_array().unwrap();
        assert!(els.iter().any(|e| e["t"] == 0x1f7 && e["known"] == true));
        assert!(els.iter().any(|e| e["t"] == 0x3fe && e["known"] == false));
        assert!(v["unknownCount"].as_u64().unwrap() >= 1);
    }

    #[test]
    fn export_includes_llm_section() {
        let tlv_bytes = tlv::build(&[tlv::Tlv::new(0x1f7, 1), tlv::Tlv::new(0x3fe, 9)]);
        let text = re_export_for_test(&hex::encode(&tlv_bytes), Some("HUM_056905_WW")).unwrap();
        assert!(text.contains("UNKNOWN") || text.contains("Unknown"));
        assert!(text.contains("HUM_056905_WW"));
        assert!(text.contains("0x3fe") || text.contains("3fe"));
    }

    #[test]
    fn catalog_nonempty() {
        assert!(!KNOWN_TAGS.is_empty());
    }
}
