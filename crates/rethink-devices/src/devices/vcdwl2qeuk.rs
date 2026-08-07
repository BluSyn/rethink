//! VCDWL2QEUK front-load washer (AABB multi-frame).

use crate::device_trait::DeviceHandler;
use rethink_core::device_base::{default_config, AabbDeviceCore};
use rethink_core::{HaConnection, Metadata, Thinq2Device};
use serde_json::{json, Map};
use std::collections::HashMap;
use std::sync::Arc;

const STATUS_FRAME_TYPE: u8 = 0x92;
const CONFIG_FRAME_TYPE: u8 = 0x88;
const STATUS_RECORD_OFF: usize = 78;
const STATUS_RECORD_LEN: usize = 63;
const STATUS_FRAME_LEN: usize = 142;
const DOOR_FRAME_TYPE: u8 = 0x41;
const DOOR_OFFSET: usize = 18;
const DOOR_CLOSED: u8 = 0x02;
const DOOR_OPEN: u8 = 0x01;
const RECORD_B_STANDBY: u8 = 0x00;
const SKYL_OFFSET: usize = 26;
const DISP_DETERGENT_EN: usize = 29;
const DISP_SOFTENER_EN: usize = 30;
const DISP_DETERGENT_ML: usize = 31;
const DISP_SOFTENER_ML: usize = 32;
const DISP_ON: u8 = 0x02;
const OPT_BYTE_A: usize = 33;
const OPT_PREWASH: u8 = 0x40;
const OPT_TURBOWASH: u8 = 0x20;
const OPT_BYTE_B: usize = 34;
const OPT_STEAM: u8 = 0x10;
const FLAGS_BYTE: usize = 36;
const FLAG_CHILD_LOCK: u8 = 0x20;
const FLAG_REMOTE_START: u8 = 0x10;
const TUB_CLEAN_OFFSET: usize = 27;

fn soil_by_level() -> HashMap<u8, &'static str> {
    HashMap::from([(1, "Light"), (3, "Medium"), (5, "Heavy")])
}
fn status_temp() -> HashMap<u8, i64> {
    HashMap::from([(1, 20), (2, 30), (3, 40), (5, 60), (6, 95)])
}
fn status_spin() -> HashMap<u8, i64> {
    HashMap::from([(0, 0), (1, 400), (4, 800), (6, 1000), (8, 1200), (9, 1400)])
}
fn status_course() -> HashMap<u8, &'static str> {
    HashMap::from([
        (0x72, "AI Wash"), (0x2e, "Cotton"), (0x54, "Towels"), (0x4b, "Quick 14"),
        (0x4e, "Spin only"), (0x55, "Drum Clean"), (0x13, "Eco 40-60"), (0x7a, "TurboWash 39"),
        (0x2b, "Mixed"), (0x16, "Delicate"), (0x1d, "Easy Care"), (0x5e, "Hand/Wool"),
        (0x4f, "Activewear"), (0x04, "Allergy Care"), (0x1b, "Duvet"), (0x11, "Cold Wash"),
        (0x81, "Bedding"), (0xa9, "Cuffs & Collars"), (0x6a, "Rainy Days"), (0x42, "Silent Wash"),
        (0x07, "Baby Steam Care"), (0x73, "Down Jacket"), (0x88, "Microplastic Care"),
        (0x37, "Rinse + Spin"),
    ])
}
fn skyl_by_index() -> HashMap<u8, &'static str> {
    HashMap::from([
        (0, "None"), (1, "Normal"), (2, "Rinse +"), (3, "Rinse ++"),
        (4, "Rinse + Hold"), (5, "Rinse+ + Hold"),
    ])
}
fn status_map() -> HashMap<u8, &'static str> {
    HashMap::from([
        (0x01, "Detecting"), (0x02, "Paused"), (0x03, "Detecting"), (0x0b, "Washing"),
        (0x0c, "Rinsing"), (0x0e, "Spinning"), (0x10, "End"), (0x25, "Detecting"),
        (0x26, "Washing"), (0x27, "Rinsing"), (0x29, "Drum Clean"),
    ])
}

pub struct Device {
    core: Arc<AabbDeviceCore>,
}

