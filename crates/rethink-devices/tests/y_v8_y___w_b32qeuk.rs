use rethink_core::{Thinq2Device, hex_decode, hex_encode, MockHaConnection, MockThinq2Device, Metadata};
use rethink_devices::device_trait::DeviceHandler;
use rethink_devices::devices::y_v8_y___w_b32qeuk::Device;

const DEVICE_ID: &str = "test-id";
fn meta() -> Metadata { Metadata::new("Y_V8_Y___W.B32QEUK", "Y_V8_Y___W.B32QEUK", "2.11.207") }
fn make() -> (std::sync::Arc<MockHaConnection>, std::sync::Arc<MockThinq2Device>, std::sync::Arc<Device>) {
    let ha = MockHaConnection::new();
    let thinq = MockThinq2Device::new(DEVICE_ID, meta());
    let dev = Device::new(ha.clone(), thinq.clone(), meta());
    (ha, thinq, dev)
}
fn prop(ha: &MockHaConnection, n: &str) -> Option<String> {
    ha.device(DEVICE_ID)?.properties.get(n).map(|p| p.as_string())
}

const SAMPLE_INITIAL: &str = "AAFF200A0039000381000100EB0027000001032603260000000000000000000000000003000011007100000000000000000000000000974EBB";
const SAMPLE_RUNNING: &str = "AAFF200A0039000398000100EB0027000006000E000E0C0003020201000000014220000101001100710000010000000000000000000056D9BB";

#[test]
fn decode_and_writes() {
    let (ha, thinq, dev) = make();
    thinq.emit_data(&hex_decode(SAMPLE_INITIAL));
    assert_eq!(prop(&ha, "power").as_deref(), Some("ON"));
    assert_eq!(prop(&ha, "status").as_deref(), Some("Ready"));
    assert_eq!(prop(&ha, "remaining_time").as_deref(), Some("218")); // 3*60+38
    assert_eq!(prop(&ha, "cycles").as_deref(), Some("17"));
    assert_eq!(prop(&ha, "door_lock").as_deref(), Some("ON"));

    thinq.emit_data(&hex_decode(SAMPLE_RUNNING));
    assert_eq!(prop(&ha, "status").as_deref(), Some("Washing"));
    assert_eq!(prop(&ha, "course").as_deref(), Some("Quick 14"));
    assert_eq!(prop(&ha, "spin").as_deref(), Some("400"));
    assert_eq!(prop(&ha, "temp").as_deref(), Some("10"));

    thinq.reset_recorder();
    dev.start();
    assert_eq!(hex_encode(&thinq.outbox()[0]), "AA0EF0ED1121010000001800B5BB");
    thinq.reset_recorder();
    dev.set_property("power", "ON");
    assert_eq!(hex_encode(&thinq.outbox()[0]), "AA08F02A010098BB");
}
