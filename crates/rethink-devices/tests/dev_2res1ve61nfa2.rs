use rethink_core::{Thinq2Device, hex_decode, hex_encode, MockHaConnection, MockThinq2Device, Metadata};
use rethink_devices::device_trait::DeviceHandler;
use rethink_devices::devices::dev_2res1ve61nfa2::Device;

const DEVICE_ID: &str = "test-id";
const SAMPLE_INITIAL: &str = "AA2110EB0202040107000000010001FFFFFF00FF0001FFFFFFFFFFFFFF020085BB";
const SAMPLE_DELTA: &str =
    "AA3C10EC0201040102000001010001FFFFFF00FF0100FFFFFFFFFFFFFF02060202040102000001010001FFFFFF00FF0100FFFFFFFFFFFFFF0206ACBB";
const SAMPLE_QUIESCENT: &str =
    "AA3C10EC0202040102000000010001FFFFFF00FF0000FFFFFFFFFFFFFF02010202040102000000010001FFFFFF00FF0000FFFFFFFFFFFFFF0202B8BB";

fn meta() -> Metadata {
    Metadata::new("2RES1VE61NFA2", "2RES1VE61NFA2", "1.0")
}

fn make() -> (std::sync::Arc<MockHaConnection>, std::sync::Arc<MockThinq2Device>, std::sync::Arc<Device>) {
    let ha = MockHaConnection::new();
    let thinq = MockThinq2Device::new(DEVICE_ID, meta());
    let dev = Device::new(ha.clone(), thinq.clone(), meta());
    (ha, thinq, dev)
}

fn prop(ha: &MockHaConnection, name: &str) -> Option<String> {
    ha.device(DEVICE_ID)?.properties.get(name).map(|p| p.as_string())
}

#[test]
fn config_not_until_status() {
    let (ha, _, _) = make();
    assert!(ha.device(DEVICE_ID).is_none());
}

#[test]
fn initial_status() {
    let (ha, thinq, _) = make();
    thinq.emit_data(&hex_decode(SAMPLE_INITIAL));
    let devinfo = ha.device(DEVICE_ID).unwrap();
    let comps = &devinfo.config.as_ref().unwrap().components;
    assert_eq!(comps["fridge_setpoint"]["unit_of_measurement"], "°C");
    assert!(comps.contains_key("express_cool"));
    assert_eq!(prop(&ha, "fridge_setpoint").as_deref(), Some("6"));
    assert_eq!(prop(&ha, "freezer_setpoint").as_deref(), Some("-18"));
    assert_eq!(prop(&ha, "door").as_deref(), Some("OFF"));
}

#[test]
fn delta_door_express() {
    let (ha, thinq, _) = make();
    thinq.emit_data(&hex_decode(SAMPLE_DELTA));
    assert_eq!(prop(&ha, "door").as_deref(), Some("ON"));
    assert_eq!(prop(&ha, "express_cool").as_deref(), Some("ON"));
    thinq.emit_data(&hex_decode(SAMPLE_QUIESCENT));
    assert_eq!(prop(&ha, "door").as_deref(), Some("OFF"));
    assert_eq!(prop(&ha, "express_cool").as_deref(), Some("OFF"));
}

#[test]
fn start_and_writes() {
    let (_, thinq, dev) = make();
    thinq.reset_recorder();
    dev.start();
    assert_eq!(hex_encode(&thinq.outbox()[0]), "AA0EF0ED1211010000010400EBBB");

    thinq.emit_data(&hex_decode(SAMPLE_INITIAL));
    thinq.reset_recorder();
    dev.set_property("fridge_setpoint", "4");
    let pkt = thinq.outbox()[0].clone();
    assert_eq!(pkt[4 + 1], 4);
    assert_eq!(pkt[4 + 8], 1);
    assert_eq!(pkt[4 + 0], 0xff);

    thinq.reset_recorder();
    dev.set_property("freezer_setpoint", "-20");
    assert_eq!(thinq.outbox()[0][4 + 2], 6);

    thinq.reset_recorder();
    dev.set_property("express_cool", "ON");
    assert_eq!(thinq.outbox()[0][4 + 16], 1);

    thinq.reset_recorder();
    dev.set_property("express_freeze", "ON");
    assert_eq!(thinq.outbox()[0][4 + 3], 2);

    thinq.reset_recorder();
    dev.set_property("does-not-exist", "1");
    assert_eq!(thinq.outbox().len(), 0);
}
