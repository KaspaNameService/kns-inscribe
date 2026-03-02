mod create;
mod create_asset;
mod list;
mod profile;
mod send;
mod transfer;

pub use create::build_create;
pub use create_asset::{build_create_asset, AssetInscription, AssetKind};
pub use list::build_list;
pub use profile::build_add_profile;
pub use send::build_send;
pub use transfer::{build_transfer, build_transfer_with_protocol};

/// The inscription content to embed in the script, plus the fee in sompi.
pub struct InscriptionContent {
    pub json: String,
    pub fee_sompi: u64,
}
