//! `users` bounded context — investor accounts.
//!
//! The [`User`] aggregate is the hub's canonical record of a person: provisioned
//! on first sign-in and kept in sync with the `auth` identity. It is
//! **identity-only** — it holds no money and no second copy of any amount. Money
//! is authoritative in TigerBeetle and read live (never re-bookkept here); the
//! UUID↔ledger-account mapping lives in the control plane (Postgres), not on this
//! aggregate.
//!
//! Pure and wasm-safe: no crypto, no I/O, no clock reads. Identities are supplied
//! by the (host-only) application layer and audit timestamps are DB-managed, so
//! this stays compilable to wasm and trivially testable.

use ev::architecture::{AggregateRoot, DomainEvent, EmitsEvents, Entity, Id};
use serde::{Deserialize, Serialize};

use crate::{auth::AuthSubject, error::DomainError};

/// The hub's canonical user id (a UUID). **This** value is the `sub` of the hub's
/// first-party JWT — never Google's `sub` (see [`AuthSubject`]).
pub type UserId = Id<UserTag>;
/// Phantom tag making [`UserId`] a distinct, incompatible identity type.
pub struct UserTag;

/// A verified email address. Parse-don't-validate: lowercased and trimmed on
/// construction, so equality and the storage form are normalized. Deliberately
/// **not** a unique key — a person may change the email behind a stable
/// [`AuthSubject`]. Serializes transparently as the bare string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Email(String);

impl Email {
	/// Normalize and minimally check an email. Full validation is the IdP's job
	/// (Google has already verified deliverability); this only guards against an
	/// obviously malformed value reaching the aggregate.
	pub fn parse(raw: &str) -> Result<Self, DomainError> {
		let normalized = raw.trim().to_lowercase();
		if normalized.len() < 3 || !normalized.contains('@') {
			return Err(DomainError::Validation("email must contain '@'".into()));
		}
		Ok(Self(normalized))
	}

	pub fn as_str(&self) -> &str {
		&self.0
	}
}

impl core::fmt::Display for Email {
	fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
		f.write_str(&self.0)
	}
}

/// The minimal user lifecycle. `Disabled` freezes sign-in/refresh without deleting
/// the record (the ledger and audit trail must outlive a deactivation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserStatus {
	Active,
	Disabled,
}

impl UserStatus {
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Active => "active",
			Self::Disabled => "disabled",
		}
	}

	/// Parse the stored form back into the enum (used by the persistence adapter).
	pub fn parse(raw: &str) -> Result<Self, DomainError> {
		match raw {
			"active" => Ok(Self::Active),
			"disabled" => Ok(Self::Disabled),
			other => Err(DomainError::Validation(format!("unknown user status: {other}"))),
		}
	}
}

/// The caller's editable profile fields (the full-replace set). All optional —
/// `None`/an empty value clears the field. Identity/auth fields (email, status) are
/// deliberately absent: they are not user-editable here.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileFields {
	pub legal_name: Option<String>,
	pub preferred_name: Option<String>,
	pub phone: Option<String>,
	pub date_of_birth: Option<String>,
	pub nationality: Option<String>,
	pub tax_residence: Option<String>,
	pub residential_address: Option<String>,
	pub language: Option<String>,
	pub base_currency: Option<String>,
	pub timezone: Option<String>,
}

/// The investor identity aggregate. Construct it with [`User::provision`] (first
/// sign-in, raises [`UserEvent::Provisioned`]) or [`User::rehydrate`] (load from
/// the store, no events). Mutating transitions accumulate [`UserEvent`]s drained
/// by the command handler into the event log in the same unit of work.
#[derive(Debug, Clone)]
pub struct User {
	id: UserId,
	auth_subject: AuthSubject,
	email: Email,
	email_verified: bool,
	status: UserStatus,
	token_version: u64,
	profile: ProfileFields,
	pending: Vec<UserEvent>,
}

impl User {
	/// Provision a brand-new user at first sign-in. The application layer mints the
	/// [`UserId`] (host-only), keeping this pure.
	pub fn provision(id: UserId, auth_subject: AuthSubject, email: Email, email_verified: bool) -> Self {
		let mut user = Self {
			id,
			auth_subject: auth_subject.clone(),
			email: email.clone(),
			email_verified,
			status: UserStatus::Active,
			token_version: 0,
			profile: ProfileFields::default(),
			pending: Vec::new(),
		};
		user.pending.push(UserEvent::Provisioned {
			user_id: id,
			auth_subject,
			email,
			email_verified,
		});
		user
	}

