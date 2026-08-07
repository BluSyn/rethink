use rethink_core::{Thinq2Device, hex_decode, hex_encode, MockHaConnection, MockThinq2Device, Metadata};
use rethink_devices::device_trait::DeviceHandler;
use rethink_devices::devices::f_v8_y___w_b_2qeuk::Device;

const DEVICE_ID: &str = "test-id";
fn meta() -> Metadata { Metadata::new("F_V8_Y___W.B_2QEUK", "F_V8_Y___W.B_2QEUK", "1.0") }
fn make() -> (std::sync::Arc<MockHaConnection>, std::sync::Arc<MockThinq2Device>, std::sync::Arc<Device>) {
    let ha = MockHaConnection::new();
    let thinq = MockThinq2Device::new(DEVICE_ID, meta());
    let dev = Device::new(ha.clone(), thinq.clone(), meta());
    (ha, thinq, dev)
}
fn prop(ha: &MockHaConnection, n: &str) -> Option<String> {
    ha.device(DEVICE_ID)?.properties.get(n).map(|p| p.as_string())
}

// Reuse 80-byte frame layout from F_V__F for basic decode of shared offsets
const SAMPLE: &str = "AA5420EC00250101000100180000000000020000000000000600001000640000000000000000000000000000250104380438130003090401020000000000000400001000640000040000000000000000000053BB";

#[test]
fn basic() {
    let (ha, thinq, dev) = make();
    assert!(ha.device(DEVICE_ID).unwrap().config.is_some());
    thinq.emit_data(&hex_decode(SAMPLE));
    assert_eq!(prop(&ha, "power").as_deref(), Some("ON"));
    assert_eq!(prop(&ha, "status").as_deref(), Some("Ready"));
    assert_eq!(prop(&ha, "temp").as_deref(), Some("40"));
    thinq.reset_recorder();
    dev.start();
    assert_eq!(hex_encode(&thinq.outbox()[0]), "AA0EF0ED1121010000001800B5BB");
}
