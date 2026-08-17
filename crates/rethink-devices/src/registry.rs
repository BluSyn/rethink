//! modelId → handler factory registry.
//!
//! Adding a new device is a localized change:
//! 1. Implement a handler module under `devices/`
//! 2. Add one line to `t1_factory` or `t2_factory` below

use crate::device_trait::{T1Factory, T2Factory};
use crate::devices;

/// ThinQ1 modelId → factory
pub fn t1_factory(model_id: &str) -> Option<T1Factory> {
    match model_id {
        "WTDN3" => Some(devices::wtdn3::create),
        _ => None,
    }
}

/// ThinQ2 modelId → factory (includes aliases that share a handler)
pub fn t2_factory(model_id: &str) -> Option<T2Factory> {
    match model_id {
        "POT_056905_WW" => Some(devices::pot_056905_ww::create),
        "RAC_056905_WW" | "RAC_0B0001_WW" | "CST_570004_WW" => Some(devices::rac_056905_ww::create),
        "WIN_056905_WW" => Some(devices::win_056905_ww::create),
        "2REF11EIDA__4" => Some(devices::dev_2ref11eida__4::create),
        "2REF11EBIVPC4" => Some(devices::dev_2ref11ebivpc4::create),
        "2REF12EII_P_2" => Some(devices::dev_2ref12eii_p_2::create),
        "2RES1VE61NFA2" => Some(devices::dev_2res1ve61nfa2::create),
        "2REB1GLVB1__2" => Some(devices::dev_2reb1glvb1__2::create),
        "2RES1VE600FWC" => Some(devices::dev_2res1ve600fwc::create),
        "Y_V8_Y___W.B32QEUK" => Some(devices::y_v8_y___w_b32qeuk::create),
        "F_V7_Y___W.B_2QEUK" | "F_V8_Y___W.B_2QEUK" | "F_V__Y___W.B_2QEUK" => {
            Some(devices::f_v8_y___w_b_2qeuk::create)
        }
        "Y_V8_F___W.B_2QEUK" => Some(devices::y_v8_f___w_b_2qeuk::create),
        "VCDWL2QEUK" => Some(devices::vcdwl2qeuk::create),
        "F_V__F___W.B_1QEUK" | "F_VA_F___W.B__QEUK" => Some(devices::f_v__f___w_b_1qeuk::create),
        "F_VB_F___W.B_2QEUK" => Some(devices::f_vb_f___w_b_2qeuk::create),
        "T1789EFH_F" => Some(devices::t1789efh_f::create),
        "T17A1EFHU_F" => Some(devices::t17a1efhu_f::create),
        "RV13U6AM8W_D_US_WIFI" => Some(devices::rv13u6am8w_d_us_wifi::create),
        "F3L2CYU__" => Some(devices::f3l2cyu__::create),
        "RV13B6BSD_D_US_WIFI" => Some(devices::rv13b6bsd_d_us_wifi::create),
        "RV13B6ES_D_US_WIFI" => Some(devices::rv13b6es_d_us_wifi::create),
        "RH10V9_CH" => Some(devices::rh10v9_ch::create),
        "WTL_FXU_BDV_NA_01" => Some(devices::wtl_fxu_bdv_na_01::create),
        "DHUM_056905_WW" => Some(devices::dhum_056905_ww::create),
        "DHUM_231006_WW" => Some(devices::dhum_231006_ww::create),
        "HUM_056905_WW" => Some(devices::hum_056905_ww::create),
        "STUDIO_HOOD" => Some(devices::studio_hood::create),
        // PR #123 model string variant (B__ vs B_2) — same F_V8 handler family
        "F_V7_Y___W.B__QEUK" => Some(devices::f_v8_y___w_b_2qeuk::create),
        _ => None,
    }
}

/// Lookup aliases used by ha_bridge / cloud.
pub fn lookup_t2(model_id: &str) -> Option<T2Factory> {
    t2_factory(model_id)
}

pub fn lookup_t1(model_id: &str) -> Option<T1Factory> {
    t1_factory(model_id)
}

/// All registered ThinQ2 modelIds (including aliases).
pub fn all_t2_model_ids() -> Vec<&'static str> {
    vec![
        "POT_056905_WW",
        "RAC_056905_WW",
        "RAC_0B0001_WW",
        "CST_570004_WW",
        "WIN_056905_WW",
        "2REF11EIDA__4",
        "2REF11EBIVPC4",
        "2REF12EII_P_2",
        "2RES1VE61NFA2",
        "2REB1GLVB1__2",
        "2RES1VE600FWC",
        "Y_V8_Y___W.B32QEUK",
        "F_V7_Y___W.B_2QEUK",
        "F_V8_Y___W.B_2QEUK",
        "Y_V8_F___W.B_2QEUK",
        "F_V__Y___W.B_2QEUK",
        "VCDWL2QEUK",
        "F_V__F___W.B_1QEUK",
        "F_VA_F___W.B__QEUK",
        "F_VB_F___W.B_2QEUK",
        "T1789EFH_F",
        "T17A1EFHU_F",
        "RV13U6AM8W_D_US_WIFI",
        "F3L2CYU__",
        "RV13B6BSD_D_US_WIFI",
        "RV13B6ES_D_US_WIFI",
        "RH10V9_CH",
        "WTL_FXU_BDV_NA_01",
        "DHUM_056905_WW",
        "DHUM_231006_WW",
        "HUM_056905_WW",
        "STUDIO_HOOD",
        "F_V7_Y___W.B__QEUK",
    ]
}

/// All registered ThinQ1 modelIds.
pub fn all_t1_model_ids() -> Vec<&'static str> {
    vec!["WTDN3"]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_t2_models_have_factories() {
        for id in all_t2_model_ids() {
            assert!(t2_factory(id).is_some(), "missing t2 factory for {id}");
        }
    }

    #[test]
    fn all_t1_models_have_factories() {
        for id in all_t1_model_ids() {
            assert!(t1_factory(id).is_some(), "missing t1 factory for {id}");
        }
    }

    #[test]
    fn pre_rewrite_aliases_registered() {
        assert!(t2_factory("RAC_0B0001_WW").is_some());
        assert!(t2_factory("CST_570004_WW").is_some());
        assert!(t2_factory("F_V7_Y___W.B_2QEUK").is_some());
        assert!(t2_factory("F_V__Y___W.B_2QEUK").is_some());
        assert!(t2_factory("F_VA_F___W.B__QEUK").is_some());
    }
}
