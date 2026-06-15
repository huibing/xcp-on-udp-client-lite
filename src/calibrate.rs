//! Non-interactive calibration CLI subcommand.
//!
//! Supports two modes:
//! - Single variable: `--var name --value 10`
//! - DCM file: `--dcm file.DCM` (bulk calibration from DAMOS file)

use dcm_utils::block::Block as DcmBlock;
use dcm_utils::value::Value as DcmValue;
use dcm_utils::DcmData;
use std::path::Path;
use xcp_client::transport::TransportConfig;
use xcp_client::{
    matrix_to_flat, A2lReader, CalibrationManager, CalibrationParam, CalibrationType,
    CalibrationValue, PageMode, StorageOrder, XcpClient,
};

/// Run the calibrate subcommand
pub async fn run_calibrate(
    a2l_path: &str,
    var: Option<String>,
    value: Option<String>,
    dcm_path: Option<String>,
    addr_override: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Validate arguments
    let mode = match (&var, &value, &dcm_path) {
        (Some(v), Some(val), None) => CalibrateMode::SingleVar {
            name: v.clone(),
            value: val.clone(),
        },
        (None, None, Some(dcm)) => CalibrateMode::DcmFile { path: dcm.clone() },
        (Some(_), None, _) => {
            eprintln!("Error: --value is required when using --var");
            std::process::exit(1);
        }
        (None, Some(_), _) => {
            eprintln!("Error: --var is required when using --value");
            std::process::exit(1);
        }
        (Some(_), Some(_), Some(_)) => {
            eprintln!("Error: cannot use --var/--value together with --dcm");
            std::process::exit(1);
        }
        (None, None, None) => {
            eprintln!("Error: specify either --var/--value or --dcm");
            std::process::exit(1);
        }
    };

    // Load A2L file
    println!("Loading A2L file: {}", a2l_path);
    let reader = A2lReader::load(a2l_path).map_err(|e| format!("Failed to load A2L: {}", e))?;

    // Get slave address (override or from A2L)
    let slave_addr = if let Some(addr) = addr_override {
        addr
    } else {
        let addrs = reader.get_slave_addresses();
        addrs
            .into_iter()
            .next()
            .ok_or("No slave address found in A2L file. Use --addr to specify one.")?
    };

    // Build calibration manager from A2L
    let cal_manager = CalibrationManager::from_a2l(a2l_path)
        .map_err(|e| format!("Failed to create calibration manager: {}", e))?;

    let memory_segments = cal_manager.get_memory_segments().to_vec();
    let param_count = cal_manager.params().len();
    println!("  {} calibration parameters loaded", param_count);
    println!("  {} memory segments", memory_segments.len());

    // Prepare write jobs
    let jobs = match &mode {
        CalibrateMode::SingleVar { name, value } => {
            let _param = cal_manager
                .params()
                .iter()
                .find(|p| p.name == *name)
                .ok_or_else(|| format!("Variable '{}' not found in A2L", name))?;
            vec![WriteJob {
                param_name: name.clone(),
                value_str: value.clone(),
                source: "CLI".to_string(),
            }]
        }
        CalibrateMode::DcmFile { path } => {
            println!("Loading DCM file: {}", path);
            build_dcm_jobs(path, &cal_manager)?
        }
    };

    if jobs.is_empty() {
        println!("No matching variables found to calibrate.");
        return Ok(());
    }

    println!("  {} variable(s) to write", jobs.len());

    // Connect to slave
    println!("\nConnecting to slave at {}...", slave_addr);
    let mut client = XcpClient::new();
    let transport_config = TransportConfig::Udp {
        addr: slave_addr.clone(),
    };
    let slave_id = client
        .connect_slave("calibrate-target", transport_config, None)
        .await
        .map_err(|e| format!("Failed to connect: {}", e))?;
    println!("  Connected (id: {})", slave_id);

    // Setup: load segments, query page state, switch to working page
    let setup_result = async {
        let xcp_slave = client
            .get_slave(slave_id)
            .ok_or("Failed to get slave handle")?;
        let mut guard = xcp_slave.lock().await;

        // Set memory segments from A2L
        guard.set_memory_segments(memory_segments, Some(std::path::PathBuf::from(a2l_path)));

        // Query current page state
        if let Err(e) = guard.update_segment_page_state().await {
            tracing::warn!("Failed to query page state: {:?}", e);
            println!("  Warning: could not query page state: {}", e);
        }

        // Switch to working page
        match guard.switch_all_pages(PageMode::Working).await {
            Ok(switched) => {
                if switched.is_empty() {
                    println!("  Already in working page mode");
                } else {
                    for (seg, page) in &switched {
                        println!("  Switched segment {} to working page {}", seg, page);
                    }
                }
            }
            Err(e) => {
                println!("  Warning: page switch failed: {} (continuing anyway)", e);
            }
        }

        Ok::<(), String>(())
    }
    .await;

    if let Err(e) = setup_result {
        eprintln!("Setup failed: {}", e);
        let _ = client.disconnect_slave(slave_id).await;
        return Err(e.into());
    }

    // Write values
    println!("\nWriting calibration values...");
    let mut success_count = 0;
    let mut fail_count = 0;
    let mut errors: Vec<String> = Vec::new();

    for job in &jobs {
        let result = write_single_value(&client, slave_id, &cal_manager, job).await;
        match result {
            Ok(()) => {
                println!("  ✓ {} = {}", job.param_name, job.value_str);
                success_count += 1;
            }
            Err(e) => {
                let msg = format!("  ✗ {} = {} → {}", job.param_name, job.value_str, e);
                eprintln!("{}", msg);
                errors.push(format!("{}: {}", job.param_name, e));
                fail_count += 1;
            }
        }
    }

    // Disconnect
    println!("\nDisconnecting...");
    if let Err(e) = client.disconnect_slave(slave_id).await {
        eprintln!("Warning: disconnect failed: {}", e);
    }

    // Summary
    println!("\n--- Calibration Summary ---");
    println!("  Total:   {}", jobs.len());
    println!("  Success: {}", success_count);
    println!("  Failed:  {}", fail_count);

    if fail_count > 0 {
        eprintln!("\nFailed variables:");
        for err in &errors {
            eprintln!("  - {}", err);
        }
        std::process::exit(1);
    } else {
        println!("\nCalibration completed successfully.");
    }

    Ok(())
}

