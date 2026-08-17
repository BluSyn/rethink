use rethink_core::{hex_decode, MockHaConnection, MockThinq2Device, Metadata, Thinq2Device};
use rethink_devices::devices::f_c__y___w_a__qeuk::Device;

const DEVICE_ID: &str = "test-id";
fn meta() -> Metadata {
    Metadata::new("F_C__Y___W.A__QEUK", "F_C__Y___W.A__QEUK", "1.0")
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

// Fixtures from upstream PR #136
const SAMPLE_WASHING_EC: &str = "AA4220EC001C06012C02010100030A0601000000004000000306000A003400000500001C06012B02010100030A0601000000004000000306000A003400000500FEBB";
const SAMPLE_STEAM_ON_EC: &str = "AA4220EC001C06012C02010100030A0601000000804000000306000A003400000500001C06012B02010100030A0601000000004000000306000A003400000500FEBB";
const SAMPLE_WRINKLE_CARE_ON_EC: &str = "AA4220EC001C06012C02010100030A0601000000204000000306000A003400000500001C06012B02010100030A0601000000004000000306000A003400000500FEBB";
const SAMPLE_CHILD_LOCK_ON_EC: &str = "AA4220EC001C06012C02010100030A060100000000C000000306000A003400000500001C06012B02010100030A0601000000004000000306000A003400000500FEBB";
const SAMPLE_REMOTE_START_ON_EC: &str = "AA4220EC001C01003B003B3200030904010000000100000001010030003400000000001C01041B041B0400030A0601000000000000000201003000340000020044BB";
const SAMPLE_OFF_EC: &str = "AA4220EC001C000000020101000000000000000000000000030A000A003400000500001C0000000201010000000000000000000000000300000A0034000005009BBB";
const SAMPLE_END_EC: &str = "AA4220EC001C0A0000020101000000000000000000400000060A000A003400000500001C0A0000020101000000000000000000000000060A000A00340000050067BB";
const SAMPLE_WASHING_EB: &str = "AA2420EB001C06003200480100000A0601000000000000000606000A003400000500C4BB";
const SAMPLE_E2_IGNORED: &str = "AA2420E2091C04032603260100030A0601000000400000000604000A003400000500B8BB";
const SAMPLE_DOOR_UNLOCKED: &str = "AA0720D800FCBB";
const SAMPLE_DOOR_LOCKED: &str = "AA0720D80BE1BB";

#[test]
fn config_components() {
    let (ha, _, _) = make();
    let devinfo = ha.device(DEVICE_ID).unwrap();
    let comps = &devinfo.config.as_ref().unwrap().components;
    assert!(comps.contains_key("power"));
    assert!(comps.contains_key("status"));
    assert!(comps.contains_key("steam"));
    assert!(comps.contains_key("wrinkle_care"));
    assert!(comps.contains_key("tub_clean"));
    assert!(comps.contains_key("delay_remaining"));
}

#[test]
fn washing_ec() {
    let (ha, thinq, _) = make();
    thinq.emit_data(&hex_decode(SAMPLE_WASHING_EC));
    assert_eq!(prop(&ha, "power").as_deref(), Some("ON"));
    assert_eq!(prop(&ha, "status").as_deref(), Some("Washing"));
    assert_eq!(prop(&ha, "course").as_deref(), Some("Cotton"));
    // 1h44 remaining, 2h01 initial
    assert_eq!(prop(&ha, "remaining_time").as_deref(), Some("104"));
    assert_eq!(prop(&ha, "initial_time").as_deref(), Some("121"));
    assert_eq!(prop(&ha, "temp").as_deref(), Some("60"));
    assert_eq!(prop(&ha, "spin").as_deref(), Some("1400"));
    assert_eq!(prop(&ha, "steam").as_deref(), Some("OFF"));
    assert_eq!(prop(&ha, "active").as_deref(), Some("ON"));
    assert_eq!(prop(&ha, "tub_clean").as_deref(), Some("10"));
}

#[test]
fn steam_wrinkle_child_lock() {
    let (ha, thinq, _) = make();
    thinq.emit_data(&hex_decode(SAMPLE_STEAM_ON_EC));
    assert_eq!(prop(&ha, "steam").as_deref(), Some("ON"));

    thinq.emit_data(&hex_decode(SAMPLE_WRINKLE_CARE_ON_EC));
    assert_eq!(prop(&ha, "wrinkle_care").as_deref(), Some("ON"));

    thinq.emit_data(&hex_decode(SAMPLE_CHILD_LOCK_ON_EC));
    // HA lock: child lock engaged → OFF (Locked semantics in TS port)
    assert_eq!(prop(&ha, "child_lock").as_deref(), Some("OFF"));
}

#[test]
fn remote_start_ready() {
    let (ha, thinq, _) = make();
    thinq.emit_data(&hex_decode(SAMPLE_REMOTE_START_ON_EC));
    assert_eq!(prop(&ha, "status").as_deref(), Some("Ready"));
    assert_eq!(prop(&ha, "remote_start").as_deref(), Some("ON"));
    // Wire buf[14]=0x01 → Cotton (PR comment said Ease Care; byte is Cotton/0x01)
    assert_eq!(prop(&ha, "course").as_deref(), Some("Cotton"));
    assert_eq!(prop(&ha, "temp").as_deref(), Some("40"));
    assert_eq!(prop(&ha, "spin").as_deref(), Some("1200"));
}

#[test]
fn off_and_end() {
    let (ha, thinq, _) = make();
    thinq.emit_data(&hex_decode(SAMPLE_END_EC));
    assert_eq!(prop(&ha, "status").as_deref(), Some("End"));
    assert_eq!(prop(&ha, "power").as_deref(), Some("ON"));
    assert_eq!(prop(&ha, "door_lock").as_deref(), Some("OFF")); // locked during End

    thinq.emit_data(&hex_decode(SAMPLE_OFF_EC));
    assert_eq!(prop(&ha, "status").as_deref(), Some("Off"));
    assert_eq!(prop(&ha, "power").as_deref(), Some("OFF"));
    assert_eq!(prop(&ha, "door_lock").as_deref(), Some("ON")); // unlocked when off
}

#[test]
fn eb_and_e2() {
    let (ha, thinq, _) = make();
    thinq.emit_data(&hex_decode(SAMPLE_WASHING_EB));
    assert_eq!(prop(&ha, "status").as_deref(), Some("Washing"));
    assert_eq!(prop(&ha, "remaining_time").as_deref(), Some("50"));

    thinq.emit_data(&hex_decode(SAMPLE_E2_IGNORED));
    // still washing — E2 ignored
    assert_eq!(prop(&ha, "status").as_deref(), Some("Washing"));
}

#[test]
fn door_d8_when_ready() {
    let (ha, thinq, _) = make();
    thinq.emit_data(&hex_decode(SAMPLE_REMOTE_START_ON_EC)); // Ready status=1
    thinq.emit_data(&hex_decode(SAMPLE_DOOR_LOCKED));
    assert_eq!(prop(&ha, "door_lock").as_deref(), Some("OFF"));
    thinq.emit_data(&hex_decode(SAMPLE_DOOR_UNLOCKED));
    assert_eq!(prop(&ha, "door_lock").as_deref(), Some("ON"));
}

#[test]
fn registry_resolves() {
    assert!(rethink_devices::registry::t2_factory("F_C__Y___W.A__QEUK").is_some());
}
