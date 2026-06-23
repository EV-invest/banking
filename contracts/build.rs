use std::{fs, path::PathBuf};

/// Compile every `proto/<pkg>/v1/*.proto` into Rust with tonic — BOTH client and
/// server stubs. The backend includes the servers; other service repos that
/// depend on this crate (by git) include the clients. Each package's generated
/// module is pulled into `src/lib.rs` via `tonic::include_proto!("<pkg>.v1")`.
///
/// `banking.v1` is the cabinet-facing surface; `signer.v1` is the internal
/// hub↔signer seam — kept in a separate package so the OpenAPI/TS generation
/// (`nix run .#gen-api`, which reads only `banking/v1`) never exposes it.
fn main() -> Result<(), Box<dyn std::error::Error>> {
	let proto_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("proto");

	let mut protos: Vec<PathBuf> = Vec::new();
	for package in ["banking/v1", "signer/v1"] {
		let dir = proto_root.join(package);
		println!("cargo:rerun-if-changed={}", dir.display());
		for entry in fs::read_dir(&dir)? {
			let path = entry?.path();
			if path.extension().is_some_and(|ext| ext == "proto") {
				println!("cargo:rerun-if-changed={}", path.display());
				protos.push(path);
			}
		}
	}
	protos.sort();

	tonic_build::configure().build_server(true).build_client(true).compile_protos(&protos, &[proto_root])?;

	Ok(())
}