impl Device {
    pub fn new(ha: Arc<dyn HaConnection>, thinq: Arc<dyn Thinq2Device>, meta: Metadata) -> Arc<Self> {
        let core = AabbDeviceCore::new(ha, thinq.clone());
        let this = Arc::new(Self { core: core.clone() });
        let mut base = default_config(&meta, Some(json!({"name": "LG Washer"})));
        let comps = [
            ("power", json!({"platform":"binary_sensor","unique_id":"$deviceid-power","state_topic":"$this/power","name":"Power","icon":"mdi:washing-machine","device_class":"running"})),
            ("status", json!({"platform":"sensor","unique_id":"$deviceid-status","state_topic":"$this/status","name":"Status","icon":"mdi:state-machine"})),
            ("course", json!({"platform":"sensor","unique_id":"$deviceid-course","state_topic":"$this/course","name":"Course","icon":"mdi:pin-outline"})),
            ("temp", json!({"platform":"sensor","unique_id":"$deviceid-temp","state_topic":"$this/temp","name":"Temperature","device_class":"temperature","unit_of_measurement":"°C","suggested_display_precision":0,"value_template":"{{ value if value | is_number else 'None' }}"})),
            ("spin", json!({"platform":"sensor","unique_id":"$deviceid-spin","state_topic":"$this/spin","name":"Spin","icon":"mdi:autorenew","unit_of_measurement":"RPM","value_template":"{{ value if value | is_number else 'None' }}"})),
            ("energy", json!({"platform":"sensor","unique_id":"$deviceid-energy","state_topic":"$this/energy","name":"Energy","icon":"mdi:lightning-bolt","device_class":"energy","state_class":"total_increasing","unit_of_measurement":"Wh"})),
            ("initial_time", json!({"platform":"sensor","unique_id":"$deviceid-initial_time","state_topic":"$this/initial_time","device_class":"duration","unit_of_measurement":"min","name":"Initial time"})),
            ("remaining_time", json!({"platform":"sensor","unique_id":"$deviceid-remaining_time","state_topic":"$this/remaining_time","device_class":"duration","unit_of_measurement":"min","name":"Remaining time"})),
            ("door", json!({"platform":"binary_sensor","unique_id":"$deviceid-door","state_topic":"$this/door","name":"Door","device_class":"door"})),
            ("detergent_dispenser", json!({"platform":"binary_sensor","unique_id":"$deviceid-detergent_dispenser","state_topic":"$this/detergent_dispenser","name":"Detergent dispenser","icon":"mdi:cup-water"})),
            ("softener_dispenser", json!({"platform":"binary_sensor","unique_id":"$deviceid-softener_dispenser","state_topic":"$this/softener_dispenser","name":"Softener dispenser","icon":"mdi:cup-water"})),
            ("detergent_dose", json!({"platform":"sensor","unique_id":"$deviceid-detergent_dose","state_topic":"$this/detergent_dose","name":"Detergent dose","icon":"mdi:cup","device_class":"volume","unit_of_measurement":"mL","state_class":"measurement"})),
            ("softener_dose", json!({"platform":"sensor","unique_id":"$deviceid-softener_dose","state_topic":"$this/softener_dose","name":"Softener dose","icon":"mdi:cup-outline","device_class":"volume","unit_of_measurement":"mL","state_class":"measurement"})),
            ("soil", json!({"platform":"sensor","unique_id":"$deviceid-soil","state_topic":"$this/soil","name":"Soil level","icon":"mdi:liquid-spot"})),
            ("rinse", json!({"platform":"sensor","unique_id":"$deviceid-rinse","state_topic":"$this/rinse","name":"Rinse","icon":"mdi:water-sync"})),
            ("prewash", json!({"platform":"binary_sensor","unique_id":"$deviceid-prewash","state_topic":"$this/prewash","name":"Pre-wash","icon":"mdi:water-sync"})),
            ("turbowash", json!({"platform":"binary_sensor","unique_id":"$deviceid-turbowash","state_topic":"$this/turbowash","name":"TurboWash","icon":"mdi:rocket-launch"})),
            ("steam", json!({"platform":"binary_sensor","unique_id":"$deviceid-steam","state_topic":"$this/steam","name":"Steam","icon":"mdi:kettle-steam"})),
            ("child_lock", json!({"platform":"binary_sensor","unique_id":"$deviceid-child_lock","state_topic":"$this/child_lock","name":"Child lock","icon":"mdi:lock","entity_category":"diagnostic"})),
            ("remote_start", json!({"platform":"binary_sensor","unique_id":"$deviceid-remote_start","state_topic":"$this/remote_start","name":"Remote start","icon":"mdi:play-circle-outline","entity_category":"diagnostic"})),
            ("tub_clean_count", json!({"platform":"sensor","unique_id":"$deviceid-tub_clean_count","state_topic":"$this/tub_clean_count","name":"Washes since drum clean","icon":"mdi:washing-machine-alert","entity_category":"diagnostic"})),
        ];
        let mut components = Map::new();
        for (k, v) in comps { components.insert(k.into(), v); }
        base.components = components.into_iter().collect();
        core.set_config(base);

        let t = this.clone();
        thinq.on_data(Box::new(move |data| {
            if let Some(inner) = t.core.process_data_envelope(data) {
                t.process_aabb(&inner);
            }
        }));
        this
    }

