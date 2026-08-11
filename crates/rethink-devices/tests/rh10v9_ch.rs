use rethink_core::{
    hex_decode, hex_encode, MockHaConnection, MockThinq2Device, Metadata, Thinq2Device,
};
use rethink_devices::device_trait::DeviceHandler;
use rethink_devices::devices::rh10v9_ch::{Device, MONITOR_INTERVAL_MS};
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;

const DEVICE_ID: &str = "test-id";
fn meta() -> Metadata {
    Metadata::new("RH10V9_CH", "RH10V9_CH", "2.10.114")
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
    ha.device(DEVICE_ID)?
        .properties
        .get(n)
        .map(|p| p.as_string())
}

#[test]
fn start_monitor_enable() {
    let (_, thinq, dev) = make();
    thinq.reset_recorder();
    MONITOR_INTERVAL_MS.store(5, Ordering::SeqCst);
    dev.start();
    assert_eq!(hex_encode(&thinq.outbox()[0]), "AA0EF0ED1121010000001800B5BB");
    thread::sleep(Duration::from_millis(80));
    let n = thinq.outbox().len();
    assert!(n >= 9, "expected >=9 got {n}");
    let after = n;
    thread::sleep(Duration::from_millis(40));
    assert_eq!(thinq.outbox().len(), after);
    dev.drop_device();
    MONITOR_INTERVAL_MS.store(15_000, Ordering::SeqCst);
}

#[test]
fn status_decode_initial_eb() {
    let (ha, thinq, dev) = make();
    // Idle/initial: H:M remaining, phase Initial, rec[4]=0
    thinq.emit_data(&hex_decode(
        "AA2130EB00190100000000000000000000000000000000000000000000750020BB",
    ));
    assert_eq!(prop(&ha, "status").as_deref(), Some("Initial"));
    assert_eq!(prop(&ha, "remaining_time").as_deref(), Some("25"));
    assert_eq!(prop(&ha, "initial_time").as_deref(), Some("25"));
    assert_eq!(prop(&ha, "power").as_deref(), Some("ON"));
    assert_eq!(prop(&ha, "flags").as_deref(), Some("0"));
    dev.drop_device();
}

#[test]
fn live_drying_ec_uses_rec4_remaining_and_phase02() {
    let (ha, thinq, dev) = make();
    // Cur record from 2026-08-11 capture mid-run:
    // programmed 25 min, phase 0x02 Drying, live remaining rec[4]=0x1b (27), course 0x37, dry 4
    thinq.emit_data(&hex_decode(
        "aa3c30ec001902011b0237040000030300000000001900030a010000007500001902011b0237040000030300000000001900030b0100000075007abb",
    ));
    assert_eq!(prop(&ha, "status").as_deref(), Some("Drying"));
    assert_eq!(prop(&ha, "remaining_time").as_deref(), Some("27"));
    assert_eq!(prop(&ha, "initial_time").as_deref(), Some("25"));
    assert_eq!(prop(&ha, "power").as_deref(), Some("ON"));
    assert_eq!(prop(&ha, "course").as_deref(), Some("Auto / Sensor"));
    assert_eq!(prop(&ha, "dry_level").as_deref(), Some("More"));
    assert_eq!(prop(&ha, "temp").as_deref(), Some("Medium"));
    assert_eq!(prop(&ha, "child_lock").as_deref(), Some("ON")); // flags 0x19 bit0
    assert_eq!(prop(&ha, "damp_dry_signal").as_deref(), Some("ON")); // bit3
    assert_eq!(prop(&ha, "tick").as_deref(), Some("11"));
    // baseline = max(27 remaining, 25 programmed) = 27 → progress 0
    assert_eq!(prop(&ha, "cycle_baseline").as_deref(), Some("27"));
    assert_eq!(prop(&ha, "progress").as_deref(), Some("0"));
    assert_eq!(prop(&ha, "option_a").as_deref(), Some("1"));
    assert_eq!(prop(&ha, "option_b").as_deref(), Some("3"));

    // Later frame: remaining 16 min
    thinq.emit_data(&hex_decode(
        "aa3c30ec001902011002370400000303000000000019000377010000007500001902011002370400000303000000000019000378010000007500a6bb",
    ));
    assert_eq!(prop(&ha, "remaining_time").as_deref(), Some("16"));
    assert_eq!(prop(&ha, "tick").as_deref(), Some("120"));
    // progress (27-16)/27 ≈ 40%
    assert_eq!(prop(&ha, "progress").as_deref(), Some("40"));
    assert_eq!(prop(&ha, "cycle_baseline").as_deref(), Some("27"));
    dev.drop_device();
}

