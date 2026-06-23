// Opaque identifier minting shared by the server-side stores (session ids, OAuth
// transaction ids): 256 bits of CSPRNG entropy, base64url-encoded.

export function randomId(): string {
  const buf = new Uint8Array(32);
  crypto.getRandomValues(buf);
  return Buffer.from(buf).toString("base64url");
}
