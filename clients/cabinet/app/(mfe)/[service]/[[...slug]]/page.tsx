import { RemoteElement } from "@/shared/mfe/RemoteElement";
import { findMfe } from "@/shared/mfe/registry";

// Page-level microfrontends: a service owns a whole route. The optional catch-all
// `[[...slug]]` means this also matches the bare `/<service>` index, and the rest
// of the path belongs to the microfrontend's own internal router. The host keeps
// its chrome; the remote owns the content region. Same custom-element contract as
// inline widgets — just mounted at a route. (Not Multi-Zones: this is runtime
// component composition, which also covers inline widgets.)
export default async function MfePage({ params }: { params: Promise<{ service: string; slug?: string[] }> }) {
  const { service } = await params;
  const entry = await findMfe(service);

  if (!entry || entry.kind !== "page") {
    return (
      <div className="container py-24">
        <h1 className="font-serif text-3xl">Unknown microfrontend</h1>
        <p className="mt-2 text-muted-foreground">
          No page microfrontend is registered for <code>/{service}</code>. Add it to{" "}
          <code>mfe-registry.json</code>.
        </p>
      </div>
    );
  }

  return (
    <RemoteElement
      tag={entry.tag}
      scriptUrl={entry.scriptUrl}
      className="block min-h-[60vh]"
      fallback={<div className="container py-24 text-muted-foreground">Loading {entry.name}…</div>}
    />
  );
}
