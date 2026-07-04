"use client";

import { TriangleAlert } from "lucide-react";
import { type ReactNode, useEffect, useState } from "react";

import { Button, Card, CardContent, Input, Skeleton } from "@evinvest/uikit";

import { fetchCabinet, setAnnouncement, setFeatureFlag, setMaintenance, setReadOnly } from "@/entities/admin/api/admin-client";
import { apiPath } from "@/shared/config/base-path";
import type { CabinetConfig, FeatureFlag } from "@/shared/contracts/admin";
import { AdminHeader, StatusDot, Toggle } from "@/views/admin/ui/shell";

interface MfeEntry {
  name: string;
  tag: string;
  scriptUrl: string;
  kind: string;
}

export function CabinetView() {
  const [config, setConfig] = useState<CabinetConfig | null>(null);
  const [mfes, setMfes] = useState<MfeEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    fetchCabinet()
      .then((c) => active && setConfig(c))
      .catch((e: Error) => active && setError(e.message));
    fetch(apiPath("/api/mfe-registry"), { headers: { accept: "application/json" } })
      .then((r) => r.json() as Promise<MfeEntry[]>)
      .then((m) => active && setMfes(Array.isArray(m) ? m : []))
      .catch(() => active && setMfes([]));
    return () => {
      active = false;
    };
  }, []);

  const platform = config?.platform;

  const toggleFlag = async (flag: FeatureFlag) => {
    try {
      const next = await setFeatureFlag({ key: flag.key, description: flag.description, enabled: !flag.enabled, rollout: flag.rollout });
      setConfig((c) => (c ? { ...c, platform: next } : c));
    } catch (e) {
      setError((e as Error).message);
    }
  };

  const toggleMaintenance = async (enabled: boolean) => {
    try {
      const next = await setMaintenance(enabled);
      setConfig((c) => (c ? { ...c, platform: next } : c));
    } catch (e) {
      setError((e as Error).message);
    }
  };

  const toggleReadOnly = async (enabled: boolean) => {
    try {
      const mode = await setReadOnly(enabled);
      setConfig((c) => (c ? { ...c, read_only: mode.read_only } : c));
    } catch (e) {
      setError((e as Error).message);
    }
  };

  return (
    <div className="space-y-8 px-8 pb-10 pt-6">
      <AdminHeader eyebrow="Administer" title="Cabinet" subtitle="Host shell — microfrontend registry, feature flags and content" />

      {error && (
        <p className="flex items-center gap-2 text-sm text-destructive">
          <TriangleAlert className="size-4" /> {error}
        </p>
      )}

      <div className="grid gap-6 lg:grid-cols-2">
        <Panel title="Microfrontend registry" subtitle="Resolved by clients/core · /api/mfe-registry">
          {!mfes ? (
            <Skeleton className="h-32 w-full" />
          ) : mfes.length === 0 ? (
            <p className="py-6 text-center text-sm text-muted-foreground">No microfrontends registered.</p>
          ) : (
            <div className="divide-y divide-border">
              {mfes.map((m) => (
                <div key={m.tag} className="flex items-center justify-between gap-3 py-3">
                  <div className="min-w-0">
                    <p className="truncate text-sm font-medium">{m.name}</p>
                    <p className="truncate font-mono-tech text-xs text-muted-foreground">{m.tag}</p>
                  </div>
                  <div className="flex shrink-0 items-center gap-3">
                    <span className="rounded-md bg-foreground/[0.06] px-2 py-0.5 text-xs capitalize text-main-mist">{m.kind}</span>
                    <StatusDot status="healthy" label="Registered" />
                  </div>
                </div>
              ))}
            </div>
          )}
        </Panel>

        <Panel title="Feature flags" subtitle="Gating cabinet features & MFE mounts">
          {!platform ? (
            <Skeleton className="h-32 w-full" />
          ) : platform.flags.length === 0 ? (
            <p className="py-6 text-center text-sm text-muted-foreground">No flags yet. PostHog experiments render here when configured.</p>
          ) : (
            <div className="divide-y divide-border">
              {platform.flags.map((f) => (
                <div key={f.key} className="flex items-center justify-between gap-3 py-3">
                  <div className="min-w-0">
                    <p className="truncate font-mono-tech text-sm">{f.key}</p>
                    <p className="truncate text-xs text-muted-foreground">
                      {f.rollout}% {f.description ? `· ${f.description}` : ""}
                    </p>
                  </div>
                  <Toggle on={f.enabled} onChange={() => toggleFlag(f)} label={f.key} />
                </div>
              ))}
            </div>
          )}
        </Panel>

        <Panel title="Announcement" subtitle="The live banner across the cabinet">
          {!config ? <Skeleton className="h-28 w-full" /> : <AnnouncementForm config={config} onSaved={(next) => setConfig((c) => (c ? { ...c, platform: next } : c))} onError={setError} />}
        </Panel>

        <Panel title="Maintenance & operations" subtitle="Cabinet holding page + money-plane kill-switch">
          {!config ? (
            <Skeleton className="h-28 w-full" />
          ) : (
            <div className="space-y-4">
              <ToggleRow
                label="Maintenance mode"
                hint="Holding page on the cabinet (identity plane)"
                on={config.platform.maintenance_mode}
                onChange={toggleMaintenance}
              />
              <ToggleRow label="Read-only mode" hint="Pause deposits & withdrawals (money plane)" on={config.read_only} onChange={toggleReadOnly} />
            </div>
          )}
        </Panel>
      </div>
    </div>
  );
}

function Panel({ title, subtitle, children }: { title: string; subtitle: string; children: ReactNode }) {
  return (
    <Card>
      <CardContent className="space-y-4 py-5">
        <div>
          <h2 className="text-base font-semibold">{title}</h2>
          <p className="text-xs text-muted-foreground">{subtitle}</p>
        </div>
        {children}
      </CardContent>
    </Card>
  );
}

function ToggleRow({ label, hint, on, onChange }: { label: string; hint: string; on: boolean; onChange: (next: boolean) => void }) {
  return (
    <div className="flex items-center justify-between gap-3">
      <div>
        <p className="text-sm">{label}</p>
        <p className="text-xs text-muted-foreground">{hint}</p>
      </div>
      <Toggle on={on} onChange={onChange} label={label} />
    </div>
  );
}

function AnnouncementForm({ config, onSaved, onError }: { config: CabinetConfig; onSaved: (next: CabinetConfig["platform"]) => void; onError: (e: string) => void }) {
  const [title, setTitle] = useState(config.platform.announcement_title);
  const [body, setBody] = useState(config.platform.announcement_body);
  const [active, setActive] = useState(config.platform.announcement_active);
  const [saving, setSaving] = useState(false);

  const save = async () => {
    setSaving(true);
    try {
      const next = await setAnnouncement({ title, body, active });
      onSaved(next);
    } catch (e) {
      onError((e as Error).message);
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="space-y-3">
      <Input value={title} onChange={(e) => setTitle(e.target.value)} placeholder="Announcement title" />
      <Input value={body} onChange={(e) => setBody(e.target.value)} placeholder="Body" />
      <div className="flex items-center justify-between">
        <ToggleRow label="Live" hint="Show the banner now" on={active} onChange={setActive} />
      </div>
      <Button type="button" variant="outline" size="sm" disabled={saving} onClick={save}>
        Save announcement
      </Button>
    </div>
  );
}
