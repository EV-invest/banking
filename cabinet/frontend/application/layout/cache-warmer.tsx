"use client";

// Warms the reads shared by most screens once the signed-in shell is up, and drops every
// cached value when the session ends. Renders nothing.
//
// The clear is the half that matters for correctness: this tab may be handed to a different
// account (the shell answers "not authenticated", the keeper moves to /login), and no figure
// belonging to the previous one may survive that.

import { useEffect } from "react";

import { warmShell } from "@/application/prefetch";
import { clearResources } from "@/shared/lib/resource";
import { onSessionChange } from "@/shared/lib/session";

export function CacheWarmer() {
  useEffect(() => warmShell(), []);

  useEffect(
    () =>
      onSessionChange((session) => {
        if (!session.authenticated) clearResources();
      }),
    [],
  );

  return null;
}
