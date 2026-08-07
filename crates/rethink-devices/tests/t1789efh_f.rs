use rethink_core::{hex_decode, MockHaConnection, MockThinq2Device, Metadata};
use rethink_devices::devices::t1789efh_f::Device;

const DEVICE_ID: &str = "test-id";

fn meta() -> Metadata { Metadata::new("T1789EFH_F", "LG WT7300CW", "1.0") }
fn make() -> (std::sync::Arc<MockHaConnection>, std::sync::Arc<MockThinq2Device>, std::sync::Arc<Device>) {
    let ha = MockHaConnection::new();
    let thinq = MockThinq2Device::new(DEVICE_ID, meta());
    let dev = Device::new(ha.clone(), thinq.clone(), meta());
    (ha, thinq, dev)
}
fn prop(ha: &MockHaConnection, n: &str) -> Option<String> {
    ha.device(DEVICE_ID)?.properties.get(n).map(|p| p.as_string())
}

#[test]
fn config_components() {
    let (ha, _, _) = make();
    let comps = &ha.device(DEVICE_ID).unwrap().config.as_ref().unwrap().components;
    assert!(comps.contains_key("power"));
    assert!(comps.contains_key("status"));
    assert!(comps.contains_key("remaining_time"));
    let opts = comps["status"]["options"].as_array().unwrap();
    assert!(opts.iter().any(|v| v == "Wash (main)"));
}

#[test]
fn eb_and_ec_frames() {
    let (ha, thinq, _) = make();
    thinq.emit_data(&hex_decode("AA2120EB00000000000000000000000000000000000000000000000000000000BB"));
    assert_eq!(prop(&ha, "power").as_deref(), Some("OFF"));
    assert_eq!(prop(&ha, "status").as_deref(), Some("Off"));
    assert_eq!(prop(&ha, "remaining_time").as_deref(), Some("0"));

    thinq.emit_data(&hex_decode("AA2120EB000005001E0000000000000000000000000000000000000000000000BB"));
    assert_eq!(prop(&ha, "power").as_deref(), Some("ON"));
    assert_eq!(prop(&ha, "status").as_deref(), Some("Wash (main)"));
    assert_eq!(prop(&ha, "remaining_time").as_deref(), Some("30"));

    thinq.emit_data(&hex_decode(
        "AA3C20EC0019050018011A0200050304000000000410000000050000006400001906001D011E0200000304000000000410000000050000006400FABB"
    ));
    assert_eq!(prop(&ha, "status").as_deref(), Some("Wash (main)"));
    assert_eq!(prop(&ha, "remaining_time").as_deref(), Some("24"));

    thinq.emit_data(&hex_decode(
        "AA3C20EC001902002B00340800030104000000000410000000050000006400001902002B00340800030104000000000010000000050000006400A9BB"
    ));
    assert_eq!(prop(&ha, "status").as_deref(), Some("Paused"));
    assert_eq!(prop(&ha, "remaining_time").as_deref(), Some("43"));
}

#[test]
fn ignores_wrong_frames() {
    let (ha, thinq, _) = make();
    thinq.emit_data(&hex_decode("AA2130EB00000000000000000000000000000000000000000000000000000000BB"));
    assert!(prop(&ha, "power").is_none());
    thinq.emit_data(&hex_decode("AA0720D80EE2BB"));
    assert!(prop(&ha, "power").is_none());
    thinq.emit_data(&hex_decode("AA2120EB0000FF00000000000000000000000000000000000000000000000000BB"));
    assert_eq!(prop(&ha, "status").as_deref(), Some("unknown"));
}
