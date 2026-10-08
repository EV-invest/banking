// Stand-in for `next/headers`, whose `headers()` exists only inside a Next request. A test
// sets `request.headers` to the headers the browser would have sent.

export const request: { headers: Headers } = { headers: new Headers() };

export async function headers(): Promise<Headers> {
  return request.headers;
}
