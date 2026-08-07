use rethink_core::{Thinq2Device, hex_decode, hex_encode, MockHaConnection, MockThinq2Device, Metadata};
use rethink_devices::device_trait::DeviceHandler;
use rethink_devices::devices::f_v__f___w_b_1qeuk::Device;

const DEVICE_ID: &str = "test-id";
fn meta() -> Metadata { Metadata::new("F_V__F___W.B_1QEUK", "F_V__F___W.B_1QEUK", "2.10.123") }
fn make() -> (std::sync::Arc<MockHaConnection>, std::sync::Arc<MockThinq2Device>, std::sync::Arc<Device>) {
    let ha = MockHaConnection::new();
    let thinq = MockThinq2Device::new(DEVICE_ID, meta());
    let dev = Device::new(ha.clone(), thinq.clone(), meta());
    (ha, thinq, dev)
}
fn prop(ha: &MockHaConnection, n: &str) -> Option<String> {
    ha.device(DEVICE_ID)?.properties.get(n).map(|p| p.as_string())
}

const SAMPLE_INITIAL: &str = "AA5420EC002500000000000000000000000000000000000000010036006400000000000000000000000000002501000000000000000000000000000000000000000036006400000000000000000000000000DFBB";
const SAMPLE_WASH: &str = "AA5420EC00250101000100180000000000020000000000000600001000640000000000000000000000000000250104380438130003090401020000000000000400001000640000040000000000000000000053BB";
const SAMPLE_RUNNING: &str = "AA5420EC002506001300140C00030204010000000142200001010036006400000100000E00000000000000002506001300140C00030204010000000142200001010036006400000100000F00000000000000A2BB";

#[test]
fn config_and_decode() {
    let (ha, thinq, dev) = make();
    let devinfo = ha.device(DEVICE_ID).unwrap();
    let comps = &devinfo.config.as_ref().unwrap().components;
    for c in ["power","start","pause","status","drying_mode","energy","remaining_time"] {
        assert!(comps.contains_key(c));
    }
    thinq.emit_data(&hex_decode(SAMPLE_INITIAL));
    assert_eq!(prop(&ha, "power").as_deref(), Some("ON"));
    assert_eq!(prop(&ha, "status").as_deref(), Some("Ready"));
    assert_eq!(prop(&ha, "error").as_deref(), Some("OFF"));
    assert_eq!(prop(&ha, "error_message").as_deref(), Some("OK"));
    assert_eq!(prop(&ha, "drying_mode").as_deref(), Some("Off"));
    assert_eq!(prop(&ha, "cycles").as_deref(), Some("54"));
    assert_eq!(prop(&ha, "remote_start").as_deref(), Some("OFF"));
    assert_eq!(prop(&ha, "door_lock").as_deref(), Some("ON"));

    thinq.emit_data(&hex_decode(SAMPLE_WASH));
    assert_eq!(prop(&ha, "course").as_deref(), Some("Wash + Dry"));
    assert_eq!(prop(&ha, "spin").as_deref(), Some("1200"));
    assert_eq!(prop(&ha, "temp").as_deref(), Some("40"));
    assert_eq!(prop(&ha, "drying_mode").as_deref(), Some("Auto"));
    assert_eq!(prop(&ha, "remaining_time").as_deref(), Some("296")); // 4*60+56

    thinq.emit_data(&hex_decode(SAMPLE_RUNNING));
    assert_eq!(prop(&ha, "status").as_deref(), Some("Washing"));

    thinq.reset_recorder();
    dev.start();
    assert_eq!(hex_encode(&thinq.outbox()[0]), "AA0EF0ED1121010000001800B5BB");
    thinq.reset_recorder();
    dev.set_property("power", "ON");
    assert_eq!(hex_encode(&thinq.outbox()[0]), "AA08F02A010098BB");
    thinq.reset_recorder();
    dev.set_property("power", "OFF");
    assert_eq!(hex_encode(&thinq.outbox()[0]), "AA09F0240101009CBB");
    thinq.reset_recorder();
    dev.set_property("pause", "");
    assert_eq!(hex_encode(&thinq.outbox()[0]), "AA09F02404010099BB");
    thinq.reset_recorder();
    dev.set_property("start", "");
    assert_eq!(hex_encode(&thinq.outbox()[0]), "AA09F02405010098BB");
}
