//! Shared fridge temperature conversion and status packing.

use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemperatureUnit {
    C,
    F,
}

pub fn fridge_range(unit: TemperatureUnit) -> (String, i32, i32) {
    match unit {
        TemperatureUnit::F => ("°F".into(), 33, 43),
        TemperatureUnit::C => ("°C".into(), 1, 7),
    }
}

pub fn freezer_range(unit: TemperatureUnit) -> (String, i32, i32) {
    match unit {
        TemperatureUnit::F => ("°F".into(), -7, 5),
        TemperatureUnit::C => ("°C".into(), -23, -15),
    }
}

/// Self-inverse conversion: fridge display <-> raw byte.
pub fn convert_fridge_temperature(unit: TemperatureUnit, input: i32) -> i32 {
    match unit {
        TemperatureUnit::F => 44 - input,
        TemperatureUnit::C => 8 - input,
    }
}

/// Self-inverse conversion: freezer display <-> raw byte.
pub fn convert_freezer_temperature(unit: TemperatureUnit, input: i32) -> i32 {
    match unit {
        TemperatureUnit::F => 6 - input,
        TemperatureUnit::C => -14 - input,
    }
}

pub const STATUS_FIELDS: &[&str] = &[
    "monStatus",
    "fridgeSetpoint",
    "freezerSetpoint",
    "expressFreeze",
    "freshAirFilter",
    "smartSaving",
    "waterFilter",
    "anyDoorOpen",
    "tempUnit",
    "smartSavingRun",
    "displayLock",
    "activeSaving",
    "ecoFriendly",
    "convertibleTemp",
    "sabbathMode",
    "dualFridge",
    "expressCool",
    "smartCare",
    "drawerMode",
    "pantryMode",
    "voiceMode",
    "dispenserMode",
    "dispenserCapacity",
    "dispenserUnit",
    "selfCare",
    "craftIce",
    "monDataNumber",
];

pub type Status = HashMap<&'static str, u8>;

pub fn unpack_status(buf: &[u8]) -> Status {
    let mut rv = Status::new();
    for (index, key) in STATUS_FIELDS.iter().enumerate() {
        if buf.len() > index {
            rv.insert(*key, buf[index]);
        }
    }
    rv
}

pub fn pack_status(status: &Status, length: usize) -> Vec<u8> {
    let mut rv = vec![0xffu8; length];
    for (index, key) in STATUS_FIELDS.iter().enumerate() {
        if let Some(&v) = status.get(key) {
            if index < length {
                rv[index] = v;
            }
        }
    }
    rv
}

pub fn status_get(s: &Status, key: &str) -> u8 {
    s.get(key).copied().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fridge_temp_roundtrip_c_and_f() {
        for unit in [TemperatureUnit::C, TemperatureUnit::F] {
            let (lo, hi) = {
                let (_u, lo, hi) = fridge_range(unit);
                (lo, hi)
            };
            for display in lo..=hi {
                let raw = convert_fridge_temperature(unit, display);
                assert_eq!(convert_fridge_temperature(unit, raw), display);
            }
        }
    }

    #[test]
    fn freezer_temp_roundtrip_c_and_f() {
        for unit in [TemperatureUnit::C, TemperatureUnit::F] {
            let (_u, lo, hi) = freezer_range(unit);
            for display in lo..=hi {
                let raw = convert_freezer_temperature(unit, display);
                assert_eq!(convert_freezer_temperature(unit, raw), display);
            }
        }
    }

    #[test]
    fn pack_unpack_preserves_known_fields() {
        let mut s = Status::new();
        s.insert("fridgeSetpoint", 3);
        s.insert("freezerSetpoint", 6);
        s.insert("anyDoorOpen", 1);
        let packed = pack_status(&s, 20);
        let back = unpack_status(&packed);
        assert_eq!(status_get(&back, "fridgeSetpoint"), 3);
        assert_eq!(status_get(&back, "freezerSetpoint"), 6);
        assert_eq!(status_get(&back, "anyDoorOpen"), 1);
        assert_eq!(status_get(&back, "missing"), 0);
    }
}
