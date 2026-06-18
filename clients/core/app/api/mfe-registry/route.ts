import { NextResponse } from "next/server";

import { loadRegistry } from "@/shared/mfe/registry";

// Serves the microfrontend registry to the browser. The host resolves each
// <RemoteElement> by looking a logical name up here, so remotes can be deployed
// independently — change the registry, not core.
export async function GET() {
  return NextResponse.json(await loadRegistry());
}
