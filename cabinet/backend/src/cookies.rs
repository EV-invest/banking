use axum_extra::extract::cookie::{Cookie, SameSite};
use time::Duration;

/// The BFF's cookie identity. Names are `__Host-`-prefixed + `Secure` in production
/// (binding the cookie to the exact host over HTTPS); in local dev the prefix and
/// Secure are dropped, since `http://localhost` rejects `__Host-`. The names MUST match
/// the frontend's `shared/config/cookies.ts` (the Next middleware reads the session
/// cookie) and `shared/lib/csrf-client.ts` (reads the csrf cookie).
#[derive(Clone)]
pub struct CookieNames {
	pub session: String,
	pub csrf: String,
	pub oauth_tx: String,
	secure: bool,
}

impl CookieNames {
	pub fn new(secure: bool) -> Self {
		let prefix = if secure { "__Host-" } else { "" };
		Self {
			session: format!("{prefix}ev_session"),
			csrf: format!("{prefix}ev_csrf"),
			oauth_tx: format!("{prefix}ev_oauth_tx"),
			secure,
		}
	}

	/// A server-side (HttpOnly) cookie carrying the shared base attributes.
	pub fn server_cookie(&self, name: String, value: String, max_age: i64) -> Cookie<'static> {
		self.build(name, value, max_age, true)
	}

	/// The CSRF cookie — NOT HttpOnly, so client JS can read it for the double-submit header.
	pub fn readable_cookie(&self, name: String, value: String, max_age: i64) -> Cookie<'static> {
		self.build(name, value, max_age, false)
	}

	/// An expiring cookie that clears `name` (empty value, `Max-Age=0`, same attributes).
	pub fn removal(&self, name: String, http_only: bool) -> Cookie<'static> {
		self.build(name, String::new(), 0, http_only)
	}

	fn build(&self, name: String, value: String, max_age: i64, http_only: bool) -> Cookie<'static> {
		let mut c = Cookie::new(name, value);
		c.set_path("/");
		c.set_http_only(http_only);
		c.set_secure(self.secure);
		c.set_same_site(SameSite::Lax);
		c.set_max_age(Duration::seconds(max_age));
		c
	}
}
