use rethink_core::{hex_decode, MockHaConnection, MockThinq2Device, Metadata, Thinq2Device};
use rethink_devices::devices::rv13b6es_d_us_wifi::Device;

const DEVICE_ID: &str = "test-id";
fn meta() -> Metadata {
    Metadata::new("RV13B6ES_D_US_WIFI", "RV13B6ES_D_US_WIFI", "1.0")
}
fn make() -> (
    std::sync::Arc<MockHaConnection>,
    std::sync::Arc<MockThinq2Device>,
    std::sync::Arc<Device>,
) {
    let ha = MockHaConnection::new();
    let thinq = MockThinq2Device::new(DEVICE_ID, meta());
    let dev = Device::new(ha.clone(), thinq.clone(), meta());
    (ha, thinq, dev)
}
fn prop(ha: &MockHaConnection, n: &str) -> Option<String> {
    ha.device(DEVICE_ID)?.properties.get(n).map(|p| p.as_string())
}

// Real fixtures from upstream PR #134 tests
const EB_IDLE: &str = "aa2330eb001b000029002900000000000100000000280000000100000064000000b6bb";
const POWER_ON_NORMAL: &str =
    "aa4030ec001b01000a000a00000000000100000040280000000000000064000000001b0100290029030003040001000000402800000000000000640000001dbb";
const WRINKLE_CARE_ON: &str =
    "aa4030ec001b010029002903000304000100000040280000000000000064000000001b010029002903000304000100000050280000000000000064000000f5bb";
const WRINKLE_CARE_OFF: &str =
    "aa4030ec001b010029002903000304000100000050280000000000000064000000001b010029002903000304000100000040280000000000000064000000f5bb";
const REDUCE_STATIC_AND_LOAD_ITEM: &str =
    "aa4030ec001b010029002903000304000100000000280000000000000064000000001b01002700270300030400010000000228000000000000056400000046bb";
const STARTS_DRYING: &str =
    "aa4030ec001b01000a000a100000020001f1000000280000000000000064000000001b32000a000a100000020001f1000000290000000100000064000000ecbb";
const ANTIBACTERIAL: &str =
    "aa4030ec001b01001f001f16000005000100000040280000000000000064000000001b01010a010a080005050001000000402800000000000000640000000cbb";

#[test]
fn config_has_es_entities() {
    let (ha, _, _) = make();
    let devinfo = ha.device(DEVICE_ID).unwrap();
    let comps = &devinfo.config.as_ref().unwrap().components;
    assert!(comps.contains_key("wrinkle_care"));
    assert!(comps.contains_key("load_item"));
    assert!(comps.contains_key("remote_start"));
    assert!(comps.contains_key("more_less_time"));
    assert!(comps.contains_key("signal"));
}

#[test]
fn eb_idle() {
    let (ha, thinq, _) = make();
    thinq.emit_data(&hex_decode(EB_IDLE));
    assert_eq!(prop(&ha, "power").as_deref(), Some("OFF"));
    assert_eq!(prop(&ha, "status").as_deref(), Some("Off"));
    assert_eq!(prop(&ha, "remaining_time").as_deref(), Some("0"));
}

#[test]
fn power_on_normal() {
    let (ha, thinq, _) = make();
    thinq.emit_data(&hex_decode(POWER_ON_NORMAL));
    assert_eq!(prop(&ha, "power").as_deref(), Some("ON"));
    assert_eq!(prop(&ha, "status").as_deref(), Some("Initial"));
    assert_eq!(prop(&ha, "course").as_deref(), Some("Normal"));
    assert_eq!(prop(&ha, "dry_level").as_deref(), Some("Normal"));
    assert_eq!(prop(&ha, "temp").as_deref(), Some("Mid High"));
    assert_eq!(prop(&ha, "remaining_time").as_deref(), Some("41"));
    assert_eq!(prop(&ha, "initial_time").as_deref(), Some("41"));
}

#[test]
fn wrinkle_care_from_flags_not_opt2() {
    // Would fail if aliased to BSD (wrinkle on opt2 0x10)
    let (ha, thinq, _) = make();
    thinq.emit_data(&hex_decode(WRINKLE_CARE_ON));
    assert_eq!(prop(&ha, "wrinkle_care").as_deref(), Some("ON"));
    assert_eq!(prop(&ha, "energy_saver").as_deref(), Some("OFF"));
    assert_eq!(prop(&ha, "turbo_steam").as_deref(), Some("OFF"));

    thinq.emit_data(&hex_decode(WRINKLE_CARE_OFF));
    assert_eq!(prop(&ha, "wrinkle_care").as_deref(), Some("OFF"));
}

#[test]
fn load_item_and_reduce_static() {
    let (ha, thinq, _) = make();
    thinq.emit_data(&hex_decode(REDUCE_STATIC_AND_LOAD_ITEM));
    assert_eq!(prop(&ha, "reduce_static").as_deref(), Some("ON"));
    assert_eq!(prop(&ha, "load_item").as_deref(), Some("5"));
}

#[test]
fn more_less_and_remote_start() {
    let (ha, thinq, _) = make();
    thinq.emit_data(&hex_decode(STARTS_DRYING));
    assert_eq!(prop(&ha, "more_less_time").as_deref(), Some("-15"));
    assert_eq!(prop(&ha, "remaining_time").as_deref(), Some("10"));
    assert_eq!(prop(&ha, "remote_start").as_deref(), Some("ON"));
    assert_eq!(prop(&ha, "status").as_deref(), Some("Drying"));
}

#[test]
fn hour_plus_estimate() {
    let (ha, thinq, _) = make();
    thinq.emit_data(&hex_decode(ANTIBACTERIAL));
    assert_eq!(prop(&ha, "course").as_deref(), Some("Antibacterial"));
    assert_eq!(prop(&ha, "dry_level").as_deref(), Some("Very"));
    assert_eq!(prop(&ha, "remaining_time").as_deref(), Some("70"));
    assert_eq!(prop(&ha, "initial_time").as_deref(), Some("70"));
}

#[test]
fn registry_resolves() {
    assert!(rethink_devices::registry::t2_factory("RV13B6ES_D_US_WIFI").is_some());
}
