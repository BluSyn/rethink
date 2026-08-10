//! Heuristic analysis of non-TLV UART payloads (UartBinary).
//!
//! Climate state is usually kind 0x87/0xa7/0x65 TLV. Other kinds share the same
//! UART envelope but carry fixed binary layouts (private commands, SUPERSET,
//! sensor/filter blobs). We cannot invent TLV tags for them, but we *can*:
//! - document the envelope (kind, b5–b7, len, CRC)
//! - scan the body for byte patterns that often encode temps / humidity / counters
//! - emit structured RE notes for LLM / human follow-up
//!
//! Goal: never discard binary frames; always attempt a best-effort reading.

use serde::Serialize;

/// Known / hypothesized UART kind families (from wiki + captures).
pub fn kind_hint(kind: u8) -> &'static str {
    match kind {
        0x65 => "toDevice climate TLV dialect",
        0x87 => "fromDevice climate TLV (classic)",
        0xa7 => "fromDevice climate TLV (CST/DHUM dialect)",
        0xa8 => "fromDevice non-TLV (seen on DHUM SUPERSET/private body)",
        0xfd => "often private-command path (filter etc.) when b5=0xfd",
        _ => "unclassified UART kind — capture + correlate with app/cloud",
    }
}

pub fn b5_hint(b5: u8) -> &'static str {
    match b5 {
        0x01 | 0x02 => "standard values/query band (with climate kinds)",
        0x03..=0x66 => "wiki SUPERSET band (non-standard body layouts common)",
        0xf0..=0xf9 => "wiki extended-TLV / special path band",
        0xfd => "private command / filter-style path (with matching b6)",
        _ => "see TLVProtocol wiki byte5 notes",
    }
}

