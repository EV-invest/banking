"use client";

// "Deployed versions": what each component of the system is running in production, by
// repository, against the newest tag that repository has. The rows are the deploy's own
// catalogue, enriched by the BFF from GitHub — this card only reads and links.

import { Package } from "lucide-react";
import { Fragment } from "react";

import { useLocale, useT } from "@evinvest/i18n/react";
import { Card, CardContent, Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle, Skeleton, Table, TableBody, TableHead, TableHeader, TableRow } from "@evinvest/uikit";

import type { AdminDeployments } from "@/shared/contracts/admin";
import { formatRfc3339Moment } from "@/shared/lib/datetime";
import { ResourceError } from "@/shared/ui/resource-error";
import { groupByRepo } from "@/views/admin/overview/lib/deployments";
import { ComponentRow, ExternalAnchor } from "@/views/admin/overview/ui/deployed-versions-row";

// The house table idiom (`views/admin/fees/ui/change-history.tsx`): uikit's `Table` carries
// the borders, the cell padding and the scroll wrapper; only the header treatment is ours.
const HEAD = "h-8 text-xs font-medium uppercase tracking-wide text-ink-soft";

interface Props {
  deployments: AdminDeployments | null;
  /** The read failed with nothing to show — `null` while stale data is still on screen. */
  error: Error | null;
  onRetry: () => void;
  retrying: boolean;
}

export function DeployedVersions({ deployments, error, onRetry, retrying }: Props) {
  const t = useT();
  const locale = useLocale();

  return (
    <Card>
      <CardContent className="space-y-4 py-5">
        <div>
          <h2 className="text-base font-semibold">{t("admin.overview.deployed")}</h2>
          <p className="text-xs text-ink-soft">{t("admin.overview.deployedSub")}</p>
        </div>
        {!deployments ? (
          error ? (
            <ResourceError error={error} onRetry={onRetry} retrying={retrying} />
          ) : (
            <Skeleton className="h-32 w-full" />
          )
        ) : !deployments.available ? (
          // No `EmptyContent`: nothing the operator does here produces the catalogue —
          // the deploy mounts it, or it is not there.
          <Empty className="border">
            <EmptyHeader>
              <EmptyMedia variant="icon">
                <Package />
              </EmptyMedia>
              <EmptyTitle>{t("admin.overview.deployedUnavailable")}</EmptyTitle>
              <EmptyDescription>{t("admin.overview.deployedUnavailableHint")}</EmptyDescription>
            </EmptyHeader>
          </Empty>
        ) : (
          <>
            <Table>
              <TableHeader>
                {/* i18n-max: 12 per header — six columns share the card's width. */}
                <TableRow>
                  <TableHead className={HEAD}>{t("admin.overview.col.component")}</TableHead>
                  <TableHead className={HEAD}>{t("admin.overview.col.version")}</TableHead>
                  <TableHead className={HEAD}>{t("admin.overview.col.released")}</TableHead>
                  <TableHead className={HEAD}>{t("admin.overview.col.pr")}</TableHead>
                  <TableHead className={HEAD}>{t("admin.overview.col.commit")}</TableHead>
                  <TableHead className={HEAD}>{t("admin.col.status")}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {groupByRepo(deployments.components).map((group) => (
                  <Fragment key={group.repo ?? ""}>
                    <TableRow className="hover:bg-transparent">
                      <TableHead scope="colgroup" colSpan={6} className="h-auto pb-1 pt-4 font-mono-tech text-xs font-medium text-ink-soft">
                        {group.repo && group.repoUrl ? <ExternalAnchor href={group.repoUrl}>{group.repo}</ExternalAnchor> : (group.repo ?? t("admin.overview.deployedOtherImages"))}
                      </TableHead>
                    </TableRow>
                    {group.components.map((c) => (
                      <ComponentRow key={c.name} component={c} />
                    ))}
                  </Fragment>
                ))}
              </TableBody>
            </Table>
            <p className="text-xs text-ink-soft">{t("admin.overview.deployedUpdated", { at: formatRfc3339Moment(deployments.fetched_at, locale) })}</p>
          </>
        )}
      </CardContent>
    </Card>
  );
}
