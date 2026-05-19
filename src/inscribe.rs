use anyhow::{anyhow, bail, Result};
use kaspa_addresses::{Address, Prefix};
use kaspa_consensus_core::{
    constants::MAX_TX_IN_SEQUENCE_NUM,
    hashing::sighash::{calc_schnorr_signature_hash, SigHashReusedValues},
    hashing::sighash_type::SIG_HASH_ALL,
    sign::sign,
    subnets::SUBNETWORK_ID_NATIVE,
    tx::{
        MutableTransaction, Transaction, TransactionInput, TransactionOutpoint, TransactionOutput,
        UtxoEntry,
    },
};
use kaspa_rpc_core::{api::rpc::RpcApi, RpcTransaction};
use kaspa_txscript::{
    opcodes::codes::*,
    script_builder::ScriptBuilder,
    standard::{
        extract_script_pub_key_address, pay_to_address_script, pay_to_script_hash_script,
        pay_to_script_hash_signature_script,
    },
};
use kaspa_wrpc_client::KaspaRpcClient;
use secp256k1::{Keypair, Message};
use std::sync::Arc;
use tracing::{debug, info};

use crate::tx_common::{select_utxos, wait_for_utxo_by_txid, DUST_THRESHOLD_SOMPI, SOMPI_PER_KAS};

const COMMIT_AMOUNT_SOMPI: u64 = 30_000_000; // 0.3 KAS — P2SH anchor; sized to keep storage mass ~33k grams
const MAX_SCRIPT_SIZE: usize = 520; // bytes

// Commit tx mass is dominated by storage mass: C / COMMIT_AMOUNT_SOMPI = 10^12 / 30M ≈ 33_333 grams.
// 40_000 gives a ~20% buffer above the theoretical value.
const COMMIT_TX_MASS_GRAMS: u64 = 40_000;
// Fallback feerate when get_fee_estimate() is unavailable (sompi/gram).
// At 10 sompi/gram: commit fee ≈ 440k sompi (0.0044 KAS), comparable to the old hardcoded value.
const FALLBACK_FEERATE: f64 = 10.0;
const MIN_FEE_SOMPI: u64 = 10_000; // 0.0001 KAS absolute floor

// ─── Fee helpers ─────────────────────────────────────────────────────────────

async fn fetch_feerate(rpc_api: &dyn RpcApi) -> f64 {
    match rpc_api.get_fee_estimate().await {
        Ok(est) if !est.normal_buckets.is_empty() => {
            let rate = est.normal_buckets[0].feerate;
            info!("Network feerate: {:.4} sompi/gram (sub-minute inclusion)", rate);
            rate
        }
        Ok(est) => {
            let rate = est.priority_bucket.feerate;
            info!("Network feerate: {:.4} sompi/gram (priority bucket; no normal buckets)", rate);
            rate
        }
        Err(e) => {
            info!("get_fee_estimate failed ({e}), using fallback {FALLBACK_FEERATE:.1} sompi/gram");
            FALLBACK_FEERATE
        }
    }
}

/// Estimate the compute mass (grams) of the reveal transaction.
/// Storage mass is negligible for reveal txs because the output value (domain fee) >> P2SH input value.
fn reveal_tx_mass_grams(redeem_script_len: usize, n_extra_inputs: usize, n_outputs: usize) -> u64 {
    const MASS_PER_BYTE: u64 = 1;
    const MASS_PER_SPK_BYTE: u64 = 10;
    const MASS_PER_SIG_OP: u64 = 1000;
    const OUT_SCRIPT_LEN: u64 = 35; // standard P2PKH script

    // P2SH sig script = raw_sig_bytes(66) + add_data(redeem_script).
    // add_data encodes as [OP_PUSHDATA1=0x4c][1-byte len][data] for scripts in 76-255 bytes.
    let p2sh_sig_script_len = 68u64 + redeem_script_len as u64;
    let p2pk_sig_script_len = 66u64; // standard Schnorr: [OP_DATA_65][64-byte sig][sighash]

    let tx_size = 94 // fixed header (version + counts + locktime + subnetwork + gas + payload)
        + (52 + p2sh_sig_script_len)                              // P2SH input
        + (52 + p2pk_sig_script_len) * n_extra_inputs as u64     // extra sender inputs
        + (18 + OUT_SCRIPT_LEN) * n_outputs as u64;              // outputs

    let spk_mass = (2 + OUT_SCRIPT_LEN) * n_outputs as u64 * MASS_PER_SPK_BYTE;
    let sig_ops_mass = (1 + n_extra_inputs as u64) * MASS_PER_SIG_OP; // 1 P2SH + extra

    tx_size * MASS_PER_BYTE + spk_mass + sig_ops_mass
}

