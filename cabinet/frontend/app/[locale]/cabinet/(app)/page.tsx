import { DashboardView } from "@/views/dashboard/ui/dashboard-view";

// The cabinet home — the investor's portfolio dashboard (Figma `cabinet/home`). Data is
// fetched client-side through the BFF, which authorizes each hub call with the session's
// access token.
export default function Page() {
  return <DashboardView />;
}
