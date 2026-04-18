//! Web UI Module
//!
//! Provides a web-based interface for XCP client operations.
//! Single-user access with simple session-based authentication.

pub mod auth;
pub mod routes;
pub mod server;
pub mod state;
pub mod templates;

pub use server::WebServer;
pub use state::AppState;