    fn process_door(&self, buf: &[u8]) {
        if buf.len() <= DOOR_OFFSET { return; }
        let v = buf[DOOR_OFFSET];
        if v == DOOR_OPEN { self.core.publish_property("door", "ON".into()); }
        else if v == DOOR_CLOSED { self.core.publish_property("door", "OFF".into()); }
    }

    fn process_config(&self, _buf: &[u8]) {
        self.core.publish_property("power", "ON".into());
    }

    fn publish_persistent(&self, rec: &[u8]) {
        self.core.publish_property("child_lock", if rec[FLAGS_BYTE] & FLAG_CHILD_LOCK != 0 { "ON" } else { "OFF" }.into());
        self.core.publish_property("remote_start", if rec[FLAGS_BYTE] & FLAG_REMOTE_START != 0 { "ON" } else { "OFF" }.into());
        self.core.publish_property("tub_clean_count", (rec[TUB_CLEAN_OFFSET] as i64).into());
    }

    fn process_status(&self, buf: &[u8]) {
        if buf.len() != STATUS_FRAME_LEN { return; }
        let rec = &buf[STATUS_RECORD_OFF..STATUS_RECORD_OFF + STATUS_RECORD_LEN];
        if rec[0] == RECORD_B_STANDBY && rec[20] == 0x00 {
            self.core.publish_property("power", "OFF".into());
            self.core.publish_property("status", "Off".into());
            self.core.publish_property("remaining_time", 0i64.into());
            self.core.publish_property("initial_time", 0i64.into());
            self.publish_persistent(rec);
            return;
        }
        self.core.publish_property("power", "ON".into());
        self.core.publish_property("status", status_map().get(&rec[20]).copied().unwrap_or("Running").into());
        self.core.publish_property("soil", soil_by_level().get(&rec[0]).copied().unwrap_or("unknown").into());
        self.core.publish_property("rinse", skyl_by_index().get(&rec[SKYL_OFFSET]).copied().unwrap_or("unknown").into());
        match status_temp().get(&rec[1]) {
            Some(n) => self.core.publish_property("temp", (*n).into()),
            None => self.core.publish_property("temp", "unknown".into()),
        }
        match status_spin().get(&rec[3]) {
            Some(n) => self.core.publish_property("spin", (*n).into()),
            None => self.core.publish_property("spin", "unknown".into()),
        }
        self.core.publish_property("course", status_course().get(&rec[4]).copied().unwrap_or("unknown").into());
        self.core.publish_property("remaining_time", (rec[13] as i64).into());
        self.core.publish_property("initial_time", (rec[15] as i64).into());
        self.core.publish_property("energy", (rec[16] as i64 * 256 + rec[17] as i64).into());
        self.core.publish_property("detergent_dispenser", if rec[DISP_DETERGENT_EN] == DISP_ON { "ON" } else { "OFF" }.into());
        self.core.publish_property("softener_dispenser", if rec[DISP_SOFTENER_EN] == DISP_ON { "ON" } else { "OFF" }.into());
        self.core.publish_property("detergent_dose", (rec[DISP_DETERGENT_ML] as i64).into());
        self.core.publish_property("softener_dose", (rec[DISP_SOFTENER_ML] as i64).into());
        self.core.publish_property("prewash", if rec[OPT_BYTE_A] & OPT_PREWASH != 0 { "ON" } else { "OFF" }.into());
        self.core.publish_property("turbowash", if rec[OPT_BYTE_A] & OPT_TURBOWASH != 0 { "ON" } else { "OFF" }.into());
        self.core.publish_property("steam", if rec[OPT_BYTE_B] & OPT_STEAM != 0 { "ON" } else { "OFF" }.into());
        self.publish_persistent(rec);
    }

    fn process_aabb(&self, buf: &[u8]) {
        if buf.len() < 6 || buf[0] != 0x20 { return; }
        match buf[3] {
            CONFIG_FRAME_TYPE => self.process_config(buf),
            STATUS_FRAME_TYPE => self.process_status(buf),
            DOOR_FRAME_TYPE => self.process_door(buf),
            _ => {}
        }
    }

    pub fn set_property(&self, _prop: &str, _mqtt_value: &str) {}
}

impl DeviceHandler for Device {
    fn id(&self) -> &str { &self.core.id }
    fn start(&self) {}
    fn drop_device(&self) { self.core.drop_device(); }
    fn set_property(&self, prop: &str, value: &str) { Device::set_property(self, prop, value); }
    fn publish_config(&self) {
        if let Some(cfg) = self.core.config.lock().clone() {
            self.core.ha.publish_property(&self.core.id, "availability", "online".into());
            self.core.ha.publish_config(&self.core.id, &cfg);
        }
    }
}

pub fn create(ha: Arc<dyn HaConnection>, thinq: Arc<dyn Thinq2Device>, meta: Metadata) -> Arc<dyn DeviceHandler> {
    Device::new(ha, thinq, meta)
}
