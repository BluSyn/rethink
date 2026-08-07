use rethink_core::{Thinq2Device, hex_decode, MockHaConnection, MockThinq2Device, Metadata};
use rethink_devices::device_trait::DeviceHandler;
use rethink_devices::devices::f_vb_f___w_b_2qeuk::Device;

const DEVICE_ID: &str = "test-id";
fn meta() -> Metadata { Metadata::new("F_VB_F___W.B_2QEUK", "F_VB_F___W.B_2QEUK", "1.0") }
fn make() -> (std::sync::Arc<MockHaConnection>, std::sync::Arc<MockThinq2Device>, std::sync::Arc<Device>) {
    let ha = MockHaConnection::new();
    let thinq = MockThinq2Device::new(DEVICE_ID, meta());
    let dev = Device::new(ha.clone(), thinq.clone(), meta());
    (ha, thinq, dev)
}
fn prop(ha: &MockHaConnection, n: &str) -> Option<String> {
    ha.device(DEVICE_ID)?.properties.get(n).map(|p| p.as_string())
}

const SAMPLE: &str = "AA5420EC00250101000100180000000000020000000000000600001000640000000000000000000000000000250104380438130003090401020000000000000400001000640000040000000000000000000053BB";

#[test]
fn basic() {
    let (ha, thinq, _) = make();
    let devinfo = ha.device(DEVICE_ID).unwrap();
    let comps = &devinfo.config.as_ref().unwrap().components;
    assert!(comps.contains_key("detergent"));
    assert!(comps.contains_key("eco_hybrid"));
    thinq.emit_data(&hex_decode(SAMPLE));
    assert_eq!(prop(&ha, "power").as_deref(), Some("ON"));
    assert_eq!(prop(&ha, "drying_mode").as_deref(), Some("Auto"));
}
