use rethink_core::{Thinq2Device, hex_decode, MockHaConnection, MockThinq2Device, Metadata};
use rethink_devices::devices::f3l2cyu__::Device;

const DEVICE_ID: &str = "test-id";
fn meta() -> Metadata { Metadata::new("F3L2CYU__", "F3L2CYU__", "1.0") }
fn make() -> (std::sync::Arc<MockHaConnection>, std::sync::Arc<MockThinq2Device>, std::sync::Arc<Device>) {
    let ha = MockHaConnection::new();
    let thinq = MockThinq2Device::new(DEVICE_ID, meta());
    let dev = Device::new(ha.clone(), thinq.clone(), meta());
    (ha, thinq, dev)
}
fn prop(ha: &MockHaConnection, n: &str) -> Option<String> {
    ha.device(DEVICE_ID)?.properties.get(n).map(|p| p.as_string())
}

fn build_eb(phase: u8, course: u8, soil: u8, spin: u8, temp: u8, flags: u8, opt2: u8, door: u8, h: u8, m: u8) -> Vec<u8> {
    // single status: inner len 28 = 3 header + 25 rec
    let mut rec = vec![0u8; 25];
    rec[0] = 0x18;
    rec[1] = phase;
    rec[2] = h; rec[3] = m;
    rec[6] = course;
    rec[8] = soil;
    rec[9] = spin;
    rec[10] = temp;
    rec[11] = 0x11; // rinse count high nibble 1
    rec[15] = flags;
    rec[16] = opt2;
    rec[17] = door;
    let mut inner = vec![0x20, 0xeb, 0x00];
    inner.extend(rec);
    assert_eq!(inner.len(), 28);
    let mut pkt = vec![0xaa, (inner.len()+4) as u8];
    pkt.extend(inner);
    pkt.push(0); pkt.push(0xbb);
    pkt
}

#[test]
fn config_and_status() {
    let (ha, thinq, _) = make();
    let devinfo = ha.device(DEVICE_ID).unwrap();
    let comps = &devinfo.config.as_ref().unwrap().components;
    assert!(comps.contains_key("turbo_wash"));
    assert!(comps.contains_key("door"));

    thinq.emit_data(&build_eb(0x00, 0, 0, 0, 0, 0, 0, 0x02, 0, 0));
    assert_eq!(prop(&ha, "power").as_deref(), Some("OFF"));
    assert_eq!(prop(&ha, "status").as_deref(), Some("Off"));
    assert_eq!(prop(&ha, "remaining_time").as_deref(), Some("0"));

    thinq.emit_data(&build_eb(0x17, 0x07, 3, 4, 4, 0x80 | 0x40, 0x80 | 0x10, 0x02, 0, 45));
    assert_eq!(prop(&ha, "power").as_deref(), Some("ON"));
    assert_eq!(prop(&ha, "status").as_deref(), Some("Washing"));
    assert_eq!(prop(&ha, "course").as_deref(), Some("Normal"));
    assert_eq!(prop(&ha, "remaining_time").as_deref(), Some("45"));
    assert_eq!(prop(&ha, "soil").as_deref(), Some("Normal"));
    assert_eq!(prop(&ha, "spin").as_deref(), Some("High"));
    assert_eq!(prop(&ha, "temp").as_deref(), Some("Warm"));
    assert_eq!(prop(&ha, "turbo_wash").as_deref(), Some("ON"));
    assert_eq!(prop(&ha, "extra_rinse").as_deref(), Some("ON"));
    assert_eq!(prop(&ha, "door_lock").as_deref(), Some("ON"));
    assert_eq!(prop(&ha, "cold_wash").as_deref(), Some("ON"));
    assert_eq!(prop(&ha, "door").as_deref(), Some("OFF"));
}
