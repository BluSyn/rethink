//! Known ThinQ TLV tag catalog for RE tooling (unknown highlighting).

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct KnownTag {
    pub id: u16,
    pub name: &'static str,
    pub family: &'static str,
}

/// Tags observed across RAC/WIN/POT/DHUM/HUM and wiki annotations.
pub static KNOWN_TAGS: &[KnownTag] = &[
    KnownTag { id: 0x1f5, name: "query_type", family: "clip" },
    KnownTag { id: 0x1f7, name: "power", family: "climate" },
    KnownTag { id: 0x1f9, name: "op_mode", family: "climate" },
    KnownTag { id: 0x1fa, name: "fan_strength", family: "climate" },
    KnownTag { id: 0x1fd, name: "current_temp_half_c", family: "climate" },
    KnownTag { id: 0x1fe, name: "setpoint_temp", family: "climate" },
    KnownTag { id: 0x253, name: "target_humidity", family: "humidity" },
    KnownTag { id: 0x336, name: "current_humidity", family: "humidity" },
    KnownTag { id: 0x2da, name: "eeprom_checksum", family: "caps" },
    KnownTag { id: 0x2cc, name: "feature_bits_2cc", family: "caps" },
    KnownTag { id: 0x2cd, name: "feature_bits_2cd", family: "caps" },
    KnownTag { id: 0x2d3, name: "feature_bits_2d3", family: "caps" },
    KnownTag { id: 0x2d5, name: "cap_tag_2d5", family: "caps" },
    KnownTag { id: 0x2d6, name: "cap_tag_2d6", family: "caps" },
    KnownTag { id: 0x20d, name: "related_20d", family: "climate" },
    KnownTag { id: 0x20e, name: "autodry", family: "climate" },
    KnownTag { id: 0x20f, name: "airclean", family: "climate" },
    KnownTag { id: 0x21a, name: "jet_related", family: "climate" },
    KnownTag { id: 0x321, name: "filter_related", family: "climate" },
    KnownTag { id: 0x109, name: "night_mode", family: "humidifier" },
    KnownTag { id: 0x1e3, name: "product_status", family: "humidifier" },
    KnownTag { id: 0x1e4, name: "sleep_mode", family: "humidifier" },
    KnownTag { id: 0x1e6, name: "auto_operation", family: "humidifier" },
    KnownTag { id: 0x3e0, name: "mood_light", family: "humidifier" },
];

pub fn known_tag_name(id: u16) -> Option<&'static str> {
    KNOWN_TAGS.iter().find(|t| t.id == id).map(|t| t.name)
}

pub fn is_known_tag(id: u16) -> bool {
    known_tag_name(id).is_some()
}

#[derive(Debug, Clone, Serialize)]
pub struct ClassifiedTlv {
    pub t: u16,
    pub v: u32,
    pub known: bool,
    pub name: Option<&'static str>,
}

/// Classify a TLV list against the known catalog.
pub fn classify_tlvs(items: &[(u16, u32)]) -> Vec<ClassifiedTlv> {
    items
        .iter()
        .map(|(t, v)| {
            let name = known_tag_name(*t);
            ClassifiedTlv {
                t: *t,
                v: *v,
                known: name.is_some(),
                name,
            }
        })
        .collect()
}

/// Build LLM-oriented export text for reverse engineering.
pub fn llm_export_text(
    model_id: Option<&str>,
    direction: Option<&str>,
    hex_packet: &str,
    classified: &[ClassifiedTlv],
) -> String {
    let mut out = String::new();
    out.push_str("# ThinQ packet RE export (rethink)\n");
    if let Some(m) = model_id {
        out.push_str(&format!("modelId: {m}\n"));
    }
    if let Some(d) = direction {
        out.push_str(&format!("direction: {d}\n"));
    }
    out.push_str(&format!("hex: {hex_packet}\n\n"));
    out.push_str("## TLV elements\n");
    let mut unknowns = Vec::new();
    for c in classified {
        if c.known {
            out.push_str(&format!(
                "- 0x{:03x} ({}) = {}\n",
                c.t,
                c.name.unwrap_or("?"),
                c.v
            ));
        } else {
            out.push_str(&format!("- 0x{:03x} **UNKNOWN** = {}\n", c.t, c.v));
            unknowns.push(c);
        }
    }
    out.push_str("\n## Unknown tags (copy into an LLM)\n");
    if unknowns.is_empty() {
        out.push_str("(none)\n");
    } else {
        out.push_str(
            "Please help reverse-engineer these ThinQ TLV tags seen on the wire.\n\
             Context: LG ThinQ2 CLIP UART TLV (10-bit type, var length). Related known tags: power=0x1f7, mode=0x1f9, fan=0x1fa, humidity=0x336/0x253.\n\n",
        );
        for c in unknowns {
            out.push_str(&format!("- tag=0x{:03x} value={} (decimal)\n", c.t, c.v));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_known_and_unknown() {
        let c = classify_tlvs(&[(0x1f7, 1), (0x999, 42)]);
        assert!(c[0].known);
        assert_eq!(c[0].name, Some("power"));
        assert!(!c[1].known);
    }

    #[test]
    fn llm_export_includes_unknown_section() {
        let c = classify_tlvs(&[(0x1f7, 1), (0xabc, 9)]);
        let text = llm_export_text(Some("RAC_056905_WW"), Some("fromDevice"), "deadbeef", &c);
        assert!(text.contains("UNKNOWN"));
        assert!(text.contains("0xabc"));
        assert!(text.contains("RAC_056905_WW"));
        assert!(text.contains("copy into an LLM") || text.contains("reverse-engineer"));
    }
}
