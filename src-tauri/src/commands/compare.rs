/// Compare sessions Tauri commands.
///
/// Ports web/routes.rs compare_* handlers.

use serde_json::{json, Value};
use tauri::State;
use xcp_client::web::compare::{
    load_a2l_hex_source, load_dcm_source, ComparePresenceFilter, CompareSide, CompareSession,
};

use crate::state::SharedState;

/// Return all compare sessions.
#[tauri::command]
pub async fn compare_sessions(state: State<'_, SharedState>) -> Result<Value, String> {
    let app = state.read().await;
    // Ensure at least one session exists
    let sessions: Vec<Value> = app
        .compare_sessions
        .iter()
        .map(|s| s.to_json())
        .collect();
    Ok(json!({ "success": true, "sessions": sessions }))
}

/// Create a new compare session.
#[tauri::command]
pub async fn compare_create_session(
    state: State<'_, SharedState>,
    name: Option<String>,
) -> Result<Value, String> {
    let mut app = state.write().await;
    let mut session = CompareSession::new(app.compare_sessions.len());
    if let Some(n) = name {
        let trimmed = n.trim().to_string();
        if !trimmed.is_empty() {
            session.name = trimmed;
        }
    }
    let json = session.to_json();
    app.compare_sessions.push(session);
    Ok(json!({ "success": true, "session": json }))
}

/// Rename an existing compare session.
#[tauri::command]
pub async fn compare_rename_session(
    state: State<'_, SharedState>,
    session_id: String,
    name: String,
) -> Result<Value, String> {
    let trimmed = name.trim().to_string();
    if trimmed.is_empty() {
        return Ok(json!({ "success": false, "error": "Session name cannot be empty" }));
    }
    let mut app = state.write().await;
    let session = app
        .compare_sessions
        .iter_mut()
        .find(|s| s.id == session_id)
        .ok_or_else(|| "Compare session not found".to_string())?;
    session.name = trimmed;
    let json = session.to_json();
    Ok(json!({ "success": true, "session": json }))
}

/// Delete a compare session.
#[tauri::command]
pub async fn compare_delete_session(
    state: State<'_, SharedState>,
    session_id: String,
) -> Result<Value, String> {
    let mut app = state.write().await;
    let before = app.compare_sessions.len();
    app.compare_sessions.retain(|s| s.id != session_id);
    if app.compare_sessions.len() == before {
        return Ok(json!({ "success": false, "error": "Compare session not found" }));
    }
    // Keep at least one session
    if app.compare_sessions.is_empty() {
        app.compare_sessions.push(CompareSession::new(0));
    }
    Ok(json!({ "success": true }))
}

/// Load a compare source (A2L+HEX or DCM) into one side of a session.
///
/// Parameters:
/// - `source_type`: `"a2l_hex"` or `"dcm"`
/// - `file_path`: primary file path (A2L for a2l_hex, DCM for dcm)
/// - `hex_path`: HEX file path (only for `a2l_hex` type)
#[tauri::command]
pub async fn compare_load_source(
    state: State<'_, SharedState>,
    session_id: String,
    side: String,
    source_type: String,
    file_path: String,
    hex_path: Option<String>,
) -> Result<Value, String> {
    let side = CompareSide::from_str(&side).map_err(|e| e)?;

    let source = match source_type.as_str() {
        "a2l_hex" => {
            let hex_path = hex_path.ok_or_else(|| "hex_path required for a2l_hex source".to_string())?;
            let a2l_name = extract_filename(&file_path);
            let hex_name = extract_filename(&hex_path);
            let a2l_bytes = std::fs::read(&file_path)
                .map_err(|e| format!("Failed to read A2L '{}': {}", file_path, e))?;
            let hex_bytes = std::fs::read(&hex_path)
                .map_err(|e| format!("Failed to read HEX '{}': {}", hex_path, e))?;
            tokio::task::spawn_blocking(move || {
                load_a2l_hex_source(&a2l_name, &a2l_bytes, &hex_name, &hex_bytes)
            })
            .await
            .map_err(|e| format!("Task error: {}", e))??
        }
        "dcm" => {
            let dcm_name = extract_filename(&file_path);
            let dcm_bytes = std::fs::read(&file_path)
                .map_err(|e| format!("Failed to read DCM '{}': {}", file_path, e))?;
            tokio::task::spawn_blocking(move || load_dcm_source(&dcm_name, &dcm_bytes))
                .await
                .map_err(|e| format!("Task error: {}", e))??
        }
        other => {
            return Ok(json!({
                "success": false,
                "error": format!("Unsupported source type '{}'", other)
            }))
        }
    };

    let mut app = state.write().await;
    let session = app
        .compare_sessions
        .iter_mut()
        .find(|s| s.id == session_id)
        .ok_or_else(|| "Compare session not found".to_string())?;
    *session.source_mut(side) = Some(source);
    let json = session.to_json();

    Ok(json!({ "success": true, "session": json, "side": side.as_str() }))
}

/// Clear one side of a compare session.
#[tauri::command]
pub async fn compare_clear_source(
    state: State<'_, SharedState>,
    session_id: String,
    side: String,
) -> Result<Value, String> {
    let side = CompareSide::from_str(&side).map_err(|e| e)?;
    let mut app = state.write().await;
    let session = app
        .compare_sessions
        .iter_mut()
        .find(|s| s.id == session_id)
        .ok_or_else(|| "Compare session not found".to_string())?;
    *session.source_mut(side) = None;
    let json = session.to_json();
    Ok(json!({ "success": true, "session": json }))
}

/// List variables available for comparison (paginated).
#[tauri::command]
pub async fn compare_variables(
    state: State<'_, SharedState>,
    session_id: String,
    presence: Option<String>,
    search: Option<String>,
    offset: Option<usize>,
    limit: Option<usize>,
) -> Result<Value, String> {
    let app = state.read().await;
    let session = app
        .compare_sessions
        .iter()
        .find(|s| s.id == session_id)
        .cloned()
        .ok_or_else(|| "Compare session not found".to_string())?;
    drop(app);

    let search_str = search.unwrap_or_default();
    let presence_filter =
        ComparePresenceFilter::from_str(presence.as_deref().unwrap_or("all"));
    let offset = offset.unwrap_or(0);
    let limit = limit.unwrap_or(100);

    let page = session.page_variables(&search_str, presence_filter, offset, limit);
    Ok(json!({ "success": true, "page": page }))
}

/// Replace the selected variable list for a session.
#[tauri::command]
pub async fn compare_select_variables(
    state: State<'_, SharedState>,
    session_id: String,
    names: Vec<String>,
) -> Result<Value, String> {
    let mut app = state.write().await;
    let session = app
        .compare_sessions
        .iter_mut()
        .find(|s| s.id == session_id)
        .ok_or_else(|| "Compare session not found".to_string())?;
    session.selected_variables = names;
    let json = session.to_json();
    Ok(json!({ "success": true, "session": json }))
}

/// Get comparison result rows for a session.
#[tauri::command]
pub async fn compare_result(
    state: State<'_, SharedState>,
    session_id: String,
) -> Result<Value, String> {
    let app = state.read().await;
    let session = app
        .compare_sessions
        .iter()
        .find(|s| s.id == session_id)
        .cloned()
        .ok_or_else(|| "Compare session not found".to_string())?;
    drop(app);

    let rows = session.comparison_rows();
    Ok(json!({
        "success": true,
        "session": session.to_json(),
        "rows": rows,
    }))
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn extract_filename(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}
