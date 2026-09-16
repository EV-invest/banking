"use client";

// One component of the "Deployed versions" table. Every link here leaves the cabinet for
// GitHub, so these are plain anchors in a new tab, not `cabinetPath()` routes.

import type { ReactNode } from "react";

import { useLocale, useT } from "@evinvest/i18n/react";
import { Badge, TableCell, TableRow } from "@evinvest/uikit";

import type { DeployedComponent } from "@/shared/contracts/admin";
import { formatRfc3339Moment } from "@/shared/lib/datetime";
import { componentStatus, shortSha } from "@/views/admin/overview/lib/deployments";

// Underlined at rest, as the console's text links are (`views/admin/users/ui/role-field.tsx`),
// with the border token for the rule so four link columns don't read as a wall of ink.
const LINK = "rounded-xs underline decoration-border underline-offset-2 outline-none hover:decoration-current focus-visible:ring-2 focus-visible:ring-ring";

export function ComponentRow({ component: c }: { component: DeployedComponent }) {
  const t = useT();
  const locale = useLocale();
  const status = componentStatus(c);
  const pr = c.release?.pr ?? null;
  return (
    <TableRow>
      <TableCell className="font-medium" title={c.image}>
        {c.name}
      </TableCell>
      <TableCell className="font-mono-tech text-xs">{c.tag_url ? <ExternalAnchor href={c.tag_url}>{c.tag}</ExternalAnchor> : c.tag}</TableCell>
      <TableCell className="tabular-nums text-ink-soft">{formatRfc3339Moment(c.release?.committed_at, locale)}</TableCell>
      <TableCell className="text-ink-soft">
        {pr ? (
          <div className="max-w-80 truncate" title={pr.title}>
            <ExternalAnchor href={pr.url}>
              <span className="tabular-nums text-ink">#{pr.number}</span> {pr.title}
            </ExternalAnchor>
          </div>
        ) : (
          "—"
        )}
      </TableCell>
      <TableCell className="font-mono-tech text-xs text-ink-soft">{c.release ? <ExternalAnchor href={c.release.commit_url}>{shortSha(c.release.commit_sha)}</ExternalAnchor> : "—"}</TableCell>
      <TableCell>
        {/* i18n-max: 12 per badge — the newer-tag chip also carries a version string. */}
        {status.kind === "upToDate" ? (
          <Badge variant="success">{t("admin.overview.deployedUpToDate")}</Badge>
        ) : status.kind === "newer" ? (
          <Badge asChild variant="outline" className="border-accent-warn/40 text-accent-warn">
            <a href={status.url} target="_blank" rel="noreferrer">
              {t("admin.overview.deployedNewer", { tag: status.tag })}
              <span className="sr-only">{t("admin.overview.opensGithub")}</span>
            </a>
          </Badge>
        ) : status.kind === "githubError" ? (
          <Badge variant="outline" className="max-w-60 border-accent-error/40 text-accent-error" title={status.error}>
            <span className="min-w-0 truncate">{t("admin.overview.deployedGithubError", { error: status.error })}</span>
          </Badge>
        ) : (
          "—"
        )}
      </TableCell>
    </TableRow>
  );
}

export function ExternalAnchor({ href, children }: { href: string; children: ReactNode }) {
  const t = useT();
  return (
    <a href={href} target="_blank" rel="noreferrer" className={LINK}>
      {children}
      <span className="sr-only">{t("admin.overview.opensGithub")}</span>
    </a>
  );
}