	/// Reconstitute an existing user from the store, including the editable profile.
	/// Raises no events.
	#[allow(clippy::too_many_arguments)]
	pub fn rehydrate(id: UserId, auth_subject: AuthSubject, email: Email, email_verified: bool, status: UserStatus, token_version: u64, profile: ProfileFields) -> Self {
		Self {
			id,
			auth_subject,
			email,
			email_verified,
			status,
			token_version,
			profile,
			pending: Vec::new(),
		}
	}

	/// Update the email (and its verified flag) to the IdP's current value. No-op
	/// (and no event) when unchanged, so a routine sign-in does not churn events.
	///
	/// An already-verified stored email is never overwritten by an unverified one: a
	/// principal whose IdP `sub` later carries an unverified (or attacker-influenced)
	/// email must not be able to downgrade the account's verified address. The verified
	/// value stands until the IdP again asserts a verified email.
	pub fn change_email(&mut self, email: Email, email_verified: bool) {
		if self.email_verified && !email_verified {
			return;
		}
		if self.email == email && self.email_verified == email_verified {
			return;
		}
		self.email = email.clone();
		self.email_verified = email_verified;
		self.pending.push(UserEvent::EmailChanged { user_id: self.id, email });
	}

	/// Full-replace the editable profile fields and raise [`UserEvent::ProfileUpdated`].
	/// The event is raised unconditionally on every call (no value diffing) — these are
	/// explicit user edits, so an audit fact per save is the desired behaviour.
	pub fn update_profile(&mut self, fields: ProfileFields) {
		self.profile = fields;
		self.pending.push(UserEvent::ProfileUpdated { user_id: self.id });
	}

	/// Bump `token_version`, invalidating every outstanding token for this user
	/// ("revoke all"). Returns the new version.
	pub fn revoke_tokens(&mut self) -> u64 {
		self.token_version += 1;
		self.pending.push(UserEvent::TokensRevoked {
			user_id: self.id,
			token_version: self.token_version,
		});
		self.token_version
	}

	/// Disable the user, freezing future sign-in/refresh. No-op when already
	/// disabled.
	pub fn disable(&mut self) {
		if self.status == UserStatus::Disabled {
			return;
		}
		self.status = UserStatus::Disabled;
		self.pending.push(UserEvent::Disabled { user_id: self.id });
	}

	pub fn id(&self) -> UserId {
		self.id
	}

	pub fn auth_subject(&self) -> &AuthSubject {
		&self.auth_subject
	}

	pub fn email(&self) -> &Email {
		&self.email
	}

	pub fn email_verified(&self) -> bool {
		self.email_verified
	}

	pub fn status(&self) -> UserStatus {
		self.status
	}

	pub fn is_active(&self) -> bool {
		self.status == UserStatus::Active
	}

	pub fn token_version(&self) -> u64 {
		self.token_version
	}

	pub fn legal_name(&self) -> Option<&str> {
		self.profile.legal_name.as_deref()
	}

	pub fn preferred_name(&self) -> Option<&str> {
		self.profile.preferred_name.as_deref()
	}

	pub fn phone(&self) -> Option<&str> {
		self.profile.phone.as_deref()
	}

	pub fn date_of_birth(&self) -> Option<&str> {
		self.profile.date_of_birth.as_deref()
	}

	pub fn nationality(&self) -> Option<&str> {
		self.profile.nationality.as_deref()
	}

	pub fn tax_residence(&self) -> Option<&str> {
		self.profile.tax_residence.as_deref()
	}

	pub fn residential_address(&self) -> Option<&str> {
		self.profile.residential_address.as_deref()
	}

	pub fn language(&self) -> Option<&str> {
		self.profile.language.as_deref()
	}

	pub fn base_currency(&self) -> Option<&str> {
		self.profile.base_currency.as_deref()
	}

	pub fn timezone(&self) -> Option<&str> {
		self.profile.timezone.as_deref()
	}
}

impl Entity for User {
	type Id = UserId;

	fn id(&self) -> UserId {
		self.id
	}
}

impl AggregateRoot for User {
	const NAME: &'static str = "user";
}

/// Facts raised by the [`User`] aggregate, persisted to the event log and projected
/// downstream. Internally tagged so the stored JSON is self-describing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UserEvent {
	Provisioned {
		user_id: UserId,
		auth_subject: AuthSubject,
		email: Email,
		email_verified: bool,
	},
	EmailChanged {
		user_id: UserId,
		email: Email,
	},
	ProfileUpdated {
		user_id: UserId,
	},
	TokensRevoked {
		user_id: UserId,
		token_version: u64,
	},
	Disabled {
		user_id: UserId,
	},
}

