// Browser → BFF profile client. Thin typed fetchers over /api/users; the shapes are the
// proto-derived types. The PATCH carries the CSRF double-submit header. No tokens here —
// the BFF holds them.

import { csrfHeader } from "@/shared/lib/csrf-client";
import type { UpdateProfileRequest, UserProfile } from "@/shared/contracts";

export async function fetchProfile(): Promise<UserProfile> {
  const res = await fetch("/api/users", { headers: { accept: "application/json" } });
  const data = (await res.json().catch(() => ({}))) as UserProfile & { error?: string };
  if (!res.ok) throw new Error(data.error ?? `request failed (${res.status})`);
  return data;
}

export async function saveProfile(fields: UpdateProfileRequest): Promise<UserProfile> {
  const res = await fetch("/api/users", {
    method: "PATCH",
    headers: { "content-type": "application/json", ...csrfHeader() },
    body: JSON.stringify(fields),
  });
  const data = (await res.json().catch(() => ({}))) as UserProfile & { error?: string };
  if (!res.ok) throw new Error(data.error ?? `save failed (${res.status})`);
  return data;
}
