import type { Translate } from "@evinvest/i18n";
import { IdCard, LogIn, Wallet } from "lucide-react";

import { Step } from "@/features/kyc";

// "What happens next" for a first visit (#391): the three things between this page and
// holding units, in order, so a newcomer sent here by a landing CTA knows what they are
// signing up for before they press the one button. Deliberately without figures — no
// minimum, no timing beyond the owner-approved sentence the verification dialog already
// uses (`kyc.dialog.timeBody`), which is reused rather than copied so the two cannot
// drift apart.
export function NextSteps({ t }: { t: Translate }) {
  return (
    <section aria-labelledby="login-next-steps" className="flex flex-col gap-4">
      <h2 id="login-next-steps" className="text-xs font-semibold tracking-widest text-ink-soft uppercase">
        {t("auth.signup.next", "What happens next")}
      </h2>
      <ol className="flex flex-col gap-4">
        <Step asChild icon={LogIn} title={t("auth.signup.step.signIn.title", "Sign in with Google")} body={t("auth.signup.step.signIn.body", "Your cabinet is created on the first sign-in.")} />
        <Step asChild icon={IdCard} title={t("auth.signup.step.verify.title", "Verify your identity")} body={t("kyc.dialog.timeBody", "A few minutes to submit. Most checks are decided within the hour.")} />
        <Step asChild icon={Wallet} title={t("auth.signup.step.fund.title", "Deposit and subscribe")} body={t("auth.signup.step.fund.body", "Fund your wallet — a deposit is credited once the network confirms it — and subscribe to an allocation.")} />
      </ol>
    </section>
  );
}
