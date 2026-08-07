use rethink_core::{Thinq2Device, hex_decode, MockHaConnection, MockThinq2Device, Metadata};
use rethink_devices::devices::y_v8_f___w_b_2qeuk::Device;

const DEVICE_ID: &str = "test-id";
fn meta() -> Metadata { Metadata::new("Y_V8_F___W.B_2QEUK", "Y_V8_F___W.B_2QEUK", "1.0") }
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
fn config_present() {
    let (ha, _, _) = make();
    let devinfo = ha.device(DEVICE_ID).unwrap();
    let comps = &devinfo.config.as_ref().unwrap().components;
    assert!(comps.contains_key("drying_mode"));
    assert!(comps.contains_key("status"));
}

// Minimal synthetic frame: 20, ..., buf[8]=0x01, length > 15+19, status at S=15
#[test]
fn synthetic_short_status() {
    let (ha, thinq, _) = make();
    // Build 53-byte-like body with type marker: length enough for S=15
    // buf[0]=0x20, buf[8]=0x01, status at 15 = 1 (Ready)
    let mut inner = vec![0u8; 40];
    inner[0] = 0x20;
    inner[8] = 0x01;
    inner[15] = 1; // Ready
    inner[16] = 0; inner[17] = 10; // remaining 10
    inner[18] = 0; inner[19] = 30; // initial 30
    // wrap AA BB
    let mut pkt = vec![0xaa, (inner.len()+4) as u8];
    pkt.extend(&inner);
    pkt.push(0); pkt.push(0xbb);
    thinq.emit_data(&pkt);
    assert_eq!(prop(&ha, "power").as_deref(), Some("ON"));
    assert_eq!(prop(&ha, "status").as_deref(), Some("Ready"));
    assert_eq!(prop(&ha, "remaining_time").as_deref(), Some("10"));
}
