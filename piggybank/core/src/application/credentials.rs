//! The emailed-token credential every governance seat is minted with — one token for the
//! link, one code for the human to type — shared by the consilium (N owner seats) and the
//! payment consent (one investor seat). The two flows must not share their STATE (the plane
//! split and the differing rules forbid it); the shape of a secret is the one thing they may
//! share, and `docs/CONSILIUM.md` § "One specification for both planes" is the contract.

use domain::error::DomainError;

use crate::{infrastructure::consilium::digest, ports::consilium::DIGEST_BYTES};

/// Crockford base32 minus `I`, `L`, `O` and `U` — the four glyphs a human misreads or
/// mistypes. 32 symbols divides 256 exactly, so sampling a byte modulo the alphabet is
/// unbiased with no rejection loop.
const CODE_ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// 10 symbols over a 32-letter alphabet ≈ 50 bits — far beyond what five attempts can reach,
/// while still being something a person will actually type off a screen.
const CODE_LEN: usize = 10;

/// 32 random bytes (256 bits), hex-encoded. Comfortably past the 24-byte floor, and the
/// token is single-use with a 72h TTL besides.
const TOKEN_BYTES: usize = 32;

/// One seat's freshly minted secrets. The plaintexts are handed to the mail queue and never
/// stored; only the digests reach a seat row.
pub struct MintedSecret {
	pub token: String,
	pub code: String,
	pub token_hash: [u8; DIGEST_BYTES],
	pub code_hash: [u8; DIGEST_BYTES],
}

/// Mint one seat's token and code from the OS CSPRNG.
pub fn mint() -> Result<MintedSecret, DomainError> {
	let mut token_bytes = [0u8; TOKEN_BYTES];
	let mut code_bytes = [0u8; CODE_LEN];
	getrandom::fill(&mut token_bytes).map_err(|_| DomainError::Repository("OS randomness unavailable".into()))?;
	getrandom::fill(&mut code_bytes).map_err(|_| DomainError::Repository("OS randomness unavailable".into()))?;
	let token = hex::encode(token_bytes);
	let code: String = code_bytes.iter().map(|byte| CODE_ALPHABET[(*byte % 32) as usize] as char).collect();
	Ok(MintedSecret {
		token_hash: digest(token.as_bytes()),
		code_hash: digest(code.as_bytes()),
		token,
		code,
	})
}

/// The digest a presented token is looked up by.
pub fn token_digest(token: &str) -> [u8; DIGEST_BYTES] {
	digest(token.as_bytes())
}