impl DomainEvent for UserEvent {
	const KIND: &'static str = "users";
}

impl EmitsEvents for User {
	type Event = UserEvent;

	fn drain_events(&mut self) -> Vec<UserEvent> {
		core::mem::take(&mut self.pending)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn fixture() -> User {
		User::provision(UserId::new(), AuthSubject::parse("g-123").unwrap(), Email::parse("Ada@Example.com").unwrap(), true)
	}

	#[test]
	fn email_is_normalized() {
		assert_eq!(Email::parse("  Ada@Example.COM ").unwrap().as_str(), "ada@example.com");
		assert!(Email::parse("nope").is_err());
	}

	#[test]
	fn provision_raises_one_event_then_drains() {
		let mut user = fixture();
		assert_eq!(user.token_version(), 0);
		assert!(user.is_active());
		let events = user.drain_events();
		assert_eq!(events.len(), 1);
		assert!(matches!(events[0], UserEvent::Provisioned { .. }));
		assert!(user.drain_events().is_empty());
	}

	#[test]
	fn change_email_is_noop_when_unchanged() {
		let mut user = fixture();
		user.drain_events();
		user.change_email(Email::parse("ada@example.com").unwrap(), true);
		assert!(user.drain_events().is_empty());
	}

	#[test]
	fn verified_email_is_not_overwritten_by_unverified() {
		let mut user = fixture();
		assert!(user.email_verified());
		user.drain_events();
		user.change_email(Email::parse("attacker@example.com").unwrap(), false);
		assert_eq!(user.email().as_str(), "ada@example.com");
		assert!(user.email_verified());
		assert!(user.drain_events().is_empty());
	}

	#[test]
	fn unverified_email_can_be_promoted_to_verified() {
		let mut user = User::provision(UserId::new(), AuthSubject::parse("g-9").unwrap(), Email::parse("pending@example.com").unwrap(), false);
		user.drain_events();
		user.change_email(Email::parse("pending@example.com").unwrap(), true);
		assert!(user.email_verified());
		let events = user.drain_events();
		assert_eq!(events.len(), 1);
		assert!(matches!(events[0], UserEvent::EmailChanged { .. }));
	}

	#[test]
	fn revoke_increments_version_and_emits() {
		let mut user = fixture();
		user.drain_events();
		assert_eq!(user.revoke_tokens(), 1);
		let events = user.drain_events();
		assert!(matches!(events[0], UserEvent::TokensRevoked { token_version: 1, .. }));
	}

	#[test]
	fn disable_is_idempotent_in_events() {
		let mut user = fixture();
		user.drain_events();
		user.disable();
		user.disable();
		assert_eq!(user.drain_events().len(), 1);
		assert!(!user.is_active());
	}

	#[test]
	fn update_profile_emits_event_and_getters_reflect_values() {
		let mut user = fixture();
		user.drain_events();
		user.update_profile(ProfileFields {
			legal_name: Some("Ada Lovelace".into()),
			preferred_name: Some("Ada".into()),
			phone: Some("+10000000000".into()),
			date_of_birth: Some("1815-12-10".into()),
			nationality: Some("GB".into()),
			tax_residence: Some("GB".into()),
			residential_address: Some("12 Analytical Engine St".into()),
			language: Some("en".into()),
			base_currency: Some("USDT".into()),
			timezone: Some("Europe/London".into()),
		});
		let events = user.drain_events();
		assert_eq!(events.len(), 1);
		assert!(matches!(events[0], UserEvent::ProfileUpdated { .. }));
		assert_eq!(user.legal_name(), Some("Ada Lovelace"));
		assert_eq!(user.preferred_name(), Some("Ada"));
		assert_eq!(user.phone(), Some("+10000000000"));
		assert_eq!(user.date_of_birth(), Some("1815-12-10"));
		assert_eq!(user.nationality(), Some("GB"));
		assert_eq!(user.tax_residence(), Some("GB"));
		assert_eq!(user.residential_address(), Some("12 Analytical Engine St"));
		assert_eq!(user.language(), Some("en"));
		assert_eq!(user.base_currency(), Some("USDT"));
		assert_eq!(user.timezone(), Some("Europe/London"));
	}

	#[test]
	fn event_round_trips_through_json() {
		let mut user = fixture();
		let event = user.drain_events().pop().unwrap();
		let json = serde_json::to_string(&event).unwrap();
		let back: UserEvent = serde_json::from_str(&json).unwrap();
		assert!(matches!(back, UserEvent::Provisioned { email_verified: true, .. }));
	}
}
