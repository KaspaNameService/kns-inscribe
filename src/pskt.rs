use anyhow::{anyhow, bail, Result};
use kaspa_addresses::{Address, Prefix};
use kaspa_consensus_core::{
    constants::MAX_TX_IN_SEQUENCE_NUM,
    hashing::sighash::{calc_schnorr_signature_hash, SigHashReusedValues},
    hashing::sighash_type::{SigHashType, SIG_HASH_ALL, SIG_HASH_ANY_ONE_CAN_PAY, SIG_HASH_SINGLE},
    subnets::SUBNETWORK_ID_NATIVE,
    tx::{
        MutableTransaction, Transaction, TransactionId, TransactionInput, TransactionOutpoint,
        TransactionOutput, UtxoEntry,
    },
};
use kaspa_rpc_core::RpcTransaction;
use kaspa_txscript::standard::{
    extract_script_pub_key_address, pay_to_address_script, pay_to_script_hash_script,
    pay_to_script_hash_signature_script,
};
use secp256k1::{Keypair, Message};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{debug, info};

use crate::inscribe::build_inscription_script;
use crate::tx_common::{select_utxos, wait_for_utxo_by_txid, DUST_THRESHOLD_SOMPI};

// Hard-coded miner fee for the marketplace send transaction.
// (This is separate from the commit/reveal fees used for inscriptions.)
const SEND_TX_FEE_SOMPI: u64 = 50_000; // 0.0005 KAS

// Receiver marker output value.
// The indexer uses output[1]'s *address* as the receiver; the amount must be large enough
// to keep the transaction standard under storage-mass rules.
const RECEIVER_MARKER_SOMPI: u64 = 30_000_000; // 0.3 KAS

