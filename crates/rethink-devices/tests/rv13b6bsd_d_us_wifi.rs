use rethink_core::{Thinq2Device, MockHaConnection, MockThinq2Device, Metadata};
use rethink_devices::devices::rv13b6bsd_d_us_wifi::Device;

const DEVICE_ID: &str = "test-id";
fn meta() -> Metadata { Metadata::new("RV13B6BSD_D_US_WIFI", "RV13B6BSD_D_US_WIFI", "1.0") }
fn make() -> (std::sync::Arc<MockHaConnection>, std::sync::Arc<MockThinq2Device>, std::sync::Arc<Device>) {
    let ha = MockHaConnection::new();
    let thinq = MockThinq2Device::new(DEVICE_ID, meta());
    let dev = Device::new(ha.clone(), thinq.clone(), meta());
    (ha, thinq, dev)
}
fn prop(ha: &MockHaConnection, n: &str) -> Option<String> {
    ha.device(DEVICE_ID)?.properties.get(n).map(|p| p.as_string())
}

fn build_eb(phase: u8, course: u8, dry: u8, temp: u8, flags: u8, opt2: u8, rh: u8, rm: u8, ih: u8, im: u8) -> Vec<u8> {
    // single status len 31 = 3 header + 28 rec
    let mut rec = vec![0u8; 28];
    rec[0] = 0x1b;
    rec[1] = phase;
    rec[2] = rh; rec[3] = rm;
    rec[4] = ih; rec[5] = im;
    rec[6] = course;
    rec[8] = dry;
    rec[9] = temp;
    rec[15] = flags;
    rec[16] = opt2;
    let mut inner = vec![0x30, 0xeb, 0x00];
    inner.extend(rec);
    assert_eq!(inner.len(), 31);
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
    assert!(comps.contains_key("turbo_steam"));
    assert!(comps.contains_key("wrinkle_care"));

    thinq.emit_data(&build_eb(0, 0, 0, 0, 0, 0, 0, 0, 0, 0));
    assert_eq!(prop(&ha, "power").as_deref(), Some("OFF"));
    assert_eq!(prop(&ha, "status").as_deref(), Some("Off"));
    assert_eq!(prop(&ha, "remaining_time").as_deref(), Some("0"));

    thinq.emit_data(&build_eb(0x32, 0x01, 3, 5, 0x01 | 0x02 | 0x08, 0x02 | 0x04 | 0x10, 0, 40, 1, 0));
    assert_eq!(prop(&ha, "power").as_deref(), Some("ON"));
    assert_eq!(prop(&ha, "status").as_deref(), Some("Drying"));
    assert_eq!(prop(&ha, "course").as_deref(), Some("Heavy Duty"));
    assert_eq!(prop(&ha, "remaining_time").as_deref(), Some("40"));
    assert_eq!(prop(&ha, "initial_time").as_deref(), Some("60"));
    assert_eq!(prop(&ha, "dry_level").as_deref(), Some("Normal"));
    assert_eq!(prop(&ha, "temp").as_deref(), Some("High"));
    assert_eq!(prop(&ha, "child_lock").as_deref(), Some("ON"));
    assert_eq!(prop(&ha, "reduce_static").as_deref(), Some("ON"));
    assert_eq!(prop(&ha, "damp_dry_signal").as_deref(), Some("ON"));
    assert_eq!(prop(&ha, "energy_saver").as_deref(), Some("ON"));
    assert_eq!(prop(&ha, "turbo_steam").as_deref(), Some("ON"));
    assert_eq!(prop(&ha, "wrinkle_care").as_deref(), Some("ON"));
}
