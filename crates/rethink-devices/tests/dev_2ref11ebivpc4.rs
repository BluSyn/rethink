use rethink_core::{hex_decode, MockHaConnection, MockThinq2Device, Metadata};
use rethink_devices::device_trait::DeviceHandler;
use rethink_devices::devices::dev_2ref11ebivpc4::Device;

const DEVICE_ID: &str = "test-id";

fn status_baseline() -> String {
    format!("{}{}", "02070701FFFFFF00FFFFFFFFFFFF00", "FF".repeat(28))
}

fn meta() -> Metadata {
    Metadata::new("2REF11EBIVPC4", "2REF11EBIVPC4", "1.0")
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
fn config_immediate_celsius() {
    let (ha, _, _) = make();
    let comps = &ha.device(DEVICE_ID).unwrap().config.as_ref().unwrap().components;
    assert_eq!(comps["fridge_setpoint"]["unit_of_measurement"], "°C");
    assert_eq!(comps["fridge_setpoint"]["min"], 1);
    assert_eq!(comps["freezer_setpoint"]["min"], -23);
    assert!(comps.contains_key("express_freeze"));
    assert!(comps.contains_key("shabbat_mode"));
    assert!(!comps.contains_key("flex_setpoint"));
}

#[test]
fn decode_status() {
    let (ha, thinq, _) = make();
    let pkt = format!("AA3110EB{}00BB", status_baseline());
    thinq.emit_data(&hex_decode(&pkt));
    assert_eq!(prop(&ha, "fridge_setpoint").as_deref(), Some("3"));
    assert_eq!(prop(&ha, "freezer_setpoint").as_deref(), Some("-18"));
    assert_eq!(prop(&ha, "door").as_deref(), Some("OFF"));
    assert_eq!(prop(&ha, "express_freeze").as_deref(), Some("OFF"));
    assert_eq!(prop(&ha, "shabbat_mode").as_deref(), Some("OFF"));

    // door open delta
    let cur = format!(
        "{}{}",
        "02070701FFFFFF01FFFFFFFFFFFF00",
        "FF".repeat(28)
    );
    let pkt = format!("AA5C10EC{}{}00BB", status_baseline(), cur);
    thinq.emit_data(&hex_decode(&pkt));
    assert_eq!(prop(&ha, "door").as_deref(), Some("ON"));
}

#[test]
fn writes_and_start() {
    let (_, thinq, dev) = make();
    thinq.reset_recorder();
    dev.start();
    assert_eq!(thinq.outbox().len(), 0);

    thinq.reset_recorder();
    dev.set_property("fridge_setpoint", "5");
    let pkt = thinq.outbox()[0].clone();
    assert_eq!(pkt[2], 0xf0);
    assert_eq!(pkt[3], 0x17);
    assert_eq!(pkt[5], 3);
    assert_eq!(pkt[12], 1);

    thinq.reset_recorder();
    dev.set_property("freezer_setpoint", "-20");
    assert_eq!(thinq.outbox()[0][6], 6);
    assert_eq!(thinq.outbox()[0][12], 1);

    thinq.reset_recorder();
    dev.set_property("express_freeze", "ON");
    assert_eq!(thinq.outbox()[0][7], 2);
    thinq.reset_recorder();
    dev.set_property("express_freeze", "OFF");
    assert_eq!(thinq.outbox()[0][7], 1);

    thinq.reset_recorder();
    dev.set_property("shabbat_mode", "ON");
    assert_eq!(thinq.outbox()[0][18], 1);
    thinq.reset_recorder();
    dev.set_property("shabbat_mode", "OFF");
    assert_eq!(thinq.outbox()[0][18], 0);

    thinq.reset_recorder();
    dev.set_property("does-not-exist", "1");
    assert_eq!(thinq.outbox().len(), 0);
}
