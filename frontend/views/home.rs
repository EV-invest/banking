use dioxus::prelude::*;

use crate::shared::ui::{Card, CardContent, CardHeader, CardTitle};

#[component]
pub fn Home() -> Element {
	rsx! {
		div {
			class: "flex flex-col gap-6",
			h1 { class: "text-2xl font-semibold", "EV Fund" }
			Card {
				CardHeader { CardTitle { "Scaffold" } }
				CardContent {
					p { class: "text-muted-foreground text-sm", "Frontend shell is wired. Build views under src/views and components under src/shared/ui." }
				}
			}
		}
	}
}
