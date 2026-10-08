import { stageMirrorScript } from "@/features/onboarding/lib/checklist-memory";

// Server-only by use: the route renders it ahead of its streaming boundary, so the script runs
// while the HTML is parsed, before the boundary's placeholder paints. In a browser render (a
// soft navigation) React inserts it without running it, and nothing needs it there: the
// skeleton reads the stage from the store directly once hydrated.
export function StageMirror({ nonce }: { nonce: string | undefined }) {
  return <script nonce={nonce} dangerouslySetInnerHTML={{ __html: stageMirrorScript() }} />;
}
