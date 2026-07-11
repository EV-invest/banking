# Prod config for the cabinet BFF (secret-free, committable). Evaluated to JSON
# at image-build time by the flake. `{ env = ... }` refs resolve at startup from
# the container env (gitops' k8s Secret `envFrom`); a missing var fails the boot
# loudly — BANKING_ISSUANCE_TOKEN absent no longer degrades money routes to
# NotConfigured, it refuses to start.
{
  bind = "0.0.0.0:50062";
  piggybank_grpc_addr = "http://ev-banking-piggybank:50051";
  banking_auth_grpc_addr = "http://ev-banking-piggybank:50052";
  banking_issuance_token.env = "BANKING_ISSUANCE_TOKEN";
  concierge_grpc_addr = "http://concierge:55670";
  # Mirror the concierge plane's (assert_plane-checked) identity values.
  auth_issuer = "https://auth.concierge.ev";
  auth_client_audience = "concierge";
  mfe_registry_path = "/mfe-registry.json";
  app_env = "production";
}
