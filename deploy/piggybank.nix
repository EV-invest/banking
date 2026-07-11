# Prod config for the piggybank hub (secret-free, committable). Evaluated to
# JSON at image-build time by the flake — the runtime container carries no
# `nix`, only the baked result. `{ env = ... }` refs resolve at startup from the
# container env (baked contract env + gitops' k8s Secret `envFrom`); a missing
# var fails the boot loudly. TIGERBEETLE_ADDRESS/TIGERBEETLE_CLUSTER_ID stay in
# the container env (flake-computed from deploy/tigerbeetle.nix, read via the
# settings env aliases; required fields, so absence still fails the boot). The
# on-chain rails (BSC/TRON/TON) are NOT here — env-gated: TON_API_URL rides the
# contract env, TON_API_KEY the k8s Secret; main.rs refuses a rail-less prod boot.
{
  database_url.env = "DATABASE_URL";
  grpc_addr = "0.0.0.0:50051";
  auth_grpc_addr = "0.0.0.0:50052";
  app_env = "production";
  # The signer runs as a loopback sidecar in this pod (non-loopback binds
  # demand TLS — signer/src/config.rs), mirroring the dev seam.
  signer_grpc_addr = "http://127.0.0.1:50053";
  concierge_bridge_addr = "http://concierge:55670";
  bridge_service_token.env = "BRIDGE_SERVICE_TOKEN";
}
