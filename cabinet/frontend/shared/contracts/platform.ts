// The `/api/platform` response — hand-written to match the BFF handler (see
// `cabinet/backend/src/routes/platform.rs`) rather than re-exported from `./gen`,
// because it is BFF-composed across planes (concierge platform config + banking
// read-only + the BFF's APP_ENV), not a passthrough of any one proto. Feature flags
// are deliberately absent — the BFF never serializes them on this route.

export interface PlatformStatus {
  environment: string;
  maintenance_mode: boolean;
  read_only: boolean;
  announcement_title: string;
  announcement_body: string;
  announcement_active: boolean;
}
