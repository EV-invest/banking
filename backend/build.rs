use std::{env, path::PathBuf};

/// Compile the gRPC contract (`proto/fund/v1/*.proto`) into Rust with tonic.
/// The generated module is `include!`d from `src/grpc/mod.rs`.
fn main() -> Result<(), Box<dyn std::error::Error>> {
	let proto_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../proto");
	let health = proto_root.join("fund/v1/health.proto");

	println!("cargo:rerun-if-changed={}", health.display());

	tonic_build::configure()
		.build_server(true)
		.build_client(false)
		.out_dir(PathBuf::from(env::var("OUT_DIR")?))
		.compile_protos(&[health], &[proto_root])?;

	Ok(())
}
