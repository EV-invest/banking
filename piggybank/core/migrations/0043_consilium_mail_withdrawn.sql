-- 0043: a governance mail WITHDRAWN before it was delivered (#319).
--
-- A holder's notice is written in the transaction that schedules the change, and it
-- leaves through the queue whenever the relay is up. Cancel the change while the relay is
-- down and the notice is still queued; the relay coming back then tells every holder that
-- terms which will never bind "change on <date>". A withdrawn row is terminal: the worker
-- never picks it up, and it is not a notice anybody is still owed.
--
-- WHY A COLUMN AND NOT THE ATTEMPT CEILING. A row pinned at the ceiling is one the mailer
-- GAVE UP on — an alert, and the figure the operator's acknowledgement (0042) is offered
-- over. A withdrawn notice is neither: nobody failed to reach anybody, the fact was taken
-- back. The audit trail has to keep the two apart.
--
-- Expand only: one nullable column, nothing rewritten. A row written by a binary that does
-- not know the column reads as "not withdrawn".
--
-- THE ROLL-OUT WINDOW. `ev-banking-piggybank` rolls with maxSurge=1 / maxUnavailable=0: the
-- new pod applies this migration and serves requests (cancel included) while the old pod
-- still holds the mailer's singleton lock. A cancel in that overlap writes `withdrawn_at`
-- that the OLD worker does not filter on: it hands the withdrawn notice to the relay — the
-- holder is told about terms that will never bind — and its `SET sent_at = now()` then trips
-- the CHECK below, so its whole pass fails on that row, every sweep, until the old pod is
-- gone. The mail was delivered; the row says withdrawn and unsent. Operator's move, per
-- such row: it is a delivered mail and the audit trail should say so — set `sent_at` and
-- clear `withdrawn_at` by hand (`UPDATE consilium_mail SET sent_at = now(), withdrawn_at =
-- NULL WHERE id = …`), with the relay's log as the evidence. Avoid the window by not
-- cancelling a fee-policy change while a roll-out is in flight.

SET lock_timeout = '3s';

ALTER TABLE consilium_mail ADD COLUMN withdrawn_at TIMESTAMPTZ;

-- A delivered mail cannot be taken back, and a withdrawn one is never delivered.
ALTER TABLE consilium_mail ADD CONSTRAINT consilium_mail_withdrawn_is_unsent
    CHECK (sent_at IS NULL OR withdrawn_at IS NULL);
