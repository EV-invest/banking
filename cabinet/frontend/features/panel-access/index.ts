// Panel access — who may open a vertical's panel (`allocation:<service>` scopes in the
// identity plane). The card is the whole management surface; the viewer hook is the one
// read every gate outside the slice (the registry row, the dashboard link) needs.
export { PanelAccessCard } from "@/features/panel-access/ui/panel-access-card";
export { usePanelViewer, type PanelViewer } from "@/features/panel-access/model/use-panel-viewer";
