
//! Shared control packets for F/Y-class AABB washers.

use rethink_core::device_base::AabbDeviceCore;
use rethink_core::hex_decode;

pub fn set_power_start_pause(core: &AabbDeviceCore, prop: &str, mqtt_value: &str) {
    if prop == "power" {
        if mqtt_value == "ON" {
            core.send(&hex_decode("F02A0100"));
        } else if mqtt_value == "OFF" {
            core.send(&hex_decode("F024010100"));
        }
    }
    if prop == "pause" {
        core.send(&hex_decode("F024040100"));
    }
    if prop == "start" {
        let payload = if mqtt_value.is_empty() {
            hex_decode("F024050100")
        } else {
            hex_decode(mqtt_value)
        };
        core.send(&payload);
    }
}

pub fn pub_temp_spin(core: &AabbDeviceCore, temp_idx: u8, spin_idx: u8) {
    use crate::washer_common::{spin_value, temperature_value};
    match temperature_value(temp_idx as usize) {
        Some(n) => core.publish_property("temp", (n as i64).into()),
        None => core.publish_property("temp", "unknown".into()),
    }
    match spin_value(spin_idx as usize) {
        Some(n) => core.publish_property("spin", (n as i64).into()),
        None => core.publish_property("spin", "unknown".into()),
    }
}

pub fn pub_error_status(core: &AabbDeviceCore, error: u8, status: u8) {
    use crate::washer_common::{error_message, state_name};
    core.publish_property("error_message", error_message(error as usize).into());
    core.publish_property("error", if error != 0 { "ON" } else { "OFF" }.into());
    core.publish_property("status", state_name(status as usize).into());
}
