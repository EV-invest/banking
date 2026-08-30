import { StatusScreen } from "@evinvest/uikit";
import { localePath, translator, type Locale } from "@evinvest/i18n";

import { messagesFor } from "@/shared/config/i18n";
import { cabinetPath } from "@/shared/config/base-path";

/**
 * The 404 / 403 / 401 surfaces, in the reader's language.
 *
 * The uikit's ready-made `NotFound` / `Forbidden` pages bake their copy in, which
 * is what the cabinet rendered until now: an English apology under Russian chrome,
 * at the one moment the product is already failing the reader. `StatusScreen` is
 * the same component underneath and takes every string as a prop, so this needs no
 * uikit release. Same shape as the conductor's `views/status`, and the catalogue
 * entries are ported from it verbatim — a reader who crosses the zone boundary
 * onto a 404 should not meet a differently-worded one.
 *
 * Server-rendered on purpose: Next hands these pages no props, so the locale comes
 * from `next/root-params` at the call site (`currentLocale()`), never a client
 * hook. The 500 cannot use this — Next requires `error.tsx` to be a Client
 * Component — and reads the catalogue through `I18nProvider` instead.
 *
 * "Home" here is the cabinet dashboard, not the landing: someone signed in who hit
 * a dead cabinet URL wants their portfolio back, not the marketing site. The
 * secondary CTA does leave the zone — contact and sign-in are shell-owned.
 */
export type StatusKind = "notFound" | "forbidden" | "unauthorized";

const ACCENT = { notFound: "teal", forbidden: "gold", unauthorized: "gold" } as const;
const CODE = { notFound: "404", forbidden: "403", unauthorized: "401" } as const;

export function LocalisedStatus({ kind, locale }: { kind: StatusKind; locale: Locale }) {
  const t = translator(messagesFor(locale), locale);
  const secondary =
    kind === "unauthorized"
      ? { label: t("status.signIn"), href: cabinetPath(locale, "/login") }
      : {
          // The landing's contact page is a conductor route on the same origin, so
          // `localePath`, not `cabinetPath`.
          label: kind === "forbidden" ? t("status.requestAccess") : t("status.contactTeam"),
          href: localePath(locale, "/contact"),
        };
  return (
    <StatusScreen
      accent={ACCENT[kind]}
      code={CODE[kind]}
      eyebrow={t(`status.${kind}.eyebrow`)}
      headlineLead={t(`status.${kind}.headlineLead`)}
      headlineAccent={t(`status.${kind}.headlineAccent`)}
      subtext={t(`status.${kind}.subtext`)}
      links={[
        { label: t("status.backHome"), href: cabinetPath(locale, "/"), leadingArrow: true },
        { ...secondary, variant: "outline" },
      ]}
    />
  );
}
