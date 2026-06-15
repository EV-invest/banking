//! Backend library crate.
//!
//! Exposes the modules so the server binary (`main.rs`) and any future tooling
//! binaries share one source of truth. Two driving adapters sit over the same
//! wired infrastructure:
//!   api  — Axum HTTP
//!   grpc — tonic gRPC
//! Both reach Postgres and TigerBeetle through `infrastructure`. Scaffold: the
//! application/domain layers are added between them as real features land.

pub mod api;
pub mod config;
pub mod error_reporter;
pub mod grpc;
pub mod infrastructure;
