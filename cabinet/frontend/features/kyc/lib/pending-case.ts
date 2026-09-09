// Whether the user has an identity-verification case in flight at the vendor. `/kyc/start`
// has no rate limit (EV-invest/concierge#44), and a provider webhook can lag well behind
// the redirect back into the cabinet — `kyc_level` staying at 0 is the expected state for
// the whole window a returning user is most likely to press Start again, not evidence that
// nothing was submitted. Nothing else in the profile or the route models "a case is already
// open", so this flag is the client's own record of that window.
//
// sessionStorage, paired with the email — the same shape as the account-chip identity cache
// (`features/account-chip/model/identity-cache.ts`): it dies with the tab, so a different
// account signed into the same browser never inherits someone else's pending case, but it
// survives the vendor redirect and the trip back, which stays inside one tab.

const PENDING_KEY = "ev.cabinet.kyc-pending";

// A vendor session and its webhook normally resolve in minutes. This is a ceiling for the
// abnormal case (a lost webhook, a stalled provider) so a case that never lands doesn't
// lock the button forever — past this point it is a support conversation regardless of
// what the button shows.
const PENDING_TTL_MS = 24 * 60 * 60 * 1000;

type Pending = { email: string; startedAt: number };

export function markVerificationPending(email: string) {
  try {
    sessionStorage.setItem(PENDING_KEY, JSON.stringify({ email, startedAt: Date.now() } satisfies Pending));
  } catch {
    // sessionStorage unavailable (private mode / disabled) — degrades to offering Start
    // again on return, no worse than before this existed.
  }
}

export function isVerificationPending(email: string): boolean {
  try {
    const raw = sessionStorage.getItem(PENDING_KEY);
    if (!raw) return false;
    const pending = JSON.parse(raw) as Pending;
    const fresh = pending.email === email && Date.now() - pending.startedAt < PENDING_TTL_MS;
    if (!fresh) sessionStorage.removeItem(PENDING_KEY);
    return fresh;
  } catch {
    return false;
  }
}
