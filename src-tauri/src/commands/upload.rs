/// ECU memory upload commands.
///
/// Reads ECU memory segments via XCP UPLOAD and writes Intel HEX files to disk.
/// Supports uploading from reference page, working page, or both.
///
/// Progress is reported via the Tauri event `upload-progress` (see `UploadProgressEvent`).

use serde::Serialize;
use serde_json::{json, Value};
use tauri::{Emitter, State};
use xcp_client::{hex::HexImage, protocol::Mta, slave::PageMode};

use crate::state::SharedState;

/// Payload emitted as a Tauri event during `upload_memory`.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadProgressEvent {
    pub device_id: String,
    /// `"reference"` or `"working"`
    pub page: String,
    pub page_index: usize,
    pub pages_total: usize,
    pub segment_name: String,
    pub segments_done: usize,
    pub segments_total: usize,
    /// Bytes uploaded in this single segment (0 on error)
    pub bytes_uploaded: u64,
    pub bytes_total: u64,
    /// Overall progress: 0.0 – 1.0
    pub fraction: f32,
    /// `"uploading"` | `"writing"` | `"done"` | `"error"`
    pub status: String,
    pub message: String,
}

/// Return the list of memory segments for a device so the UI can display them.
///
/// Falls back to re-reading the A2L file if the CalibrationManager has no segments yet.
#[tauri::command]
pub async fn upload_get_segments(
    state: State<'_, SharedState>,
    device_id: String,
) -> Result<Value, String> {
    let (segments, a2l_path) = {
        let app = state.read().await;
        let dev = app
            .get_device(&device_id)
            .ok_or_else(|| format!("Device '{}' not found", device_id))?;
        let segs = dev.cal_manager.read().await.get_memory_segments().to_vec();
        let path = dev.a2l_path.clone();
        (segs, path)
    };

    // If CalibrationManager has no segments, try loading from A2L
    let segments = if segments.is_empty() && !a2l_path.is_empty() {
        tokio::task::spawn_blocking(move || {
            xcp_client::A2lReader::load(&a2l_path)
                .map(|reader| reader.get_memory_segments())
        })
        .await
        .map_err(|e| format!("Task error: {}", e))?
        .map_err(|e| format!("A2L load error: {}", e))?
    } else {
        segments
    };

    let result: Vec<Value> = segments
        .iter()
        .enumerate()
        .map(|(i, seg)| {
            json!({
                "index": i,
                "name": seg.name,
                "description": seg.description,
                "address": format!("0x{:08X}", seg.address),
                "size": seg.size,
                "size_kb": seg.size / 1024,
                "prg_type": seg.prg_type,
                "memory_type": seg.memory_type,
                "calibration_allowed": seg.is_calibration_allowed(),
            })
        })
        .collect();

    Ok(json!({ "segments": result }))
}

