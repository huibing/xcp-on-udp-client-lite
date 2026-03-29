//! Web Server
//!
//! Axum-based web server for XCP client web interface.
//! Single-user access with simple session authentication.

use axum::{
    routing::{get, post},
    Router,
};
use std::net::SocketAddr;
use tower_http::trace::TraceLayer;

use crate::web::auth::auth_middleware;
use crate::web::routes;
use crate::web::state::AppState;

/// Web server struct
pub struct WebServer {
    state: AppState,
    addr: SocketAddr,
}

impl WebServer {
    /// Create a new web server (binds to all interfaces)
    pub fn new(state: AppState, port: u16) -> Self {
        let addr = SocketAddr::from(([0, 0, 0, 0], port));
        Self { state, addr }
    }

    /// Create a new web server with specific host
    pub fn new_with_host(state: AppState, host: &str, port: u16) -> Self {
        let addr: SocketAddr = format!("{}:{}", host, port)
            .parse()
            .unwrap_or_else(|_| SocketAddr::from(([0, 0, 0, 0], port)));
        Self { state, addr }
    }

    /// Run the web server
    pub async fn run(self) -> Result<(), Box<dyn std::error::Error>> {
        let app = Self::create_router(self.state);

        let listener = tokio::net::TcpListener::bind(self.addr).await?;
        tracing::info!("Web server listening on http://{}", self.addr);

        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await?;
        Ok(())
    }

    /// Create the router with all routes
    fn create_router(state: AppState) -> Router {
        Router::new()
            // Public routes
            .route("/login", get(routes::login_page))
            .route("/api/login", post(routes::login_api))
            // Protected routes
            .route("/", get(routes::dashboard))
            .route("/logout", get(routes::logout))
            // Slave management
            .route("/slaves", get(routes::slaves_page))
            .route("/api/slave/select/:id", get(routes::select_slave_api))
            .route("/api/disconnect/:id", get(routes::disconnect_slave_api))
            // Connection
            .route("/connect", get(routes::connect_page))
            .route("/api/connect", post(routes::connect_api))
            .route("/api/disconnect", get(routes::disconnect_api))
            // Calibration (with slave ID)
            .route("/calibration", get(routes::calibration_page))
            .route("/api/cal/read/:slave_id/:idx", get(routes::cal_read))
            .route("/cal/write/:slave_id/:idx", get(routes::cal_write_page))
            .route("/api/cal/write/:slave_id/:idx", post(routes::cal_write_api))
            // DAQ
            .route("/daq", get(routes::daq_page))
            .route("/api/daq/start", get(routes::daq_start))
            .route("/api/daq/stop", get(routes::daq_stop))
            .route("/api/daq/toggle", post(routes::daq_toggle))
            .route("/api/daq/clear", get(routes::daq_clear))
            .route("/api/daq/data", get(routes::daq_data))
            // A2L
            .route("/a2l", get(routes::a2l_page))
            .route("/api/a2l/load", post(routes::a2l_load))
            // State and middleware
            .layer(axum::middleware::from_fn(auth_middleware))
            .layer(TraceLayer::new_for_http())
            .with_state(state)
    }
}
