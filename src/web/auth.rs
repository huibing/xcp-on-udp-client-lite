//! Simple Authentication Module
//!
//! Provides session-based authentication for single-user access.
//! Credentials are hardcoded for simplicity - admin/admin

use axum::{
    extract::Request,
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use once_cell::sync::Lazy;
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;
use std::time::{SystemTime, UNIX_EPOCH};

/// Hardcoded credentials (username: admin, password: admin)
/// Password hash: SHA256("admin")
const PASSWORD_HASH: &str = "8c6976e5b5410415bde908bd4dee15dfb167a9c873fc4bb8a81f6f2ab448a918";
const USERNAME: &str = "admin";

/// Global session store for single-user access
static SESSION: Lazy<RwLock<Session>> = Lazy::new(|| {
    RwLock::new(Session {
        token: None,
        created_at: 0,
        client_ip: None,
    })
});

/// Session counter for generating tokens
static SESSION_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Session data structure
#[derive(Debug, Clone)]
struct Session {
    token: Option<String>,
    #[allow(dead_code)]
    created_at: u64,
    client_ip: Option<String>,
}

/// Check if there is already an active session (another user logged in)
pub fn has_active_session() -> bool {
    let session = SESSION.read().unwrap();
    session.token.is_some()
}

/// Get info about the current active session
pub fn get_active_session_info() -> Option<(String, u64)> {
    let session = SESSION.read().unwrap();
    if session.token.is_some() {
        return Some((
            session.client_ip.clone().unwrap_or_else(|| "unknown".to_string()),
            session.created_at,
        ));
    }
    None
}

/// Check if a session token is valid
pub fn is_valid_session(token: &str) -> bool {
    let session = SESSION.read().unwrap();

    if let Some(ref valid_token) = session.token {
        return valid_token == token;
    }
    false
}

/// Try to create a new session
/// Returns Ok(token) if successful, Err(error_msg) if another user is already connected
pub fn try_create_session(client_ip: &str) -> Result<String, String> {
    // Check if there's already an active session
    if has_active_session() {
        let (existing_ip, _created_at) = get_active_session_info()
            .unwrap_or_else(|| ("unknown".to_string(), 0));
        return Err(format!(
            "Another user is already connected from: {}. Please wait until they logout.",
            existing_ip
        ));
    }

    let token = generate_token();
    let mut session = SESSION.write().unwrap();
    session.token = Some(token.clone());
    session.created_at = current_timestamp();
    session.client_ip = Some(client_ip.to_string());
    Ok(token)
}

/// Clear the current session (logout)
pub fn clear_session() {
    let mut session = SESSION.write().unwrap();
    session.token = None;
    session.created_at = 0;
}

/// Verify login credentials
pub fn verify_credentials(username: &str, password: &str) -> bool {
    if username != USERNAME {
        return false;
    }

    let mut hasher = Sha256::new();
    hasher.update(password.as_bytes());
    let result = hex::encode(hasher.finalize());

    result == PASSWORD_HASH
}

/// Extract session cookie from request
pub fn extract_session_cookie(request: &Request) -> Option<String> {
    let headers = request.headers();
    let cookie_header = headers.get("cookie")?;
    let cookie_str = cookie_header.to_str().ok()?;

    for cookie in cookie_str.split(';') {
        let cookie = cookie.trim();
        if let Some((name, value)) = cookie.split_once('=') {
            if name == "session" {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// Authentication middleware
pub async fn auth_middleware(request: Request, next: Next) -> Response {
    // Allow access to login page and static assets
    let path = request.uri().path();
    if path == "/login" || path == "/api/login" || path.starts_with("/static/") {
        return next.run(request).await;
    }

    // Check session cookie
    if let Some(token) = extract_session_cookie(&request) {
        if is_valid_session(&token) {
            return next.run(request).await;
        }
    }

    // Redirect to login page
    Redirect::to("/login").into_response()
}

/// Generate a unique session token
fn generate_token() -> String {
    let counter = SESSION_COUNTER.fetch_add(1, Ordering::SeqCst);
    let timestamp = current_timestamp();
    let data = format!("{}:{}", counter, timestamp);
    BASE64.encode(data.as_bytes())
}

/// Get current timestamp in seconds
fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verify_credentials() {
        assert!(verify_credentials("admin", "admin"));
        assert!(!verify_credentials("admin", "wrong"));
        assert!(!verify_credentials("wrong", "admin"));
    }

    #[test]
    fn test_single_user_restriction_first_user_can_login() {
        // Given: No active session
        clear_session();
        assert!(!has_active_session());

        // When: First user tries to create session
        let result = try_create_session("192.168.1.10");

        // Then: Should succeed
        assert!(result.is_ok());
        let token = result.unwrap();
        assert!(!token.is_empty());
        assert!(is_valid_session(&token));

        // Cleanup
        clear_session();
    }

    #[test]
    fn test_single_user_restriction_second_user_blocked() {
        // Given: First user is logged in (use unique IP to avoid conflict with other tests)
        clear_session();
        let _first_token = try_create_session("192.168.2.10").unwrap();

        // Verify first session is valid
        assert!(has_active_session(), "Should have active session after creation");

        // When: Second user from different IP tries to login
        let result = try_create_session("192.168.2.20");

        // Then: Should be rejected with error message
        assert!(result.is_err());
        let error_msg = result.unwrap_err();
        assert!(error_msg.contains("Another user is already connected"));
        assert!(error_msg.contains("192.168.2.10"));

        // Cleanup
        clear_session();
    }

    #[test]
    fn test_single_user_restriction_after_logout_new_user_can_login() {
        // Given: First user is logged in (use unique IPs to avoid parallel test conflicts)
        clear_session();
        let first_token = try_create_session("10.1.1.10").unwrap();
        assert!(is_valid_session(&first_token));

        // When: First user logs out
        clear_session();

        // Then: Session should be cleared
        assert!(!is_valid_session(&first_token));
        assert!(!has_active_session());

        // And: New user can now login
        let result = try_create_session("10.1.1.20");
        assert!(result.is_ok());
        let new_token = result.unwrap();
        assert!(is_valid_session(&new_token));

        // Cleanup
        clear_session();
    }

    #[test]
    fn test_session_info_tracks_ip_address() {
        // Given: User logs in from specific IP (use unique IP to avoid conflict)
        clear_session();
        let unique_ip = "10.0.1.50";
        let _token = try_create_session(unique_ip).unwrap();

        // Verify session is active before getting info
        assert!(has_active_session(), "Session should be active");

        // When: Get session info
        let info = get_active_session_info();

        // Then: Should return the IP address
        assert!(info.is_some(), "Should have active session info");
        let (ip, _timestamp) = info.unwrap();
        assert_eq!(ip, unique_ip);

        // Cleanup
        clear_session();
    }
}
