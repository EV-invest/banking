/// Rust analog of the landing `cn` helper (`landing/shared/lib/utils.ts`).
///
/// Fuses any number of class fragments into a single `String` with real
/// Tailwind conflict resolution: like `clsx` it drops empty fragments and joins
/// the rest, and like `tailwind-merge` it resolves conflicting utilities so the
/// rightmost class wins (`cn!("p-4", "p-2")` → `"p-2"`). A caller's `class`
/// override, passed last, therefore beats the base classes instead of relying
/// on CSS source order.
///
/// Each fragment may be a `&str`, `String`, or `Option<_>` thereof — anything
/// `tailwind_fuse::tw_merge!` accepts.
#[macro_export]
macro_rules! cn {
	($($frag:expr),* $(,)?) => {
		::tailwind_fuse::tw_merge!($($frag),*)
	};
}

#[cfg(test)]
mod tests {
	#[test]
	fn joins_distinct_classes() {
		assert_eq!(cn!("flex", "items-center", "justify-center"), "flex items-center justify-center");
	}

	#[test]
	fn rightmost_wins_on_conflict() {
		assert_eq!(cn!("p-4", "p-2"), "p-2");
		assert_eq!(cn!("bg-primary", "bg-secondary"), "bg-secondary");
	}

	#[test]
	fn keeps_refinements() {
		assert_eq!(cn!("p-4", "py-2"), "p-4 py-2");
	}

	#[test]
	fn drops_empty_fragments() {
		assert_eq!(cn!("px-2", "", "py-1"), "px-2 py-1");
		assert_eq!(cn!(""), "");
	}

	#[test]
	fn mixes_str_and_owned_override() {
		let base = "h-9 px-4 py-2";
		let class = String::from("px-6");
		assert_eq!(cn!(base, class), "h-9 py-2 px-6");
	}

	#[test]
	fn trailing_comma_and_single_fragment() {
		assert_eq!(cn!("rounded-md",), "rounded-md");
	}
}
