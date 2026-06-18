import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";
import { toast } from "@evinvest/uikit";

// Kept local (not re-exported from the kit) so it stays server-safe: the kit
// bundle is a `"use client"` module, whereas `cn` is used in server components.
export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

// Every interactive control in the design mock is a stub. Clicking one raises
// the same "this is a concept" toast — sourced from the kit so it matches the
// kit's <Toaster/>. Only ever invoked from client components.
export function notifyPlaceholder(featureName: string) {
  toast.info(`${featureName}: Концепт-интерфейс`, {
    description: "Данный элемент является частью интерактивного дизайн-макета.",
    duration: 3000,
  });
}
