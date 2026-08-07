//! Shared washer course/state/error tables.

pub const ERRORS: &[Option<&str>] = &[
    Some("OK"),
    Some("Door lock error (DE2)"),
    Some("Door open error (DE1)"),
    Some("Water supply error (IE)"),
    Some("Water drain error (OE)"),
    Some("Out of balance error (UE)"),
    Some("Overfill error (FE)"),
    Some("Water level sensor error (PE)"),
    Some("Temperature sensor error (TE)"),
    Some("Locked motor error (LE)"),
    None,
    Some("Unknown error (dHE)"),
    Some("Power fail error (PF)"),
    Some("Unknown error (FF)"),
    Some("Unknown error (DCE)"),
    Some("Unknown error (AE)"),
    Some("EEPROM error"),
    Some("Unknown error (PS)"),
    Some("Door sensor error (DE4)"),
    Some("Vibration sensor error (VS)"),
    Some("Unknown error (LE8)"),
    Some("Unknown error (LE9)"),
    Some("Unknown error (ED1)"),
    Some("Unknown error (ED2)"),
    Some("Unknown error (ED3)"),
    Some("Unknown error (ED4)"),
    Some("Unknown error (ED5)"),
];

pub const STATES: &[Option<&str>] = &[
    Some("Off"),
    Some("Ready"),
    Some("Paused"),
    Some("Delayed"),
    Some("Measuring"),
    Some("Pre-wash"),
    Some("Washing"),
    Some("Rinsing"),
    Some("Spinning"),
    Some("Drying"),
    Some("End"),
    Some("Cooling"),
    Some("Rinse hold"),
    None,
    Some("Refreshing"),
    Some("Steam softening"),
    Some("Demo"),
    None,
    Some("Error"),
    Some("Auto DT Open Pause"),
];

pub fn course_name(code: u32) -> Option<&'static str> {
    match code {
        0x1 => Some("Cotton"),
        0x2 => Some("Ease Care"),
        0x4 => Some("Eco 40-60"),
        0x5 => Some("Duvet"),
        0x7 => Some("Mix"),
        0x8 => Some("Sports Wear"),
        0x9 => Some("Night Wash"),
        0xb => Some("Gentle Care"),
        0xc => Some("Quick 14"),
        0xd => Some("Steam refresh"),
        0xe => Some("Rinse + Spin"),
        0x12 => Some("Drum Clean"),
        0x13 => Some("Wash + Dry"),
        0x17 => Some("Spin + Dry"),
        0x18 => Some("Drying"),
        0x1b => Some("Wool"),
        0x20 => Some("Delicate"),
        0x22 => Some("Quick 30"),
        0x24 => Some("Direct Wear"),
        0x2d => Some("Allergy Care"),
        0x2c => Some("Baby Steam Care"),
        0x31 => Some("TurboWash 39"),
        0x32 => Some("TurboWash 59"),
        0x33 => Some("Baby Clothes"),
        0x34 => Some("Children Clothing"),
        0x35 => Some("School Uniform"),
        0x36 => Some("Swimwear"),
        0x37 => Some("Rainy Season Care"),
        0x38 => Some("Lightly Soiled Refresh"),
        0x39 => Some("Denim"),
        0x3a => Some("Bedding"),
        0x3b => Some("Sweat Stains"),
        0x3e => Some("Single Garments"),
        0x40 => Some("Overnight"),
        0x42 => Some("Fast Wash + Dry"),
        0x45 => Some("Turbo drying"),
        0x46 => Some("Drying shirts"),
        0x48 => Some("Sanitary"),
        0x49 => Some("Small Load"),
        0x4a => Some("Delicate Dresses"),
        0x4b => Some("Wool"),
        0x4d => Some("Cold Wash"),
        0x64 => Some("Rinse + Spin"),
        0x66 => Some("Powder Residue"),
        0x6b => Some("Cuffs + Collars"),
        0x6c => Some("Juice + Food Stains"),
        0x6e => Some("Saving Time"),
        0x6f => Some("Reducing Wrinkles"),
        _ => None,
    }
}

pub const TEMPERATURES: &[Option<u32>] = &[
    None,
    Some(10),
    Some(20),
    Some(30),
    Some(40),
    Some(50),
    Some(60),
    Some(95),
];

pub const SPINS: &[Option<u32>] = &[
    None,
    Some(0),
    Some(400),
    Some(500),
    Some(700),
    Some(800),
    Some(900),
    Some(1000),
    Some(1100),
    Some(1200),
    Some(1400),
    Some(1600),
];

pub fn drying_mode(code: u32) -> Option<&'static str> {
    match code {
        0x0 => Some("Off"),
        0x2 => Some("Auto"),
        0x3 => Some("00:30"),
        0x4 => Some("01:00"),
        0x5 => Some("01:30"),
        0x6 => Some("02:00"),
        0x7 => Some("02:30"),
        0xa => Some("Iron"),
        0xb => Some("Delicate"),
        0xc => Some("Eco"),
        _ => None,
    }
}

pub const DOSES: &[&str] = &["Off", "Low", "Medium", "High"];

pub fn error_message(code: usize) -> &'static str {
    ERRORS.get(code).and_then(|x| *x).unwrap_or("unknown")
}

pub fn state_name(code: usize) -> &'static str {
    STATES.get(code).and_then(|x| *x).unwrap_or("unknown")
}

pub fn temperature_value(code: usize) -> Option<u32> {
    TEMPERATURES.get(code).and_then(|x| *x)
}

pub fn spin_value(code: usize) -> Option<u32> {
    SPINS.get(code).and_then(|x| *x)
}

/// Non-None STATE labels for HA enum options.
pub fn state_options() -> Vec<&'static str> {
    STATES.iter().filter_map(|s| *s).collect()
}

/// Non-None ERROR labels for HA enum options.
pub fn error_options() -> Vec<&'static str> {
    ERRORS.iter().filter_map(|s| *s).collect()
}