enum CalibrateMode {
    SingleVar { name: String, value: String },
    DcmFile { path: String },
}

struct WriteJob {
    param_name: String,
    value_str: String,
    #[allow(dead_code)]
    source: String,
}

/// Write a single calibration value to the slave
async fn write_single_value(
    client: &XcpClient,
    slave_id: xcp_client::SlaveId,
    cal_manager: &CalibrationManager,
    job: &WriteJob,
) -> Result<(), String> {
    let param = cal_manager
        .params()
        .iter()
        .find(|p| p.name == job.param_name)
        .ok_or_else(|| format!("Parameter '{}' not found", job.param_name))?;

    let value = cal_manager.parse_value(param, &job.value_str)?;
    tracing::debug!(
        "write_single_value: param='{}', data_type={:?}, storage_order={:?}, address=0x{:08X}, ext={}",
        param.name, param.data_type, param.storage_order, param.address, param.address_ext
    );
    tracing::debug!("write_single_value: parsed CalibrationValue: {:?}", value);

    let xcp_slave = client.get_slave(slave_id).ok_or("Slave handle lost")?;
    let guard = xcp_slave.lock().await;

    write_to_slave(&guard, param, &value).await
}

/// Convert a CalibrationValue matrix to ECU byte representation.
///
/// The A2L reader maps axis_descr[0] (X-axis) → rows and axis_descr[1] (Y-axis) → cols,
/// and `parse_storage_order` swaps A2L COLUMN_DIR→RowDir, ROW_DIR→ColumnDir so that
/// `matrix_to_flat(storage_order)` on a [rows=X][cols=Y] matrix produces the correct ASAM
/// byte order. No additional invert is needed — the same pattern used by
/// `calibration.rs::write_calibration` (web UI path).
fn matrix_value_to_bytes(
    value: &CalibrationValue,
    rows: usize,
    cols: usize,
    storage_order: StorageOrder,
) -> Option<Vec<u8>> {
    tracing::debug!(
        "matrix_value_to_bytes: rows={}, cols={}, storage_order={:?}",
        rows,
        cols,
        storage_order
    );
    let result = match value {
        CalibrationValue::U8Matrix(mat) => Some(matrix_to_flat(mat, rows, cols, storage_order)),
        CalibrationValue::U16Matrix(mat) => {
            let flat: Vec<u16> = matrix_to_flat(mat, rows, cols, storage_order);
            Some(flat.iter().flat_map(|v| v.to_le_bytes()).collect())
        }
        CalibrationValue::U32Matrix(mat) => {
            let flat: Vec<u32> = matrix_to_flat(mat, rows, cols, storage_order);
            Some(flat.iter().flat_map(|v| v.to_le_bytes()).collect())
        }
        CalibrationValue::U64Matrix(mat) => {
            let flat: Vec<u64> = matrix_to_flat(mat, rows, cols, storage_order);
            Some(flat.iter().flat_map(|v| v.to_le_bytes()).collect())
        }
        CalibrationValue::I8Matrix(mat) => {
            let flat: Vec<i8> = matrix_to_flat(mat, rows, cols, storage_order);
            Some(flat.iter().map(|v| *v as u8).collect())
        }
        CalibrationValue::I16Matrix(mat) => {
            let flat: Vec<i16> = matrix_to_flat(mat, rows, cols, storage_order);
            Some(flat.iter().flat_map(|v| v.to_le_bytes()).collect())
        }
        CalibrationValue::I32Matrix(mat) => {
            let flat: Vec<i32> = matrix_to_flat(mat, rows, cols, storage_order);
            Some(flat.iter().flat_map(|v| v.to_le_bytes()).collect())
        }
        CalibrationValue::I64Matrix(mat) => {
            let flat: Vec<i64> = matrix_to_flat(mat, rows, cols, storage_order);
            Some(flat.iter().flat_map(|v| v.to_le_bytes()).collect())
        }
        CalibrationValue::F32Matrix(mat) => {
            let flat: Vec<f32> = matrix_to_flat(mat, rows, cols, storage_order);
            Some(flat.iter().flat_map(|v| v.to_le_bytes()).collect())
        }
        CalibrationValue::F64Matrix(mat) => {
            let flat: Vec<f64> = matrix_to_flat(mat, rows, cols, storage_order);
            Some(flat.iter().flat_map(|v| v.to_le_bytes()).collect())
        }
        CalibrationValue::BoolMatrix(mat) => {
            let flat: Vec<bool> = matrix_to_flat(mat, rows, cols, storage_order);
            Some(flat.iter().map(|v| if *v { 1u8 } else { 0u8 }).collect())
        }
        _ => None,
    };
    if let Some(ref data) = result {
        tracing::debug!(
            "matrix_value_to_bytes: output {} bytes, first 32 bytes: {:02X?}",
            data.len(),
            &data[..data.len().min(32)]
        );
        // For F32, also log the f32 values in memory order
        if data.len() % 4 == 0 {
            let f32_vals: Vec<f32> = data
                .chunks(4)
                .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect();
            tracing::debug!(
                "matrix_value_to_bytes: F32 memory order (first 16): {:?}",
                &f32_vals[..f32_vals.len().min(16)]
            );
        }
    }
    result
}

