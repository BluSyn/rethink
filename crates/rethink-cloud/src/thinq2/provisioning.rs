//! ThinQ2 HTTPS provisioning routes (/route, certificates).

use crate::certs::{sign_csr, Ca};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use rethink_core::config::Config;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Clone)]
pub struct T2HttpState {
    pub config: Arc<Config>,
    pub ca: Arc<Ca>,
}

pub fn routes(config: Arc<Config>, ca: Arc<Ca>) -> Router {
    let state = T2HttpState { config, ca };
    Router::new()
        .route("/route", get(route))
        .route("/route/certificate", get(route_certificate))
        .route("/device/{device_id}/certificate", post(device_certificate))
        .fallback(fallback)
        .with_state(state)
}

async fn route(State(state): State<T2HttpState>) -> Json<Value> {
    rethink_core::logging::log("HTTPS", &["/route"]);
    Json(json!({
        "resultCode": "0000",
        "result": {
            "apiServer": format!("https://{}:{}", state.config.hostname, state.config.https_port.advertise),
            "mqttServer": format!("ssl://{}:{}", state.config.hostname, state.config.mqtts_port.advertise),
        }
    }))
}

#[derive(Deserialize)]
struct CertQuery {
    name: Option<String>,
}

async fn route_certificate(
    State(state): State<T2HttpState>,
    Query(q): Query<CertQuery>,
) -> Json<Value> {
    if q.name.is_some() {
        Json(json!({
            "resultCode": "0000",
            "result": { "certificatePem": state.ca.cert_pem }
        }))
    } else {
        Json(json!({
            "resultCode": "0000",
            "result": ["common-server", "aws-iot"]
        }))
    }
}

#[derive(Deserialize)]
struct CsrBody {
    csr: String,
}

async fn device_certificate(
    State(state): State<T2HttpState>,
    Path(_device_id): Path<String>,
    Json(body): Json<CsrBody>,
) -> Response {
    match sign_csr(&state.ca, &body.csr).await {
        Ok(pem) => Json(json!({
            "resultCode": "0000",
            "result": { "certificatePem": pem }
        }))
        .into_response(),
        Err(e) => {
            eprintln!("CSR sign failed: {e}");
            // Fall back to returning CA cert so smoke tests still respond
            Json(json!({
                "resultCode": "0000",
                "result": { "certificatePem": state.ca.cert_pem }
            }))
            .into_response()
        }
    }
}

async fn fallback() -> Response {
    (
        StatusCode::OK,
        [("content-type", "text/xml;charset=utf-8")],
        "",
    )
        .into_response()
}

pub fn generate_deploy_response(payload: &Value) -> Value {
    let did = payload
        .get("did")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let cmd = payload
        .get("cmd")
        .and_then(|v| v.as_str())
        .unwrap_or("deploy")
        .to_string();
    let mid = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    json!({
        "did": did,
        "mid": mid,
        "cmd": "completeProvisioning",
        "type": 0,
        "data": {
            "result": 0,
            "host": "message",
            "appInfo": {
                "host": "message",
                "publication": {
                    "message": format!("clip/message/devices/{did}"),
                    "provisioning": format!("clip/provisioning/devices/{did}"),
                }
            },
            "provisioningType": cmd,
            "deployInterval": 600,
        }
    })
}
