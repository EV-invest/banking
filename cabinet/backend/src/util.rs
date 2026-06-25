use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};

/// An opaque identifier: `n` bytes of CSPRNG entropy, base64url-encoded (no padding).
/// Mirrors the frontend's `randomId` / OAuth token minting.
pub fn random_token(n: usize) -> String {
	URL_SAFE_NO_PAD.encode(random_bytes(n))
}
/// base64url (no padding) — the encoding for the PKCE code challenge and tokens.
pub fn base64url(bytes: &[u8]) -> String {
	URL_SAFE_NO_PAD.encode(bytes)
}
/// Current unix time in seconds (matches the proto `*_expires_at` / `*_at` fields).
pub fn now_secs() -> i64 {
	std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}
/// `n` bytes of CSPRNG entropy.
fn random_bytes(n: usize) -> Vec<u8> {
	let mut buf = vec![0u8; n];
	getrandom::fill(&mut buf).expect("CSPRNG unavailable");
	buf
}
