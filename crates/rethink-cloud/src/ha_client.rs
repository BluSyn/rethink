//! Real Home Assistant MQTT client via rumqttc, wired to HaMqttSink.

use anyhow::{Context, Result};
use rethink_core::ha::HaMqttSink;
use rumqttc::{AsyncClient, Event, Incoming, LastWill, MqttOptions, QoS};
use std::sync::Arc;
use std::time::Duration;
use url::Url;

pub async fn start_ha_client(sink: Arc<HaMqttSink>) -> Result<()> {
    let cfg = sink.config.clone();
    let (host, port, use_tls) = parse_mqtt_url(&cfg.mqtt_url)?;
    let mut opts = MqttOptions::new("rethink-cloud", host, port);
    opts.set_keep_alive(Duration::from_secs(30));
    if !cfg.mqtt_user.is_empty() {
        opts.set_credentials(&cfg.mqtt_user, &cfg.mqtt_pass);
    }
    let will = LastWill::new(
        format!("{}/availability", cfg.rethink_prefix),
        b"offline".to_vec(),
        QoS::AtLeastOnce,
        true,
    );
    opts.set_last_will(will);
    if use_tls {
        // rumqttc 0.24 uses Transport — leave default TCP for mqtt://
    }

    let (client, mut eventloop) = AsyncClient::new(opts, 64);
    let client_pub = client.clone();
    sink.set_publish_fn(move |topic, payload, retain| {
        let client = client_pub.clone();
        let topic = topic.to_string();
        let payload = payload.to_vec();
        tokio::spawn(async move {
            let _ = client
                .publish(topic, QoS::AtLeastOnce, retain, payload)
                .await;
        });
    });

    let prefix = cfg.rethink_prefix.clone();
    let discovery = cfg.discovery_prefix.clone();
    let sink2 = sink.clone();

    tokio::spawn(async move {
        loop {
            match eventloop.poll().await {
                Ok(Event::Incoming(Incoming::ConnAck(_))) => {
                    *sink2.connected.lock() = true;
                    rethink_core::logging::log("status", &["HA mqtt connection established"]);
                    sink2.published_availability.lock().clear();
                    let _ = client
                        .subscribe(format!("{discovery}/status"), QoS::AtLeastOnce)
                        .await;
                    let _ = client
                        .subscribe(format!("{prefix}/+/+/set"), QoS::AtLeastOnce)
                        .await;
                    let _ = client
                        .subscribe(format!("{prefix}/+/+/+/set"), QoS::AtLeastOnce)
                        .await;
                    let _ = client
                        .subscribe(format!("{prefix}/+/availability"), QoS::AtLeastOnce)
                        .await;
                    let _ = client
                        .publish(
                            format!("{prefix}/availability"),
                            QoS::AtLeastOnce,
                            true,
                            b"online".to_vec(),
                        )
                        .await;
                    sink2.emit_discovery();
                }
                Ok(Event::Incoming(Incoming::Publish(p))) => {
                    sink2.handle_message(&p.topic, &p.payload, p.retain);
                }
                Ok(Event::Incoming(Incoming::Disconnect)) => {
                    *sink2.connected.lock() = false;
                    rethink_core::logging::log("status", &["HA mqtt connection lost"]);
                }
                Ok(_) => {}
                Err(e) => {
                    *sink2.connected.lock() = false;
                    rethink_core::logging::log(
                        "status",
                        &[&format!("HA mqtt error: {e}")],
                    );
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        }
    });

    Ok(())
}

fn parse_mqtt_url(url: &str) -> Result<(String, u16, bool)> {
    let u = Url::parse(url).with_context(|| format!("parse mqtt_url {url}"))?;
    let host = u.host_str().unwrap_or("127.0.0.1").to_string();
    let use_tls = u.scheme() == "mqtts" || u.scheme() == "ssl";
    let port = u.port().unwrap_or(if use_tls { 8883 } else { 1883 });
    Ok((host, port, use_tls))
}
