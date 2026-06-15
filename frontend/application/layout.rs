use dioxus::prelude::*;

use crate::application::router::Route;

#[component]
pub fn Layout() -> Element {
	rsx! {
		div {
			class: "flex h-screen bg-background text-foreground overflow-hidden",

			// ── sidebar ─────────────────────────────────────────────
			nav {
				class: "flex flex-col flex-shrink-0 border-r border-sidebar-border bg-sidebar p-3 gap-1",
				style: "width: var(--sidebar-width);",

				div {
					class: "px-3 pb-4 mb-2 border-b border-sidebar-border",
					span { class: "font-bold text-base tracking-wide text-foreground", "EV Fund" }
				}

				Link {
					class: "nav-link",
					to: Route::Home {},
					active_class: "active",
					"Home"
				}
			}

			// ── main content ─────────────────────────────────────────
			div {
				class: "flex-1 overflow-y-auto p-8",
				Outlet::<Route> {}
			}
		}
	}
}
