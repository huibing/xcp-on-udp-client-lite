/// Checksum/HEX verification Tauri commands.
///
/// Ports web/routes.rs hex_load + checksum_verify.
/// File selection uses tauri-plugin-dialog instead of multipart upload.

use serde_json::{json, Value};
use tauri::State;
use xcp_client::hex::HexImage;
use xcp_client::slave::PageMode;

use crate::state::{derive_working_hex_path, SharedState};

/// Load a HEX file from `file_path` and store it in the device state.
#[tauri::command]
pub async fn hex_load(
    state: State<'_, SharedState>,
    device_id: String,
    file_path: String,
) -> Result<Value, String> {
    // Read file content
    let hex_str = std::fs::read_to_string(&file_path)
        .map_err(|e| format!("Failed to read HEX file '{}': {}", file_path, e))?;

    let fname = std::path::Path::new(&file_path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| file_path.clone());

    // Parse in blocking context (CPU-intensive)
    let hex_image = tokio::task::spawn_blocking(move || HexImage::from_str(&hex_str))
        .await
        .map_err(|e| format!("Task error: {}", e))?
        .map_err(|e| format!("HEX parse error: {}", e))?;

    let byte_count = hex_image.total_bytes();

    // Derive working page HEX path
    let working_path = derive_working_hex_path(&file_path);

    // Try to load an existing working page HEX; fall back to cloning the reference
    let working_hex = std::fs::read_to_string(&working_path)
        .ok()
        .and_then(|s| HexImage::from_str(&s).ok())
        .unwrap_or_else(|| hex_image.clone());

    // Store both the reference and working page hex images
    {
        let app = state.read().await;
        let dev = app
            .get_device(&device_id)
            .ok_or_else(|| format!("Device '{}' not found", device_id))?;
        *dev.hex_image.write().await = Some(hex_image);
        *dev.working_hex_image.write().await = Some(working_hex);
        *dev.hex_filename.write().await = Some(fname.clone());
        *dev.checksum_report.write().await = None;
    }

    {
        let mut app = state.write().await;
        let dev = app
            .get_device_mut(&device_id)
            .ok_or_else(|| format!("Device '{}' not found", device_id))?;
        dev.hex_path = file_path.clone();
        dev.working_hex_path = working_path;
        app.save_config()?;
    }

    Ok(json!({
        "success": true,
        "message": format!("{} loaded ({} bytes of data)", fname, byte_count),
        "byte_count": byte_count,
        "filename": fname,
    }))
}

