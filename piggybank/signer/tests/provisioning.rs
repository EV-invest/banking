//! Integration tests for key provisioning — real Postgres, no mocks (per the project
//! rules). They run when `SIGNER_DATABASE_URL` (or `DATABASE_URL`) is set and skip
//! otherwise. Each test uses a fresh `user_id`, so runs are isolated on shared infra.
//!
//! What's under test end-to-end: the migration applies; provisioning is idempotent per
//! (user, network); and a provisioned key round-trips through the DB — the sealed blob
//! opens only under the correct `(chain, row-id)` AAD, and the recovered private key
//! reproduces the stored public key (so the sealed secret really backs the address).

use domain::money::Network;
use piggybank_signer::{
	key_vault::{Chain, Vault, ed25519_pubkey, secp256k1_pubkey},
	provision,
	secrets::WalletSecrets,
};
use sqlx::PgPool;
use uuid::Uuid;

async fn pool() -> Option<PgPool> {
	let url = std::env::var("SIGNER_DATABASE_URL")
		.ok()
		.or_else(|| std::env::var("DATABASE_URL").ok())
		.filter(|s| !s.is_empty())?;
	let pool = sqlx::postgres::PgPoolOptions::new().max_connections(2).connect(&url).await.expect("connect to Postgres");
	// Applying the embedded migration here also proves it applies cleanly.
	sqlx::migrate!().run(&pool).await.expect("apply signer migrations");
	Some(pool)
}

fn test_vault() -> Vault {
	Vault::from_hex(&hex::encode([9u8; 32])).unwrap()
}

fn chain_of(network: Network) -> Chain {
	match network {
		Network::Bep20 => Chain::BscBep20,
		Network::Trc20 => Chain::TronTrc20,
		Network::Ton => Chain::Ton,
	}
}

#[tokio::test]
async fn provisions_seals_and_round_trips_each_network() {
	let Some(pool) = pool().await else {
		eprintln!("DATABASE_URL/SIGNER_DATABASE_URL unset — skipping signer provisioning test");
		return;
	};
	let vault = test_vault();
	let secrets = WalletSecrets::new(pool.clone());

	for network in Network::ALL {
		let user = Uuid::new_v4();

		let provisioned = provision::provision(&vault, &secrets, user, network).await.expect("provision");
		assert!(!provisioned.address.is_empty());
		// Every rail now computes its true on-chain image, so the signer reports it as derived
		// (fundable) rather than a placeholder.
		assert_eq!(provisioned.kind, provision::KIND_DERIVED, "{network} address is derived from the stored key");
		let address = provisioned.address;

		// Idempotent: a second call returns the same address and mints no new key.
		let again = provision::provision(&vault, &secrets, user, network).await.expect("re-provision");
		assert_eq!(address, again.address, "{network} provisioning must be idempotent");
		assert_eq!(again.kind, provision::KIND_DERIVED, "{network} re-read reports the stored kind");

		// Read the sealed row + the stored (watch-only) public key.
		let sealed = secrets.find_sealed(user, network).await.expect("query").expect("row exists");
		let stored_pubkey: Vec<u8> = sqlx::query_scalar("SELECT public_key FROM wallet_secrets WHERE user_id = $1 AND network = $2")
			.bind(user)
			.bind(network.as_str())
			.fetch_one(&pool)
			.await
			.expect("public_key");
		assert_eq!(sealed.key_version, 1);

		// Open under the correct (chain, row-id) AAD → recovers the private key.
		let chain = chain_of(network);
		let opened = vault.open(chain, &sealed.id.to_string(), &sealed.sealed_key).expect("open sealed key");
		let seed: [u8; 32] = opened[..].try_into().expect("32-byte secret");

		// End-to-end: the recovered private key reproduces the stored public key.
		let derived = match network {
			Network::Ton => ed25519_pubkey(&seed).to_vec(),
			Network::Bep20 | Network::Trc20 => secp256k1_pubkey(&seed),
		};
		assert_eq!(derived, stored_pubkey, "{network} sealed key must back the stored public key");

		// AAD is load-bearing: a wrong chain or a wrong row id cannot open the blob.
		let wrong_chain = if matches!(chain, Chain::Ton) { Chain::BscBep20 } else { Chain::Ton };
		assert!(vault.open(wrong_chain, &sealed.id.to_string(), &sealed.sealed_key).is_err());
		assert!(vault.open(chain, &Uuid::new_v4().to_string(), &sealed.sealed_key).is_err());
	}
}
