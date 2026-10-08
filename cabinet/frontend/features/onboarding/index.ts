// The slice's whole contract: the block Home shows a new account, and the hook that decides
// what it says. The step rule itself (`lib/checklist`) stays internal — it is the hook's
// business to feed it, and a second caller would be a second reading of "funded".
export { type ChecklistExpect, type ChecklistState, useChecklist, type VerificationRead } from "@/features/onboarding/model/use-checklist";
export { ChecklistSkeleton } from "@/features/onboarding/ui/checklist-skeleton";
export { StageMirror } from "@/features/onboarding/ui/stage-mirror";
export { GetStarted } from "@/features/onboarding/ui/get-started";
export { type ChecklistShape, isChecklistShape, SHAPE_COOKIE } from "@/features/onboarding/lib/checklist-shape";
