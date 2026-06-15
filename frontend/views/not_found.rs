use dioxus::prelude::*;

#[component]
pub fn NotFound(route: Vec<String>) -> Element {
	let _ = route;
	rsx! {
		div {
			class: "flex flex-col gap-2",
			h1 { class: "text-2xl font-semibold", "404" }
			p { class: "text-muted-foreground text-sm", "Page not found." }
		}
	}
}