fn calc_fee(mass_grams: u64, feerate: f64) -> u64 {
    ((mass_grams as f64 * feerate * 1.1).ceil() as u64).max(MIN_FEE_SOMPI)
}

// ─── Script builders ────────────────────────────────────────────────────────

/// Build the standard inscription script (text/JSON content):
///   <xonly_pubkey> OP_CHECKSIG OP_FALSE OP_IF <"kns"> OP_0 <json_bytes> OP_ENDIF
pub fn build_inscription_script(xonly_pubkey: &[u8; 32], json_content: &str) -> Result<Vec<u8>> {
    build_script_inner(xonly_pubkey, json_content.as_bytes(), None)
}

/// Build the mime-type inscription script (file content):
///   <xonly_pubkey> OP_CHECKSIG OP_FALSE OP_IF <"kns"> OP_1 OP_1 <mime_type> OP_0 <file_bytes> OP_ENDIF
pub fn build_inscription_script_with_mime(
    xonly_pubkey: &[u8; 32],
    data: &[u8],
    mime_type: &str,
) -> Result<Vec<u8>> {
    build_script_inner(xonly_pubkey, data, Some(mime_type))
}

fn build_script_inner(
    xonly_pubkey: &[u8; 32],
    data: &[u8],
    mime_type: Option<&str>,
) -> Result<Vec<u8>> {
    let mut b = ScriptBuilder::new();
    macro_rules! s {
        ($r:expr) => {
            $r.map_err(|e| anyhow!("script build error: {e}"))?
        };
    }

    s!(b.add_data(xonly_pubkey));
    s!(b.add_op(OpCheckSig));
    s!(b.add_op(OpFalse));
    s!(b.add_op(OpIf));
    s!(b.add_data(b"kns"));

    if let Some(mt) = mime_type {
        // NOTE: These are OP_1 pushes (not OP_DATA_1). Using add_i64(1) ensures
        // the opcode is 0x51 rather than a single-byte push.
        s!(b.add_i64(1)); // signals mime-type variant
        s!(b.add_i64(1)); // field count (1 mime field)
        s!(b.add_data(mt.as_bytes()));
    }

    s!(b.add_i64(0)); // content separator (OP_0)
    s!(b.add_data(data));
    s!(b.add_op(OpEndIf));

    let script = b.drain();
    if script.len() > MAX_SCRIPT_SIZE {
        bail!(
            "Script too large: {} bytes (max {}). Content may be too long.",
            script.len(),
            MAX_SCRIPT_SIZE
        );
    }
    Ok(script)
}

// ─── Public inscription entry points ─────────────────────────────────────────

/// Inscribe a general asset — delegates to `run_inscribe_with_script`.
pub async fn run_inscribe_asset(
    rpc: &Arc<KaspaRpcClient>,
    sender_address: &Address,
    keypair: &Keypair,
    xonly_pubkey: &[u8; 32],
    asset: &crate::operations::AssetInscription,
    pay_to: Option<&str>,
    prefix: Prefix,
) -> Result<String> {
    use crate::operations::AssetKind;
    match &asset.kind {
        AssetKind::Text(text) => {
            run_inscribe_with_script(
                rpc,
                sender_address,
                keypair,
                xonly_pubkey,
                build_inscription_script(xonly_pubkey, text)?,
                asset.fee_sompi,
                pay_to,
                prefix,
            )
            .await
        }
        AssetKind::File { bytes, mime_type } => {
            run_inscribe_with_script(
                rpc,
                sender_address,
                keypair,
                xonly_pubkey,
                build_inscription_script_with_mime(xonly_pubkey, bytes, mime_type)?,
                asset.fee_sompi,
                pay_to,
                prefix,
            )
            .await
        }
    }
}

/// Run the full inscribe flow: commit tx → poll for UTXO → reveal tx.
pub async fn run_inscribe(
    rpc: &Arc<KaspaRpcClient>,
    sender_address: &Address,
    keypair: &Keypair,
    xonly_pubkey: &[u8; 32],
    json_content: &str,
    fee_sompi: u64,
    pay_to: Option<&str>,
    prefix: Prefix,
) -> Result<String> {
    let redeem_script = build_inscription_script(xonly_pubkey, json_content)?;
    run_inscribe_with_script(
        rpc,
        sender_address,
        keypair,
        xonly_pubkey,
        redeem_script,
        fee_sompi,
        pay_to,
        prefix,
    )
    .await
}

// ─── Core commit + reveal flow ────────────────────────────────────────────────

