//! HTTP/WebSocket API server for the OpenParlant Agent OS daemon.
//!
//! Exposes agent management, status, and chat via JSON REST endpoints.
//! The kernel runs in-process; the CLI connects over HTTP.

pub mod channel_bridge;
pub mod compat_routes;
pub mod control_routes;
pub mod dashboard_routes;
pub mod middleware;
pub mod notification_routes;
pub mod openai_compat;
pub mod rate_limiter;
pub mod routes;
pub mod server;
pub mod session_auth;
pub mod stream_chunker;
pub mod stream_dedup;
pub mod types;
pub mod webchat;
pub mod ws;
