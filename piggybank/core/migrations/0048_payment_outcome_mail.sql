-- 0048: the governance mail queue learns `payment_outcome` (#238).
--
-- A payment decided by its subject's consent can die without a verdict: the consent link
-- burns on five wrong codes, or is voided under the subject when their sessions are revoked
-- or their mailbox changes. Until now the order was rejected (or failed at execution) and
-- nobody was told — neither the investor whose money it was nor the admin who opened it.
-- Concierge now relays a kind for exactly this (concierge#95, `PAYMENT_OUTCOME`), and the
-- row is written in the same transaction as the rejection, like every governance mail.
--
-- The row names its PAYMENT, so `consilium_mail_names_one_subject` (0036) is untouched:
-- one subject per row, `payment_id` here, as the consent invitation's is.
--
-- WHY THIS SHIPS WITH THE RUST ARM. The worker deserializes every row by its `kind`; a row
-- of a kind the binary does not know is retired unread. The CHECK and the
-- `GovernanceMail::PaymentOutcome` variant therefore land together.
--
-- The list is the one 0047 left — `payout_approval` is gone for good, the revenue-payout
-- invitation nothing writes any more — plus this kind. Rebuilt in one DROP + ADD, so any
-- sibling migration changing the list conflicts here visibly. No row of the new kind exists
-- yet, so the ADD validates only rows of the six kinds 0047 admits; `consilium_mail` is a
-- queue of governance mail — small — and the lock is held for the validation scan alone.
--
-- EXPAND ONLY, and reversible while no `payment_outcome` row exists: narrow the CHECK back
-- to the six kinds of 0047. With such rows present, narrowing requires deleting them
-- first — they are notices about orders that moved nothing, not money records.
--
-- THE ROLL-OUT WINDOW. The old pod keeps the mailer's singleton lock while the new pod
-- serves: a `payment_outcome` row written by the new pod in that overlap is read by the old
-- worker, whose `GovernanceMail` has no such variant: it charges the row one attempt per
-- sweep as an unreadable payload (`last_error` says so) and gives up after ten, about five
-- minutes. A roll-out shorter than that costs such a row a few attempts and nothing else;
-- one given up on is recoverable by hand once the new pod holds the lock:
-- `UPDATE consilium_mail SET attempts = 0, last_error = NULL
--  WHERE kind = 'payment_outcome' AND sent_at IS NULL AND withdrawn_at IS NULL`.

SET lock_timeout = '3s';
SET statement_timeout = '30s';

ALTER TABLE consilium_mail DROP CONSTRAINT consilium_mail_kind_check;
ALTER TABLE consilium_mail ADD CONSTRAINT consilium_mail_kind_check
    CHECK (kind IN ('payout_outcome', 'token_burned', 'payment_consent', 'payment_approval', 'fee_policy_approval', 'fee_policy_notice', 'payment_outcome'));
