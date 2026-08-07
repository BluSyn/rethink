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
    body: Option<Json<EnableBody>>,
) -> StatusCode {
    let Some(bridge) = &state.bridge else {
        return StatusCode::NOT_FOUND;
    };
    let dt = body.and_then(|b| b.0.device_type);
    let report = |s: &str| {
        broadcast(&state, &json!({ "status": s }));
    };
    match bridge
        .enable(&device_id, dt.as_deref(), Some(&report))
        .await
    {
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
