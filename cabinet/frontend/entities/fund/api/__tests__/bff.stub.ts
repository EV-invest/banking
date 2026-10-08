// Stand-in for `@/shared/api/server/bff` in `fund-server.test.ts`. The real `bffRead` reads
// the request's cookies through `next/headers`, which exists only inside a Next request. This
// one records what it was asked — the path and the guard the body would be checked with —
// and answers "no data", the same answer the real one gives on any failure.

export interface RecordedRead {
  path: string;
  accept: (body: unknown) => boolean;
}

export const reads: RecordedRead[] = [];

export async function bffRead(path: string, accept: (body: unknown) => boolean): Promise<null> {
  reads.push({ path, accept });
  return null;
}
