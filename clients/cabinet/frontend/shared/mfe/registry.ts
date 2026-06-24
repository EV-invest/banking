// Server-side reader for the microfrontend registry.
//
// The registry maps logical names to {tag, scriptUrl, kind}. Independent deploys
// land by editing this file (or, in production, a per-env config the BFF fetches)
// — never by rebuilding cabinet. Served to the browser via /api/mfe-registry.

import { promises as fs } from "node:fs";
import path from "node:path";

import type { MfeEntry } from "./types";
import { parseRegistry } from "./validate";

export async function loadRegistry(): Promise<MfeEntry[]> {
  const file = path.join(process.cwd(), "mfe-registry.json");
  const raw = await fs.readFile(file, "utf8");
  return parseRegistry(JSON.parse(raw));
}

export async function findMfe(name: string): Promise<MfeEntry | undefined> {
  const registry = await loadRegistry();
  return registry.find((entry) => entry.name === name);
}
