use dioxus::prelude::*;

use crate::cn;

#[derive(Clone, Default, PartialEq)]
pub enum ButtonVariant {
	#[default]
	Default,
	Secondary,
	Outline,
	Ghost,
	Destructive,
	Link,
}

#[derive(Clone, Default, PartialEq)]
pub enum ButtonSize {
	#[default]
	Default,
	Sm,
	Lg,
	Icon,
}

#[component]
pub fn Button(
	#[props(default)] variant: ButtonVariant,
	#[props(default)] size: ButtonSize,
	#[props(default)] class: String,
	#[props(default)] disabled: bool,
	onclick: Option<EventHandler<MouseEvent>>,
	children: Element,
) -> Element {
	let base = "inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-md \
                text-sm font-medium transition-all outline-none cursor-pointer \
                disabled:pointer-events-none disabled:opacity-50 \
                focus-visible:ring-ring/50 focus-visible:ring-[3px]";

	let variant_cls = match variant {
		ButtonVariant::Default => "bg-primary text-primary-foreground hover:bg-primary/90",
		ButtonVariant::Secondary => "bg-secondary text-secondary-foreground hover:bg-secondary/80",
		ButtonVariant::Outline => "border border-input bg-transparent hover:bg-accent hover:text-accent-foreground",
		ButtonVariant::Ghost => "hover:bg-accent hover:text-accent-foreground",
		ButtonVariant::Destructive => "bg-destructive text-white hover:bg-destructive/90",
		ButtonVariant::Link => "text-primary underline-offset-4 hover:underline",
	};

	let size_cls = match size {
		ButtonSize::Default => "h-9 px-4 py-2",
		ButtonSize::Sm => "h-8 px-3 rounded-md",
		ButtonSize::Lg => "h-10 px-6 rounded-md",
		ButtonSize::Icon => "size-9",
	};

	let cls = cn!(base, variant_cls, size_cls, class);

	rsx! {
		button {
			class: cls,
			disabled,
			onclick: move |e| { if let Some(h) = onclick { h.call(e); } },
			{children}
		}
	}
}
