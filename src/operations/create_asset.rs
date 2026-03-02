use anyhow::{anyhow, Result};

const SOMPI_PER_KAS: u64 = 100_000_000;
const GENERAL_FEE_KAS: f64 = 1.0;

/// Build an inscription for a general (non-domain) asset.
///
/// - `protocol`: protocol namespace (expected to be "asset")
/// - `value`: text value to inscribe (stored as raw text, not wrapped in JSON)
/// - `file`: optional file path; if provided, reads the file as bytes
///
/// Returns `InscriptionContent` where:
/// - `.json` holds either the JSON payload (text mode) or a placeholder (file mode)
/// - `.mime_type` is `Some(...)` when inscribing a file
/// - `.file_bytes` is `Some(...)` when inscribing a file
pub fn build_create_asset(
    protocol: &str,
    value: Option<&str>,
    file: Option<&str>,
) -> Result<AssetInscription> {
    if protocol != "asset" {
        return Err(anyhow!("Unsupported asset protocol '{protocol}'"));
    }
    let fee_sompi = (GENERAL_FEE_KAS * SOMPI_PER_KAS as f64) as u64;

    if let Some(path) = file {
        let bytes =
            std::fs::read(path).map_err(|e| anyhow!("Cannot read file '{}': {}", path, e))?;
        let mime_type = mime_guess::from_path(path)
            .first_or_octet_stream()
            .to_string();
        return Ok(AssetInscription {
            fee_sompi,
            kind: AssetKind::File { bytes, mime_type },
        });
    }

    Ok(AssetInscription {
        fee_sompi,
        kind: AssetKind::Text(
            value
                .ok_or_else(|| anyhow!("Either a value or --file is required for create-asset"))?
                .to_string(),
        ),
    })
}

pub enum AssetKind {
    Text(String),
    File { bytes: Vec<u8>, mime_type: String },
}

pub struct AssetInscription {
    pub fee_sompi: u64,
    pub kind: AssetKind,
}
