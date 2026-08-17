use rethink_core::{hex_decode, MockHaConnection, MockThinq2Device, Metadata, Thinq2Device};
use rethink_devices::devices::f3l7cyk5w_us_wifi::Device;

const DEVICE_ID: &str = "test-id";
fn meta() -> Metadata {
    Metadata::new("F3L7CYK5W_US_WIFI", "F3L7CYK5W_US_WIFI", "1.0")
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

// Fixtures from upstream PR #135
const EB_OFF: &str = "aa2020eb00180000010000fe0000000000000000000000000005332b00001abb";
const DIAL_NORMAL: &str =
    "aa3a20ec00180502150215050005050402000000000000000000332e000000180501030103060003040402000000000000000000332e00001fbb";
const DIAL_SPEED_WASH: &str =
    "aa3a20ec001805010101010a0003050403000000000000000000332e0000001805000f000f0b0001050601000000000000000000332e000713bb";
const CHILD_LOCK_ON: &str =
    "aa3a20ec00180501030103060003040402000000000000000000332e000000180501030103060003040402000000010000000000332e000076bb";
const COLD_WASH_ON: &str =
    "aa3a20ec00180501210121060003030702000000000000000000332e000000180501170117060003030202000000001000000000332e0000c0bb";
const WASHING_START: &str =
    "aa3a20ec00181401030103060003040402000000008000000005332b000000181701140114060003040402000000008002000014332b0004d5bb";
const CYCLE_COMPLETE: &str =
    "aa3a20ec0018280001011406000004000000000000800200671e332b000400183c00010000fe0000000000000000000000006c28332c0000aabb";
const E2_STALE_REPLAY: &str = "aa2020e203181401030103060003040402000000008000006c05332b000030bb";

#[test]
fn config_components() {
    let (ha, _, _) = make();
    let devinfo = ha.device(DEVICE_ID).unwrap();
    let comps = &devinfo.config.as_ref().unwrap().components;
    assert!(comps.contains_key("power"));
    assert!(comps.contains_key("course"));
    assert!(comps.contains_key("child_lock"));
    assert!(comps.contains_key("rinse_spin"));
    assert!(comps.contains_key("tub_clean_count"));
    assert!(!comps.contains_key("turbo_wash")); // model has no TurboWash
}

#[test]
fn eb_off() {
    let (ha, thinq, _) = make();
    thinq.emit_data(&hex_decode(EB_OFF));
    assert_eq!(prop(&ha, "power").as_deref(), Some("OFF"));
    assert_eq!(prop(&ha, "status").as_deref(), Some("Off"));
}

#[test]
fn course_table_not_f3l2() {
    // 0x06 is Normal here; F3L2 would call it Heavy Duty
    let (ha, thinq, _) = make();
    thinq.emit_data(&hex_decode(DIAL_NORMAL));
    assert_eq!(prop(&ha, "course").as_deref(), Some("Normal"));
    assert_eq!(prop(&ha, "status").as_deref(), Some("Initial"));

    thinq.emit_data(&hex_decode(DIAL_SPEED_WASH));
    assert_eq!(prop(&ha, "course").as_deref(), Some("Speed Wash"));
}

#[test]
fn child_lock_and_cold_wash() {
    let (ha, thinq, _) = make();
    thinq.emit_data(&hex_decode(CHILD_LOCK_ON));
    assert_eq!(prop(&ha, "child_lock").as_deref(), Some("ON"));

    thinq.emit_data(&hex_decode(COLD_WASH_ON));
    assert_eq!(prop(&ha, "cold_wash").as_deref(), Some("ON"));
}

#[test]
fn washing_and_complete_timers() {
    let (ha, thinq, _) = make();
    thinq.emit_data(&hex_decode(WASHING_START));
    assert_eq!(prop(&ha, "status").as_deref(), Some("Washing"));
    assert_eq!(prop(&ha, "power").as_deref(), Some("ON"));
    // remaining = 1*60+20 = 80
    assert_eq!(prop(&ha, "remaining_time").as_deref(), Some("80"));

    thinq.emit_data(&hex_decode(CYCLE_COMPLETE));
    assert_eq!(prop(&ha, "status").as_deref(), Some("Complete"));
    // idle phases zero timers
    assert_eq!(prop(&ha, "remaining_time").as_deref(), Some("0"));
    assert_eq!(prop(&ha, "initial_time").as_deref(), Some("0"));
}

#[test]
fn ignores_e2_stale_replay() {
    let (ha, thinq, _) = make();
    thinq.emit_data(&hex_decode(CYCLE_COMPLETE));
    assert_eq!(prop(&ha, "status").as_deref(), Some("Complete"));
    thinq.emit_data(&hex_decode(E2_STALE_REPLAY));
    // must not knock back to Sensing
    assert_eq!(prop(&ha, "status").as_deref(), Some("Complete"));
}

#[test]
fn registry_resolves() {
    assert!(rethink_devices::registry::t2_factory("F3L7CYK5W_US_WIFI").is_some());
}