pub fn b6_hint(b6: u8) -> &'static str {
    match b6 {
        0x01 => "often query / ACK-related on climate path",
        0x02 => "often command path on climate path",
        0x04 => "often values push (fromDevice climate)",
        0x10 => "ACK-like on some paths; with non-TLV kind may mean binary reply",
        _ => "model-specific",
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct UartEnvelope {
    pub kind: u8,
    pub byte5: u8,
    pub byte6: u8,
    pub byte7: u8,
    pub body_len: usize,
    pub crc_ok: Option<bool>,
    pub kind_hint: &'static str,
    pub b5_hint: &'static str,
    pub b6_hint: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct HeuristicHit {
    /// Byte offset within the **body** (not full packet).
    pub offset: usize,
    pub width: u8,
    pub endian: &'static str,
    pub raw: u32,
    pub interpretation: String,
    pub confidence: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct UartBinaryAnalysis {
    pub envelope: UartEnvelope,
    pub body_hex: String,
    pub body_len: usize,
    pub zero_bytes: usize,
    pub nonzero_bytes: usize,
    pub unique_bytes: usize,
    pub heuristics: Vec<HeuristicHit>,
    pub re_notes: Vec<String>,
}

/// Analyze a UART binary body with envelope metadata from the outer frame.
pub fn analyze_uart_binary(
    kind: u8,
    byte5: u8,
    byte6: u8,
    byte7: u8,
    body: &[u8],
    crc_ok: Option<bool>,
) -> UartBinaryAnalysis {
    let mut hist = [0u32; 256];
    let mut zero = 0usize;
    for &b in body {
        hist[b as usize] += 1;
        if b == 0 {
            zero += 1;
        }
    }
    let unique = hist.iter().filter(|&&c| c > 0).count();

    let mut heuristics = Vec::new();
    // Half-degree temps: wire 32–80 → 16–40 °C
    for (i, &b) in body.iter().enumerate() {
        if (32..=80).contains(&b) {
            let c = b as f64 / 2.0;
            heuristics.push(HeuristicHit {
                offset: i,
                width: 1,
                endian: "u8",
                raw: b as u32,
                interpretation: format!("plausible half-°C temperature → {c:.1} °C (like TLV 0x1fd)"),
                confidence: "low",
            });
        }
        // Humidity % for DHUM-style 0–100
        if (20..=100).contains(&b) && b % 5 == 0 {
            heuristics.push(HeuristicHit {
                offset: i,
                width: 1,
                endian: "u8",
                raw: b as u32,
                interpretation: format!("plausible humidity % (RH={b})"),
                confidence: "low",
            });
        }
    }
    // u16 LE / BE scans for humidity×10 (200–1000) and filter-like counters
    if body.len() >= 2 {
        for i in 0..body.len() - 1 {
            let le = u16::from_le_bytes([body[i], body[i + 1]]) as u32;
            let be = u16::from_be_bytes([body[i], body[i + 1]]) as u32;
            for (raw, endian) in [(le, "u16le"), (be, "u16be")] {
                if (200..=1000).contains(&raw) && raw % 10 == 0 {
                    heuristics.push(HeuristicHit {
                        offset: i,
                        width: 2,
                        endian,
                        raw,
                        interpretation: format!(
                            "plausible RH×10 → {:.1}% (like CST TLV 0x336)",
                            raw as f64 / 10.0
                        ),
                        confidence: "low",
                    });
                }
                if (500..=5000).contains(&raw) {
                    heuristics.push(HeuristicHit {
                        offset: i,
                        width: 2,
                        endian,
                        raw,
                        interpretation: format!(
                            "counter-scale value (filter hours / runtime / energy?)"
                        ),
                        confidence: "low",
                    });
                }
            }
        }
    }
    // Cap heuristic noise
    if heuristics.len() > 40 {
        heuristics.truncate(40);
        heuristics.push(HeuristicHit {
            offset: 0,
            width: 0,
            endian: "—",
            raw: 0,
            interpretation: "(truncated — many low-confidence hits; prefer paired captures)".into(),
            confidence: "info",
        });
    }

    let mut re_notes = vec![
        "This is NOT climate TLV. Do not invent 10-bit tags from the body.".into(),
        "Long-term goal: map each (kind,b5,b6) family to a struct layout per model.".into(),
        "Method: capture pairs before/after one app or panel action; diff body bytes.".into(),
        "Correlate with LG cloud decode (bridge) when available — labelled ground truth.".into(),
        "Once a field is stable, promote it to a named decoder + HA entity if useful.".into(),
    ];
    if kind == 0xa8 {
        re_notes.push(
            "kind 0xa8 seen on DHUM: SUPERSET/private blob; may embed sensor/filter-like fields."
                .into(),
        );
    }
    if byte5 >= 0x03 && byte5 <= 0x66 {
        re_notes.push(
            "b5 in SUPERSET band (0x03–0x66): expect fixed binary layouts, not values TLV.".into(),
        );
    }

    UartBinaryAnalysis {
        envelope: UartEnvelope {
            kind,
            byte5,
            byte6,
            byte7,
            body_len: body.len(),
            crc_ok,
            kind_hint: kind_hint(kind),
            b5_hint: b5_hint(byte5),
            b6_hint: b6_hint(byte6),
        },
        body_hex: hex::encode(body),
        body_len: body.len(),
        zero_bytes: zero,
        nonzero_bytes: body.len().saturating_sub(zero),
        unique_bytes: unique,
        heuristics,
        re_notes,
    }
}

/// Compact binary-frame breakdown (envelope + body + top heuristics).
pub fn uart_binary_export_text(
    model_id: Option<&str>,
    direction: Option<&str>,
    full_hex: &str,
    analysis: &UartBinaryAnalysis,
) -> String {
    let mut out = String::new();
    out.push_str("# ThinQ UART binary\n");
    if let Some(m) = model_id {
        out.push_str(&format!("modelId: {m}\n"));
    }
    if let Some(d) = direction {
        out.push_str(&format!("direction: {d}\n"));
    }
    let e = &analysis.envelope;
    out.push_str(&format!(
        "kind=0x{:02x} b5=0x{:02x} b6=0x{:02x} b7=0x{:02x} body_len={}\n",
        e.kind, e.byte5, e.byte6, e.byte7, e.body_len
    ));
    out.push_str(&format!("kind: {}\n", e.kind_hint));
    if let Some(c) = e.crc_ok {
        out.push_str(&format!("crc_ok: {c}\n"));
    }
    out.push_str(&format!("hex: {full_hex}\n"));
    out.push_str(&format!("body: {}\n", analysis.body_hex));
    out.push_str(&format!(
        "stats: zero={} nonzero={} unique={}\n",
        analysis.zero_bytes, analysis.nonzero_bytes, analysis.unique_bytes
    ));
    out.push_str("candidates:\n");
    let mut n = 0;
    for h in &analysis.heuristics {
        if h.width == 0 {
            continue;
        }
        // Prefer medium-signal hits; cap list
        if n >= 12 {
            out.push_str("  …\n");
            break;
        }
        out.push_str(&format!(
            "  +{} {} raw={} — {}\n",
            h.offset, h.endian, h.raw, h.interpretation
        ));
        n += 1;
    }
    if n == 0 {
        out.push_str("  (none)\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analyzes_dhum_a8_body() {
        let body = hex::decode(
            "0a010d10cf0111320200000000000001000000030100000000000000331e0007b81e0000000002260226024e365000fa00002100000000000000000222011e011c1e011e1e2f90bc00",
        )
        .unwrap();
        let a = analyze_uart_binary(0xa8, 0x66, 0x10, 0x01, &body, Some(true));
        assert_eq!(a.envelope.kind, 0xa8);
        assert!(!a.body_hex.is_empty());
        assert!(a.heuristics.iter().any(|h| h.interpretation.contains("half-°C") || h.interpretation.contains("RH")));
        let text = uart_binary_export_text(Some("DHUM_056905_WW"), Some("fromDevice"), "00", &a);
        assert!(text.contains("UART binary"));
        assert!(text.contains("kind=0xa8") || text.contains("0xa8"));
        assert!(text.contains("body:"));
        assert!(!text.to_lowercase().contains("please help"));
    }
}
