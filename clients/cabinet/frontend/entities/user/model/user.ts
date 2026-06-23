// The authenticated principal as the cabinet knows it — the user snapshot the hub
// returns alongside its tokens. KYC state, roles, and token_version grow here as
// the investor portal does; for now the BFF only ever surfaces these three fields
// to the browser (never a token).

export interface User {
  userId: string;
  email: string;
  status: string;
}