#[test]
fn flags_and_end_event() {
    let (ha, thinq, dev) = make();
    thinq.emit_data(&hex_decode(
        "AA2130EB00190100000000000000000000000000000800000000000000750028BB",
    ));
    assert_eq!(prop(&ha, "flags").as_deref(), Some("8"));
    assert_eq!(prop(&ha, "damp_dry_signal").as_deref(), Some("ON"));
    assert_eq!(prop(&ha, "child_lock").as_deref(), Some("OFF"));

    // phase End → cycle_complete event
    let mut rec = vec![0u8; 27];
    rec[2] = 0x04; // End
    rec[25] = 0x75;
    let mut frame = vec![0x30, 0xeb];
    frame.extend_from_slice(&rec);
    // wrap AABB
    let mut pkt = vec![0xaa, (frame.len() + 4) as u8];
    pkt.extend_from_slice(&frame);
    pkt.push(0);
    pkt.push(0xbb);
    let sum: u32 = pkt.iter().take(pkt.len() - 2).map(|&b| u32::from(b)).sum();
    let last = pkt.len() - 2;
    pkt[last] = ((sum & 0xff) as u8) ^ 0x55;
    thinq.emit_data(&pkt);
    assert_eq!(prop(&ha, "status").as_deref(), Some("End"));
    let events = &ha.device(DEVICE_ID).unwrap().events;
    assert!(
        events
            .iter()
            .any(|(t, p)| t == "events/cycle_complete" && p.contains("cycle_complete")),
        "cycle complete event: {events:?}"
    );
    dev.drop_device();
}

#[test]
fn telemetry_0x3e() {
    let (ha, thinq, dev) = make();
    thinq.emit_data(&hex_decode("aa0b303e0092031b068cbb"));
    assert_eq!(prop(&ha, "telemetry").as_deref(), Some("0092031B06"));
    assert_eq!(prop(&ha, "telemetry_u16").as_deref(), Some("146"));
    assert_eq!(prop(&ha, "telemetry_opt").as_deref(), Some("3"));
    // Second session payload from later capture
    thinq.emit_data(&hex_decode("aa0b303e00970448085bbb"));
    assert_eq!(prop(&ha, "telemetry_u16").as_deref(), Some("151"));
    assert_eq!(prop(&ha, "telemetry_opt").as_deref(), Some("4"));
    dev.drop_device();
}

#[test]
fn progress_when_remaining_exceeds_programmed() {
    let (ha, thinq, dev) = make();
    // programmed 25, live remaining 56 (extended sensor dry) — second capture style
    thinq.emit_data(&hex_decode(
        "aa3c30ec00190200380237040000030300000000001900044001000000750000190200380237040000030300000000001900044001000000750099bb",
    ));
    assert_eq!(prop(&ha, "remaining_time").as_deref(), Some("56"));
    assert_eq!(prop(&ha, "initial_time").as_deref(), Some("25"));
    assert_eq!(prop(&ha, "cycle_baseline").as_deref(), Some("56"));
    assert_eq!(prop(&ha, "progress").as_deref(), Some("0"));
    assert_eq!(prop(&ha, "option_a").as_deref(), Some("0"));
    assert_eq!(prop(&ha, "option_b").as_deref(), Some("4"));
    // remaining drops to 53
    thinq.emit_data(&hex_decode(
        "aa3c30ec00190200350237040000030300000000001900045b01000000750000190200350237040000030300000000001900045c01000000750050bb",
    ));
    assert_eq!(prop(&ha, "remaining_time").as_deref(), Some("53"));
    assert_eq!(prop(&ha, "cycle_baseline").as_deref(), Some("56"));
    // (56-53)/56 ≈ 5%
    assert_eq!(prop(&ha, "progress").as_deref(), Some("5"));
    dev.drop_device();
}

#[test]
fn config_exposes_new_entities() {
    let (ha, _, dev) = make();
    let cfg = ha.device(DEVICE_ID).unwrap().config.unwrap();
    for k in [
        "course",
        "dry_level",
        "temp",
        "progress",
        "initial_time",
        "cycle_baseline",
        "option_a",
        "option_b",
        "telemetry_u16",
        "child_lock",
        "cycle_complete",
    ] {
        assert!(cfg.components.contains_key(k), "missing component {k}");
    }
    dev.drop_device();
}