/// Extract (rows, cols) from a matrix CalibrationType, or None for non-matrix types.
fn matrix_dimensions(dt: &CalibrationType) -> Option<(usize, usize)> {
    match dt {
        CalibrationType::U8Matrix(r, c)
        | CalibrationType::U16Matrix(r, c)
        | CalibrationType::U32Matrix(r, c)
        | CalibrationType::U64Matrix(r, c)
        | CalibrationType::I8Matrix(r, c)
        | CalibrationType::I16Matrix(r, c)
        | CalibrationType::I32Matrix(r, c)
        | CalibrationType::I64Matrix(r, c)
        | CalibrationType::F32Matrix(r, c)
        | CalibrationType::F64Matrix(r, c)
        | CalibrationType::BoolMatrix(r, c) => Some((*r, *c)),
        _ => None,
    }
}

/// Write calibration data to the slave via XCP DOWNLOAD
async fn write_to_slave(
    slave: &xcp_client::XcpSlave,
    param: &CalibrationParam,
    value: &CalibrationValue,
) -> Result<(), String> {
    use xcp_client::protocol::Mta;

    let mta = Mta::new(param.address, param.address_ext);
    tracing::debug!(
        "write_to_slave: param='{}', address=0x{:08X}, ext={}, data_type={:?}",
        param.name,
        param.address,
        param.address_ext,
        param.data_type
    );

    // For matrix types, use the dedicated helper that handles the dimension convention
    if let Some((rows, cols)) = matrix_dimensions(&param.data_type) {
        if let Some(data) = matrix_value_to_bytes(value, rows, cols, param.storage_order) {
            tracing::debug!(
                "write_to_slave: matrix {} bytes to 0x{:08X}, first 32 bytes: {:02X?}",
                data.len(),
                param.address,
                &data[..data.len().min(32)]
            );
            return if data.len() <= 255 {
                slave
                    .write_memory_sequential(mta, &data)
                    .await
                    .map_err(|e| e.to_string())
            } else {
                slave
                    .write_u8_array_sequential(mta, &data)
                    .await
                    .map_err(|e| e.to_string())
            };
        }
        return Err(format!("Type mismatch for matrix parameter {}", param.name));
    }

    let data: Vec<u8> = match (value, &param.data_type) {
        (CalibrationValue::U8(v), CalibrationType::U8) => vec![*v],
        (CalibrationValue::U16(v), CalibrationType::U16) => v.to_le_bytes().to_vec(),
        (CalibrationValue::U32(v), CalibrationType::U32) => v.to_le_bytes().to_vec(),
        (CalibrationValue::U64(v), CalibrationType::U64) => v.to_le_bytes().to_vec(),
        (CalibrationValue::I8(v), CalibrationType::I8) => vec![*v as u8],
        (CalibrationValue::I16(v), CalibrationType::I16) => v.to_le_bytes().to_vec(),
        (CalibrationValue::I32(v), CalibrationType::I32) => v.to_le_bytes().to_vec(),
        (CalibrationValue::I64(v), CalibrationType::I64) => v.to_le_bytes().to_vec(),
        (CalibrationValue::F32(v), CalibrationType::F32) => v.to_le_bytes().to_vec(),
        (CalibrationValue::F64(v), CalibrationType::F64) => v.to_le_bytes().to_vec(),
        (CalibrationValue::Bool(v), CalibrationType::Bool) => vec![if *v { 1 } else { 0 }],
        // Arrays
        (CalibrationValue::U8Array(arr), CalibrationType::U8Array(_)) => arr.clone(),
        (CalibrationValue::U16Array(arr), CalibrationType::U16Array(_)) => {
            arr.iter().flat_map(|v| v.to_le_bytes()).collect()
        }
        (CalibrationValue::U32Array(arr), CalibrationType::U32Array(_)) => {
            arr.iter().flat_map(|v| v.to_le_bytes()).collect()
        }
        (CalibrationValue::I8Array(arr), CalibrationType::I8Array(_)) => {
            arr.iter().map(|v| *v as u8).collect()
        }
        (CalibrationValue::I16Array(arr), CalibrationType::I16Array(_)) => {
            arr.iter().flat_map(|v| v.to_le_bytes()).collect()
        }
        (CalibrationValue::I32Array(arr), CalibrationType::I32Array(_)) => {
            arr.iter().flat_map(|v| v.to_le_bytes()).collect()
        }
        (CalibrationValue::F32Array(arr), CalibrationType::F32Array(_)) => {
            arr.iter().flat_map(|v| v.to_le_bytes()).collect()
        }
        (CalibrationValue::F64Array(arr), CalibrationType::F64Array(_)) => {
            arr.iter().flat_map(|v| v.to_le_bytes()).collect()
        }
        _ => {
            return Err(format!(
                "Unsupported type combination: {:?}",
                param.data_type
            ))
        }
    };

    if data.len() <= 255 {
        slave
            .write_memory_sequential(mta, &data)
            .await
            .map_err(|e| e.to_string())
    } else {
        slave
            .write_u8_array_sequential(mta, &data)
            .await
            .map_err(|e| e.to_string())
    }
}

