use anyhow::{anyhow, Result};
use kaspa_consensus_core::network::NetworkId;

pub struct Config {
    pub private_key_hex: String,
    pub rpc_url: String,
    pub network_id: NetworkId,
    pub funds_address: String,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        dotenvy::dotenv().ok();

        let private_key_hex =
            std::env::var("PRIVATE_KEY").map_err(|_| anyhow!("PRIVATE_KEY env var is required"))?;

        let node_ip = std::env::var("NODE_IP").unwrap_or_else(|_| "127.0.0.1".to_string());
        let node_port = std::env::var("NODE_WS_PORT").unwrap_or_else(|_| "17110".to_string());
        let rpc_url = format!("ws://{}:{}", node_ip, node_port);

        let network_str = std::env::var("NETWORK_ID").unwrap_or_else(|_| "mainnet".to_string());
        let network_id = network_str
            .parse::<NetworkId>()
            .map_err(|e| anyhow!("Invalid NETWORK_ID '{}': {}", network_str, e))?;

        let funds_address = std::env::var("KNS_FUNDS_ADDRESS").unwrap_or_default();

        Ok(Config {
            private_key_hex,
            rpc_url,
            network_id,
            funds_address,
        })
    }
}
