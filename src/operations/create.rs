use super::InscriptionContent;
use anyhow::Result;
use serde_json::json;
use unicode_segmentation::UnicodeSegmentation;

/// Domain length fees in KAS (1-char, 2-char, 3-char, 4-char, 5+-char)
const DEFAULT_FEES: [f64; 5] = [4200.0, 4200.0, 2100.0, 525.0, 35.0];
const SOMPI_PER_KAS: u64 = 100_000_000;

fn get_domain_fee_sompi(name: &str) -> u64 {
    let char_count = name.graphemes(true).count();
    let idx = if char_count == 0 {
        4
    } else {
        (char_count - 1).min(4)
    };
    (DEFAULT_FEES[idx] * SOMPI_PER_KAS as f64) as u64
}

/// Build a "create" inscription for a KNS domain.
/// name: the domain label (e.g. "alice" for alice.kas)
pub fn build_create(name: &str) -> Result<InscriptionContent> {
    let fee_sompi = get_domain_fee_sompi(name);
    let content = json!({
        "op": "create",
        "p": "domain",
        "v": name,
    });
    Ok(InscriptionContent {
        json: serde_json::to_string(&content)?,
        fee_sompi,
    })
}
