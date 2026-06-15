#[cfg(not(target_arch = "wasm32"))]
fn main() {
	let port: u16 = std::env::var("PORT").ok().and_then(|s| s.parse().ok()).unwrap_or(3000);
	let status = std::process::Command::new("dx")
		.args(["serve", "--package", "frontend", "--port", &port.to_string()])
		.status()
		.expect("dx serve failed — is `dx` installed? (`cargo install dioxus-cli`)");
	std::process::exit(status.code().unwrap_or(1));
}

#[cfg(target_arch = "wasm32")]
fn main() {
	dioxus::launch(frontend::App);
}
