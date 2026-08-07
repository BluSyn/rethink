//! Structural checks that the shipped main path uses one HaMqttSink and management.

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
