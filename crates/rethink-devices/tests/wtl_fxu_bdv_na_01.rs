use rethink_core::{Thinq2Device, hex_decode, hex_encode, MockHaConnection, MockThinq2Device, Metadata};
use rethink_devices::device_trait::DeviceHandler;
use rethink_devices::devices::wtl_fxu_bdv_na_01::Device;

const DEVICE_ID: &str = "test-id";
const STATUS_WASHER_RUNNING: &str = "aad0360a00d0008542000100ec00be013200030e0e0e1600000000000000002d002d00000016010000070000020f0202022d2d00000000000c380000000000070401002a000000000000000000003c00000001000200000200000000201800810700000000070000000000000000013200030e0e0e1600000000000000002d002d000000160b0100070000020f0202022d2d00000010000c380000000000070401002a000000000000000000003c0000000100020000020000000020180081070000000007000000000000000023a9bb";
const STATE_RESYNC: &str = "aa71360a00710085f3000100eb005f013200030e0e0e1600000000000000002a002d000200160b2600070000020f0202022d2d00000010010c380000000000060401002a000000000000000000000000000001000200000000000000000000810700000000060000000000000000d05dbb";
const WASHER_DOOR_OPEN: &str = "aa42360a0042007d83000201030007100c010b1000330105002557544c5f4658555f4244565f4e415f30310000000102d71c0b8b010700000000000000000018babb";
const WASHER_DOOR_CLOSE: &str = "aa42360a0042007d84000201030007100c010b1001330105002557544c5f4658555f4244565f4e415f30310000000102d51c0b8b0107000000000000000000c4cebb";
const DRYER_DOOR_OPEN: &str = "aa4e360a004e007d850002010300130a0a01040a000021ff000000000000000105340105002557544c5f4658555f4244565f4e415f30310000000102d81c0b8b0107000000000000000000b590bb";
const DRYER_DOOR_CLOSE: &str = "aa4e360a004e007d860002010300130a0a01040a00002200000000000000000005340105002557544c5f4658555f4244565f4e415f30310000000102d71c0b8b0107000000000000000000b490bb";

fn meta() -> Metadata { Metadata::new("WTL_FXU_BDV_NA_01", "WKEX200HBA", "1.0") }
fn make() -> (std::sync::Arc<MockHaConnection>, std::sync::Arc<MockThinq2Device>, std::sync::Arc<Device>) {
    let ha = MockHaConnection::new();
    let thinq = MockThinq2Device::new(DEVICE_ID, meta());
    let dev = Device::new(ha.clone(), thinq.clone(), meta());
    (ha, thinq, dev)
}
fn prop(ha: &MockHaConnection, n: &str) -> Option<String> {
    ha.device(DEVICE_ID)?.properties.get(n).map(|p| p.as_string())
}

#[test]
fn config() {
    let (ha, _, _) = make();
    let devinfo = ha.device(DEVICE_ID).unwrap();
    let comps = &devinfo.config.as_ref().unwrap().components;
    for c in ["washer_state","washer_power","washer_door","dryer_state","dryer_power","init_lcd"] {
        assert!(comps.contains_key(c), "missing {c}");
    }
    let opts = comps["init_lcd"]["options"].as_array().unwrap();
    assert!(opts.iter().any(|v| v == "Default"));
    assert!(opts.iter().any(|v| v == "Christmas"));
}

#[test]
fn status_resync_doors_writes() {
    let (ha, thinq, dev) = make();
    thinq.emit_data(&hex_decode(STATUS_WASHER_RUNNING));
    assert_eq!(prop(&ha, "washer/power").as_deref(), Some("ON"));
    assert_eq!(prop(&ha, "washer/state").as_deref(), Some("RUNNING"));
    assert_eq!(prop(&ha, "washer/course").as_deref(), Some("DELICATES"));
    assert_eq!(prop(&ha, "washer/temp").as_deref(), Some("N/A"));
    assert_eq!(prop(&ha, "washer/remaining_time").as_deref(), Some("45"));
    assert_eq!(prop(&ha, "shared/init_lcd").as_deref(), Some("Summer 2"));

    thinq.emit_data(&hex_decode(STATE_RESYNC));
    assert_eq!(prop(&ha, "washer/power").as_deref(), Some("ON"));
    assert_eq!(prop(&ha, "washer/course").as_deref(), Some("DELICATES"));

    thinq.emit_data(&hex_decode(WASHER_DOOR_OPEN));
    assert_eq!(prop(&ha, "washer/door").as_deref(), Some("OPEN"));
    thinq.emit_data(&hex_decode(WASHER_DOOR_CLOSE));
    assert_eq!(prop(&ha, "washer/door").as_deref(), Some("CLOSE"));
    thinq.emit_data(&hex_decode(DRYER_DOOR_OPEN));
    assert_eq!(prop(&ha, "dryer/door").as_deref(), Some("OPEN"));
    thinq.emit_data(&hex_decode(DRYER_DOOR_CLOSE));
    assert_eq!(prop(&ha, "dryer/door").as_deref(), Some("CLOSE"));

    thinq.reset_recorder();
    dev.start();
    assert_eq!(prop(&ha, "washer/door").as_deref(), Some("CLOSE"));
    assert_eq!(prop(&ha, "dryer/door").as_deref(), Some("CLOSE"));
    assert_eq!(hex_encode(&thinq.outbox()[0]), "AA0EF0ED1121010000001800B5BB");

    thinq.reset_recorder();
    dev.set_property("washer/power", "ON");
    assert_eq!(hex_encode(&thinq.outbox()[0]), "AA0DF0E50002013301020193BB");
    thinq.reset_recorder();
    dev.set_property("washer/power", "OFF");
    assert_eq!(hex_encode(&thinq.outbox()[0]), "AA0DF0E50002013301020090BB");
    thinq.reset_recorder();
    dev.set_property("dryer/power", "ON");
    assert_eq!(hex_encode(&thinq.outbox()[0]), "AA0DF0E50002013401020192BB");
    thinq.reset_recorder();
    dev.set_property("shared/init_lcd", "Default");
    assert_eq!(hex_encode(&thinq.outbox()[0]), "AA0DF0E50002013301510041BB");
    thinq.reset_recorder();
    dev.set_property("washer/buzzer", "Low");
    assert_eq!(hex_encode(&thinq.outbox()[0]), "AA0DF0E50002013301130182BB");
    thinq.reset_recorder();
    dev.set_property("washer/remote_maintain", "ON");
    assert_eq!(hex_encode(&thinq.outbox()[0]), "AA0AF0241001013358BB");
}
