"use client";

// The one composition primitive the cabinet host knows. It loads a microfrontend's
// self-registering ESM bundle by URL, waits for its custom element to upgrade,
// then mounts <tag> inside a host node — mapping props to attributes and
// CustomEvents back to callbacks. Identical for React and Rust/WASM microfrontends.
//
// Remotes are client-only (registration runs in the browser), so this is a
// 'use client' island: server components above stream normally and show the
// fallback until the element is ready. The element is created imperatively (not
// via JSX) so it works for any framework's custom element without React's
// attribute/property quirks.

import { useEffect, useRef, useState, type ReactNode } from "react";

export interface RemoteElementProps {
  /** Custom-element tag the remote registers, e.g. "mfe-risk-pnl-chart". */
  tag: string;
  /** URL of the remote's self-registering ESM bundle. */
  scriptUrl: string;
  /** Attributes passed down to the element (objects are JSON-encoded). */
  attributes?: Record<string, string | number | boolean | object>;
  /** CustomEvent name → handler, e.g. { select: (detail) => ... }. */
  events?: Record<string, (detail: unknown) => void>;
  className?: string;
  /** Shown until the element is registered (and if loading fails). */
  fallback?: ReactNode;
}

// Stable empty defaults so the mount effect doesn't re-run on every render.
const NO_ATTRS: Record<string, string | number | boolean | object> = {};
const NO_EVENTS: Record<string, (detail: unknown) => void> = {};

export function RemoteElement({ tag, scriptUrl, attributes = NO_ATTRS, events = NO_EVENTS, className, fallback = null }: RemoteElementProps) {
  const hostRef = useRef<HTMLDivElement>(null);
  const [ready, setReady] = useState(false);

  // Load the remote's bundle (once per tag), then wait for the element to upgrade.
  useEffect(() => {
    let cancelled = false;
    const whenReady = () => customElements.whenDefined(tag).then(() => !cancelled && setReady(true));

    if (customElements.get(tag) || document.querySelector(`script[data-mfe="${tag}"]`)) {
      void whenReady();
      return () => {
        cancelled = true;
      };
    }

    const script = document.createElement("script");
    script.type = "module";
    script.src = scriptUrl;
    script.dataset.mfe = tag;
    script.addEventListener("load", () => void whenReady());
    document.head.appendChild(script);
    return () => {
      cancelled = true;
    };
  }, [tag, scriptUrl]);

  // Once the element is defined, mount it into the host and wire events.
  useEffect(() => {
    const host = hostRef.current;
    if (!host || !ready) return;

    const element = document.createElement(tag);
    for (const [name, value] of Object.entries(attributes)) {
      element.setAttribute(name, typeof value === "object" ? JSON.stringify(value) : String(value));
    }
    const bound = Object.entries(events).map(([name, handler]): [string, EventListener] => {
      const listener: EventListener = (event) => handler((event as CustomEvent).detail);
      element.addEventListener(name, listener);
      return [name, listener];
    });
    host.appendChild(element);

    return () => {
      bound.forEach(([name, listener]) => element.removeEventListener(name, listener));
      element.remove();
    };
  }, [ready, tag, attributes, events]);

  return (
    <div ref={hostRef} className={className}>
      {ready ? null : fallback}
    </div>
  );
}
