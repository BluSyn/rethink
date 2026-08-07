use rethink_core::{Thinq2Device, hex_decode, hex_encode, MockHaConnection, MockThinq2Device, Metadata, PropertyValue};
use rethink_devices::device_trait::DeviceHandler;
use rethink_devices::devices::dev_2reb1glvb1__2::Device;

const DEVICE_ID: &str = "test-id";
const SAMPLE_INITIAL: &str = "AA1710EB020504010000000201000100000000000099BB";

fn meta() -> Metadata {
    Metadata::new("2REB1GLVB1__2", "2REB1GLVB1__2", "1.0")
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
fn config_not_published_until_status() {
    let (ha, _, _) = make();
    assert!(ha.device(DEVICE_ID).is_none());
}

#[test]
fn initial_status_celsius() {
    let (ha, thinq, _) = make();
    thinq.emit_data(&hex_decode(SAMPLE_INITIAL));
    let dev = ha.device(DEVICE_ID).unwrap();
    let comps = &dev.config.as_ref().unwrap().components;
    assert_eq!(comps["fridge_setpoint"]["unit_of_measurement"], "°C");
    assert_eq!(comps["fridge_setpoint"]["min"], 1);
    assert_eq!(comps["fridge_setpoint"]["max"], 7);
    assert_eq!(comps["freezer_setpoint"]["min"], -23);
    assert_eq!(prop(&ha, "fridge_setpoint").as_deref(), Some("3"));
    assert_eq!(prop(&ha, "freezer_setpoint").as_deref(), Some("-18"));
    assert_eq!(prop(&ha, "door").as_deref(), Some("OFF"));
    assert_eq!(prop(&ha, "express_cool").as_deref(), Some("OFF"));
    assert_eq!(prop(&ha, "express_freeze").as_deref(), Some("OFF"));
}

#[test]
fn ignores_bad_frames() {
    let (ha, thinq, _) = make();
    thinq.emit_data(&hex_decode("001122"));
    assert!(ha.device(DEVICE_ID).is_none());
    thinq.emit_data(&hex_decode("AA08109901020304BB"));
    assert!(ha.device(DEVICE_ID).is_none());
}

#[test]
fn start_sends_query() {
    let (_, thinq, dev) = make();
    thinq.reset_recorder();
    dev.start();
    assert_eq!(thinq.outbox().len(), 1);
    assert_eq!(hex_encode(&thinq.outbox()[0]), "AA0EF0ED1211010000010400EBBB");
}

#[test]
fn ha_writes() {
    let (_, thinq, dev) = make();
    thinq.emit_data(&hex_decode(SAMPLE_INITIAL));
    thinq.reset_recorder();

    dev.set_property("fridge_setpoint", "4");
    let pkt = thinq.outbox()[0].clone();
    assert_eq!(pkt[2], 0xf0);
    assert_eq!(pkt[3], 0x17);
    assert_eq!(pkt[4 + 1], 4);
    assert_eq!(pkt[4 + 8], 1);
    assert_eq!(pkt[4 + 2], 0xff);

    thinq.reset_recorder();
    dev.set_property("freezer_setpoint", "-20");
    let pkt = thinq.outbox()[0].clone();
    assert_eq!(pkt[4 + 2], 6);
    assert_eq!(pkt[4 + 1], 0xff);

    thinq.reset_recorder();
    dev.set_property("express_cool", "ON");
    assert_eq!(thinq.outbox()[0][4 + 16], 1);

    thinq.reset_recorder();
    dev.set_property("express_freeze", "ON");
    assert_eq!(thinq.outbox()[0][4 + 3], 2);

    thinq.reset_recorder();
    dev.set_property("nonsense", "whatever");
    assert_eq!(thinq.outbox().len(), 0);
}

// silence unused
#[allow(dead_code)]
fn _pv() -> PropertyValue {
    PropertyValue::Int(0)
}
