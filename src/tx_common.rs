use anyhow::{bail, Result};
use kaspa_addresses::Address;
use kaspa_consensus_core::tx::TransactionId;
use kaspa_rpc_core::{api::rpc::RpcApi, RpcUtxosByAddressesEntry};

pub const SOMPI_PER_KAS: u64 = 100_000_000;

/// Outputs below this value are dropped rather than included as change.
pub const DUST_THRESHOLD_SOMPI: u64 = 10_000; // 0.0001 KAS

/// Select UTXOs greedily to cover the target amount.
pub fn select_utxos(
    utxos: &[RpcUtxosByAddressesEntry],
    target_sompi: u64,
) -> Result<(Vec<RpcUtxosByAddressesEntry>, u64)> {
    let mut selected = Vec::new();
    let mut total = 0u64;

    let mut sorted = utxos.to_vec();
    sorted.sort_by(|a, b| b.utxo_entry.amount.cmp(&a.utxo_entry.amount));

    for utxo in sorted {
        if total >= target_sompi {
            break;
        }
        total += utxo.utxo_entry.amount;
        selected.push(utxo);
    }

    if total < target_sompi {
        bail!(
            "Insufficient funds: have {:.4} KAS, need {:.4} KAS",
            total as f64 / SOMPI_PER_KAS as f64,
            target_sompi as f64 / SOMPI_PER_KAS as f64
        );
    }

    Ok((selected, total))
}

/// Poll until a UTXO for `tx_id` appears at `address`.
pub async fn wait_for_utxo_by_txid(
    rpc_api: &dyn RpcApi,
    address: &Address,
    tx_id: &TransactionId,
) -> Result<RpcUtxosByAddressesEntry> {
    let max_attempts = 120;
    for attempt in 0..max_attempts {
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        let utxos = rpc_api
            .get_utxos_by_addresses(vec![address.clone()])
            .await?;
        if let Some(utxo) = utxos
            .into_iter()
            .find(|u| &u.outpoint.transaction_id == tx_id)
        {
            return Ok(utxo);
        }
        if attempt % 15 == 14 {
            println!("  Still waiting... ({}/{})", attempt + 1, max_attempts);
        }
    }
    bail!(
        "Timed out waiting for UTXO at {} from tx {}",
        address,
        tx_id
    );
}
