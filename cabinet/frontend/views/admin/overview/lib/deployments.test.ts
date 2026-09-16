// Run with `npm run test` (Node's built-in runner, native type-stripping).
//
// The card has three readings of a row — up to date, behind the newest tag, and one
// GitHub could not describe — plus the "not mounted" catalogue that must read as an
// empty state rather than a fault. Pinned here on the module the card renders from.
import assert from "node:assert/strict";
import test from "node:test";

import type { AdminDeployments, DeployedComponent } from "../../../../shared/contracts/admin.ts";
import { componentStatus, groupByRepo, shortSha } from "./deployments.ts";

function component(over: Partial<DeployedComponent>): DeployedComponent {
  return {
    name: "ev-banking-piggybank",
    image: "ghcr.io/ev-invest/ev_banking-piggybank",
    tag: "v0.17.0",
    repo: "ev-invest/banking",
    repo_url: "https://github.com/ev-invest/banking",
    tag_url: "https://github.com/ev-invest/banking/releases/tag/v0.17.0",
    release: {
      commit_sha: "3b90faff0123456789abcdef0123456789abcdef",
      commit_url: "https://github.com/ev-invest/banking/commit/3b90faff",
      committed_at: "2026-09-16T10:00:00Z",
      pr: { number: "344", title: "Merge pull request #344", url: "https://github.com/ev-invest/banking/pull/344", merged_at: "2026-09-16T10:00:00Z" },
    },
    latest_tag: { tag: "v0.17.0", url: "https://github.com/ev-invest/banking/releases/tag/v0.17.0" },
    github_error: null,
    ...over,
  };
}

test("a component on its repository's newest tag is up to date", () => {
  assert.deepEqual(componentStatus(component({})), { kind: "upToDate" });
});

test("a newer tag in the repository is reported with its link", () => {
  const c = component({ latest_tag: { tag: "v0.18.0", url: "https://github.com/ev-invest/banking/releases/tag/v0.18.0" } });
  assert.deepEqual(componentStatus(c), { kind: "newer", tag: "v0.18.0", url: "https://github.com/ev-invest/banking/releases/tag/v0.18.0" });
  assert.equal(c.release?.pr?.number, "344", "the PR is still there to link beside the verdict");
});

test("a newer tag outranks a GitHub error recorded beside it", () => {
  const c = component({ latest_tag: { tag: "v0.18.0", url: "u" }, github_error: "pull request lookup failed" });
  assert.equal(componentStatus(c).kind, "newer");
});

test("a row GitHub could not describe carries the error, never a false 'up to date'", () => {
  assert.deepEqual(componentStatus(component({ latest_tag: null, release: null, github_error: "rate limited" })), { kind: "githubError", error: "rate limited" });
  assert.deepEqual(componentStatus(component({ latest_tag: null, release: null, github_error: null })), { kind: "unknown" });
});

test("components group under their repository in first-seen order, ownerless images last", () => {
  const groups = groupByRepo([
    component({ name: "postgres", image: "docker.io/library/postgres", tag: "16", repo: null, repo_url: null, tag_url: null, release: null, latest_tag: null }),
    component({ name: "ev-banking-piggybank" }),
    component({ name: "ev-concierge", repo: "ev-invest/concierge", repo_url: "https://github.com/ev-invest/concierge" }),
    component({ name: "ev-banking-cabinet" }),
  ]);
  assert.deepEqual(
    groups.map((g) => [g.repo, g.components.map((c) => c.name)]),
    [
      ["ev-invest/banking", ["ev-banking-piggybank", "ev-banking-cabinet"]],
      ["ev-invest/concierge", ["ev-concierge"]],
      [null, ["postgres"]],
    ],
  );
  assert.equal(groups[0]?.repoUrl, "https://github.com/ev-invest/banking");
});

test("an unmounted catalogue has no groups to draw — the card shows its empty state", () => {
  const unavailable: AdminDeployments = { available: false, fetched_at: "2026-09-16T10:00:00Z", components: [] };
  assert.equal(unavailable.available, false);
  assert.deepEqual(groupByRepo(unavailable.components), []);
});

test("a commit abbreviates to GitHub's seven characters", () => {
  assert.equal(shortSha("3b90faff0123456789abcdef0123456789abcdef"), "3b90faf");
  assert.equal(shortSha("3b9"), "3b9");
});
