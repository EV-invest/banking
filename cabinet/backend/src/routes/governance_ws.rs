//! `GET /api/owners/consilium/ws` — the live consilium feed.
//!
//! A thin bridge from concierge's `GovernanceService.WatchGovernance` server-stream to
//! the browser. Three rules make it safe to keep open:
//!
//! - **It is not the source of truth.** A frame carries a REVISION and a timestamp, never
//!   a tally, never an address, never a token or a code. The client refetches the
//!   authoritative snapshot when the revision moves, so a stale, replayed or spoofed
//!   frame cannot render a wrong count.
//! - **It is authorized at the handshake and only until the token says so.** `ev_access`
//!   is verified exactly as on the REST routes, and the socket closes itself when that
//!   token expires rather than outliving the session that opened it.
//! - **It is mounted outside the request-deadline layer** (see [`crate::routes::router`]),
//!   because that layer exists to kill a wedged request and would kill this instead.

use std::time::Duration;

use axum::{
	extract::{
		State,
		ws::{CloseFrame, Message, Utf8Bytes, WebSocket, WebSocketUpgrade},
	},
	http::{HeaderMap, header},
	response::Response,
};
use axum_extra::extract::cookie::CookieJar;
use serde_json::json;

use crate::{
	error::ApiError,
	governance::{self, Tick},
	routes::require_identity,
	state::AppState,
	util::now_secs,
};

/// Keepalive cadence. Also the worst-case lag on noticing a browser that vanished without
/// a close frame: the send that fails is how a dead peer is reaped.
const HEARTBEAT: Duration = Duration::from_secs(25);

/// Application close codes (the 4000–4999 range is ours). The client distinguishes "your
/// session ended, sign in again" from "the feed is down, fall back to polling".
const CLOSE_NORMAL: u16 = 1000;
const CLOSE_TOKEN_EXPIRED: u16 = 4401;
const CLOSE_FEED_UNAVAILABLE: u16 = 4503;

/// The handshake. Everything that can refuse this connection refuses it here, as a plain
/// HTTP status, before any upgrade.
pub async fn upgrade(State(st): State<AppState>, jar: CookieJar, headers: HeaderMap, ws: WebSocketUpgrade) -> Result<Response, ApiError> {
	// A websocket handshake is exempt from the same-origin policy but still carries
	// cookies, so without this any page could open the signed-in owner's feed. This is the
	// socket's half of what CSRF double-submit does for the mutating REST routes.
	if !origin_allowed(&st, &headers) {
		return Err(ApiError::Csrf);
	}
	let (token, claims) = require_identity(&st, &jar).await?;
	Ok(ws.on_upgrade(move |socket| serve(st, socket, token, claims.exp)))
}

/// Whether the handshake's `Origin` is the one browser origin this BFF serves. An unset
/// `CABINET_WS_ORIGIN` disables the check for local development; production cannot reach
/// that state, because the config layer refuses to boot without the value.
fn origin_allowed(st: &AppState, headers: &HeaderMap) -> bool {
	let Some(expected) = st.config.cabinet_ws_origin.as_deref() else {
		return true;
	};
	headers.get(header::ORIGIN).and_then(|value| value.to_str().ok()).is_some_and(|origin| origin == expected)
}

