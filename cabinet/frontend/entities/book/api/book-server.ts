import "server-only";

import { bffRead, type ServerRead } from "@/shared/api/server/bff";
import { isJsonObject } from "@/shared/api/server/shape";
import type { BookPolicy } from "@/shared/contracts/book";

// The book's terms for server components — the path `book-client.ts` asks from the browser
// (`bookPath` spells a lone `service` the same way), so the answer seeds
// `bookPolicyResource` under the same key.

const isBookPolicy = (body: unknown): body is BookPolicy => isJsonObject(body);

export async function readBookPolicy(service: string): Promise<ServerRead<BookPolicy> | null> {
  if (!service.trim()) return null;
  return bffRead(`/api/book/policy?service=${encodeURIComponent(service)}`, isBookPolicy);
}