/// Build write jobs from a DCM file by matching blocks to A2L parameters
fn build_dcm_jobs(
    dcm_path: &str,
    cal_manager: &CalibrationManager,
) -> Result<Vec<WriteJob>, String> {
    let dcm = DcmData::new(Path::new(dcm_path));
    let mut jobs = Vec::new();
    let mut skipped = Vec::new();

    for (name, block) in &dcm.blocks {
        // Find matching CalibrationParam
        let param = match cal_manager.params().iter().find(|p| p.name == *name) {
            Some(p) => p,
            None => {
                skipped.push(name.clone());
                continue;
            }
        };

        match block {
            DcmBlock::Constant(festwert) => {
                let value_str = dcm_value_to_raw_string(&festwert.value, param)?;
                jobs.push(WriteJob {
                    param_name: name.clone(),
                    value_str,
                    source: "DCM/FESTWERT".to_string(),
                });
            }
            DcmBlock::ConstantBlock(fbl) => {
                let value_str = dcm_array_to_raw_string(&fbl.value, param)?;
                jobs.push(WriteJob {
                    param_name: name.clone(),
                    value_str,
                    source: "DCM/FESTWERTEBLOCK".to_string(),
                });
            }
            DcmBlock::Table(table) => {
                // For GRUPPENKENNLINIE, write the WERT values (axis is handled separately)
                let value_str = dcm_array_to_raw_string(&table.value, param)?;
                jobs.push(WriteJob {
                    param_name: name.clone(),
                    value_str,
                    source: "DCM/GRUPPENKENNLINIE".to_string(),
                });
            }
            DcmBlock::Distribution(dist) => {
                // STUETZSTELLENVERTEILUNG — axis breakpoints
                let value_str = dcm_array_to_raw_string(&dist.value, param)?;
                jobs.push(WriteJob {
                    param_name: name.clone(),
                    value_str,
                    source: "DCM/STUETZSTELLENVERTEILUNG".to_string(),
                });
            }
            DcmBlock::Map(map) => {
                // For GRUPPENKENNFELD, flatten the 2D values row by row
                let flat_values = dcm_map_to_raw_string(map, param)?;
                jobs.push(WriteJob {
                    param_name: name.clone(),
                    value_str: flat_values,
                    source: "DCM/GRUPPENKENNFELD".to_string(),
                });
            }
        }
    }

    if !skipped.is_empty() {
        println!(
            "  Skipped {} DCM variable(s) not found in A2L:",
            skipped.len()
        );
        for name in skipped.iter().take(10) {
            println!("    - {}", name);
        }
        if skipped.len() > 10 {
            println!("    ... and {} more", skipped.len() - 10);
        }
    }

    Ok(jobs)
}

