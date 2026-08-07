use rethink_core::{Thinq2Device, MockHaConnection, MockThinq2Device, Metadata};
use rethink_devices::devices::vcdwl2qeuk::Device;

const DEVICE_ID: &str = "test-id";
fn meta() -> Metadata { Metadata::new("VCDWL2QEUK", "VCDWL2QEUK", "1.0") }
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
fn config_and_config_frame_power() {
    let (ha, thinq, _) = make();
    let devinfo = ha.device(DEVICE_ID).unwrap();
    let comps = &devinfo.config.as_ref().unwrap().components;
    assert!(comps.contains_key("tub_clean_count"));
    assert!(comps.contains_key("detergent_dose"));

    // config frame: buf[3]=0x88
    let mut inner = vec![0u8; 20];
    inner[0] = 0x20;
    inner[3] = 0x88;
    let mut pkt = vec![0xaa, (inner.len()+4) as u8];
    pkt.extend(inner);
    pkt.push(0); pkt.push(0xbb);
    thinq.emit_data(&pkt);
    assert_eq!(prop(&ha, "power").as_deref(), Some("ON"));
}

#[test]
fn status_standby_and_running() {
    let (ha, thinq, _) = make();
    // Build 142-byte status frame
    let mut inner = vec![0u8; 142];
    inner[0] = 0x20;
    inner[3] = 0x92;
    // record B at 78: all zero -> standby OFF
    let mut pkt = vec![0xaa, 0]; // fix len later
    pkt.extend(&inner);
    pkt.push(0); pkt.push(0xbb);
    pkt[1] = (pkt.len()) as u8; // not exact but envelope doesn't check
    // Actually process_data_envelope doesn't use length byte. OK.
    thinq.emit_data(&pkt);
    assert_eq!(prop(&ha, "power").as_deref(), Some("OFF"));
    assert_eq!(prop(&ha, "status").as_deref(), Some("Off"));

    // running: soil=3, temp idx=2 (30), spin idx=4 (800), course=0x2e Cotton, status=0x0b washing
    let mut inner = vec![0u8; 142];
    inner[0] = 0x20;
    inner[3] = 0x92;
    let off = 78;
    inner[off] = 3; // soil medium
    inner[off+1] = 2; // temp 30
    inner[off+3] = 4; // spin 800
    inner[off+4] = 0x2e; // cotton
    inner[off+13] = 42; // remaining
    inner[off+15] = 60; // initial
    inner[off+16] = 0x01; inner[off+17] = 0x00; // energy 256
    inner[off+20] = 0x0b; // washing
    inner[off+26] = 1; // rinse normal
    inner[off+29] = 0x02; // detergent on
    inner[off+30] = 0x02; // softener on
    inner[off+31] = 45;
    inner[off+32] = 30;
    inner[off+33] = 0x40 | 0x20; // prewash+turbo
    inner[off+34] = 0x10; // steam
    inner[off+36] = 0x20 | 0x10; // child+remote
    inner[off+27] = 7; // tub clean count
    let mut pkt = vec![0xaa, 0];
    pkt.extend(&inner);
    pkt.push(0); pkt.push(0xbb);
    thinq.emit_data(&pkt);
    assert_eq!(prop(&ha, "power").as_deref(), Some("ON"));
    assert_eq!(prop(&ha, "status").as_deref(), Some("Washing"));
    assert_eq!(prop(&ha, "soil").as_deref(), Some("Medium"));
    assert_eq!(prop(&ha, "temp").as_deref(), Some("30"));
    assert_eq!(prop(&ha, "spin").as_deref(), Some("800"));
    assert_eq!(prop(&ha, "course").as_deref(), Some("Cotton"));
    assert_eq!(prop(&ha, "remaining_time").as_deref(), Some("42"));
    assert_eq!(prop(&ha, "initial_time").as_deref(), Some("60"));
    assert_eq!(prop(&ha, "energy").as_deref(), Some("256"));
    assert_eq!(prop(&ha, "rinse").as_deref(), Some("Normal"));
    assert_eq!(prop(&ha, "detergent_dispenser").as_deref(), Some("ON"));
    assert_eq!(prop(&ha, "softener_dose").as_deref(), Some("30"));
    assert_eq!(prop(&ha, "prewash").as_deref(), Some("ON"));
    assert_eq!(prop(&ha, "turbowash").as_deref(), Some("ON"));
    assert_eq!(prop(&ha, "steam").as_deref(), Some("ON"));
    assert_eq!(prop(&ha, "child_lock").as_deref(), Some("ON"));
    assert_eq!(prop(&ha, "remote_start").as_deref(), Some("ON"));
    assert_eq!(prop(&ha, "tub_clean_count").as_deref(), Some("7"));
}

#[test]
fn door_frame() {
    let (ha, thinq, _) = make();
    let mut inner = vec![0u8; 30];
    inner[0] = 0x20;
    inner[3] = 0x41;
    inner[18] = 0x01; // open
    let mut pkt = vec![0xaa, 0];
    pkt.extend(&inner); pkt.push(0); pkt.push(0xbb);
    thinq.emit_data(&pkt);
    assert_eq!(prop(&ha, "door").as_deref(), Some("ON"));
    inner[18] = 0x02;
    let mut pkt = vec![0xaa, 0];
    pkt.extend(&inner); pkt.push(0); pkt.push(0xbb);
    thinq.emit_data(&pkt);
    assert_eq!(prop(&ha, "door").as_deref(), Some("OFF"));
}
