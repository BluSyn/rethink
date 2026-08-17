use rethink_core::{hex_decode, MockHaConnection, MockThinq2Device, Metadata, Thinq2Device};
use rethink_devices::devices::t17a1efhu_f::Device;

const DEVICE_ID: &str = "test-id";

fn meta() -> Metadata {
    Metadata::new("T17A1EFHU_F", "LG WT7305CV", "1.0")
}
fn make() -> (
    std::sync::Arc<MockHaConnection>,
    std::sync::Arc<MockThinq2Device>,
    std::sync::Arc<Device>,
) {
    let ha = MockHaConnection::new();
    let thinq = MockThinq2Device::new(DEVICE_ID, meta());
    let dev = Device::new(ha.clone(), thinq.clone(), meta());
    (ha, thinq, dev)
}
fn prop(ha: &MockHaConnection, n: &str) -> Option<String> {
    ha.device(DEVICE_ID)?.properties.get(n).map(|p| p.as_string())
}

/// Build 0x20/0xDE status: header + 27-byte record (phase at rec[2], mins at rec[4]).
fn build_de(phase: u8, mins: u8) -> Vec<u8> {
    let mut rec = vec![0u8; 27];
    rec[2] = phase;
    rec[4] = mins;
    let mut inner = vec![0x20, 0xde];
    inner.extend(rec);
    assert!(inner.len() >= 29);
    let mut pkt = vec![0xaa, (inner.len() + 4) as u8];
    pkt.extend(inner);
    pkt.push(0);
    pkt.push(0xbb);
    pkt
}

#[test]
fn config_components() {
    let (ha, _, _) = make();
    let devinfo = ha.device(DEVICE_ID).unwrap();
    let comps = &devinfo.config.as_ref().unwrap().components;
    assert!(comps.contains_key("power"));
    assert!(comps.contains_key("status"));
    assert!(comps.contains_key("remaining_time"));
}

#[test]
fn de_status_frames() {
    let (ha, thinq, _) = make();
    thinq.emit_data(&build_de(0x00, 0));
    assert_eq!(prop(&ha, "power").as_deref(), Some("OFF"));
    assert_eq!(prop(&ha, "status").as_deref(), Some("Off"));
    assert_eq!(prop(&ha, "remaining_time").as_deref(), Some("0"));

    thinq.emit_data(&build_de(0x05, 30));
    assert_eq!(prop(&ha, "power").as_deref(), Some("ON"));
    assert_eq!(prop(&ha, "status").as_deref(), Some("Wash"));
    assert_eq!(prop(&ha, "remaining_time").as_deref(), Some("30"));

    thinq.emit_data(&build_de(0x08, 5));
    assert_eq!(prop(&ha, "power").as_deref(), Some("OFF"));
    assert_eq!(prop(&ha, "status").as_deref(), Some("End"));
    assert_eq!(prop(&ha, "remaining_time").as_deref(), Some("0"));
}

#[test]
fn ignores_eb_ec_like_t1789() {
    let (ha, thinq, _) = make();
    // T1789-style 0xEB must not be decoded by this model
    thinq.emit_data(&hex_decode(
        "AA2120EB000005001E0000000000000000000000000000000000000000000000BB",
    ));
    assert!(prop(&ha, "power").is_none());
}

#[test]
fn registry_resolves() {
    assert!(rethink_devices::registry::t2_factory("T17A1EFHU_F").is_some());
}
