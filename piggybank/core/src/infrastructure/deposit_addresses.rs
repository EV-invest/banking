//! Stub deposit-address adapter — a stand-in for HD derivation from the fund's xpub.
//!
//! It deterministically derives a **structurally valid** per-(user, network) address
//! from a hash of the pair and caches it in `user_deposit_addresses`, so a user always
//! sees the same address and an operator can map an incoming deposit back to a user.
//! It is NOT a real, spendable address — no key backs it. Swap for the real derivation
//! service when custody lands; the [`DepositAddresses`] port and the application layer
//! stay unchanged.

use async_trait::async_trait;
use domain::{
	error::DomainError,
	money::{Network, WalletAddress},
	users::UserId,
};
use sqlx::PgPool;
use uuid::Uuid;

use crate::ports::deposit_addresses::DepositAddresses;

const BASE58: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
const BASE64URL: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

pub struct StubDepositAddresses {
	pool: PgPool,
}

impl StubDepositAddresses {
	pub fn new(pool: PgPool) -> Self {
		Self { pool }
	}
}

#[async_trait]
impl DepositAddresses for StubDepositAddresses {
	async fn address(&self, user: UserId, network: Network) -> Result<WalletAddress, DomainError> {
		if let Some(existing) = sqlx::query_scalar::<_, String>("SELECT address FROM user_deposit_addresses WHERE user_id = $1 AND network = $2")
			.bind(user.raw())
			.bind(network.as_str())
			.fetch_optional(&self.pool)
			.await
			.map_err(repo_err)?
		{
			return WalletAddress::parse(network, &existing);
		}
		let derived = derive_address(user, network);
		sqlx::query("INSERT INTO user_deposit_addresses (user_id, network, address) VALUES ($1, $2, $3) ON CONFLICT (user_id, network) DO NOTHING")
			.bind(user.raw())
			.bind(network.as_str())
			.bind(derived.as_str())
			.execute(&self.pool)
			.await
			.map_err(repo_err)?;
		Ok(derived)
	}
}

/// Deterministic per-(user, network) material via chained UUID v5 (stable across
/// runs/platforms — SHA-1 with fixed namespaces, no RNG).
fn derive_bytes(seed: &str, n: usize) -> Vec<u8> {
	let mut out = Vec::with_capacity(n);
	let mut acc = Uuid::new_v5(&Uuid::NAMESPACE_OID, seed.as_bytes());
	while out.len() < n {
		out.extend_from_slice(acc.as_bytes());
		acc = Uuid::new_v5(&acc, seed.as_bytes());
	}
	out.truncate(n);
	out
}

/// Build a structurally valid address for `network` from the derived bytes — mapping
/// each byte onto the chain's alphabet so [`WalletAddress::parse`] always accepts it.
fn derive_address(user: UserId, network: Network) -> WalletAddress {
	let seed = format!("{user}:{network}");
	let address = match network {
		// EVM: 0x + 40 hex (20 bytes).
		Network::Bep20 => {
			let bytes = derive_bytes(&seed, 20);
			let mut s = String::from("0x");
			for byte in bytes {
				s.push_str(&format!("{byte:02x}"));
			}
			s
		}
		// TRON: 'T' + 33 base58 chars.
		Network::Trc20 => {
			let bytes = derive_bytes(&seed, 33);
			let mut s = String::from("T");
			for byte in bytes {
				s.push(BASE58[byte as usize % BASE58.len()] as char);
			}
			s
		}
		// TON: 48-char user-friendly base64url.
		Network::Ton => {
			let bytes = derive_bytes(&seed, 48);
			let mut s = String::with_capacity(48);
			for byte in bytes {
				s.push(BASE64URL[byte as usize % BASE64URL.len()] as char);
			}
			s
		}
	};
	WalletAddress::parse(network, &address).expect("derived stub address is structurally valid by construction")
}

fn repo_err(err: sqlx::Error) -> DomainError {
	DomainError::Repository(err.to_string())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn derived_addresses_are_valid_and_stable() {
		let user = UserId::new();
		for network in Network::ALL {
			let a = derive_address(user, network);
			let b = derive_address(user, network);
			assert_eq!(a, b, "derivation must be deterministic");
			assert_eq!(a.network(), network);
			// Re-parsing the rendered form must succeed (structural validity).
			assert!(WalletAddress::parse(network, a.as_str()).is_ok());
		}
	}
}
