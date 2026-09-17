//! `piggybank-signer` — the separate-process key vault.
//!
//! A distinct trust domain from the hub: it holds chain private keys ENCRYPTED AT
//! REST and is the only process that ever sees the KEK or a plaintext key. The hub
//! reaches it over gRPC (the `signer.v1` contract) and asks only for key
//! provisioning today; signing/broadcast is a later feature that adds an RPC.
//!
//! Threat-model boundary (see [`key_vault`]): the at-rest encryption protects a
//! stolen DB/disk image, NOT an RCE on this running process. Use it for a small
//! hot-float only — real balances belong behind MPC/HSM + an offline cold tier.
//!
//! Layout mirrors the hub's hexagonal split, kept lean:
//!   service     — gRPC driving adapter
//!   backend     — the key-backend port (where a key lives, how a digest is signed)
//!   provision   — the provisioning use case (keygen → seal → store)
//!   key_vault   — the crypto core (XChaCha20-Poly1305 envelope + per-curve keygen)
//!   kek_guard   — boot-time KEK-epoch enforcement (sentinel + per-row fingerprints)
//!   turnkey     — the remote key-backend implementation (custody; off by default)
//!   secrets     — the `wallet_secrets` driven store (signer's own database)
//!   native_spend — the sliding-window spend ledger the policy consults (same database)
//!   jetton_wallets — the first-use jetton wallet pins the policy consults (same database)
//!   spend_brake — the operator's brake row that tightens or halts the policy per request (same database)

pub mod backend;
pub mod config;
pub mod error;
pub mod evm_tx;
pub mod jetton_wallets;
pub mod kek_guard;
pub mod key_vault;
pub mod native_spend;
pub mod policy;
pub mod provision;
pub mod secrets;
pub mod service;
pub mod spend_brake;
pub mod ton_tx;
pub mod tron_tx;
pub mod turnkey;
