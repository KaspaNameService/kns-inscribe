mod config;
mod inscribe;
mod operations;
mod pskt;
mod tx_common;

use anyhow::{anyhow, bail, Result};
use clap::{Parser, Subcommand};
use config::Config;
use inscribe::{run_inscribe, run_inscribe_asset};
use kaspa_addresses::{Address, Prefix, Version};
use kaspa_consensus_core::network::NetworkType;
use kaspa_consensus_core::tx::TransactionId;
use kaspa_wrpc_client::{prelude::*, KaspaRpcClient};
use secp256k1::{Keypair, Secp256k1, SecretKey};
use std::str::FromStr;
use std::sync::Arc;

#[derive(Parser)]
#[command(
    name = "kns-inscribe",
    about = "CLI for inscribing on the Kaspa Name Service (KNS)",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new inscription. Protocol defaults to "domain"; use --protocol asset for a general asset.
    Create {
        /// Value to inscribe — a domain label (e.g. "alice") for the domain protocol,
        /// or any text value for a general asset. Omit when using --file.
        value: Option<String>,
        /// Protocol namespace. Defaults to "domain". Use "asset" for a general asset.
        #[arg(long, default_value = "domain")]
        protocol: String,
        /// Path to a file to inscribe as an asset (MIME type detected automatically).
        /// Only valid when --protocol is "asset".
        #[arg(long)]
        file: Option<String>,
        /// Override the payment address for the domain registration fee (domain protocol only).
        #[arg(long)]
        pay_to: Option<String>,
        /// Override the inscription fee in KAS (e.g. 0.5). Defaults to the protocol-defined fee.
        #[arg(long)]
        fee: Option<f64>,
    },

    /// Transfer a domain to another address
    Transfer {
        /// Inscription ID of the domain (format: <txid>i0)
        inscription_id: String,

        /// Protocol namespace. Defaults to "domain". Use "asset" to transfer an asset inscription.
        #[arg(long, default_value = "domain")]
        protocol: String,

        /// Recipient Kaspa address
        #[arg(long)]
        to: String,
    },

    /// List a domain for sale on the KNS marketplace
    List {
        /// Inscription ID of the domain to list (format: <txid>i0)
        inscription_id: String,

        /// Price in KAS.
        #[arg(long)]
        price: f64,

        /// If set, write the generated PSKT JSON to this path.
        #[arg(long)]
        pskt_out: Option<String>,
    },

    /// Add or update a domain profile key/value
    Profile {
        /// Inscription ID of the domain (format: <txid>i0)
        inscription_id: String,

        /// Profile key (e.g. telegram, twitter)
        key: String,

        /// Profile value (defaults to empty string)
        value: Option<String>,
    },

    /// Buy a listed domain (purchase / send payment)
    Send {
        /// PSKT JSON (from `list --price ...`). You may also pass a file with @path,
        /// e.g. `--pskt @./pskt.json`.
        #[arg(long)]
        pskt: String,

        /// Receiver address for the domain. Defaults to the buyer (sender) address.
        #[arg(long)]
        to: Option<String>,
    },

    /// Cancel a PSKT listing (spend the listing UTXO with a cancel-style send)
    Cancel {
        /// PSKT JSON (from `list --price ...`). You may also pass a file with @path,
        /// e.g. `--pskt @./pskt.json`.
        #[arg(long)]
        pskt: String,

        /// Address to receive the returned listing UTXO funds.
        /// Defaults to the sender (your wallet) address.
        #[arg(long)]
        to: Option<String>,
    },
}

enum Completed {
    Inscription { reveal_tx_id: String },
    Transaction { tx_id: String },
}

fn format_inscription_id(reveal_tx_id: &str) -> String {
    // KNS inscription IDs use an 'i' delimiter between txid and output index.
    // The index is always 0 for this CLI.
    format!("{reveal_tx_id}i0")
}

fn read_inline_or_at_file(value: &str) -> Result<String> {
    if let Some(path) = value.strip_prefix('@') {
        Ok(std::fs::read_to_string(path)
            .map_err(|e| anyhow!("Failed to read file '{path}': {e}"))?)
    } else {
        Ok(value.to_string())
    }
}

