import type { Locale, Messages } from "@evinvest/i18n";

// The four strings the account chip renders, and nothing else.
//
// The chip is not a cabinet screen. It is bundled as a custom element and injected
// by the conductor into **every page of the public site**, where there is no
// `I18nProvider` above it — so it reads the locale itself and builds its own
// translator. The obvious way to feed that translator is `messagesFor()`, and the
// obvious way is wrong: that module imports all five `common.json` catalogues and
// runs the resolve policy over them at module scope. The chip bundle came out at
// 964 KB, over half of it catalogue, and an anonymous visitor to the marketing site
// was downloading the German translation of the admin treasury invariant note to
// render the word "Verified".
//
// Four keys times five locales is small enough to inline, so it is inlined. The
// values here are copies, which is a real duplication — `chip-messages.test.ts`
// exists to make it a checked one: it fails if any of these drifts from
// `messages/<locale>/common.json`.
export const CHIP_KEYS = ["auth.investorPortal", "auth.signOut", "ui.account", "ui.verified"] as const;

const CHIP_MESSAGES: Record<Locale, Messages> = {
  en: {
    "auth.investorPortal": "Investor Portal",
    "auth.signOut": "Sign out",
    "ui.account": "Account",
    "ui.verified": "Verified"
  },
  ru: {
    "auth.investorPortal": "Кабинет инвестора",
    "auth.signOut": "Выйти",
    "ui.account": "Аккаунт",
    "ui.verified": "Проверен"
  },
  vi: {
    "auth.investorPortal": "Cổng nhà đầu tư",
    "auth.signOut": "Đăng xuất",
    "ui.account": "Tài khoản",
    "ui.verified": "Đã xác minh"
  },
  fr: {
    "auth.investorPortal": "Espace investisseur",
    "auth.signOut": "Se déconnecter",
    "ui.account": "Compte",
    "ui.verified": "Vérifié"
  },
  de: {
    "auth.investorPortal": "Investorenportal",
    "auth.signOut": "Abmelden",
    "ui.account": "Konto",
    "ui.verified": "Bestätigt"
  }
};

/** The chip's catalogue for `locale`, falling back to English. */
export function chipMessages(locale: Locale): Messages {
  return CHIP_MESSAGES[locale] ?? CHIP_MESSAGES.en;
}
