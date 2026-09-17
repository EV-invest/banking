// The dashboard's card chrome, shared by the view and the hero it extracts.

// Cards inset 16px on mobile (Figma `cabinet/mobile/home`), the uikit 24px from `lg`.
export const CARD_PAD = "px-4 lg:px-6";

// Mobile reads the hero and the stat strip as page content rather than as cards: those two
// surfaces sit flat on the background and only take their Card chrome from `lg`.
export const CARD_FROM_LG = "rounded-none border-0 bg-transparent py-0 shadow-none lg:rounded-xl lg:border lg:bg-card lg:py-5 lg:shadow-sm";

// uikit's Empty draws a dashed frame but leaves the border width to the caller, and doubles
// its padding at `md`; these sit inside cards, not on a page of their own.
export const EMPTY_BOX = "border md:p-6";
