//! The gRPC driving adapter — the thin hub↔signer seam.
//!
//! It only validates the wire request and delegates to [`provision`]; all key
//! material handling lives below it. `Result<_, Status>` is tonic's mandated
//! handler signature and `Status` is a large type we don't control.
#![allow(clippy::result_large_err)]

use domain::money::Network;
use evbanking_contracts::signer::v1::{ProvisionAddressRequest, ProvisionAddressResponse, signer_service_server::SignerService};
use tonic::{Request, Response, Status};
use uuid::Uuid;

use crate::{key_vault::Vault, provision, secrets::WalletSecrets};

/// The signer service: the loaded [`Vault`] (holding the KEK) plus the
/// `wallet_secrets` store.
pub struct Signer {
	vault: Vault,
	secrets: WalletSecrets,
}

impl Signer {
	pub fn new(vault: Vault, secrets: WalletSecrets) -> Self {
		Self { vault, secrets }
	}
}

#[tonic::async_trait]
impl SignerService for Signer {
	async fn provision_address(&self, request: Request<ProvisionAddressRequest>) -> Result<Response<ProvisionAddressResponse>, Status> {
		let req = request.into_inner();
		let user_id = Uuid::parse_str(&req.user_id).map_err(|_| Status::invalid_argument("user_id must be a UUID"))?;
		let network = Network::parse(&req.network).map_err(|_| Status::invalid_argument(format!("unknown network: {}", req.network)))?;
		let address = provision::provision(&self.vault, &self.secrets, user_id, network).await?;
		Ok(Response::new(ProvisionAddressResponse { address }))
	}
}
