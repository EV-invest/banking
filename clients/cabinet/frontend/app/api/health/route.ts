import { NextResponse } from "next/server";

import { checkHealth } from "@/shared/api/grpc";

// BFF smoke endpoint: browser → this handler → hub gRPC HealthService.Check.
export async function GET() {
  try {
    const res = await checkHealth();
    return NextResponse.json({ ok: true, backend: res.status });
  } catch (error) {
    return NextResponse.json({ ok: false, error: (error as Error).message }, { status: 502 });
  }
}
