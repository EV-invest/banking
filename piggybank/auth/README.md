# `evbanking_auth`

The hub's auth — a **service** (token issuance, run inside `piggybank-core`) and a
**shared verification flow** (what other service repos import). One crate, two
audiences; this README is the downstream-adoption guide. Architecture rationale
lives in [`docs/ARCHITECTURE.md`](../../docs/ARCHITECTURE.md); per-pattern detail in
[`PATTERNS.md`](./PATTERNS.md). Wasm-unsafe by design — never a dependency of
`domain`.

## What it gives a downstream service

A separate service repo that receives the hub's gRPC calls (and/or calls back into
the hub) needs exactly two things, both here:

1. **Verify the hub's tokens locally** — no per-request round trip, no per-service
   token storage. A [`Verifier`] caches the hub's JWKS and validates EdDSA access
   tokens against it, refreshing on an unknown `kid` (a key rotation).
2. **Authenticate its own onward calls into the hub** — a [`ServiceTokenSource`]
   attaches a `typ=service`, distinct-`aud` token to outgoing requests.

## Adopt it in three steps

```toml
# Cargo.toml — two git deps, no protoc toolchain, no Redis client.
evbanking_contracts = { git = "https://github.com/EV-invest/banking.git" }
evbanking_auth      = { git = "https://github.com/EV-invest/banking.git" }
```

```rust
use evbanking_auth::{grpc_auth_layer, VerifierConfig, Verifier, claims_of};
use tower::Layer;

// 1. Build a verifier (warms the JWKS cache with one fetch).
let verifier = Verifier::connect(VerifierConfig::from_env()?).await?;

// 2. Mount the async auth layer per service. It rejects unauthenticated calls with
//    gRPC UNAUTHENTICATED and injects the verified Claims into the request.
let svc = grpc_auth_layer(verifier).layer(MyServiceServer::new(MyService::default()));
tonic::transport::Server::builder().add_service(svc).serve(addr).await?;

// 3. Read the verified principal in a handler.
async fn handle(&self, req: Request<Foo>) -> Result<Response<Bar>, Status> {
    let claims = claims_of(&req).ok_or_else(|| Status::unauthenticated("no claims"))?;
    // claims.sub is the hub user id (or service name); claims.typ / claims.aud
    // tell you human-vs-service. ...
}
```

Environment for the verifier:

| Var | Meaning |
| --- | ------- |
| `AUTH_JWKS_GRPC_ENDPOINT` | gRPC address of the hub auth service, e.g. `http://hub:50052` |
| `AUTH_ISSUER` | expected `iss` (default `https://auth.banking.ev`) |
| `AUTH_CLIENT_AUDIENCE` | the audience your service accepts (default `banking-core`) |

## Onward calls into the hub

```rust
use evbanking_auth::ServiceTokenSource;

let tokens = ServiceTokenSource::from_env().expect("SERVICE_TOKEN set");
let mut request = tonic::Request::new(payload);
request = tokens.authorize(request);            // adds `authorization: Bearer …`
hub_client.some_rpc(request).await?;
```

## Do / Don't

- **Do** verify locally against cached JWKS. **Don't** call the hub to authorize
  each request, and **don't** keep a per-service token denylist — both reintroduce
  the round trip JWTs exist to avoid.
- **Do** rely on the short access-token TTL for revocation downstream;
  `token_version` "revoke all" is enforced hub-side at refresh, not by stateless
  verifiers.
- **Don't** pin a single `aud` if your service legitimately accepts several — the
  policy takes an audience **set**.

## Inside the hub (`piggybank-core` only)

`core` builds the [`AuthService`] (which owns keys/JWKS/Google/refresh), takes an
[`Authorizer`] (core → auth verify channel), hands it a [`Provisioner`] (auth →
core user-upsert channel), and mounts `grpc_auth_layer(authorizer)` on each data
service. Both channels cross a task boundary, never the network. See
`piggybank/core/src/main.rs`.

[`Verifier`]: src/verifier.rs
[`ServiceTokenSource`]: src/service_token.rs
[`AuthService`]: src/service.rs
[`Authorizer`]: src/authorizer.rs
[`Provisioner`]: src/provisioner.rs
