import { RemoteElement } from "@/shared/mfe/RemoteElement";
import { HealthBadge } from "@/views/home/ui/health-badge";

export function HomeView() {
  return (
    <div className="container space-y-16 py-20">
      <section className="space-y-4">
        <p className="font-mono-tech text-xs uppercase tracking-widest text-main-accent-t1">EV Banking · Console</p>
        <h1 className="max-w-3xl text-5xl leading-tight">The hub shell</h1>
        <p className="max-w-2xl text-muted-foreground">
          <code>core</code> composes microfrontends from other services — React or Rust/WASM, inline widgets or whole
          pages — as custom elements, and proxies browser requests to the hub&apos;s gRPC backend.
        </p>
        <HealthBadge />
      </section>

      <section className="space-y-4">
        <h2 className="text-2xl">Embedded microfrontend</h2>
        <p className="max-w-2xl text-sm text-muted-foreground">
          An example inline widget mounted via <code>&lt;RemoteElement&gt;</code>. With no remote deployed it shows its
          fallback; register a real bundle in <code>mfe-registry.json</code> to light it up.
        </p>
        <div className="rounded-lg border border-border p-6">
          <RemoteElement
            tag="mfe-example-widget"
            scriptUrl="/mfe/example-widget.js"
            fallback={<div className="text-sm text-muted-foreground">example widget not deployed — showing fallback</div>}
          />
        </div>
      </section>
    </div>
  );
}