/// Verify checksums of all memory segments for a device.
#[tauri::command]
pub async fn checksum_verify(
    state: State<'_, SharedState>,
    device_id: String,
    max_block_size: Option<u32>,
) -> Result<Value, String> {
    // Gather everything we need under a brief read lock
    let (slave_arc, hex_image, segments, a2l_path) = {
        let app = state.read().await;
        let dev = app
            .get_device(&device_id)
            .ok_or_else(|| format!("Device '{}' not found", device_id))?;

        if !dev.connected {
            return Ok(json!({ "success": false, "error": "Device is not connected" }));
        }

        // Use working page hex when in working page mode, reference hex otherwise
        let page_mode = *dev.page_mode.read().await;
        let hex_image = if matches!(page_mode, PageMode::Working) {
            dev.working_hex_image
                .read()
                .await
                .clone()
                .ok_or_else(|| "No working page HEX file for this device".to_string())?
        } else {
            dev.hex_image
                .read()
                .await
                .clone()
                .ok_or_else(|| "No HEX file loaded for this device".to_string())?
        };

        let sid = dev
            .slave_id
            .ok_or_else(|| "Device has no slave ID".to_string())?;
        let slave_arc = app
            .client
            .get_slave(sid)
            .ok_or_else(|| "Slave not found in client".to_string())?;

        // Get memory segments from CalibrationManager (populated during A2L load)
        let segments = dev.cal_manager.read().await.get_memory_segments().to_vec();
        let a2l_path = dev.a2l_path.clone();

        (slave_arc, hex_image, segments, a2l_path)
    };

    // If CalibrationManager has no segments, try re-reading from A2L
    let segments = if segments.is_empty() {
        if a2l_path.is_empty() {
            return Ok(json!({
                "success": false,
                "error": "No A2L file loaded — cannot determine memory segments"
            }));
        }
        let path = a2l_path.clone();
        tokio::task::spawn_blocking(move || {
            xcp_client::A2lReader::load(&path)
                .map(|reader| reader.get_memory_segments())
        })
        .await
        .map_err(|e| format!("Task error: {}", e))?
        .map_err(|e| format!("A2L load error: {:?}", e))?
    } else {
        segments
    };

    if segments.is_empty() {
        return Ok(json!({
            "success": false,
            "error": "No memory segments found in A2L file"
        }));
    }

    // Run verification (XCP I/O — slave lock only, no state lock)
    let mut slave_guard = slave_arc.lock().await;
    let report =
        xcp_client::checksum::verify_segments(&mut slave_guard, &hex_image, &segments, max_block_size)
            .await
            .map_err(|e| format!("Verification error: {}", e))?;
    drop(slave_guard);

    // Store report
    let app = state.read().await;
    if let Some(dev) = app.get_device(&device_id) {
        *dev.checksum_report.write().await = Some(report.clone());
    }

    let all_passed = report.all_passed();
    let segment_results: Vec<Value> = report
        .results
        .iter()
        .map(|s| {
            let mut seg = json!({
                "name": s.name,
                "address": format!("0x{:08X}", s.address),
                "size": s.size,
                "algorithm": s.algorithm,
                "master_checksum": format!("0x{:08X}", s.master_checksum),
                "slave_checksum": format!("0x{:08X}", s.slave_checksum),
                "match": s.passed,
                "status": format!("{}", s.status),
            });
            if let Some(chunks) = &s.chunks {
                seg["chunks"] = json!(chunks
                    .iter()
                    .map(|c| json!({
                        "offset": format!("0x{:08X}", c.offset),
                        "size": c.size,
                        "master_checksum": format!("0x{:08X}", c.master_checksum),
                        "slave_checksum": format!("0x{:08X}", c.slave_checksum),
                        "match": c.passed,
                    }))
                    .collect::<Vec<_>>());
            }
            seg
        })
        .collect();

    Ok(json!({
        "success": true,
        "all_passed": all_passed,
        "summary": report.summary(),
        "total": report.total_segments(),
        "passed": report.passed(),
        "failed": report.failed(),
        "segments": segment_results,
    }))
}

/// Load a dedicated HEX file as the working page (without replacing the reference HEX).
///
/// The user can supply any HEX file (e.g. a previously saved working image or a
/// partial patch) to use as the working page instead of the auto-derived copy.
/// Subsequent calibration writes will patch this file in place.
#[tauri::command]
pub async fn working_hex_load(
    state: State<'_, SharedState>,
    device_id: String,
    file_path: String,
) -> Result<Value, String> {
    let hex_str = std::fs::read_to_string(&file_path)
        .map_err(|e| format!("Failed to read HEX file '{}': {}", file_path, e))?;

    let fname = std::path::Path::new(&file_path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| file_path.clone());

    let hex_image = tokio::task::spawn_blocking(move || HexImage::from_str(&hex_str))
        .await
        .map_err(|e| format!("Task error: {}", e))?
        .map_err(|e| format!("HEX parse error: {}", e))?;

    let byte_count = hex_image.total_bytes();

    // Overwrite only the working page HEX; reference stays unchanged
    {
        let app = state.read().await;
        let dev = app
            .get_device(&device_id)
            .ok_or_else(|| format!("Device '{}' not found", device_id))?;
        *dev.working_hex_image.write().await = Some(hex_image);
        *dev.checksum_report.write().await = None;
    }
    {
        let mut app = state.write().await;
        let dev = app
            .get_device_mut(&device_id)
            .ok_or_else(|| format!("Device '{}' not found", device_id))?;
        dev.working_hex_path = file_path.clone();
    }

    Ok(json!({
        "success": true,
        "message": format!("{} loaded as working page ({} bytes)", fname, byte_count),
        "byte_count": byte_count,
        "filename": fname,
    }))
}
