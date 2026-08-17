use rethink_core::{hex_decode, MockHaConnection, MockThinq2Device, Metadata, Thinq2Device};
use rethink_devices::device_trait::DeviceHandler;
use rethink_devices::devices::h11::Device;

const DEVICE_ID: &str = "test-id";
fn meta() -> Metadata {
    Metadata::new("H11", "DUE2BG.AKOR", "1.0")
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

/// Match AABB-wrapped outbox packets by inner payload bytes after AA/len.
fn has_inner(sent: &[Vec<u8>], inner: &[u8]) -> bool {
    sent.iter()
        .any(|p| p.len() >= 4 + inner.len() && p[2..2 + inner.len()] == *inner)
}

/// Build a minimal 0x32/0xEC dual-half frame with current status in the second half.
fn build_ec_status(data: &[u8; 24]) -> Vec<u8> {
    let mut half = vec![0x00, 0x18];
    half.extend_from_slice(data);
    while half.len() < 26 {
        half.push(0);
    }
    let old = half.clone();
    let mut inner = vec![0x32, 0xec];
    inner.extend(old);
    inner.extend(half);
    let mut pkt = vec![0xaa, (inner.len() + 4) as u8];
    pkt.extend(inner);
    pkt.push(0);
    pkt.push(0xbb);
    pkt
}

#[test]
fn config_has_full_control_surface() {
    let (ha, _, _) = make();
    let devinfo = ha.device(DEVICE_ID).unwrap();
    let comps = &devinfo.config.as_ref().unwrap().components;

    // Status / sensors
    for k in [
        "power",
        "state",
        "course",
        "remain_time",
        "door",
        "remote_start",
    ] {
        assert!(comps.contains_key(k), "missing {k}");
    }

    // Writable settings (must have command_topic — not demoted sensors)
    for k in [
        "rinse_level",
        "salt_level",
        "buzzer_level",
        "end_alarm_sound",
        "clean_reminder",
        "auto_dry",
        "brightness",
        "remote_start_mode",
    ] {
        assert!(comps.contains_key(k), "missing settings entity {k}");
        assert!(
            comps[k].get("command_topic").is_some(),
            "{k} must have command_topic for HA control"
        );
    }
    assert_eq!(comps["rinse_level"]["platform"], "number");
    assert_eq!(comps["buzzer_level"]["platform"], "select");
    assert_eq!(comps["auto_dry"]["platform"], "switch");
    assert_eq!(comps["brightness"]["platform"], "switch");

    // Target + start_course (PR #139 control surface)
    for k in [
        "target_course",
        "target_delay",
        "target_high_temp",
        "target_extra_dry",
        "target_extra_rinse",
        "start_course",
    ] {
        assert!(comps.contains_key(k), "missing control entity {k}");
        assert!(
            comps[k].get("command_topic").is_some(),
            "{k} must have command_topic"
        );
    }
    assert_eq!(comps["start_course"]["platform"], "button");
    assert_eq!(comps["target_course"]["platform"], "select");
    assert_eq!(comps["target_delay"]["platform"], "number");
}

#[test]
fn start_publishes_default_targets() {
    let (ha, _, dev) = make();
    DeviceHandler::start(dev.as_ref());
    assert_eq!(prop(&ha, "target_course").as_deref(), Some("AUTO"));
    assert_eq!(prop(&ha, "target_delay").as_deref(), Some("0"));
    assert_eq!(prop(&ha, "target_high_temp").as_deref(), Some("OFF"));
    assert_eq!(prop(&ha, "target_extra_dry").as_deref(), Some("OFF"));
    assert_eq!(prop(&ha, "target_extra_rinse").as_deref(), Some("0"));
}

#[test]
fn standby_power_off() {
    let (ha, thinq, _) = make();
    let mut data = [0u8; 24];
    data[0] = 4; // STANDBY
    data[5] = 0x01; // AUTO
    thinq.emit_data(&build_ec_status(&data));
    assert_eq!(prop(&ha, "state").as_deref(), Some("STANDBY"));
    assert_eq!(prop(&ha, "power").as_deref(), Some("OFF"));
    assert_eq!(prop(&ha, "course").as_deref(), Some("AUTO"));
}

#[test]
fn running_with_times_and_door() {
    let (ha, thinq, _) = make();
    let mut data = [0u8; 24];
    data[0] = 2; // RUNNING
    data[1] = 0x01;
    data[3] = 1;
    data[4] = 30;
    data[5] = 0x05; // NORMAL/ECO
    data[7] = 0;
    data[8] = 45;
    data[11] = 0x02; // door open
    data[12] = 0x0c; // extra dry + sterilize
    data[15] = 0x02; // remote start
    thinq.emit_data(&build_ec_status(&data));
    assert_eq!(prop(&ha, "state").as_deref(), Some("RUNNING"));
    assert_eq!(prop(&ha, "power").as_deref(), Some("ON"));
    assert_eq!(prop(&ha, "course").as_deref(), Some("NORMAL/ECO"));
    assert_eq!(prop(&ha, "course_time").as_deref(), Some("90"));
    assert_eq!(prop(&ha, "remain_time").as_deref(), Some("45"));
    assert_eq!(prop(&ha, "door").as_deref(), Some("OPEN"));
    assert_eq!(prop(&ha, "high_temp_dry").as_deref(), Some("ON"));
    assert_eq!(prop(&ha, "sterilize").as_deref(), Some("ON"));
    assert_eq!(prop(&ha, "remote_start").as_deref(), Some("ON"));
}

#[test]
fn smart_course_overrides_base() {
    let (ha, thinq, _) = make();
    let mut data = [0u8; 24];
    data[0] = 1;
    data[5] = 0x01;
    data[20] = 0x05; // GREASY_TABLEWARE
    thinq.emit_data(&build_ec_status(&data));
    assert_eq!(prop(&ha, "course").as_deref(), Some("GREASY_TABLEWARE"));
}

#[test]
fn power_pause_commands() {
    let (_ha, thinq, dev) = make();
    dev.set_property("power", "ON");
    dev.set_property("power", "OFF");
    dev.set_property("pause", "PRESS");
    let sent = thinq.outbox();
    assert!(has_inner(&sent, &hex_decode("F02616")), "wake: {sent:?}");
    assert!(has_inner(&sent, &hex_decode("F02612")), "off: {sent:?}");
    assert!(has_inner(&sent, &hex_decode("F02613")), "pause: {sent:?}");
}

/// start_course builds F0 26 10 [course][delay][0][opt3][opt4][0] from targets.
#[test]
fn start_course_sends_f02610_payload() {
    let (ha, thinq, dev) = make();
    thinq.reset_recorder();

    // Configure targets then press start_course
    dev.set_property("target_course", "HEAVY/INTENSIVE"); // 0x02
    dev.set_property("target_delay", "3");
    dev.set_property("target_high_temp", "ON"); // opt3 |= 0x08
    dev.set_property("target_extra_dry", "ON"); // opt3 |= 0x04
    dev.set_property("target_extra_rinse", "2"); // opt4 |= 0x10
    assert_eq!(prop(&ha, "target_course").as_deref(), Some("HEAVY/INTENSIVE"));
    assert_eq!(prop(&ha, "target_delay").as_deref(), Some("3"));
    assert_eq!(prop(&ha, "target_high_temp").as_deref(), Some("ON"));

    thinq.reset_recorder();
    dev.set_property("start_course", "PRESS");

    // f0 26 10 course=0x02 delay=3 opt2=0 opt3=0x0c opt4=0x10 opt5=0
    let expected = [0xf0, 0x26, 0x10, 0x02, 0x03, 0x00, 0x0c, 0x10, 0x00];
    let sent = thinq.outbox();
    assert!(
        has_inner(&sent, &expected),
        "start_course payload missing, outbox={sent:?}"
    );

    // Download cycle sets opt4 bit 0x40
    thinq.reset_recorder();
    dev.set_property("target_course", "DOWNLOAD_CYCLE");
    dev.set_property("target_high_temp", "OFF");
    dev.set_property("target_extra_dry", "OFF");
    dev.set_property("target_extra_rinse", "0");
    dev.set_property("target_delay", "0");
    thinq.reset_recorder();
    dev.set_property("start_course", "PRESS");
    let expected_dl = [0xf0, 0x26, 0x10, 0x0b, 0x00, 0x00, 0x00, 0x40, 0x00];
    assert!(
        has_inner(&thinq.outbox(), &expected_dl),
        "download start missing: {:?}",
        thinq.outbox()
    );
}

/// Settings write goes through send_settings → F0 26 [rinse][salt][opt1][opt2][opt3]…
#[test]
fn settings_write_sends_f026_packet() {
    let (_ha, thinq, dev) = make();
    thinq.reset_recorder();

    // Seed from a status frame so cache matches device (rinse/salt/buzzer bits)
    let mut data = [0u8; 24];
    data[0] = 1; // INITIAL
    data[13] = 1; // rinse
    data[14] = 2; // salt
    data[15] = 0x40; // LOW buzzer
    data[16] = 0x40; // ONE_TIME remote mode
    thinq.emit_data(&build_ec_status(&data));
    thinq.reset_recorder();

    // Change rinse_level → must call sendSettings with new rinse
    dev.set_property("rinse_level", "3");
    // opt1: LOW buzzer only → 0x02; opt2 ONE_TIME → 0x40; opt3 no brightness → 0
    let expected = [0xf0, 0x26, 0x03, 0x02, 0x02, 0x40, 0x00, 0x00, 0x00, 0x00];
    let sent = thinq.outbox();
    assert!(
        has_inner(&sent, &expected),
        "rinse_level settings write missing: {sent:?}"
    );

    // auto_dry ON adds opt1 bit 0x20
    thinq.reset_recorder();
    dev.set_property("auto_dry", "ON");
    let expected_ad = [0xf0, 0x26, 0x03, 0x02, 0x22, 0x40, 0x00, 0x00, 0x00, 0x00];
    assert!(
        has_inner(&thinq.outbox(), &expected_ad),
        "auto_dry settings: {:?}",
        thinq.outbox()
    );
}

#[test]
fn registry_resolves() {
    assert!(rethink_devices::registry::t2_factory("H11").is_some());
}
