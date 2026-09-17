import type { Translate } from "@evinvest/i18n";
import { Item, ItemContent, ItemDescription, ItemMedia, ItemTitle } from "@evinvest/uikit";
import { IdCard, LogIn, Wallet } from "lucide-react";
import type { LucideIcon } from "lucide-react";

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
        {t("auth.signup.next")}
      </h2>
      <ol className="flex flex-col gap-4">
        <Step icon={LogIn} title={t("auth.signup.step.signIn.title")} body={t("auth.signup.step.signIn.body")} />
        <Step icon={IdCard} title={t("auth.signup.step.verify.title")} body={t("kyc.dialog.timeBody")} />
        <Step icon={Wallet} title={t("auth.signup.step.fund.title")} body={t("auth.signup.step.fund.body")} />
      </ol>
    </section>
  );
}

// The same `Item` composition as the verification dialog's steps, so the story a reader
// is told here and the one they meet after signing in look like one story.
function Step({ icon: Icon, title, body }: { icon: LucideIcon; title: string; body: string }) {
  return (
    <Item asChild className="items-start gap-3 rounded-none p-0">
      <li>
        <ItemMedia variant="icon" className="rounded-lg border-0 bg-secondary text-ink-mid">
          <Icon aria-hidden />
        </ItemMedia>
        <ItemContent className="min-w-0">
          <ItemTitle className="font-semibold text-ink">{title}</ItemTitle>
          <ItemDescription className="line-clamp-none leading-snug">{body}</ItemDescription>
        </ItemContent>
      </li>
    </Item>
  );
}
