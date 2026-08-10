//! Pure protocol utilities for rethink — LG ThinQ de-cloud codecs.
//!
//! These modules have no I/O dependencies and are safe to unit-test in isolation.

pub mod crc16;
pub mod json_splitter;
pub mod length_prefixed_frame;
pub mod mtosp;
pub mod packet_codec;
pub mod tlv;
pub mod tlv_catalog;

pub use crc16::crc16;
pub use packet_codec::{decode_packet, encode_packet, Decoded, Direction, EncodeInput, Protocol};
pub use tlv::{build as tlv_build, parse as tlv_parse, Tlv};
