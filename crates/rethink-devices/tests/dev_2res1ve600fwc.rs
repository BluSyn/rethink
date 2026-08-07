use rethink_core::{Thinq2Device, hex_decode, hex_encode, MockHaConnection, MockThinq2Device, Metadata};
use rethink_devices::device_trait::DeviceHandler;
use rethink_devices::devices::dev_2res1ve600fwc::Device;

const DEVICE_ID: &str = "test-id";

fn meta() -> Metadata {
    Metadata::new("2RES1VE600FWC", "2RES1VE600FWC", "1.0")
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
fn door_and_temps() {
    let (ha, thinq, _) = make();
    assert!(ha.device(DEVICE_ID).is_none());

    thinq.emit_data(&hex_decode("AA1E10EC02030501FFFFFF0001FF01FF02030501FFFFFF0101FF01FF80BB"));
    let devinfo = ha.device(DEVICE_ID).unwrap();
    let comps = &devinfo.config.as_ref().unwrap().components;
    assert_eq!(comps["fridge_setpoint"]["unit_of_measurement"], "°C");
    assert_eq!(prop(&ha, "fridge_setpoint").as_deref(), Some("5"));
    assert_eq!(prop(&ha, "freezer_setpoint").as_deref(), Some("-18"));
    assert_eq!(prop(&ha, "door").as_deref(), Some("ON"));

    thinq.emit_data(&hex_decode("AA1E10EC02030501FFFFFF0101FF01FF02030501FFFFFF0001FF01FF80BB"));
    assert_eq!(prop(&ha, "door").as_deref(), Some("OFF"));

    thinq.emit_data(&hex_decode("AA1E10EC02030501FFFFFF0001FF01FF02040501FFFFFF0001FF01FF80BB"));
    assert_eq!(prop(&ha, "fridge_setpoint").as_deref(), Some("4"));

    thinq.emit_data(&hex_decode("AA1E10EC02030501FFFFFF0001FF01FF02030601FFFFFF0001FF01FF80BB"));
    assert_eq!(prop(&ha, "freezer_setpoint").as_deref(), Some("-19"));

    thinq.emit_data(&hex_decode("AA1E10EC02030501FFFFFF0001FF01FF02030502FFFFFF0001FF01FF80BB"));
    assert_eq!(prop(&ha, "express_freeze").as_deref(), Some("ON"));
    thinq.emit_data(&hex_decode("AA1E10EC02030502FFFFFF0001FF01FF02030501FFFFFF0001FF01FF80BB"));
    assert_eq!(prop(&ha, "express_freeze").as_deref(), Some("OFF"));
}

#[test]
fn writes() {
    let (_, thinq, dev) = make();
    thinq.emit_data(&hex_decode("AA1E10EC02030501FFFFFF0001FF01FF02030501FFFFFF0101FF01FF80BB"));
    thinq.reset_recorder();
    dev.start();
    assert_eq!(hex_encode(&thinq.outbox()[0]), "AA0EF0ED1211010000010400EBBB");

    thinq.reset_recorder();
    dev.set_property("fridge_setpoint", "5");
    let pkt = thinq.outbox()[0].clone();
    assert_eq!(pkt[4 + 1], 3);
    assert_eq!(pkt[4 + 8], 1);

    thinq.reset_recorder();
    dev.set_property("freezer_setpoint", "-19");
    assert_eq!(thinq.outbox()[0][4 + 2], 6);

    thinq.reset_recorder();
    dev.set_property("express_freeze", "ON");
    assert_eq!(thinq.outbox()[0][4 + 3], 0x02);
    thinq.reset_recorder();
    dev.set_property("express_freeze", "OFF");
    assert_eq!(thinq.outbox()[0][4 + 3], 0x01);

    thinq.reset_recorder();
    dev.set_property("nonsense", "x");
    assert_eq!(thinq.outbox().len(), 0);
}
