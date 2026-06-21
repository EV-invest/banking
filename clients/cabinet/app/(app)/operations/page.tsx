import { ListChecks } from "lucide-react";

import { PagePlaceholder } from "@/application/layout/page-placeholder";

export default function OperationsPage() {
  return (
    <PagePlaceholder
      eyebrow="Operations"
      title="Operations"
      blurb="Your full activity history — subscriptions, redemptions, deposits and withdrawals in one timeline — lands here next. For now, deposits and withdrawals live on the wallet."
      icon={<ListChecks className="size-6" />}
    />
  );
}
