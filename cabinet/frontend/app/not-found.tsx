import { NotFound } from "@evinvest/uikit";

// The shared 404 (from @evinvest/uikit). "Back to home" points at the cabinet
// dashboard; the secondary CTA falls through to the landing's contact page.
export default function NotFoundPage() {
  return <NotFound homeHref="/cabinet" />;
}
