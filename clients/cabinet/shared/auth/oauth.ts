// Google OAuth2 authorize-request construction: PKCE (S256), state, and nonce,
// minted at the BFF with the Web Crypto API (Node runtime). The code verifier
// never leaves the server (stored with the OAuth transaction); only the derived
// challenge goes to Google.

const AUTHORIZE_ENDPOINT = "https://accounts.google.com/o/oauth2/v2/auth";
const SCOPE = "openid email profile";

export interface OAuthChallenge {
  state: string;
  nonce: string;
  codeVerifier: string;
  codeChallenge: string;
}

function base64url(buf: ArrayBuffer | Uint8Array): string {
  const bytes = buf instanceof Uint8Array ? buf : new Uint8Array(buf);
  return Buffer.from(bytes).toString("base64url");
}

function randomToken(bytes = 32): string {
  const buf = new Uint8Array(bytes);
  crypto.getRandomValues(buf);
  return base64url(buf);
}

async function sha256(input: string): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(input));
  return base64url(digest);
}

/** Mint a fresh PKCE verifier/challenge plus an anti-forgery state and nonce. */
export async function newChallenge(): Promise<OAuthChallenge> {
  const codeVerifier = randomToken(32);
  return {
    state: randomToken(16),
    nonce: randomToken(16),
    codeVerifier,
    codeChallenge: await sha256(codeVerifier),
  };
}

/** Build the Google authorize URL to redirect the browser to. */
export function authorizeUrl(opts: { clientId: string; redirectUri: string; state: string; nonce: string; codeChallenge: string }): string {
  const params = new URLSearchParams({
    client_id: opts.clientId,
    redirect_uri: opts.redirectUri,
    response_type: "code",
    scope: SCOPE,
    state: opts.state,
    nonce: opts.nonce,
    code_challenge: opts.codeChallenge,
    code_challenge_method: "S256",
    access_type: "online",
    prompt: "select_account",
  });
  return `${AUTHORIZE_ENDPOINT}?${params.toString()}`;
}

/** Keep a post-login redirect target same-origin to defeat open-redirects. */
export function safeReturnTo(raw: string | null): string {
  if (!raw || !raw.startsWith("/")) return "/";
  // Reject protocol-relative ("//evil", "/\evil") and any backslash, which some
  // browsers normalize to "/" — both can escape to another origin.
  if (raw[1] === "/" || raw[1] === "\\" || raw.includes("\\")) return "/";
  return raw;
}
