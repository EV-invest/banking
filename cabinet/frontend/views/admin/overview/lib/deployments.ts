// The reading behind the "Deployed versions" card, kept out of the component so that the
// three verdicts a row can carry are pinned by `node --test` rather than by eye.
//
// Import-free on purpose (types only): the runner does not resolve the `@/` alias for
// value imports, so a module that reached for one could not be exercised.

import type { DeployedComponent } from "../../../../shared/contracts/admin.ts";

/** Components under the repository that built them; images we do not own come last. */
export interface RepoGroup {
  /** "owner/name", or null for the images that are not ours. */
  repo: string | null;
  repoUrl: string | null;
  components: DeployedComponent[];
}

/**
 * What the Status column says about one component.
 *
 * `newer` outranks the error: when GitHub answered with a tag, that answer is the fact
 * the operator came for, and whatever failed beside it (a PR lookup, say) is noise. Only
 * a row GitHub could not describe at all falls through to the error text, and a row with
 * neither is `unknown` — a dash, never a false "up to date".
 */
export type ComponentStatus =
  | { kind: "upToDate" }
  | { kind: "newer"; tag: string; url: string }
  | { kind: "githubError"; error: string }
  | { kind: "unknown" };

export function componentStatus(c: DeployedComponent): ComponentStatus {
  if (c.latest_tag) {
    return c.latest_tag.tag === c.tag ? { kind: "upToDate" } : { kind: "newer", tag: c.latest_tag.tag, url: c.latest_tag.url };
  }
  if (c.github_error) return { kind: "githubError", error: c.github_error };
  return { kind: "unknown" };
}

/** Group by owning repository, in first-seen order, with the ownerless images last. */
export function groupByRepo(components: readonly DeployedComponent[]): RepoGroup[] {
  const groups = new Map<string | null, RepoGroup>();
  for (const c of components) {
    const group = groups.get(c.repo);
    if (group) {
      group.components.push(c);
      // The URL is per component on the wire; any one of them is the repository's.
      group.repoUrl ??= c.repo_url;
    } else {
      groups.set(c.repo, { repo: c.repo, repoUrl: c.repo_url, components: [c] });
    }
  }
  const named = [...groups.values()].filter((g) => g.repo !== null);
  const ownerless = groups.get(null);
  return ownerless ? [...named, ownerless] : named;
}

/** The seven-character prefix GitHub itself abbreviates a commit to. */
export function shortSha(sha: string): string {
  return sha.slice(0, 7);
}