/// Convert a DCM scalar value to a raw value string suitable for parse_value
fn dcm_value_to_raw_string(value: &DcmValue, param: &CalibrationParam) -> Result<String, String> {
    match value {
        DcmValue::WERT(values) => {
            if values.len() != 1 {
                return Err(format!("Expected scalar, got {} values", values.len()));
            }
            physical_to_raw_string(values[0], param)
        }
        DcmValue::TEXT(texts) => {
            if texts.len() != 1 {
                return Err(format!("Expected single text, got {} values", texts.len()));
            }
            // For enum types, return the text directly — parse_value handles enum lookup
            Ok(texts[0].clone())
        }
    }
}

/// Convert a DCM array/vector value to a comma-separated raw value string
fn dcm_array_to_raw_string(value: &DcmValue, param: &CalibrationParam) -> Result<String, String> {
    match value {
        DcmValue::WERT(values) => {
            let raw_strings: Result<Vec<String>, String> = values
                .iter()
                .map(|v| physical_to_raw_string(*v, param))
                .collect();
            Ok(raw_strings?.join(", "))
        }
        DcmValue::TEXT(texts) => Ok(texts.join(", ")),
    }
}

/// Convert a DCM 2D map to a comma-separated raw value string.
///
/// DCM stores MAP data as [Y-axis rows][X-axis cols], but the A2L reader
/// maps axis_descr[0] (X) → rows and axis_descr[1] (Y) → cols.
/// So we must transpose from DCM's [Y][X] to A2L's [X][Y] ordering.
fn dcm_map_to_raw_string(
    map: &dcm_utils::blocks::GRUPPENKENNFELD,
    param: &CalibrationParam,
) -> Result<String, String> {
    let (dcm_ncol, dcm_nrow) = map.dim; // ncol = X-axis size, nrow = Y-axis size
    tracing::debug!(
        "dcm_map_to_raw_string: param='{}', dcm_ncol(X)={}, dcm_nrow(Y)={}, storage_order={:?}",
        param.name,
        dcm_ncol,
        dcm_nrow,
        param.storage_order
    );

    // Collect DCM values into [nrow(Y)][ncol(X)] numeric matrix
    let dcm_values: Vec<Vec<f64>> = map
        .value
        .iter()
        .filter_map(|row| match row {
            DcmValue::WERT(values) => Some(values.clone()),
            DcmValue::TEXT(_) => None,
        })
        .collect();

    tracing::debug!(
        "dcm_map_to_raw_string: DCM raw matrix rows={}, first row len={}",
        dcm_values.len(),
        dcm_values.first().map(|r| r.len()).unwrap_or(0)
    );
    for (y, row) in dcm_values.iter().enumerate() {
        tracing::debug!("dcm_map_to_raw_string: DCM Y={} row: {:?}", y, row);
    }

    // Handle TEXT maps (no transpose needed for enum lookups)
    if dcm_values.is_empty() {
        let mut all_raw: Vec<String> = Vec::new();
        for row in &map.value {
            if let DcmValue::TEXT(texts) = row {
                all_raw.extend(texts.iter().cloned());
            }
        }
        return Ok(all_raw.join(", "));
    }

    // Transpose from [Y][X] → [X][Y] to match A2L's (rows=X, cols=Y) convention
    let mut all_raw: Vec<String> = Vec::new();
    for x in 0..dcm_ncol {
        for y in 0..dcm_nrow {
            if y >= dcm_values.len() || x >= dcm_values[y].len() {
                return Err(format!(
                    "DCM dimension mismatch: expected [{}][{}] but got insufficient data",
                    dcm_nrow, dcm_ncol
                ));
            }
            all_raw.push(physical_to_raw_string(dcm_values[y][x], param)?);
        }
    }
    tracing::debug!(
        "dcm_map_to_raw_string: transposed to [X][Y] flat string ({} values): {}",
        all_raw.len(),
        all_raw.join(", ")
    );
    Ok(all_raw.join(", "))
}

