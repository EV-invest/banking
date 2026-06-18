use std::{fs, path::PathBuf};

/// Compile every `proto/fund/v1/*.proto` into Rust with tonic — BOTH client and
/// server stubs. The backend includes the servers; other service repos that
/// depend on this crate (by git) include the clients. The generated module is
/// pulled into `src/lib.rs` via `tonic::include_proto!("fund.v1")`.
fn main() -> Result<(), Box<dyn std::error::Error>> {
	let proto_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("proto");
	let v1 = proto_root.join("fund/v1");

	let mut protos: Vec<PathBuf> = fs::read_dir(&v1)?
		.filter_map(|entry| entry.ok().map(|entry| entry.path()))
		.filter(|path| path.extension().is_some_and(|ext| ext == "proto"))
		.collect();
	protos.sort();

	println!("cargo:rerun-if-changed={}", v1.display());
	for proto in &protos {
		println!("cargo:rerun-if-changed={}", proto.display());
	}

	tonic_build::configure().build_server(true).build_client(true).compile_protos(&protos, &[proto_root])?;

	Ok(())
}
