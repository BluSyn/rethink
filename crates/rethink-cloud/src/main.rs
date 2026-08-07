//! rethink-cloud: emulates LG ThinQ cloud and bridges to Home Assistant MQTT.

mod certs;
mod devmgr;
mod ha_bridge;
mod ha_client;
mod mqtt_broker;
mod thinq1;
mod thinq2;

use anyhow::{Context, Result};
use axum::routing::get;
use axum::{Json, Router};
use rethink_core::config::load_config;
use rethink_core::ha::HaMqttSink;
use rethink_core::logging;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let config_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("./config.json"));
    let config_dir = config_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();

    let mut config = load_config(&config_path).with_context(|| {
        format!(
            "failed to load config from {} (pass a JSONC config path as argv[1])",
            config_path.display()
        )
    })?;

    config.ca_key_file = config_dir
        .join(&config.ca_key_file)
        .to_string_lossy()
        .into();
    config.ca_cert_file = config_dir
        .join(&config.ca_cert_file)
        .to_string_lossy()
        .into();
    if let Some(ref mut bridge) = config.bridge {
        bridge.storage_path = config_dir
            .join(&bridge.storage_path)
            .to_string_lossy()
            .into();
    }

    let enabled: std::collections::HashMap<String, bool> =
        config.log.iter().map(|k| (k.clone(), true)).collect();
    logging::set_filter(move |topic| {
        enabled.get(topic).copied().unwrap_or(false)
            || enabled.get("all").copied().unwrap_or(false)
    });

    eprintln!(
        "[status] rethink-cloud starting hostname={} https={} mqtts={} mqtt={}",
        config.hostname,
        config.https_port.bind,
        config.mqtts_port.bind,
        config.mqtt_port.bind
    );

    let ca = certs::load_or_create(
        &config.hostname,
        Path::new(&config.ca_key_file),
        Path::new(&config.ca_cert_file),
    )?;
    eprintln!("[status] CA certificate ready");

    let manager = devmgr::DeviceManager::new();
    let ha_sink: Arc<dyn rethink_core::ha::HaConnection> =
        HaMqttSink::new(config.homeassistant.clone());
    let bridge = ha_bridge::HaBridge::new(ha_sink.clone());

    {
        let bridge = bridge.clone();
        manager.on_new_device(move |dev| {
            bridge.new_device(dev);
        });
    }

    // HA MQTT client
    if config.mqtt {
        if let Ok(sink) = Arc::downcast::<HaMqttSink>(
            // HaMqttSink is behind trait object — start via typed clone
            // fallback: spawn helper that takes config URL
            {
                // Use ha_client::start_ha_client with a fresh sink if we kept Arc
                // We already have ha_sink as trait object; re-create typed for client
                HaMqttSink::new(config.homeassistant.clone())
            },
        ) {
            let _ = sink;
        }
        let sink = HaMqttSink::new(config.homeassistant.clone());
        tokio::spawn(async move {
            if let Err(e) = ha_client::start_ha_client(sink).await {
                eprintln!("[status] HA MQTT client ended: {e}");
            }
        });
    }

    let broker = Arc::new(mqtt_broker::Broker::new());
    let t2_acceptor = thinq2::device::DeviceAcceptor::new(broker.clone(), manager.clone());
    let _ = t2_acceptor; // wired via broker publish hooks in DeviceAcceptor::new

    // Plain MQTT for local testing
    if config.mqtt {
        let b = broker.clone();
        let port = config.mqtt_port.bind;
        tokio::spawn(async move {
            match TcpListener::bind(("0.0.0.0", port)).await {
                Ok(listener) => {
                    eprintln!("[status] MQTT listening on {port}");
                    loop {
                        match listener.accept().await {
                            Ok((stream, _)) => {
                                let b = b.clone();
                                tokio::spawn(async move {
                                    b.accept_tcp(stream).await;
                                });
                            }
                            Err(e) => {
                                eprintln!("[status] MQTT accept error: {e}");
                                break;
                            }
                        }
                    }
                }
                Err(e) => eprintln!("[status] MQTT bind failed on {port}: {e}"),
            }
        });
    }

    // MQTTS
    {
        let b = broker.clone();
        let port = config.mqtts_port.bind;
        match certs::server_config(&ca) {
            Ok(tls_cfg) => {
                let acceptor = tokio_rustls::TlsAcceptor::from(tls_cfg);
                tokio::spawn(async move {
                    match TcpListener::bind(("0.0.0.0", port)).await {
                        Ok(listener) => {
                            eprintln!("[status] MQTTS listening on {port}");
                            loop {
                                match listener.accept().await {
                                    Ok((stream, _)) => {
                                        let acceptor = acceptor.clone();
                                        let b = b.clone();
                                        tokio::spawn(async move {
                                            match acceptor.accept(stream).await {
                                                Ok(tls) => b.accept_tls(tls).await,
                                                Err(e) => {
                                                    tracing::debug!("TLS accept: {e}");
                                                }
                                            }
                                        });
                                    }
                                    Err(e) => {
                                        eprintln!("[status] MQTTS accept error: {e}");
                                        break;
                                    }
                                }
                            }
                        }
                        Err(e) => eprintln!("[status] MQTTS bind failed on {port}: {e}"),
                    }
                });
            }
            Err(e) => eprintln!("[status] TLS config failed: {e}"),
        }
    }

    // HTTPS ThinQ2 provisioning
    {
        let port = config.https_port.bind;
        let ca = Arc::new(ca.clone());
        let cfg = Arc::new(config.clone());
        let router = thinq2::provisioning::routes(cfg, ca.clone());
        match certs::server_config(&ca) {
            Ok(tls_cfg) => {
                let acceptor = tokio_rustls::TlsAcceptor::from(tls_cfg);
                tokio::spawn(async move {
                    match TcpListener::bind(("0.0.0.0", port)).await {
                        Ok(listener) => {
                            eprintln!("[status] HTTPS listening on {port}");
                            loop {
                                match listener.accept().await {
                                    Ok((stream, _)) => {
                                        let acceptor = acceptor.clone();
                                        let router = router.clone();
                                        tokio::spawn(async move {
                                            if let Ok(tls) = acceptor.accept(stream).await {
                                                let _ = hyper_util::server::conn::auto::Builder::new(
                                                    hyper_util::rt::TokioExecutor::new(),
                                                )
                                                .serve_connection(
                                                    hyper_util::rt::TokioIo::new(tls),
                                                    hyper_util::service::TowerToHyperService::new(
                                                        router,
                                                    ),
                                                )
                                                .await;
                                            }
                                        });
                                    }
                                    Err(_) => break,
                                }
                            }
                        }
                        Err(e) => eprintln!("[status] HTTPS bind failed on {port}: {e}"),
                    }
                });
            }
            Err(e) => eprintln!("[status] HTTPS TLS config failed: {e}"),
        }
    }

    // ThinQ1 HTTP (plain, typically)
    {
        let port = config.thinq1_https_port.bind;
        let cfg = Arc::new(config.clone());
        let meta = thinq1::http::device_metadata_store();
        let router = thinq1::http::routes(cfg, meta.clone());
        let acceptor = thinq1::device::DeviceAcceptor::new(meta, manager.clone());
        tokio::spawn(async move {
            match TcpListener::bind(("0.0.0.0", port)).await {
                Ok(listener) => {
                    eprintln!("[status] ThinQ1 HTTP listening on {port}");
                    if let Err(e) = axum::serve(listener, router).await {
                        eprintln!("[status] ThinQ1 HTTP ended: {e}");
                    }
                }
                Err(e) => eprintln!("[status] ThinQ1 HTTP bind failed on {port}: {e}"),
            }
            let _ = acceptor;
        });
    }

    // ThinQ1 device TCP port
    {
        let port = config.thinq1_port.bind;
        let meta = thinq1::http::device_metadata_store();
        let acceptor = thinq1::device::DeviceAcceptor::new(meta, manager.clone());
        tokio::spawn(async move {
            match TcpListener::bind(("0.0.0.0", port)).await {
                Ok(listener) => {
                    eprintln!("[status] ThinQ1 device port listening on {port}");
                    loop {
                        match listener.accept().await {
                            Ok((stream, _)) => {
                                let a = acceptor.clone();
                                tokio::spawn(async move {
                                    a.accept(stream).await;
                                });
                            }
                            Err(e) => {
                                eprintln!("[status] ThinQ1 accept error: {e}");
                                break;
                            }
                        }
                    }
                }
                Err(e) => eprintln!("[status] ThinQ1 port bind failed on {port}: {e}"),
            }
        });
    }

    // Management UI
    let mgmt_port = config
        .management_port
        .as_ref()
        .map(|p| p.bind)
        .unwrap_or(44401);
    let mgr_for_api = manager.clone();
    let app = Router::new()
        .route("/", get(|| async { management_index() }))
        .route(
            "/api/devices",
            get(move || {
                let m = mgr_for_api.clone();
                async move { Json(m.list_json()) }
            }),
        )
        .route(
            "/api/health",
            get(|| async {
                Json(serde_json::json!({"ok": true, "service": "rethink-cloud"}))
            }),
        );

    let addr = SocketAddr::from(([0, 0, 0, 0], mgmt_port));
    match TcpListener::bind(addr).await {
        Ok(listener) => {
            eprintln!("[status] management UI listening on {mgmt_port}");
            eprintln!("[status] rethink-cloud ready");
            axum::serve(listener, app).await?;
        }
        Err(e) => {
            eprintln!("[status] management bind failed on {mgmt_port}: {e}");
            eprintln!("[status] rethink-cloud running without management UI");
            std::future::pending::<()>().await;
        }
    }

    Ok(())
}

fn management_index() -> axum::response::Html<&'static str> {
    axum::response::Html(include_str!("../../../html/index.html"))
}