/// Upload ECU memory segments from the slave to Intel HEX files on the master PC.
///
/// # Arguments
/// * `device_id`        — Device to upload from (must be connected).
/// * `segment_indices`  — Indices into the A2L segment list to upload.
/// * `pages`            — `"reference"`, `"working"`, or `"both"`.
/// * `output_folder`    — Folder where HEX files are written.
///
/// # Output files
/// `{output_folder}/{safe_device_name}_{page}.hex`
///
/// # Events
/// Emits `"upload-progress"` (`UploadProgressEvent`) after each segment.
///
/// The original page mode is restored after the operation.
#[tauri::command]
pub async fn upload_memory(
    app_handle: tauri::AppHandle,
    state: State<'_, SharedState>,
    device_id: String,
    segment_indices: Vec<usize>,
    pages: String,
    output_folder: String,
) -> Result<Value, String> {
    if segment_indices.is_empty() {
        return Ok(json!({ "success": false, "error": "No segments selected" }));
    }

    // Gather handles under a brief read lock
    let (slave_arc, segments, device_name, original_page_mode) = {
        let app = state.read().await;
        let dev = app
            .get_device(&device_id)
            .ok_or_else(|| format!("Device '{}' not found", device_id))?;

        if !dev.connected {
            return Ok(json!({ "success": false, "error": "Device is not connected" }));
        }

        let sid = dev
            .slave_id
            .ok_or_else(|| "No slave ID for device".to_string())?;
        let slave_arc = app
            .client
            .get_slave(sid)
            .ok_or_else(|| "Slave not found in client".to_string())?;

        let segs = dev.cal_manager.read().await.get_memory_segments().to_vec();
        let original = *dev.page_mode.read().await;
        (slave_arc, segs, dev.name.clone(), original)
    };

    // Fall back to A2L if cal_manager has no segments
    let segments = if segments.is_empty() {
        let a2l_path = {
            let app = state.read().await;
            app.get_device(&device_id)
                .map(|d| d.a2l_path.clone())
                .unwrap_or_default()
        };
        if a2l_path.is_empty() {
            return Ok(json!({ "success": false, "error": "No A2L file loaded — cannot determine memory segments" }));
        }
        tokio::task::spawn_blocking(move || {
            xcp_client::A2lReader::load(&a2l_path)
                .map(|reader| reader.get_memory_segments())
        })
        .await
        .map_err(|e| format!("Task error: {}", e))?
        .map_err(|e| format!("A2L load error: {}", e))?
    } else {
        segments
    };

    if segments.is_empty() {
        return Ok(json!({ "success": false, "error": "No memory segments found in A2L file" }));
    }

    // Resolve the selected segment references (filter invalid indices)
    let selected: Vec<_> = segment_indices
        .iter()
        .filter_map(|&i| segments.get(i))
        .collect();

    if selected.is_empty() {
        return Ok(json!({ "success": false, "error": "All selected segment indices are out of range" }));
    }

    // Determine page sequence
    let page_list: Vec<PageMode> = match pages.to_lowercase().trim() {
        "reference" => vec![PageMode::Reference],
        "working" => vec![PageMode::Working],
        _ => vec![PageMode::Reference, PageMode::Working],
    };

    // Sanitize device name for use in filenames
    let safe_name: String = device_name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '_' })
        .collect();

    let output_path = std::path::Path::new(&output_folder);
    let mut files_written: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    // Total work units for overall progress (segments × pages)
    let total_steps = selected.len() * page_list.len();
    let mut steps_done: usize = 0;
    let bytes_total: u64 = selected.iter().map(|s| s.size as u64).sum();

    let mut slave_guard = slave_arc.lock().await;

    for (page_idx, page_mode) in page_list.iter().enumerate() {
        let page_name = match page_mode {
            PageMode::Reference => "reference",
            PageMode::Working => "working",
        };

        // Switch all segments to the target page
        match slave_guard.switch_all_pages(*page_mode).await {
            Ok(switched) => {
                if switched.is_empty() {
                    tracing::debug!(
                        "upload_memory: no segments switched to {} page (no XCP page info)",
                        page_name
                    );
                } else {
                    tracing::info!(
                        "upload_memory: switched {} segment(s) to {} page",
                        switched.len(),
                        page_name
                    );
                }
            }
            Err(e) => {
                let msg = format!("Cannot switch to {} page: {}", page_name, e);
                tracing::warn!("upload_memory: {}", msg);
                warnings.push(msg.clone());
                steps_done += selected.len();
                continue;
            }
        }

        // Upload each selected segment
        let mut image = HexImage::new();

        for (seg_idx, seg) in selected.iter().enumerate() {
            let mta = Mta { address: seg.address, extension: 0 };
            let (bytes_uploaded, status, message) =
                match slave_guard.read_large_data(mta, seg.size as usize, None).await {
                    Ok(data) => {
                        let uploaded = data.len() as u64;
                        image.add_data(seg.address, &data);
                        tracing::info!(
                            "upload_memory: segment '{}' — {} bytes from {} page",
                            seg.name,
                            data.len(),
                            page_name
                        );
                        (uploaded, "uploading", format!("{} – {} bytes", seg.name, uploaded))
                    }
                    Err(e) => {
                        let msg = format!("Segment '{}' upload failed: {}", seg.name, e);
                        tracing::warn!("upload_memory: {}", msg);
                        warnings.push(msg.clone());
                        (0u64, "error", msg)
                    }
                };

            steps_done += 1;
            let fraction = if total_steps == 0 { 1.0 } else { steps_done as f32 / total_steps as f32 };

            let _ = app_handle.emit(
                "upload-progress",
                UploadProgressEvent {
                    device_id: device_id.clone(),
                    page: page_name.to_string(),
                    page_index: page_idx,
                    pages_total: page_list.len(),
                    segment_name: seg.name.clone(),
                    segments_done: seg_idx + 1,
                    segments_total: selected.len(),
                    bytes_uploaded,
                    bytes_total,
                    fraction,
                    status: status.to_string(),
                    message,
                },
            );
        }

        if image.total_bytes() == 0 {
            warnings.push(format!("No data uploaded for {} page — file skipped", page_name));
            continue;
        }

        // Write Intel HEX file
        let filename = format!("{}_{}.hex", safe_name, page_name);
        let file_path = output_path.join(&filename);
        let hex_str = image.to_intel_hex();

        match std::fs::write(&file_path, &hex_str) {
            Ok(()) => {
                tracing::info!(
                    "upload_memory: wrote {} ({} bytes of data)",
                    file_path.display(),
                    image.total_bytes()
                );
                files_written.push(filename);
            }
            Err(e) => {
                let msg = format!("Failed to write '{}': {}", filename, e);
                tracing::error!("upload_memory: {}", msg);
                warnings.push(msg);
            }
        }
    }

    // Always restore original page mode
    if let Err(e) = slave_guard.switch_all_pages(original_page_mode).await {
        tracing::warn!("upload_memory: failed to restore original page mode: {}", e);
    }
    drop(slave_guard);

    // Final "done" event
    let _ = app_handle.emit(
        "upload-progress",
        UploadProgressEvent {
            device_id: device_id.clone(),
            page: String::new(),
            page_index: page_list.len(),
            pages_total: page_list.len(),
            segment_name: String::new(),
            segments_done: selected.len(),
            segments_total: selected.len(),
            bytes_uploaded: 0,
            bytes_total,
            fraction: 1.0,
            status: if files_written.is_empty() { "error" } else { "done" }.to_string(),
            message: if files_written.is_empty() {
                warnings.first().cloned().unwrap_or_else(|| "Upload failed".to_string())
            } else {
                format!("Done — {} file(s) written", files_written.len())
            },
        },
    );

    if files_written.is_empty() {
        Ok(json!({
            "success": false,
            "error": if warnings.is_empty() {
                "No files were written".to_string()
            } else {
                warnings.join("; ")
            },
            "warnings": warnings,
        }))
    } else {
        let message = if files_written.len() == 1 {
            format!("Uploaded → {}", files_written[0])
        } else {
            format!("Uploaded {} file(s)", files_written.len())
        };
        Ok(json!({
            "success": true,
            "files": files_written,
            "warnings": warnings,
            "message": message,
            "output_folder": output_folder,
        }))
    }
}
