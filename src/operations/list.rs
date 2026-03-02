use super::InscriptionContent;
use anyhow::Result;
use serde_json::json;

// Listing UTXO amount. Must not be dust; storage mass is inversely proportional to value.
// Keep this sufficiently high to satisfy standardness rules on public nodes.
const LISTING_UTXO_SOMPI: u64 = 30_000_000; // 0.3 KAS

/// Build a "list" inscription to list a KNS domain for sale.
/// inscription_id: the inscription ID (format: <txid>i0) of the domain to list
pub fn build_list(inscription_id: &str) -> Result<InscriptionContent> {
    let content = json!({
        "op": "list",
        "p": "domain",
        "id": inscription_id,
    });
    Ok(InscriptionContent {
        json: serde_json::to_string(&content)?,
        fee_sompi: LISTING_UTXO_SOMPI,
    })
}
