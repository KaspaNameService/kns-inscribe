use super::InscriptionContent;
use anyhow::Result;
use serde_json::json;

/// Build an "addProfile" inscription to add or update a domain profile key/value.
///
/// - `inscription_id`: domain inscription ID (format: <txid>i0)
/// - `key`: profile key (e.g. "telegram", "twitter")
/// - `value`: profile value (defaults to empty string)
pub fn build_add_profile(
    inscription_id: &str,
    key: &str,
    value: Option<&str>,
) -> Result<InscriptionContent> {
    let content = json!({
        "op": "addProfile",
        "id": inscription_id,
        "key": key,
        "value": value.unwrap_or(""),
    });

    Ok(InscriptionContent {
        json: serde_json::to_string(&content)?,
        // V2 profiles require no fee.
        fee_sompi: 0,
    })
}
