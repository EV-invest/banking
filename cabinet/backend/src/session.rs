use std::{collections::HashMap, sync::Arc};

use evbanking_contracts::banking::v1 as bk;
use tokio::sync::Mutex;
use tonic::{Code, Status};

use crate::{state::Grpc, util::now_secs};

/// The resolution of a money-plane token for a request. The money plane has its own
/// issuer and `aud=banking-core`, so the concierge identity token never authorizes it;
/// the banking pair is minted via the concierge→banking exchange seam (`IssueUserToken`).
pub enum MoneyToken {
	/// A live banking `aud=banking-core` token to forward to the money plane.
	Token(String),
	/// No banking token could be obtained (issuance unconfigured, the bridge hasn't
	/// mirrored the user yet, or upstream is down); a later request re-mints.
	NotIssued,
}

/// The banking money-token pair, cached per concierge user id.
///
/// Auth is shell-owned now: the shared `ev_access` cookie (verified against the
/// concierge JWKS) proves identity per request, so the BFF holds NO session — this
/// cache is the one piece of server-side state left, and it is exactly a cache: a
/// lost entry re-mints through the issuance seam. After sign-out an entry idles
/// until its refresh window lapses, but it is unreachable without a freshly
/// verified JWT for that same user, so the exposure window is the access TTL.
///
/// ponytail: in-process, per-replica — parallel replicas each mint their own
/// banking family, which the plane already tolerates (one per login historically).
pub struct BankingTokens {
	entries: Mutex<HashMap<String, Arc<Mutex<BankingPair>>>>,
}

#[derive(Default)]
struct BankingPair {
	access_token: String,
	access_expires_at: i64,
	refresh_token: String,
	refresh_expires_at: i64,
}

impl BankingPair {
	fn apply(&mut self, tokens: bk::TokenResponse) {
		self.access_token = tokens.access_token;
		self.access_expires_at = tokens.access_expires_at;
		self.refresh_token = tokens.refresh_token;
		self.refresh_expires_at = tokens.refresh_expires_at;
	}
}

impl BankingTokens {
	pub fn new() -> Self {
		Self {
			entries: Mutex::new(HashMap::new()),
		}
	}

	/// The fresh banking `aud=banking-core` token for `user_id` (a VERIFIED concierge
	/// subject — the caller must have checked the JWT signature first, or this would
	/// mint money-plane tokens for arbitrary ids). Returns the cached token if still
	/// valid, rotates the family if near expiry, or mints a fresh pair. Per-user
	/// single-flight, so concurrent money requests coalesce to one mint/rotate.
	pub async fn token_for(&self, user_id: &str, grpc: &Grpc) -> MoneyToken {
		let slot = {
			let mut entries = self.entries.lock().await;
			entries.retain(|_, e| e.try_lock().map(|p| p.refresh_expires_at > now_secs() || p.access_expires_at > now_secs()).unwrap_or(true));
			entries.entry(user_id.to_string()).or_default().clone()
		};
		let mut pair = slot.lock().await;

		if !pair.access_token.is_empty() && pair.access_expires_at > now_secs() + 30 {
			return MoneyToken::Token(pair.access_token.clone());
		}
		// Prefer rotating the existing family; fall back to minting a new one only when
		// the family is dead (absent, expired, or rejected upstream). A transport-class
		// rotation failure keeps the stored pair for retry rather than minting a
		// parallel family.
		let rotated = if !pair.refresh_token.is_empty() && pair.refresh_expires_at > now_secs() {
			match grpc.refresh_banking_token(&pair.refresh_token).await {
				Ok(tokens) => Some(tokens),
				Err(status) if refresh_rejected(&status) => {
					tracing::warn!(code = ?status.code(), "banking refresh rejected; re-minting via the exchange seam");
					None
				}
				Err(status) => {
					tracing::warn!(code = ?status.code(), detail = %status.message(), "banking refresh failed transiently; keeping the pair for retry");
					return MoneyToken::NotIssued;
				}
			}
		} else {
			None
		};
		let tokens = match rotated {
			Some(tokens) => tokens,
			// Device metadata is a login-surface concern that lives with the shell-owned
			// session now; the banking family carries none.
			None => match grpc.issue_banking_token(user_id, "", "").await {
				Ok(tokens) => tokens,
				Err(status) => {
					tracing::warn!(code = ?status.code(), detail = %status.message(), "money-plane token mint failed");
					return MoneyToken::NotIssued;
				}
			},
		};
		pair.apply(tokens);
		MoneyToken::Token(pair.access_token.clone())
	}
}

/// Whether a failed banking refresh is a terminal auth verdict — the family is dead
/// upstream (revoked, or rotated-out reuse read as theft) — as opposed to a
/// transport-class failure (`Unavailable`/`DeadlineExceeded`/…).
fn refresh_rejected(status: &Status) -> bool {
	matches!(status.code(), Code::Unauthenticated | Code::PermissionDenied)
}

#[cfg(test)]
mod tests {
	use super::*;

	/// A lazy `Grpc` to a black-hole address with NO issuance token: minting cannot
	/// succeed, so the seam test exercises pure token resolution.
	fn grpc() -> Grpc {
		Grpc::connect_lazy("http://127.0.0.1:1", "http://127.0.0.1:1", "http://127.0.0.1:1", None).expect("lazy channels")
	}

	// BANK-ARCH-01 / CROSS-1: the money path never falls back to any identity-plane
	// credential. With issuance unconfigured it resolves to `NotIssued` (the route
	// surfaces NotConfigured) — never a token it didn't mint from the banking plane.
	#[tokio::test]
	async fn unconfigured_issuance_resolves_to_not_issued() {
		let cache = BankingTokens::new();
		match cache.token_for("u-1", &grpc()).await {
			MoneyToken::NotIssued => {}
			MoneyToken::Token(t) => panic!("no banking token can exist here, got {t:?}"),
		}
	}
}
