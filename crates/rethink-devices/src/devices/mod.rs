//! Per-model device handler modules.
//!
//! To add a new device:
//! 1. Create a module here implementing `DeviceHandler`
//! 2. Register the modelId in `crate::registry`
//!
//! Module names intentionally track LG `modelId` strings (double underscores, etc.),
//! which are not always strict Rust snake_case.

#![allow(non_snake_case)]

pub mod washer_ctrl;

// AABB fridges
pub mod dev_2reb1glvb1__2;
pub mod dev_2ref11ebivpc4;
pub mod dev_2ref11eida__4;
pub mod dev_2ref12eii_p_2;
pub mod dev_2res1ve600fwc;
pub mod dev_2res1ve61nfa2;

// AABB laundry / dryer
pub mod f3l2cyu__;
pub mod f_v__f___w_b_1qeuk;
pub mod f_v8_y___w_b_2qeuk;
pub mod f_vb_f___w_b_2qeuk;
pub mod rh10v9_ch;
pub mod rv13b6bsd_d_us_wifi;
pub mod rv13u6am8w_d_us_wifi;
pub mod t1789efh_f;
pub mod vcdwl2qeuk;
pub mod wtl_fxu_bdv_na_01;
pub mod y_v8_f___w_b_2qeuk;
pub mod y_v8_y___w_b32qeuk;

// TLV climate / dehumidifier / humidifier / pot
pub mod dhum_056905_ww;
pub mod dhum_231006_ww;
pub mod hum_056905_ww;
pub mod pot_056905_ww;
pub mod rac_056905_ww;
pub mod win_056905_ww;

// AABB cooking / hood
pub mod studio_hood;

// ThinQ1
pub mod wtdn3;