// Miner fee for listing cancellation transaction.
const CANCEL_TX_FEE_SOMPI: u64 = 50_000; // 0.0005 KAS

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PsktOutpoint {
    pub transaction_id: String,
    pub index: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnsPskt {
    pub version: u8,
    pub domain_id: String,
    pub p2sh_address: String,
    pub listing_outpoint: PsktOutpoint,
    pub listing_utxo_amount: String,
    pub seller_signature_script: String,
    pub seller_address: String,
    pub price_in_sompi: String,
}

pub fn derive_send_p2sh_address(
    prefix: Prefix,
    xonly_pubkey: &[u8; 32],
    domain_id: &str,
) -> Result<(Address, Vec<u8>)> {
    let send = crate::operations::build_send(domain_id)?;
    let redeem_script = build_inscription_script(xonly_pubkey, &send.json)?;
    let p2sh_script_pubkey = pay_to_script_hash_script(&redeem_script);
    let p2sh_address = extract_script_pub_key_address(&p2sh_script_pubkey, prefix)
        .map_err(|e| anyhow!("Failed to derive P2SH address: {e}"))?;
    Ok((p2sh_address, redeem_script))
}

pub async fn generate_pskt(
    rpc: &Arc<kaspa_wrpc_client::KaspaRpcClient>,
    prefix: Prefix,
    keypair: &Keypair,
    xonly_pubkey: &[u8; 32],
    seller_address: &Address,
    domain_id: &str,
    price_sompi: u64,
    listing_tx_id: &TransactionId,
) -> Result<KnsPskt> {
    let rpc_api = rpc.rpc_api();

    let (p2sh_address, send_redeem_script) =
        derive_send_p2sh_address(prefix, xonly_pubkey, domain_id)?;
    info!("Send P2SH address: {}", p2sh_address);

    let listing_utxo =
        wait_for_utxo_by_txid(rpc_api.as_ref(), &p2sh_address, listing_tx_id).await?;
    debug!("Listing UTXO outpoint: {:?}", listing_utxo.outpoint);

    // Build a partial transaction: listing utxo in -> seller price out.
    let input = TransactionInput::new(
        TransactionOutpoint::new(
            listing_utxo.outpoint.transaction_id,
            listing_utxo.outpoint.index,
        ),
        vec![],
        MAX_TX_IN_SEQUENCE_NUM,
        1,
    );
    let output = TransactionOutput::new(price_sompi, pay_to_address_script(seller_address));
    let tx = Transaction::new(
        0,
        vec![input],
        vec![output],
        0,
        SUBNETWORK_ID_NATIVE,
        0,
        vec![],
    );
    let entries = vec![UtxoEntry::from(listing_utxo.utxo_entry.clone())];
    let signable = MutableTransaction::with_entries(tx, entries);

    let sighash_val = SIG_HASH_SINGLE.to_u8() | SIG_HASH_ANY_ONE_CAN_PAY.to_u8();
    let sighash_type =
        SigHashType::from_u8(sighash_val).map_err(|e| anyhow!("Invalid sighash type: {e}"))?;

    let mut reused = SigHashReusedValues::new();
    let sig_hash =
        calc_schnorr_signature_hash(&signable.as_verifiable(), 0, sighash_type, &mut reused);
    let msg = Message::from_digest_slice(sig_hash.as_bytes().as_slice())
        .map_err(|e| anyhow!("Message error: {e}"))?;
    let sig: [u8; 64] = *keypair.sign_schnorr(msg).as_ref();

    let signature_bytes: Vec<u8> = std::iter::once(65u8)
        .chain(sig)
        .chain([sighash_type.to_u8()])
        .collect();
    let seller_signature_script =
        pay_to_script_hash_signature_script(send_redeem_script, signature_bytes)
            .map_err(|e| anyhow!("P2SH signature script error: {e}"))?;

    Ok(KnsPskt {
        version: 1,
        domain_id: domain_id.to_string(),
        p2sh_address: p2sh_address.to_string(),
        listing_outpoint: PsktOutpoint {
            transaction_id: listing_utxo.outpoint.transaction_id.to_string(),
            index: listing_utxo.outpoint.index,
        },
        listing_utxo_amount: listing_utxo.utxo_entry.amount.to_string(),
        seller_signature_script: hex::encode(seller_signature_script),
        seller_address: seller_address.to_string(),
        price_in_sompi: price_sompi.to_string(),
    })
}

pub async fn send_with_pskt(
    rpc: &Arc<kaspa_wrpc_client::KaspaRpcClient>,
    buyer_address: &Address,
    keypair: &Keypair,
    pskt: &KnsPskt,
    receiver_address: &Address,
) -> Result<String> {
    let rpc_api = rpc.rpc_api();

    if pskt.version != 1 {
        bail!("Unsupported PSKT version: {}", pskt.version);
    }

    let p2sh_address = Address::try_from(pskt.p2sh_address.as_str())
        .map_err(|e| anyhow!("Invalid p2shAddress '{}': {e}", pskt.p2sh_address))?;

    let listing_txid: TransactionId =
        pskt.listing_outpoint.transaction_id.parse().map_err(|e| {
            anyhow!(
                "Invalid listing txid '{}': {e}",
                pskt.listing_outpoint.transaction_id
            )
        })?;
    let listing_index: u32 = pskt.listing_outpoint.index;

    let seller_address = Address::try_from(pskt.seller_address.as_str())
        .map_err(|e| anyhow!("Invalid sellerAddress '{}': {e}", pskt.seller_address))?;

    let price_sompi: u64 = pskt
        .price_in_sompi
        .parse()
        .map_err(|e| anyhow!("Invalid priceInSompi '{}': {e}", pskt.price_in_sompi))?;

    let seller_signature_script = hex::decode(&pskt.seller_signature_script)
        .map_err(|e| anyhow!("Invalid sellerSignatureScript hex: {e}"))?;

    // Fetch the listing UTXO at the seller P2SH address
    let listing_utxos = rpc_api
        .get_utxos_by_addresses(vec![p2sh_address.clone()])
        .await?;
    let listing_utxo = listing_utxos
        .into_iter()
        .find(|u| u.outpoint.transaction_id == listing_txid && u.outpoint.index == listing_index)
        .ok_or_else(|| {
            anyhow!(
                "Listing UTXO not found at {} for outpoint {}:{}",
                p2sh_address,
                listing_txid,
                listing_index
            )
        })?;

    // Fetch buyer UTXOs for payment
    let buyer_utxos = rpc_api
        .get_utxos_by_addresses(vec![buyer_address.clone()])
        .await?;
    if buyer_utxos.is_empty() {
        bail!("No UTXOs found for buyer address {}", buyer_address);
    }

    // outputs: [seller price, receiver marker]
    let outputs_value = price_sompi
        .saturating_add(RECEIVER_MARKER_SOMPI)
        .saturating_add(SEND_TX_FEE_SOMPI);

    let needed_from_buyer = outputs_value.saturating_sub(listing_utxo.utxo_entry.amount);
    let (selected, selected_total) = if needed_from_buyer > 0 {
        select_utxos(&buyer_utxos, needed_from_buyer)?
    } else {
        (vec![], 0)
    };

    let mut inputs = Vec::with_capacity(1 + selected.len());
    let mut entries = Vec::with_capacity(1 + selected.len());

    inputs.push(TransactionInput::new(
        TransactionOutpoint::new(
            listing_utxo.outpoint.transaction_id,
            listing_utxo.outpoint.index,
        ),
        vec![],
        MAX_TX_IN_SEQUENCE_NUM,
        1,
    ));
    entries.push(UtxoEntry::from(listing_utxo.utxo_entry.clone()));

    for u in &selected {
        inputs.push(TransactionInput::new(
            TransactionOutpoint::new(u.outpoint.transaction_id, u.outpoint.index),
            vec![],
            MAX_TX_IN_SEQUENCE_NUM,
            1,
        ));
        entries.push(UtxoEntry::from(u.utxo_entry.clone()));
    }

    let mut outputs = vec![
        TransactionOutput::new(price_sompi, pay_to_address_script(&seller_address)),
        TransactionOutput::new(
            RECEIVER_MARKER_SOMPI,
            pay_to_address_script(receiver_address),
        ),
    ];

    let total_input = listing_utxo
        .utxo_entry
        .amount
        .saturating_add(selected_total);
    let change =
        total_input.saturating_sub(price_sompi + RECEIVER_MARKER_SOMPI + SEND_TX_FEE_SOMPI);
    if change > DUST_THRESHOLD_SOMPI {
        outputs.push(TransactionOutput::new(
            change,
            pay_to_address_script(buyer_address),
        ));
    }

    let tx = Transaction::new(0, inputs, outputs, 0, SUBNETWORK_ID_NATIVE, 0, vec![]);
    let mut signable = MutableTransaction::with_entries(tx, entries);

    // Sign buyer inputs (index 1+) with SIG_HASH_ALL
    let mut reused = SigHashReusedValues::new();
    for i in 1..signable.tx.inputs.len() {
        signable.tx.inputs[i].sig_op_count = 1;
        let sig_hash =
            calc_schnorr_signature_hash(&signable.as_verifiable(), i, SIG_HASH_ALL, &mut reused);
        let msg = Message::from_digest_slice(sig_hash.as_bytes().as_slice())
            .map_err(|e| anyhow!("Message error: {e}"))?;
        let sig: [u8; 64] = *keypair.sign_schnorr(msg).as_ref();
        signable.tx.inputs[i].signature_script = std::iter::once(65u8)
            .chain(sig)
            .chain([SIG_HASH_ALL.to_u8()])
            .collect();
    }

    // Fill listing input (index 0) with seller's pre-signed signature script
    signable.tx.inputs[0].signature_script = seller_signature_script;
    signable.tx.inputs[0].sig_op_count = 1;

    let rpc_tx = RpcTransaction::from(signable.tx.as_ref());
    let tx_id = rpc_api.submit_transaction(rpc_tx, false).await?;
    Ok(tx_id.to_string())
}

/// Cancel a marketplace listing by spending the listing UTXO with the embedded `send` inscription,
/// but producing only a single output.
///
/// Indexers interpret `send` transactions with <2 outputs as a cancellation.
pub async fn cancel_listing_with_pskt(
    rpc: &Arc<kaspa_wrpc_client::KaspaRpcClient>,
    prefix: Prefix,
    xonly_pubkey: &[u8; 32],
    payout_address: &Address,
    keypair: &Keypair,
    pskt: &KnsPskt,
) -> Result<String> {
    let rpc_api = rpc.rpc_api();

    if pskt.version != 1 {
        bail!("Unsupported PSKT version: {}", pskt.version);
    }

    // Re-derive the redeem script for the embedded `send` inscription.
    // This must match the listing P2SH.
    let (derived_p2sh, redeem_script) =
        derive_send_p2sh_address(prefix, xonly_pubkey, &pskt.domain_id)?;
    if derived_p2sh.to_string() != pskt.p2sh_address {
        bail!(
            "PSKT P2SH mismatch: pskt={}, derived={}",
            pskt.p2sh_address,
            derived_p2sh
        );
    }

    let p2sh_address = derived_p2sh;
    let listing_txid: TransactionId =
        pskt.listing_outpoint.transaction_id.parse().map_err(|e| {
            anyhow!(
                "Invalid listing txid '{}': {e}",
                pskt.listing_outpoint.transaction_id
            )
        })?;
    let listing_index: u32 = pskt.listing_outpoint.index;

    // Fetch the listing UTXO at the P2SH address
    let listing_utxos = rpc_api
        .get_utxos_by_addresses(vec![p2sh_address.clone()])
        .await?;
    let listing_utxo = listing_utxos
        .into_iter()
        .find(|u| u.outpoint.transaction_id == listing_txid && u.outpoint.index == listing_index)
        .ok_or_else(|| {
            anyhow!(
                "Listing UTXO not found at {} for outpoint {}:{}",
                p2sh_address,
                listing_txid,
                listing_index
            )
        })?;

    if listing_utxo.utxo_entry.amount <= CANCEL_TX_FEE_SOMPI {
        bail!(
            "Listing UTXO too small to cancel: {} sompi",
            listing_utxo.utxo_entry.amount
        );
    }

    // Single-output cancellation: return funds to payout address, no receiver marker.
    let out_value = listing_utxo.utxo_entry.amount - CANCEL_TX_FEE_SOMPI;

    let input = TransactionInput::new(
        TransactionOutpoint::new(
            listing_utxo.outpoint.transaction_id,
            listing_utxo.outpoint.index,
        ),
        vec![],
        MAX_TX_IN_SEQUENCE_NUM,
        1,
    );
    let output = TransactionOutput::new(out_value, pay_to_address_script(payout_address));
    let tx = Transaction::new(
        0,
        vec![input],
        vec![output],
        0,
        SUBNETWORK_ID_NATIVE,
        0,
        vec![],
    );
    let entries = vec![UtxoEntry::from(listing_utxo.utxo_entry.clone())];
    let mut signable = MutableTransaction::with_entries(tx, entries);

    let mut reused = SigHashReusedValues::new();
    let sig_hash =
        calc_schnorr_signature_hash(&signable.as_verifiable(), 0, SIG_HASH_ALL, &mut reused);
    let msg = Message::from_digest_slice(sig_hash.as_bytes().as_slice())
        .map_err(|e| anyhow!("Message error: {e}"))?;
    let sig: [u8; 64] = *keypair.sign_schnorr(msg).as_ref();

    let signature_bytes: Vec<u8> = std::iter::once(65u8)
        .chain(sig)
        .chain([SIG_HASH_ALL.to_u8()])
        .collect();
    signable.tx.inputs[0].signature_script =
        pay_to_script_hash_signature_script(redeem_script, signature_bytes)
            .map_err(|e| anyhow!("P2SH signature script error: {e}"))?;
    signable.tx.inputs[0].sig_op_count = 1;

    let rpc_tx = RpcTransaction::from(signable.tx.as_ref());
    let tx_id = rpc_api.submit_transaction(rpc_tx, false).await?;
    Ok(tx_id.to_string())
}