fn network_to_prefix(network_type: NetworkType) -> Prefix {
    match network_type {
        NetworkType::Mainnet => Prefix::Mainnet,
        NetworkType::Testnet => Prefix::Testnet,
        NetworkType::Simnet => Prefix::Simnet,
        NetworkType::Devnet => Prefix::Devnet,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("kns_inscribe=info".parse()?),
        )
        .init();

    let cli = Cli::parse();
    let config = Config::from_env()?;

    // Parse private key and derive keypair
    let secret_key = SecretKey::from_str(&config.private_key_hex)
        .map_err(|e| anyhow::anyhow!("Invalid PRIVATE_KEY: {e}"))?;
    let secp = Secp256k1::new();
    let keypair = Keypair::from_secret_key(&secp, &secret_key);
    let (xonly_pubkey, _parity) = keypair.x_only_public_key();
    let xonly_bytes: [u8; 32] = xonly_pubkey.serialize();

    let prefix = network_to_prefix(config.network_id.network_type);
    let sender_address = Address::new(prefix, Version::PubKey, &xonly_bytes);

    println!("Sender address: {}", sender_address);
    println!("Network:        {}", config.network_id);
    println!("RPC endpoint:   {}", config.rpc_url);

    // Connect to Kaspa node via wRPC
    let rpc = Arc::new(KaspaRpcClient::new(
        WrpcEncoding::Borsh,
        Some(&config.rpc_url),
        None,
        Some(config.network_id),
        None,
    )?);
    rpc.connect(Some(ConnectOptions::default())).await?;
    println!("Connected to Kaspa node.\n");

    let completed = match &cli.command {
        Commands::Create {
            value,
            protocol,
            file,
            pay_to,
            fee,
        } => {
            let fee_override = fee.map(|kas| (kas * 100_000_000.0) as u64);
            if protocol == "domain" {
                let name = value
                    .as_deref()
                    .ok_or_else(|| anyhow!("A domain name is required for the domain protocol"))?;
                let mut inscription = operations::build_create(name)?;
                if let Some(f) = fee_override {
                    inscription.fee_sompi = f;
                }
                println!("Operation: create domain '{}'", name);
                println!(
                    "Registration fee: {:.4} KAS",
                    inscription.fee_sompi as f64 / 100_000_000.0
                );
                let pt = pay_to.as_deref().or(if config.funds_address.is_empty() {
                    None
                } else {
                    Some(config.funds_address.as_str())
                });
                println!("Inscription JSON: {}\n", inscription.json);
                let reveal_tx_id = run_inscribe(
                    &rpc,
                    &sender_address,
                    &keypair,
                    &xonly_bytes,
                    &inscription.json,
                    inscription.fee_sompi,
                    pt,
                    prefix,
                )
                .await?;
                Completed::Inscription { reveal_tx_id }
            } else if protocol == "asset" {
                if file.is_some() && value.is_some() {
                    bail!("Specify either a value or --file, not both");
                }
                if pay_to.is_some() {
                    bail!("--pay-to is only supported for --protocol domain. For assets, set KNS_FUNDS_ADDRESS instead.");
                }
                let mut asset =
                    operations::build_create_asset(protocol, value.as_deref(), file.as_deref())?;
                if let Some(f) = fee_override {
                    asset.fee_sompi = f;
                }
                println!("Operation: create asset (protocol: {:?})", protocol);
                println!("Fee: {:.4} KAS", asset.fee_sompi as f64 / 100_000_000.0);
                let pt = if config.funds_address.is_empty() {
                    None
                } else {
                    Some(config.funds_address.as_str())
                };
                let reveal_tx_id = run_inscribe_asset(
                    &rpc,
                    &sender_address,
                    &keypair,
                    &xonly_bytes,
                    &asset,
                    pt,
                    prefix,
                )
                .await?;
                Completed::Inscription { reveal_tx_id }
            } else {
                bail!(
                    "Unsupported protocol '{protocol}'. Use --protocol domain or --protocol asset"
                );
            }
        }
        Commands::Transfer {
            inscription_id,
            protocol,
            to,
        } => {
            if protocol != "domain" && protocol != "asset" {
                bail!(
                    "Unsupported protocol '{protocol}'. Use --protocol domain or --protocol asset"
                );
            }
            let inscription = if protocol == "domain" {
                operations::build_transfer(inscription_id, to)?
            } else {
                operations::build_transfer_with_protocol(protocol, inscription_id, to)?
            };
            println!(
                "Operation: transfer {} ({}) to {}",
                inscription_id, protocol, to
            );
            println!("Inscription JSON: {}\n", inscription.json);
            let reveal_tx_id = run_inscribe(
                &rpc,
                &sender_address,
                &keypair,
                &xonly_bytes,
                &inscription.json,
                inscription.fee_sompi,
                None,
                prefix,
            )
            .await?;
            Completed::Inscription { reveal_tx_id }
        }
        Commands::List {
            inscription_id,
            price,
            pskt_out,
        } => {
            let inscription = operations::build_list(inscription_id)?;

            let price_sompi = (*price * 100_000_000.0) as u64;
            let (p2sh_address, _redeem_script) =
                pskt::derive_send_p2sh_address(prefix, &xonly_bytes, inscription_id)?;
            let p2sh_address_str = p2sh_address.to_string();

            println!("Operation: list {} (PSKT listing)", inscription_id);
            println!("Price: {:.4} KAS", price);
            println!("Listing P2SH: {}", p2sh_address_str);
            println!("Inscription JSON: {}\n", inscription.json);

            let reveal_tx_id = run_inscribe(
                &rpc,
                &sender_address,
                &keypair,
                &xonly_bytes,
                &inscription.json,
                inscription.fee_sompi,
                Some(p2sh_address_str.as_str()),
                prefix,
            )
            .await?;

            let reveal_txid: TransactionId = reveal_tx_id
                .parse()
                .map_err(|e| anyhow!("Invalid reveal tx id '{reveal_tx_id}': {e}"))?;

            let pskt = pskt::generate_pskt(
                &rpc,
                prefix,
                &keypair,
                &xonly_bytes,
                &sender_address,
                inscription_id,
                price_sompi,
                &reveal_txid,
            )
            .await?;

            println!("\nPSKT (share with buyers):");
            let pskt_json = serde_json::to_string_pretty(&pskt)?;
            println!("{}", pskt_json);

            if let Some(path) = pskt_out {
                std::fs::write(path, &pskt_json)
                    .map_err(|e| anyhow!("Failed to write PSKT to '{path}': {e}"))?;
                println!("\nWrote PSKT to: {}", path);
            }

            Completed::Inscription { reveal_tx_id }
        }
        Commands::Send { pskt: pskt_arg, to } => {
            let pskt_json = read_inline_or_at_file(pskt_arg)?;
            let pskt: pskt::KnsPskt =
                serde_json::from_str(&pskt_json).map_err(|e| anyhow!("Invalid PSKT JSON: {e}"))?;

            let receiver_address = match to.as_deref() {
                Some(addr) => Address::try_from(addr)
                    .map_err(|e| anyhow!("Invalid receiver address '{addr}': {e}"))?,
                None => sender_address.clone(),
            };

            println!("Operation: send (marketplace PSKT purchase)");
            println!("Receiver:  {}", receiver_address);
            let tx_id =
                pskt::send_with_pskt(&rpc, &sender_address, &keypair, &pskt, &receiver_address)
                    .await?;

            Completed::Transaction { tx_id }
        }

        Commands::Profile {
            inscription_id,
            key,
            value,
        } => {
            let inscription = operations::build_add_profile(inscription_id, key, value.as_deref())?;
            println!(
                "Operation: profile {} set {}={}",
                inscription_id,
                key,
                value.clone().unwrap_or_default()
            );
            println!("Inscription JSON: {}\n", inscription.json);
            let reveal_tx_id = run_inscribe(
                &rpc,
                &sender_address,
                &keypair,
                &xonly_bytes,
                &inscription.json,
                inscription.fee_sompi,
                None,
                prefix,
            )
            .await?;
            Completed::Inscription { reveal_tx_id }
        }

        Commands::Cancel { pskt: pskt_arg, to } => {
            let pskt_json = read_inline_or_at_file(pskt_arg)?;
            let pskt: pskt::KnsPskt =
                serde_json::from_str(&pskt_json).map_err(|e| anyhow!("Invalid PSKT JSON: {e}"))?;

            let payout_address = match to.as_deref() {
                Some(addr) => Address::try_from(addr)
                    .map_err(|e| anyhow!("Invalid payout address '{addr}': {e}"))?,
                None => sender_address.clone(),
            };

            println!("Operation: cancel listing (PSKT)");
            println!("Payout:    {}", payout_address);
            let tx_id = pskt::cancel_listing_with_pskt(
                &rpc,
                prefix,
                &xonly_bytes,
                &payout_address,
                &keypair,
                &pskt,
            )
            .await?;

            Completed::Transaction { tx_id }
        }
    };

    match completed {
        Completed::Inscription { reveal_tx_id } => {
            println!("\nInscription complete!");
            println!("  Reveal tx:      {}", reveal_tx_id);
            println!("  Inscription ID: {}", format_inscription_id(&reveal_tx_id));
        }
        Completed::Transaction { tx_id } => {
            println!("\nTransaction submitted!");
            println!("  Tx: {}", tx_id);
        }
    }

    rpc.disconnect().await?;
    Ok(())
}
