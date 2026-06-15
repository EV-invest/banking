//! API: the driving HTTP adapter (Axum). Translates requests into use-case
//! calls and results into HTTP responses. Scaffold: only a liveness probe and
//! the Swagger UI are wired; handlers and DTOs are added per feature.

pub mod handler;
pub mod openapi;
pub mod router;
pub mod state;
