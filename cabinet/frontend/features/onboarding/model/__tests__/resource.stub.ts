// Stand-in for `@/shared/lib/resource` in `use-checklist.test.ts`: `useResource` answers
// whatever snapshot the test set for that resource, and a resource nobody set is still loading.

export interface FakeSnapshot {
  data: unknown;
  isLoading: boolean;
}

export const snapshots = new Map<unknown, FakeSnapshot>();

const LOADING: FakeSnapshot = { data: undefined, isLoading: true };

export function useResource(resource: unknown) {
  const snapshot = snapshots.get(resource) ?? LOADING;
  return { ...snapshot, error: null, isValidating: false, refresh: async () => undefined };
}
