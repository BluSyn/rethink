use rethink_core::{hex_decode, hex_encode, MockHaConnection, MockThinq2Device, Metadata};
use rethink_devices::device_trait::DeviceHandler;
use rethink_devices::devices::rh10v9_ch::{Device, MONITOR_INTERVAL_MS};
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;

const DEVICE_ID: &str = "test-id";
fn meta() -> Metadata { Metadata::new("RH10V9_CH", "RH10V9_CH", "2.10.114") }
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
fn start_monitor_enable() {
    let (_, thinq, dev) = make();
    thinq.reset_recorder();
    MONITOR_INTERVAL_MS.store(5, Ordering::SeqCst);
    dev.start();
    assert_eq!(hex_encode(&thinq.outbox()[0]), "AA0EF0ED1121010000001800B5BB");
    // wait for retries
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
fn status_decode() {
    let (ha, thinq, dev) = make();
    thinq.emit_data(&hex_decode("AA2130EB00190100000000000000000000000000000000000000000000750020BB"));
    assert_eq!(prop(&ha, "status").as_deref(), Some("Initial"));
    assert_eq!(prop(&ha, "remaining_time").as_deref(), Some("25"));
    assert_eq!(prop(&ha, "power").as_deref(), Some("ON"));
    assert_eq!(prop(&ha, "flags").as_deref(), Some("0"));

    thinq.emit_data(&hex_decode("AA2130EB00190100000000000000000000000000000800000000000000750028BB"));
    assert_eq!(prop(&ha, "flags").as_deref(), Some("8"));

    thinq.emit_data(&hex_decode(
        "AA3C30EC0019010000000000000000000000000000000000000000000075000019010000000000000000000000000000080000000000000075007DBB"
    ));
    assert_eq!(prop(&ha, "flags").as_deref(), Some("8"));

    // drying synthetic
    thinq.emit_data(&hex_decode(&format!("AA2130EB{}{}00BB", "010532", "00".repeat(22)+"7500")));
    assert_eq!(prop(&ha, "status").as_deref(), Some("Drying"));
    assert_eq!(prop(&ha, "remaining_time").as_deref(), Some("65"));
    dev.drop_device();
}
