use rethink_core::{Thinq2Device, hex_decode, MockHaConnection, MockThinq2Device, Metadata};
use rethink_devices::devices::rv13u6am8w_d_us_wifi::Device;

const DEVICE_ID: &str = "test-id";
fn meta() -> Metadata { Metadata::new("RV13U6AM8W_D_US_WIFI", "LG DLE7300WE", "1.0") }
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
fn config_and_status() {
    let (ha, thinq, _) = make();
    let devinfo = ha.device(DEVICE_ID).unwrap();
    let comps = &devinfo.config.as_ref().unwrap().components;
    for c in ["status","remaining_time","power","drum_running","cycle","temp","dry_level"] {
        assert!(comps.contains_key(c), "missing {c}");
    }
    thinq.emit_data(&hex_decode("AA2330EB000000000000000000000000000000000000000000000000000000000000BB"));
    assert_eq!(prop(&ha, "power").as_deref(), Some("OFF"));
    assert_eq!(prop(&ha, "status").as_deref(), Some("Off"));

    thinq.emit_data(&hex_decode("AA2330EB000032002D00000000000000000000000000000000000000000000000000BB"));
    assert_eq!(prop(&ha, "status").as_deref(), Some("Drying"));
    assert_eq!(prop(&ha, "remaining_time").as_deref(), Some("45"));

    thinq.emit_data(&hex_decode(
        "AA4030EC001B320036003601000305000100000000A90000000100000064000000001B320035003601000305000100000000A90000530100000064000000AFBB"
    ));
    assert_eq!(prop(&ha, "status").as_deref(), Some("Drying"));
    assert_eq!(prop(&ha, "remaining_time").as_deref(), Some("54"));
    assert_eq!(prop(&ha, "cycle").as_deref(), Some("Heavy Duty"));
    assert_eq!(prop(&ha, "drum_running").as_deref(), Some("ON"));
}
