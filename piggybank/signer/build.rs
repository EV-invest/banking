//! Generate Rust structs for the minimal vendored TRON protocol (`proto/tron.proto`) with prost.
//! TRON transactions are protobuf (unlike EVM's RLP), so the signer needs these to build the
//! `Transaction.raw` whose sha256 is the txID it signs. Pulled in via `tron::proto` (see `tron_tx`).

fn main() {
	println!("cargo:rerun-if-changed=proto/tron.proto");
	prost_build::compile_protos(&["proto/tron.proto"], &["proto"]).expect("compile the vendored tron proto");
}
