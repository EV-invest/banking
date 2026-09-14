//! `GET /api/owners/consilium/ws` — the live consilium feed.
//!
//! A thin bridge from concierge's `GovernanceService.WatchGovernance` server-stream to
//! the browser. On top of the rules every bridge shares ([`super::ws`]), one is its own:
//!
//! - **It is not the source of truth.** A frame carries a REVISION and a timestamp, never
//!   a tally, never an address, never a token or a code. The client refetches the
//!   authoritative snapshot when the revision moves, so a stale, replayed or spoofed
//!   frame cannot render a wrong count.

use axum::{
	extract::{
		State,
		ws::{Message, Utf8Bytes, WebSocket, WebSocketUpgrade},
	},
	http::HeaderMap,
	response::Response,
};
use axum_extra::extract::cookie::CookieJar;
use serde_json::json;

use crate::{
	error::ApiError,
	governance::{self, Tick},
	routes::{
		require_identity,
		ws::{CLOSE_FEED_UNAVAILABLE, CLOSE_NORMAL, CLOSE_TOKEN_EXPIRED, HEARTBEAT, close, origin_allowed, time_left},
	},
	state::AppState,
	util::now_secs,
};

/// The handshake. Everything that can refuse this connection refuses it here, as a plain
/// HTTP status, before any upgrade.
pub async fn upgrade(State(st): State<AppState>, jar: CookieJar, headers: HeaderMap, ws: WebSocketUpgrade) -> Result<Response, ApiError> {
	if !origin_allowed(&st, &headers) {
		return Err(ApiError::Csrf);
	}
	let (token, claims) = require_identity(&st, &jar).await?;
	Ok(ws.on_upgrade(move |socket| serve(st, socket, token, claims.exp)))
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
}
