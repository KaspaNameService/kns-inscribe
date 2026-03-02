use super::InscriptionContent;
use anyhow::Result;
use serde_json::json;

/// Build a "transfer" inscription for a KNS domain.
/// inscription_id: the inscription ID (format: <txid>i0)
/// to: recipient Kaspa address
pub fn build_transfer(inscription_id: &str, to: &str) -> Result<InscriptionContent> {
    build_transfer_with_protocol("domain", inscription_id, to)
}

/// Build a "transfer" inscription for any supported protocol namespace.
pub fn build_transfer_with_protocol(
    protocol: &str,
    inscription_id: &str,
    to: &str,
) -> Result<InscriptionContent> {
    let content = json!({
        "op": "transfer",
        "p": protocol,
        "id": inscription_id,
        "to": to,
    });
    Ok(InscriptionContent {
        json: serde_json::to_string(&content)?,
        fee_sompi: 0,
    })
}
