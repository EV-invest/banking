// The slice's whole contract: the block Home shows a new account, and the hook that decides
// what it says. The step rule itself (`lib/checklist`) stays internal — it is the hook's
// business to feed it, and a second caller would be a second reading of "funded".
export { type ChecklistState, useChecklist, type VerificationRead } from "@/features/onboarding/model/use-checklist";
export { GetStarted } from "@/features/onboarding/ui/get-started";
