-- The parking lot in front of the identity -> money mirror: lifecycle events whose subject
-- has no local `users` row yet.
--
-- NUMBERED 0028, NOT 0027, ON PURPOSE. Open PR #209 already claims 0027; two files sharing
-- a version is a duplicate-version failure at the next boot, and migrations run on startup
-- here, so the collision would be an outage rather than a merge conflict. A gap is the
-- cheaper artifact.
--
-- WHY THIS EXISTS. `bridge::apply` used to DROP an event for a subject it did not know:
-- someone who has never signed in to banking, or anyone at all when banking was deployed
-- after the concierge and their CREATED has already fallen out of the outbox window. The
-- global cursor advanced past it anyway (`drain` moves it after the loop, unconditionally)
-- and the concierge never re-delivers, since it reads `WHERE position > after_position`.
-- The event was consumed into the void: a KYC_CHANGED that never landed left the user at
-- the schema default of level 0 and locked out of deposits and withdrawals, and a
-- SUSPENDED that never landed left the row created at sign-in with `frozen = FALSE` --
-- a fail-OPEN freeze on a money plane.
--
-- WHY PARK INSTEAD OF STOPPING THE CURSOR. Head-of-line blocking is simpler, and it is
-- what an event this build cannot *read* gets instead (see the `Unspecified` arm in
-- `infrastructure/bridge.rs`) -- that one is unblocked by a deploy that is already on its
-- way. An orphan subject is not: it may never acquire a local row at all, and one of them
-- would wedge the mirror for EVERY user -- freezes and tier revocations included -- with
-- no operator action that resolves it. Parking keeps the stream moving for everyone else.
--
-- WHY COLUMNS AND NOT AN ENCODED BLOB. An operator asking "who is stuck, on what, and
-- since when" gets an answer out of psql. The price is a rule, enforced where it bites:
-- every field `bridge::apply` reads off a `UserLifecycleEvent` needs a column here, or a
-- parked event replays with that field blank. `replay_deferred` rebuilds the event from
-- exactly these columns.
CREATE TABLE bridge_deferred_event (
    -- The concierge outbox event id verbatim -- TEXT, not UUID, because the contract says
    -- string and this table must never reject an event it exists to preserve. As the key
    -- it also makes a redelivery park once.
    event_id          TEXT        PRIMARY KEY,
    auth_subject      TEXT        NOT NULL,
    -- The numeric proto enum value. Always a kind this build can name: an unreadable kind
    -- stops the cursor instead of parking, because nothing here could ever replay it.
    kind              INTEGER     NOT NULL,
    sequence          BIGINT      NOT NULL,
    concierge_user_id UUID,
    email             TEXT        NOT NULL,
    email_verified    BOOLEAN     NOT NULL,
    kyc_level         INTEGER     NOT NULL,
    role              TEXT        NOT NULL,
    token_version     BIGINT      NOT NULL,
    occurred_at       BIGINT      NOT NULL,
    deferred_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Serves both readers: the replay sweep, which drains a subject's backlog in `sequence`
-- order, and the apply path's "is an earlier event still parked for this subject?" guard
-- that keeps a late arrival from overtaking a parked one.
CREATE INDEX bridge_deferred_event_subject_sequence_idx ON bridge_deferred_event (auth_subject, sequence);
