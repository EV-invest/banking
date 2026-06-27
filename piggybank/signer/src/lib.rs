#![feature(default_field_values)]
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
//!   provision   — the provisioning use case (keygen → seal → store)
//!   key_vault   — the crypto core (XChaCha20-Poly1305 envelope + per-curve keygen)
//!   secrets     — the `wallet_secrets` driven store (signer's own database)

pub mod config;
pub mod error;
pub mod evm_tx;
pub mod key_vault;
pub mod provision;
pub mod secrets;
pub mod service;