async fn run_inscribe_with_script(
    rpc: &Arc<KaspaRpcClient>,
    sender_address: &Address,
    keypair: &Keypair,
    _xonly_pubkey: &[u8; 32],
    redeem_script: Vec<u8>,
    fee_sompi: u64,
    pay_to: Option<&str>,
    prefix: Prefix,
) -> Result<String> {
    debug!(
        "Redeem script ({} bytes): {}",
        redeem_script.len(),
        hex::encode(&redeem_script)
    );

    let p2sh_script_pubkey = pay_to_script_hash_script(&redeem_script);
    let p2sh_address = extract_script_pub_key_address(&p2sh_script_pubkey, prefix)
        .map_err(|e| anyhow!("Failed to get P2SH address: {e}"))?;
    info!("P2SH address: {}", p2sh_address);

    let sender_script_pubkey = pay_to_address_script(sender_address);
    let rpc_api = rpc.rpc_api();
    let feerate = fetch_feerate(rpc_api.as_ref()).await;

    // ── Commit transaction ────────────────────────────────────────────────
    let sender_utxos = rpc_api
        .get_utxos_by_addresses(vec![sender_address.clone()])
        .await?;
    if sender_utxos.is_empty() {
        bail!(
            "No UTXOs found for address {}.\nFund this address before inscribing.",
            sender_address
        );
    }

    let commit_fee = calc_fee(COMMIT_TX_MASS_GRAMS, feerate);
    info!(
        "Commit fee: {} sompi ({:.6} KAS, feerate={:.4} sompi/gram)",
        commit_fee,
        commit_fee as f64 / SOMPI_PER_KAS as f64,
        feerate
    );

    let (selected_utxos, total_input) =
        select_utxos(&sender_utxos, COMMIT_AMOUNT_SOMPI + commit_fee)?;

    let commit_inputs: Vec<TransactionInput> = selected_utxos
        .iter()
        .map(|u| {
            TransactionInput::new(
                TransactionOutpoint::new(u.outpoint.transaction_id, u.outpoint.index),
                vec![],
                MAX_TX_IN_SEQUENCE_NUM,
                1,
            )
        })
        .collect();

    let change_sompi = total_input.saturating_sub(COMMIT_AMOUNT_SOMPI + commit_fee);
    let mut commit_outputs = vec![TransactionOutput::new(
        COMMIT_AMOUNT_SOMPI,
        p2sh_script_pubkey.clone(),
    )];
    if change_sompi > DUST_THRESHOLD_SOMPI {
        commit_outputs.push(TransactionOutput::new(
            change_sompi,
            sender_script_pubkey.clone(),
        ));
    }

    let commit_utxo_entries: Vec<UtxoEntry> = selected_utxos
        .iter()
        .map(|u| UtxoEntry::from(u.utxo_entry.clone()))
        .collect();

    let commit_tx = Transaction::new(
        0,
        commit_inputs,
        commit_outputs,
        0,
        SUBNETWORK_ID_NATIVE,
        0,
        vec![],
    );
    let signable_commit = MutableTransaction::with_entries(commit_tx, commit_utxo_entries);
    let signed_commit = sign(signable_commit, *keypair);

    let commit_rpc_tx = RpcTransaction::from(signed_commit.tx.as_ref());
    let commit_tx_id = rpc_api.submit_transaction(commit_rpc_tx, false).await?;
    println!("  Commit tx: {}", commit_tx_id);

    // ── Wait for P2SH UTXO ───────────────────────────────────────────────
    println!("  Waiting for commit to mature (~10s)...");
    let p2sh_utxo = wait_for_utxo_by_txid(rpc_api.as_ref(), &p2sh_address, &commit_tx_id).await?;
    info!("P2SH UTXO found: {:?}", p2sh_utxo.outpoint);

    // ── Reveal transaction ───────────────────────────────────────────────
    // When fee_sompi = 0 (e.g. transfer/list), return the commit anchor to sender
    // to avoid a dust/storage-mass penalty on a 1-sompi output.
    let reveal_amount = if fee_sompi > 0 {
        fee_sompi
    } else {
        COMMIT_AMOUNT_SOMPI
    };

    let (pay_to_script, pay_to_display) = match pay_to.filter(|s| !s.is_empty()) {
        Some(addr_str) => {
            let pay_addr = Address::try_from(addr_str)
                .map_err(|e| anyhow!("Invalid pay-to address '{}': {}", addr_str, e))?;
            (pay_to_address_script(&pay_addr), pay_addr.to_string())
        }
        None => (sender_script_pubkey.clone(), sender_address.to_string()),
    };
    info!("Reveal output 0 pays to {}", pay_to_display);

    // 1 extra sender input (covers miner fee), 2 outputs (payment + change).
    let reveal_fee = calc_fee(reveal_tx_mass_grams(redeem_script.len(), 1, 2), feerate);
    info!(
        "Reveal fee: {} sompi ({:.6} KAS, feerate={:.4} sompi/gram)",
        reveal_fee,
        reveal_fee as f64 / SOMPI_PER_KAS as f64,
        feerate
    );

    // The P2SH UTXO (COMMIT_AMOUNT_SOMPI) contributes to the reveal tx.
    // Only fetch additional sender UTXOs if the shortfall requires it.
    let needed_from_sender = (reveal_amount + reveal_fee).saturating_sub(COMMIT_AMOUNT_SOMPI);
    let fresh_utxos = rpc_api
        .get_utxos_by_addresses(vec![sender_address.clone()])
        .await?;
    let (reveal_fee_utxos, reveal_fee_total) = if needed_from_sender > 0 {
        select_utxos(&fresh_utxos, needed_from_sender)?
    } else {
        (vec![], 0u64)
    };

    // P2SH input first (index 0), then any sender fee inputs
    let p2sh_input = TransactionInput::new(
        TransactionOutpoint::new(p2sh_utxo.outpoint.transaction_id, p2sh_utxo.outpoint.index),
        vec![],
        MAX_TX_IN_SEQUENCE_NUM,
        1,
    );
    let mut reveal_inputs = vec![p2sh_input];
    let mut reveal_utxo_entries = vec![UtxoEntry::from(p2sh_utxo.utxo_entry.clone())];

    for u in &reveal_fee_utxos {
        reveal_inputs.push(TransactionInput::new(
            TransactionOutpoint::new(u.outpoint.transaction_id, u.outpoint.index),
            vec![],
            MAX_TX_IN_SEQUENCE_NUM,
            1,
        ));
        reveal_utxo_entries.push(UtxoEntry::from(u.utxo_entry.clone()));
    }

    let total_reveal_input = COMMIT_AMOUNT_SOMPI + reveal_fee_total;
    let reveal_change = total_reveal_input.saturating_sub(reveal_amount + reveal_fee);
    let mut reveal_outputs = vec![TransactionOutput::new(reveal_amount, pay_to_script)];
    if reveal_change > DUST_THRESHOLD_SOMPI {
        reveal_outputs.push(TransactionOutput::new(
            reveal_change,
            sender_script_pubkey.clone(),
        ));
    }

    let reveal_tx = Transaction::new(
        0,
        reveal_inputs,
        reveal_outputs,
        0,
        SUBNETWORK_ID_NATIVE,
        0,
        vec![],
    );
    let mut signable_reveal = MutableTransaction::with_entries(reveal_tx, reveal_utxo_entries);

    // Sign regular inputs (index 1+)
    let mut reused = SigHashReusedValues::new();
    for i in 1..signable_reveal.tx.inputs.len() {
        signable_reveal.tx.inputs[i].sig_op_count = 1;
        let sig_hash = calc_schnorr_signature_hash(
            &signable_reveal.as_verifiable(),
            i,
            SIG_HASH_ALL,
            &mut reused,
        );
        let msg = Message::from_digest_slice(sig_hash.as_bytes().as_slice())
            .map_err(|e| anyhow!("Message error: {e}"))?;
        let sig: [u8; 64] = *keypair.sign_schnorr(msg).as_ref();
        signable_reveal.tx.inputs[i].signature_script = std::iter::once(65u8)
            .chain(sig)
            .chain([SIG_HASH_ALL.to_u8()])
            .collect();
    }

    // Sign P2SH input (index 0)
    signable_reveal.tx.inputs[0].sig_op_count = 1;
    let p2sh_sig_hash = calc_schnorr_signature_hash(
        &signable_reveal.as_verifiable(),
        0,
        SIG_HASH_ALL,
        &mut reused,
    );
    let p2sh_msg = Message::from_digest_slice(p2sh_sig_hash.as_bytes().as_slice())
        .map_err(|e| anyhow!("Message error: {e}"))?;
    let p2sh_sig: [u8; 64] = *keypair.sign_schnorr(p2sh_msg).as_ref();
    let signature_bytes: Vec<u8> = std::iter::once(65u8)
        .chain(p2sh_sig)
        .chain([SIG_HASH_ALL.to_u8()])
        .collect();
    signable_reveal.tx.inputs[0].signature_script =
        pay_to_script_hash_signature_script(redeem_script, signature_bytes)
            .map_err(|e| anyhow!("P2SH signature script error: {e}"))?;

    let reveal_rpc_tx = RpcTransaction::from(signable_reveal.tx.as_ref());
    let reveal_tx_id = rpc_api.submit_transaction(reveal_rpc_tx, false).await?;
    info!("Reveal tx: {}", reveal_tx_id);

    Ok(reveal_tx_id.to_string())
}
