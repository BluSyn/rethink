use rethink_core::{hex_decode, hex_encode, MockHaConnection, MockThinq2Device, Metadata};
use rethink_devices::device_trait::DeviceHandler;
use rethink_devices::devices::dev_2ref11eida__4::Device;

const DEVICE_ID: &str = "test-id";
const SAMPLE_INITIAL: &str =
    "AA4A10EB0209060202020400000001FFFF0300FFFF00FFFFFFFFFFFFFF020001010100000102FF6161FFFFFF01FF00FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF0078FF000079BB";
const SAMPLE_DOOR: &str =
    "AA8E10EC0209060202020400000001FFFF0300FFFF00FFFFFFFFFFFFFF020001010100000102FF6161FFFFFF01FF00FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF0078FF00000209060202020401000001FFFF0300FFFF00FFFFFFFFFFFFFF020001010100000102FF6161FFFFFF01FF00FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF0078FF0000FABB";
const SAMPLE_FRIDGE42: &str =
    "AA8E10EC0201060202020401000001FFFF0300FFFF00FFFFFFFFFFFFFF020001010100000102FF6161FFFFFF01FF00FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF0078FF00000202060202020401000001FFFF0300FFFF00FFFFFFFFFFFFFF020001010100000102FF6161FFFFFF01FF00FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF0078FF0000F4BB";
const SAMPLE_FLEX: &str =
    "AA8E10EC0209060202020401000001FFFF0300FFFF00FFFFFFFFFFFFFF020001010100000102FF6161FFFFFF01FF00FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF0078FF00000209060202020401000001FFFF0500FFFF00FFFFFFFFFFFFFF020001010100000102FF6161FFFFFF01FF00FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF0078FF0000E7BB";

fn meta() -> Metadata {
    Metadata::new("2REF11EIDA__4", "2REF11EIDA__4", "1.0")
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
fn not_until_status() {
    let (ha, _, _) = make();
    assert!(ha.device(DEVICE_ID).is_none());
}

#[test]
fn initial_fahrenheit() {
    let (ha, thinq, _) = make();
    thinq.emit_data(&hex_decode(SAMPLE_INITIAL));
    let comps = &ha.device(DEVICE_ID).unwrap().config.as_ref().unwrap().components;
    assert_eq!(comps["fridge_setpoint"]["unit_of_measurement"], "°F");
    assert_eq!(comps["fridge_setpoint"]["min"], 33);
    assert_eq!(comps["freezer_setpoint"]["min"], -7);
    assert_eq!(
        comps["flex_setpoint"]["options"],
        serde_json::json!([
            "Chilled Wine",
            "Deli/Snacks",
            "Cold Drink",
            "Meat/Seafood",
            "Freezer"
        ])
    );
    assert_eq!(prop(&ha, "fridge_setpoint").as_deref(), Some("35"));
    assert_eq!(prop(&ha, "freezer_setpoint").as_deref(), Some("0"));
    assert_eq!(prop(&ha, "flex_setpoint").as_deref(), Some("Cold Drink"));
    assert_eq!(prop(&ha, "door").as_deref(), Some("OFF"));
}

#[test]
fn deltas_and_writes() {
    let (ha, thinq, dev) = make();
    thinq.emit_data(&hex_decode(SAMPLE_DOOR));
    assert_eq!(prop(&ha, "door").as_deref(), Some("ON"));
    thinq.emit_data(&hex_decode(SAMPLE_FRIDGE42));
    assert_eq!(prop(&ha, "fridge_setpoint").as_deref(), Some("42"));
    thinq.emit_data(&hex_decode(SAMPLE_FLEX));
    assert_eq!(prop(&ha, "flex_setpoint").as_deref(), Some("Freezer"));

    thinq.reset_recorder();
    dev.start();
    assert_eq!(hex_encode(&thinq.outbox()[0]), "AA0EF0ED1211010000010400EBBB");

    thinq.emit_data(&hex_decode(SAMPLE_INITIAL));
    thinq.reset_recorder();
    dev.set_property("fridge_setpoint", "42");
    let pkt = thinq.outbox()[0].clone();
    assert_eq!(pkt[5], 2);
    assert_eq!(pkt[2 + 2 + 8], 0);

    thinq.reset_recorder();
    dev.set_property("freezer_setpoint", "-5");
    assert_eq!(thinq.outbox()[0][2 + 2 + 2], 11);

    thinq.reset_recorder();
    dev.set_property("flex_setpoint", "Freezer");
    assert_eq!(thinq.outbox()[0][2 + 2 + 13], 5);

    thinq.reset_recorder();
    dev.set_property("flex_setpoint", "NotARealOption");
    assert_eq!(thinq.outbox().len(), 0);
}
