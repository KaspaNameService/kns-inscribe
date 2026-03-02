use super::InscriptionContent;
use anyhow::Result;
use serde_json::json;

/// Build a "send" inscription to purchase a listed KNS domain.
/// inscription_id: the inscription ID (format: <txid>i0) of the listed domain
pub fn build_send(inscription_id: &str) -> Result<InscriptionContent> {
    let content = json!({
        "op": "send",
        "id": inscription_id,
    });
    Ok(InscriptionContent {
        json: serde_json::to_string(&content)?,
        fee_sompi: 0,
    })
}
