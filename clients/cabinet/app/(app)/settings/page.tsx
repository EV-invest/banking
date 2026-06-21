import { Settings } from "lucide-react";

import { PagePlaceholder } from "@/application/layout/page-placeholder";

export default function SettingsPage() {
  return (
    <PagePlaceholder
      eyebrow="Settings"
      title="Settings"
      blurb="Account, security, notifications and preferences — the full settings surface from the design is the next screen to build out."
      icon={<Settings className="size-6" />}
    />
  );
}