/// Convert a physical f64 value to a raw value string for a given parameter.
///
/// For float types (F32/F64), the physical value IS the stored value (no conversion).
/// For integer types, applies reverse COMPU_METHOD to get the raw ECU value.
fn physical_to_raw_string(physical: f64, param: &CalibrationParam) -> Result<String, String> {
    match &param.data_type {
        // Float types: physical = stored value, write directly
        CalibrationType::F32
        | CalibrationType::F64
        | CalibrationType::F32Array(_)
        | CalibrationType::F64Array(_)
        | CalibrationType::F32Matrix(_, _)
        | CalibrationType::F64Matrix(_, _) => Ok(physical.to_string()),
        // Bool
        CalibrationType::Bool
        | CalibrationType::BoolArray(_)
        | CalibrationType::BoolMatrix(_, _) => {
            Ok(if physical != 0.0 { "true" } else { "false" }.to_string())
        }
        // Integer types: convert physical → raw via compu_method
        _ => {
            let raw = param.compu_method.to_raw(&physical.to_string())?;
            Ok(raw.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xcp_client::CompuMethod;

    fn make_param(name: &str, dt: CalibrationType, cm: CompuMethod) -> CalibrationParam {
        let mut p = CalibrationParam::new(name, 0x1000, 0, dt);
        p.compu_method = cm;
        p
    }

    #[test]
    fn test_physical_to_raw_f32() {
        let param = make_param("test", CalibrationType::F32, CompuMethod::Identical);
        assert_eq!(physical_to_raw_string(3.14, &param).unwrap(), "3.14");
    }

    #[test]
    fn test_physical_to_raw_u8_identical() {
        let param = make_param("test", CalibrationType::U8, CompuMethod::Identical);
        assert_eq!(physical_to_raw_string(42.0, &param).unwrap(), "42");
    }

    #[test]
    fn test_physical_to_raw_u8_linear() {
        let param = make_param(
            "test",
            CalibrationType::U8,
            CompuMethod::Linear {
                factor: 0.5,
                offset: 10.0,
            },
        );
        // physical = factor * raw + offset → raw = (physical - offset) / factor
        // physical=60 → raw = (60-10)/0.5 = 100
        assert_eq!(physical_to_raw_string(60.0, &param).unwrap(), "100");
    }

    #[test]
    fn test_physical_to_raw_bool() {
        let param = make_param("test", CalibrationType::Bool, CompuMethod::Identical);
        assert_eq!(physical_to_raw_string(1.0, &param).unwrap(), "true");
        assert_eq!(physical_to_raw_string(0.0, &param).unwrap(), "false");
    }

    #[test]
    fn test_dcm_scalar_to_raw() {
        let param = make_param("test", CalibrationType::F64, CompuMethod::Identical);
        let val = DcmValue::WERT(vec![3.14]);
        assert_eq!(dcm_value_to_raw_string(&val, &param).unwrap(), "3.14");
    }

    #[test]
    fn test_dcm_text_enum() {
        let param = make_param("test", CalibrationType::U8, CompuMethod::Identical);
        let val = DcmValue::TEXT(vec!["OFF".to_string()]);
        assert_eq!(dcm_value_to_raw_string(&val, &param).unwrap(), "OFF");
    }

    #[test]
    fn test_dcm_array_to_raw() {
        let param = make_param("test", CalibrationType::F32Array(3), CompuMethod::Identical);
        let val = DcmValue::WERT(vec![1.0, 2.0, 3.0]);
        assert_eq!(dcm_array_to_raw_string(&val, &param).unwrap(), "1, 2, 3");
    }

    fn make_matrix_param(name: &str, dt: CalibrationType, order: StorageOrder) -> CalibrationParam {
        let mut p = CalibrationParam::new(name, 0x1000, 0, dt);
        p.compu_method = CompuMethod::Identical;
        p.storage_order = order;
        p
    }

    /// Build a mock DCM GRUPPENKENNFELD with known [Y-rows][X-cols] data.
    ///
    /// Example: 3 X breakpoints, 2 Y breakpoints → dim = (3, 2)
    ///   Y=0 row: v(x=0,y=0)=1  v(x=1,y=0)=2  v(x=2,y=0)=3
    ///   Y=1 row: v(x=0,y=1)=4  v(x=1,y=1)=5  v(x=2,y=1)=6
    fn make_dcm_map(x_size: usize, y_size: usize) -> dcm_utils::blocks::GRUPPENKENNFELD {
        let mut value = Vec::new();
        let mut flat_vals = Vec::new();
        for y in 0..y_size {
            let row: Vec<f64> = (0..x_size)
                .map(|x| (y * x_size + x + 1) as f64) // 1-based sequential
                .collect();
            flat_vals.extend(row.iter().copied());
            value.push(DcmValue::WERT(row));
        }
        dcm_utils::blocks::GRUPPENKENNFELD {
            name: "test_map".to_string(),
            attrs: Vec::new(),
            value,
            value_flat: DcmValue::WERT(flat_vals),
            x_axis_name: String::new(),
            y_axis_name: String::new(),
            dim: (x_size, y_size),
            x_axis: Vec::new(),
            y_axis: Vec::new(),
        }
    }

    /// Simulate the full DCM→ECU pipeline for F32 matrices:
    /// dcm_map_to_raw_string → parse_value → matrix_value_to_bytes
    fn run_dcm_f32_pipeline(x_size: usize, y_size: usize, order: StorageOrder) -> Vec<f32> {
        let map = make_dcm_map(x_size, y_size);
        // A2L convention: rows = X-axis, cols = Y-axis
        let param = make_matrix_param("test", CalibrationType::F32Matrix(x_size, y_size), order);
        let cal = CalibrationManager::new();
        let raw_str = dcm_map_to_raw_string(&map, &param).unwrap();
        let value = cal.parse_value(&param, &raw_str).unwrap();
        let bytes = matrix_value_to_bytes(&value, x_size, y_size, order).unwrap();
        // Convert bytes back to f32 for readable assertions
        bytes
            .chunks(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect()
    }

    #[test]
    fn test_dcm_matrix_layout_rowdir_3x2() {
        // 3 X breakpoints, 2 Y breakpoints, ROW_DIR storage
        // DCM data (Y-rows, X-cols):
        //   Y=0: 1 2 3
        //   Y=1: 4 5 6
        //
        // A2L reader maps rows=X, cols=Y and parse_storage_order swaps
        // COLUMN_DIR↔RowDir, ROW_DIR↔ColumnDir. On the [Xrows][Ycols] matrix
        // from parse_value, RowDir stores rows sequentially → X-column order:
        //   v(0,0), v(0,1), v(1,0), v(1,1), v(2,0), v(2,1)
        //   = 1, 4, 2, 5, 3, 6
        let flat = run_dcm_f32_pipeline(3, 2, StorageOrder::RowDir);
        assert_eq!(flat, vec![1.0, 4.0, 2.0, 5.0, 3.0, 6.0]);
    }

    #[test]
    fn test_dcm_matrix_layout_columndir_3x2() {
        // 3 X breakpoints, 2 Y breakpoints, COLUMN_DIR storage
        // DCM data (Y-rows, X-cols):
        //   Y=0: 1 2 3
        //   Y=1: 4 5 6
        //
        // A2L reader maps rows=X, cols=Y and parse_storage_order swaps.
        // On the [Xrows][Ycols] matrix from parse_value, ColumnDir stores
        // columns sequentially → Y-row order:
        //   v(0,0), v(1,0), v(2,0), v(0,1), v(1,1), v(2,1)
        //   = 1, 2, 3, 4, 5, 6
        let flat = run_dcm_f32_pipeline(3, 2, StorageOrder::ColumnDir);
        assert_eq!(flat, vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
    }

    #[test]
    fn test_dcm_matrix_layout_rowdir_4x3() {
        // 4 X breakpoints, 3 Y breakpoints, ROW_DIR
        // DCM data (Y-rows, X-cols):
        //   Y=0:  1  2  3  4
        //   Y=1:  5  6  7  8
        //   Y=2:  9 10 11 12
        //
        // On [Xrows][Ycols] matrix, RowDir stores rows sequentially → X-columns:
        //   v(0,*): 1, 5, 9
        //   v(1,*): 2, 6, 10
        //   v(2,*): 3, 7, 11
        //   v(3,*): 4, 8, 12
        let flat = run_dcm_f32_pipeline(4, 3, StorageOrder::RowDir);
        assert_eq!(
            flat,
            vec![1.0, 5.0, 9.0, 2.0, 6.0, 10.0, 3.0, 7.0, 11.0, 4.0, 8.0, 12.0]
        );
    }

    #[test]
    fn test_dcm_matrix_layout_columndir_4x3() {
        // 4 X breakpoints, 3 Y breakpoints, COLUMN_DIR
        // DCM data (Y-rows, X-cols):
        //   Y=0:  1  2  3  4
        //   Y=1:  5  6  7  8
        //   Y=2:  9 10 11 12
        //
        // On [Xrows][Ycols] matrix, ColumnDir stores columns sequentially → Y-rows:
        //   1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12
        let flat = run_dcm_f32_pipeline(4, 3, StorageOrder::ColumnDir);
        assert_eq!(
            flat,
            vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0]
        );
    }

    #[test]
    fn test_dcm_matrix_layout_square_2x2_rowdir() {
        // Square matrix: 2x2, ROW_DIR
        // DCM: Y=0: [1, 2], Y=1: [3, 4]
        // On [Xrows][Ycols] matrix, RowDir stores rows → X-columns: [1, 3, 2, 4]
        let flat = run_dcm_f32_pipeline(2, 2, StorageOrder::RowDir);
        assert_eq!(flat, vec![1.0, 3.0, 2.0, 4.0]);
    }

    #[test]
    fn test_dcm_matrix_layout_square_2x2_columndir() {
        // Square matrix: 2x2, COLUMN_DIR
        // DCM: Y=0: [1, 2], Y=1: [3, 4]
        // On [Xrows][Ycols] matrix, ColumnDir stores cols → Y-rows: [1, 2, 3, 4]
        let flat = run_dcm_f32_pipeline(2, 2, StorageOrder::ColumnDir);
        assert_eq!(flat, vec![1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn test_matrix_value_to_bytes_u8() {
        let mat = vec![vec![1u8, 2], vec![3, 4], vec![5, 6]];
        // RowDir on [Xrows][Ycols] matrix: rows sequentially → [1, 2, 3, 4, 5, 6]
        let bytes =
            matrix_value_to_bytes(&CalibrationValue::U8Matrix(mat), 3, 2, StorageOrder::RowDir)
                .unwrap();
        assert_eq!(bytes, vec![1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn test_matrix_dimensions() {
        assert_eq!(
            matrix_dimensions(&CalibrationType::F32Matrix(3, 9)),
            Some((3, 9))
        );
        assert_eq!(
            matrix_dimensions(&CalibrationType::U8Matrix(2, 4)),
            Some((2, 4))
        );
        assert_eq!(matrix_dimensions(&CalibrationType::F32), None);
        assert_eq!(matrix_dimensions(&CalibrationType::F32Array(5)), None);
    }
}
