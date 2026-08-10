//! Structural checks that the shipped main path uses one HaMqttSink and management.

#[test]
fn management_exposes_re_and_detail_routes() {
    let mgmt = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/management.rs"));
    assert!(mgmt.contains("/api/decode"), "decode API route required");
    assert!(mgmt.contains("/api/re/export"), "LLM export API route required");
    assert!(mgmt.contains("/api/tlv/catalog"), "TLV catalog route required");
    assert!(
        mgmt.contains("/api/devices/{device_id}") || mgmt.contains("/api/devices/"),
        "device detail route required"
    );
    assert!(mgmt.contains("llm_export_text") || mgmt.contains("re_export"), "LLM export helper");
    assert!(mgmt.contains("classify_tlvs") || mgmt.contains("decode_hex_payload"));

    let index = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../html/index.html"));
    assert!(index.contains("decode_hex") || index.contains("TLV decode"));
    assert!(index.contains("llm_export") || index.contains("Export for LLM"));
    assert!(index.contains("device_detail") || index.contains("Device detail"));

    let panel = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../html/panel.js"));
    assert!(panel.contains("api/decode"));
    assert!(panel.contains("api/re/export"));
    assert!(panel.contains("api/devices/"));
}

#[test]
fn main_uses_single_ha_sink_and_management_router() {
    let main_src = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/main.rs"));
    let news: Vec<_> = main_src.match_indices("HaMqttSink::new").collect();
    assert_eq!(
        news.len(),
        1,
        "expected exactly one HaMqttSink::new in main.rs, found {}",
        news.len()
    );
    assert!(
        main_src.contains("attach_ha_mqtt_sink"),
        "main must call attach_ha_mqtt_sink so set/discovery reach HaBridge"
    );
    assert!(
        main_src.contains("start_ha_client(sink)") || main_src.contains("start_ha_client(ha_sink"),
        "HA client must receive the same sink clone"
    );
    assert!(
        main_src.contains("mod management"),
        "management module must be linked"
    );
    assert!(
        main_src.contains("management::router"),
        "management::router must be used (not stub-only routes)"
    );
}

#[test]
fn ha_bridge_registers_set_and_discovery() {
    let src = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/ha_bridge.rs"));
    assert!(src.contains("fn attach_ha_mqtt_sink"));
    assert!(src.contains("on_set_property"));
    assert!(src.contains("on_discovery"));
    assert!(src.contains("T2Clip") || src.contains("SendToDevice::T2Clip") || {
        // send path may reference T2Clip via fully qualified path
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/ha_bridge.rs")).contains("T2Clip")
    });
}

#[test]
fn t2_adapter_send_not_no_op() {
    let src = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/ha_bridge.rs"));
    // Must not be empty body discard
    assert!(
        !src.contains("fn send(&self, cmd: &str, msg_type: i32, data: serde_json::Value) {\n        let _ = (cmd, msg_type, data);\n    }"),
        "T2Adapter::send must not discard CLIP commands"
    );
    assert!(src.contains("T2Clip"), "T2Adapter::send must use T2Clip");
}

#[test]
fn bridge_crate_has_real_upstream_connections() {
    let lib = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../rethink-bridge/src/lib.rs"
    ));
    let t2 = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../rethink-bridge/src/thinq2_conn.rs"
    ));
    let t1 = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../rethink-bridge/src/thinq1_conn.rs"
    ));
    let pair = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../rethink-bridge/src/pair.rs"
    ));
    assert!(t2.contains("connect_thinq2") || t2.contains("fn connect"));
    assert!(t2.contains("send_from_local"));
    assert!(t2.contains("device_packet") || t2.contains("format_device_packet"));
    assert!(t1.contains("connect_thinq1") || t1.contains("fn connect"));
    assert!(t1.contains("send_from_local"));
    assert!(pair.contains("pair_thinq2"));
    assert!(pair.contains("mqtt_server") || pair.contains("mqttServer"));
    // start_session must open upstream and wire on_data
    assert!(lib.contains("connect_thinq2") || lib.contains("connect_thinq2"));
    assert!(lib.contains("send_from_local"));
    assert!(lib.contains("on_data"));
    assert!(
        !lib.contains("let _ = buf"),
        "local→LG on_data must not ignore buffer"
    );
}
