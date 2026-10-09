// Panel access — what users hold inside a tenant's namespace, the grants its panel gates on.
// The card is the whole management surface; the hooks are the reads every gate outside the
// slice (the registry row, the dashboard link) needs.
export { PanelAccessCard } from "@/features/panel-access/ui/panel-access-card";
export type { GrantPrefill } from "@/features/panel-access/ui/grant-form";
export { usePanelManager, usePanelViewer, type PanelViewer } from "@/features/panel-access/model/use-panel-viewer";