/// One task per socket, holding both ends of the bridge; it returns — dropping the
/// upstream subscription with it — on client disconnect, upstream end, or token expiry.
async fn serve(st: AppState, mut socket: WebSocket, token: String, expires_at: u64) {
	let mut feed = match governance::watch(&st.grpc, &token).await {
		Ok(feed) => feed,
		// The page still works without the socket: it polls. Saying so beats a socket that
		// looks alive and silently never ticks.
		Err(status) => {
			tracing::warn!(code = ?status.code(), "governance feed unavailable; closing the socket");
			return close(socket, CLOSE_FEED_UNAVAILABLE, "governance feed unavailable").await;
		}
	};

	let expiry = tokio::time::sleep(time_left(expires_at));
	tokio::pin!(expiry);
	let mut heartbeat = tokio::time::interval(HEARTBEAT);
	heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
	let mut revision = 0;

	loop {
		// The socket is touched only OUTSIDE this block: `select!` holds every branch
		// future at once, so a branch that sent would collide with the one that reads.
		let event = tokio::select! {
			tick = feed.recv() => match tick {
				Some(tick) => Event::Upstream(tick),
				None => Event::UpstreamEnded,
			},
			_ = heartbeat.tick() => Event::Heartbeat,
			incoming = socket.recv() => match incoming {
				// Nothing the client says on this socket is input; only its going away is.
				// Ping frames are answered by axum from inside `recv`.
				None | Some(Err(_)) | Some(Ok(Message::Close(_))) => Event::Gone,
				Some(Ok(_)) => continue,
			},
			() = &mut expiry => Event::Expired,
		};

		let frame = match event {
			Event::Upstream(tick) => {
				revision = tick.revision;
				frame(&tick)
			}
			Event::Heartbeat => frame(&Tick {
				revision,
				at: now_secs(),
				heartbeat: true,
			}),
			// An ended upstream is a normal event (a concierge replica rolling); the client
			// reconnects and refetches, and the snapshot it refetches is authoritative.
			Event::UpstreamEnded => return close(socket, CLOSE_NORMAL, "feed ended").await,
			Event::Expired => return close(socket, CLOSE_TOKEN_EXPIRED, "access token expired").await,
			Event::Gone => return,
		};
		if socket.send(frame).await.is_err() {
			return;
		}
	}
}

enum Event {
	Upstream(Tick),
	UpstreamEnded,
	Heartbeat,
	Expired,
	Gone,
}

/// One frame: a revision and when it was produced. 64-bit values are strings, as
/// everywhere else on this wire.
fn frame(tick: &Tick) -> Message {
	Message::Text(Utf8Bytes::from(
		json!({
			"type": "tick",
			"revision": tick.revision.to_string(),
			"at": tick.at.to_string(),
			"heartbeat": tick.heartbeat,
		})
		.to_string(),
	))
}

/// How long the verified token still authorizes this socket. Saturating, so a token that
/// expired between the verify and here closes immediately rather than sleeping forever.
fn time_left(expires_at: u64) -> Duration {
	Duration::from_secs(expires_at.saturating_sub(now_secs().max(0) as u64))
}

async fn close(mut socket: WebSocket, code: u16, reason: &'static str) {
	let _ = socket
		.send(Message::Close(Some(CloseFrame {
			code,
			reason: Utf8Bytes::from_static(reason),
		})))
		.await;
}

#[cfg(test)]
mod tests {
	use super::*;

	fn payload(message: &Message) -> serde_json::Value {
		match message {
			Message::Text(text) => serde_json::from_str(text.as_str()).expect("a frame is JSON"),
			other => panic!("a governance frame is text, got {other:?}"),
		}
	}

	/// Pitfalls 21 and 23. A frame carries the revision and the time, and nothing else —
	/// no tally the client could render without refetching, and no secret at all.
	#[test]
	fn a_frame_carries_a_revision_and_no_tally() {
		let frame = frame(&Tick {
			revision: 42,
			at: 1_756_000_000,
			heartbeat: false,
		});
		let value = payload(&frame);
		assert_eq!(value["revision"], "42", "the revision travels as a string, like every other 64-bit value");
		assert_eq!(value["at"], "1756000000");
		assert_eq!(value["heartbeat"], false);
		let keys: Vec<&str> = value.as_object().expect("an object").keys().map(String::as_str).collect();
		assert_eq!(keys, vec!["at", "heartbeat", "revision", "type"], "no field may be added here without a second look");
	}

	/// The keepalive repeats the current revision unchanged, so it can never be mistaken
	/// for movement the client should refetch on.
	#[test]
	fn a_heartbeat_repeats_the_revision_it_already_sent() {
		let value = payload(&frame(&Tick {
			revision: 7,
			at: 1,
			heartbeat: true,
		}));
		assert_eq!(value["revision"], "7");
		assert_eq!(value["heartbeat"], true);
	}

	/// Pitfall 22. A token that is already expired must not buy a socket that sleeps until
	/// the far future before noticing.
	#[test]
	fn an_expired_token_closes_the_socket_at_once() {
		assert_eq!(time_left(0), Duration::ZERO);
		let soon = now_secs() as u64 + 30;
		assert!(time_left(soon) <= Duration::from_secs(30));
	}
}
