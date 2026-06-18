import type {
  FullResult,
  Reporter,
  TestCase,
  TestResult,
} from "@playwright/test/reporter";

// Visual-regression reporter. A failure here is never a code bug — it's an
// image mismatch — so the default `list` reporter's step trace and code-frame
// are noise. We print only the test title and the three screenshot paths
// (expected / actual / diff), the diff being the magenta pixel-diff.
class VisualReporter implements Reporter {
  private failed: { title: string; lines: string[] }[] = [];

  onTestEnd(test: TestCase, result: TestResult) {
    const mark = result.status === "passed" ? "✓" : result.status === "skipped" ? "-" : "✗";
    process.stdout.write(`  ${mark} ${test.title}\n`);
    if (result.status !== "failed" && result.status !== "timedOut") return;

    const lines: string[] = [];
    for (const a of result.attachments) {
      if (a.contentType !== "image/png" || !a.path) continue;
      const kind = a.name.includes("expected")
        ? "expected"
        : a.name.includes("actual")
          ? "actual  "
          : a.name.includes("diff")
            ? "diff    "
            : a.name;
      lines.push(`      ${kind}  ${a.path}`);
    }
    if (lines.length === 0) {
      // Non-screenshot failure (e.g. timeout before the assertion): keep the
      // bare error message so it isn't silently swallowed.
      lines.push(`      ${result.error?.message?.split("\n")[0] ?? "failed"}`);
    }
    this.failed.push({ title: test.title, lines });
  }

  onEnd(result: FullResult) {
    if (this.failed.length === 0) return;
    process.stdout.write(`\n  ${this.failed.length} screenshot(s) changed:\n`);
    for (const f of this.failed) {
      process.stdout.write(`\n    ${f.title}\n${f.lines.join("\n")}\n`);
    }
    process.stdout.write("\n");
    void result;
  }
}

export default VisualReporter;
