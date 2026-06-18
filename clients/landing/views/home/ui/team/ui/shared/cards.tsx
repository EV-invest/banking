import type { ReactNode } from "react";
import { type LucideIcon } from "lucide-react";
import { Button } from "@evinvest/uikit";
import { Text } from "@/shared/ui/text";

/**
 * Grid cell shell — 3:4 framed visual on top, name/label below.
 * Frame contents (photo, icon) vary per card; the shell is constant.
 */
export function Card({
  children,
  heading,
  headingClassName,
  sub,
}: {
  children: ReactNode;
  heading: string;
  headingClassName: string;
  sub: ReactNode;
}) {
  return (
    <div className="group space-y-4">
      <div className="relative aspect-[3/4] rounded-xl overflow-hidden border border-main-mist/10 bg-main-card">
        {children}
      </div>
      <div>
        <h4 className={`font-serif-display font-bold text-base ${headingClassName}`}>
          {heading}
        </h4>
        {sub}
      </div>
    </div>
  );
}

/**
 * Non-person card — centred icon-disc, title, blurb, outline CTA button.
 * Used for open-role and LP-network slots. Only the icon, accent color, and
 * copy differ between instances. `onCtaClick` is supplied by the
 * {@link TeamPlaceholders} client island.
 */
export function PlaceholderCard({
  icon: Icon,
  iconClassName,
  title,
  body,
  cta,
  onCtaClick,
  heading,
  sub,
}: {
  icon: LucideIcon;
  iconClassName: string;
  title: string;
  body: string;
  cta: string;
  onCtaClick: () => void;
  heading: string;
  sub: string;
}) {
  return (
    <Card
      heading={heading}
      headingClassName="text-main-mist/40"
      sub={
        <Text variant="secondary" className="text-xs font-mono-tech mt-1">
          {sub}
        </Text>
      }
    >
      <div className="absolute inset-0 flex items-center justify-center p-6 text-center">
        <div className="space-y-4">
          <div
            className={`w-12 h-12 rounded-full bg-main-mist/5 border border-main-mist/10 flex items-center justify-center mx-auto ${iconClassName}`}
          >
            <Icon className="w-6 h-6" />
          </div>
          <div>
            <h5 className="font-serif-display font-bold text-white text-sm">{title}</h5>
            <Text variant="secondary" className="text-xs mt-2 font-light">
              {body}
            </Text>
          </div>
          <Button
            onClick={onCtaClick}
            variant="outline"
            className="border-main-mist/15 text-main-mist/80 hover:border-main-accent-t1 hover:text-main-accent-t1 text-xs py-1 h-auto bg-transparent"
          >
            {cta}
          </Button>
        </div>
      </div>
    </Card>
  );
}
