//! Minimal XCP-on-CAN smoke test against a real ECU (default: `pms-a.a2l`).
//!
//! Exercises the smallest useful XCP sequence through the CAN transport:
//!
//! 1. load the A2L and resolve the transport from its `XCP_ON_CAN` block
//! 2. `CONNECT` (and print the negotiated parameters)
//! 3. `GET_ID` (ECU identification)
//! 4. `GET_CAL_PAGE` for every memory segment (XCP and ECU access)
//! 5. `SET_CAL_PAGE` → working page, re-read it, then switch back to reference
//! 6. `SET_MTA` + `UPLOAD` a few bytes from the calibration segment
//! 7. `DISCONNECT`
//!
//! Usage:
//!   cargo run --example es582_xcp_test
//!   cargo run --example es582_xcp_test -- path/to/ecu.a2l
//!
//! Environment overrides (all optional):
//!   XCP_TEST_A2L          A2L file, default `pms-a.a2l`
//!   XCP_TEST_DRIVER       CAN driver id, default `es582` (`pcan` also works)
//!   XCP_TEST_CHANNEL      CAN channel/URI, default `auto`
//!   XCP_TEST_BAUDRATE     nominal bit rate, default: from the A2L
//!   XCP_TEST_READ_BYTES   bytes to upload from the segment, default `16`
//!   XCP_TEST_TIMEOUT_MS   CTO response timeout, default `5000` (this demo ECU
//!                         answers in 0.3–1.5 s, well above the 1 s default)
//!   RUST_LOG              tracing filter, e.g. `info` or `debug`

use std::time::{Duration, Instant};

use xcp_client::protocol::Mta;
use xcp_client::transport::{
    resolve_transport_config, transport_display, TransportOverrides, TransportType,
};
use xcp_client::{A2lReader, PageMode, XcpClient};

/// Access modes for `GET_CAL_PAGE` / `SET_CAL_PAGE`.
const ACCESS_ECU: u8 = 0x01;
const ACCESS_XCP: u8 = 0x02;
const ACCESS_BOTH: u8 = 0x03;

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn hex(data: &[u8]) -> String {
    data.iter()
        .map(|b| format!("{:02X}", b))
        .collect::<Vec<_>>()
        .join(" ")
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_target(false)
        .init();

    let a2l_path = std::env::args()
        .nth(1)
        .or_else(|| std::env::var("XCP_TEST_A2L").ok())
        .unwrap_or_else(|| "pms-a.a2l".to_string());
    let driver = env_or("XCP_TEST_DRIVER", "es582");
    let channel = env_or("XCP_TEST_CHANNEL", "auto");
    let read_bytes: usize = env_or("XCP_TEST_READ_BYTES", "16").parse().unwrap_or(16);
    let baudrate: Option<u32> = std::env::var("XCP_TEST_BAUDRATE")
        .ok()
        .and_then(|v| v.parse().ok());
    let timeout_ms: u64 = env_or("XCP_TEST_TIMEOUT_MS", "5000")
        .parse()
        .unwrap_or(5000);

    println!("=== XCP-on-CAN smoke test ===");
    println!("A2L: {}", a2l_path);
    println!("CTO timeout: {} ms", timeout_ms);
    println!();

    // -----------------------------------------------------------------------
    // 1. A2L + transport resolution
    // -----------------------------------------------------------------------
    println!("[1/7] load A2L and resolve transport");
    let reader = match A2lReader::load(&a2l_path) {
        Ok(reader) => reader,
        Err(error) => {
            eprintln!("    FAIL: cannot load A2L: {}", error);
            std::process::exit(1);
        }
    };

    let segments = reader.get_memory_segments();
    let a2l_transports = reader.get_transport_configs();
    println!("    memory segments: {}", segments.len());
    for segment in &segments {
        match segment.xcp_info.as_ref() {
            Some(info) => {
                let pages: Vec<String> = info
                    .pages
                    .iter()
                    .map(|p| {
                        format!(
                            "{}:{}",
                            p.page_number,
                            if p.is_write_allowed() { "rw" } else { "ro" }
                        )
                    })
                    .collect();
                println!(
                    "      seg {} '{}' @0x{:08X} size={} ext={} pages=[{}]",
                    info.segment_number,
                    segment.name,
                    segment.address,
                    segment.size,
                    info.address_extension,
                    pages.join(" ")
                );
            }
            None => println!(
                "      '{}' @0x{:08X} size={} (no XCP SEGMENT info)",
                segment.name, segment.address, segment.size
            ),
        }
    }
    for config in &a2l_transports {
        println!("    A2L transport: {:?}", config);
    }

    let overrides = TransportOverrides {
        transport_type: Some(TransportType::Can),
        driver: Some(driver.clone()),
        channel: Some(channel.clone()),
        baudrate,
        ..Default::default()
    };
    let transport_config = match resolve_transport_config(Some(&overrides), &a2l_transports) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("    FAIL: cannot resolve transport: {}", error);
            std::process::exit(1);
        }
    };
    println!("    resolved: {}", transport_display(&transport_config));
    println!();

    // -----------------------------------------------------------------------
    // 2. CONNECT
    // -----------------------------------------------------------------------
    println!("[2/7] CONNECT");
    let mut client = XcpClient::new();
    client.set_command_timeout(Duration::from_millis(timeout_ms));
    let connect_start = Instant::now();
    let slave_id = match client
        .connect_slave("es582-ecu", transport_config.clone(), None)
        .await
    {
        Ok(id) => id,
        Err(error) => {
            eprintln!("    FAIL: {}", error);
            std::process::exit(1);
        }
    };
    println!(
        "    connect took {:.0} ms",
        connect_start.elapsed().as_secs_f64() * 1000.0
    );

    let slave_handle = client.get_slave(slave_id).expect("slave handle");
    {
        let mut slave = slave_handle.lock().await;
        if let Some(info) = slave.info.connect_info.as_ref() {
            println!(
                "    OK: protocol v{}, max_cto={}, max_dto={}, comm_mode=0x{:02X}",
                info.protocol_version, info.max_cto, info.max_dto, info.comm_mode_basic
            );
            println!(
                "    resources: cal_pgm={} daq={} stim={} pgm={}",
                info.resource.cal_pgm, info.resource.daq, info.resource.stim, info.resource.pgm
            );
        }
        // A2L data drives segment/page handling.
        if let Err(error) = slave.load_a2l(&a2l_path) {
            println!(
                "    WARN: A2L could not be attached to the slave: {}",
                error
            );
        }
    }
    println!();

    // -----------------------------------------------------------------------
    // 3. GET_ID
    // -----------------------------------------------------------------------
    println!("[3/7] GET_ID");
    for (id_type, label) in [(0x00u8, "ASCII text"), (0x01, "ASAM-MC2 filename")] {
        match slave_handle.lock().await.get_id(id_type).await {
            Ok(text) => println!("    type 0x{:02X} ({}): '{}'", id_type, label, text),
            Err(error) => println!("    type 0x{:02X} ({}): {}", id_type, label, error),
        }
    }
    println!();

    // -----------------------------------------------------------------------
    // 4. GET_CAL_PAGE
    // -----------------------------------------------------------------------
    println!("[4/7] GET_CAL_PAGE");
    let segment_numbers: Vec<u8> = segments
        .iter()
        .filter_map(|s| s.xcp_info.as_ref().map(|i| i.segment_number))
        .collect();
    if segment_numbers.is_empty() {
        println!("    (A2L declares no SEGMENT, probing segment 0)");
    }
    // The A2L may declare several MEMORY_SEGMENTs that share one logical XCP
    // segment number (e.g. calibration data + code flash); query each number
    // once.
    let mut probe_segments: Vec<u8> = if segment_numbers.is_empty() {
        vec![0]
    } else {
        segment_numbers.clone()
    };
    probe_segments.sort_unstable();
    probe_segments.dedup();

    let mut initial_pages: Vec<(u8, u8)> = Vec::new();
    for segment in &probe_segments {
        let xcp_page = slave_handle
            .lock()
            .await
            .get_cal_page(*segment, ACCESS_XCP)
            .await;
        let ecu_page = slave_handle
            .lock()
            .await
            .get_cal_page(*segment, ACCESS_ECU)
            .await;
        match (&xcp_page, &ecu_page) {
            (Ok(xcp), Ok(ecu)) => {
                println!(
                    "    segment {}: XCP page {} / ECU page {}",
                    segment, xcp, ecu
                );
                initial_pages.push((*segment, *xcp));
            }
            _ => println!(
                "    segment {}: XCP page {:?} / ECU page {:?}",
                segment, xcp_page, ecu_page
            ),
        }
    }
    println!();

    // -----------------------------------------------------------------------
    // 5. SET_CAL_PAGE
    // -----------------------------------------------------------------------
    println!("[5/7] SET_CAL_PAGE");
    for (segment, original) in initial_pages.clone() {
        // Prefer a write-allowed page (working page) declared by the A2L.
        let target = {
            let slave = slave_handle.lock().await;
            slave
                .find_page_for_mode(segment, PageMode::Working)
                .or_else(|| slave.find_page_for_mode(segment, PageMode::Reference))
        };
        let Some(target) = target else {
            println!(
                "    segment {}: no page declared in the A2L, skipping",
                segment
            );
            continue;
        };

        // Bind the guard to a variable (never to a `match` scrutinee): a
        // scrutinee temporary lives for the whole match, which would deadlock
        // the next `lock()`.
        let set_result = {
            let mut slave = slave_handle.lock().await;
            slave.set_cal_page(segment, target, ACCESS_BOTH).await
        };
        match set_result {
            Ok(()) => {
                let readback = {
                    let slave = slave_handle.lock().await;
                    slave.get_cal_page(segment, ACCESS_XCP).await
                };
                match readback {
                    Ok(page) if page == target => println!(
                        "    segment {}: switched to page {} (verified)",
                        segment, page
                    ),
                    Ok(page) => println!(
                        "    segment {}: switched to page {}, readback reports {}",
                        segment, target, page
                    ),
                    Err(error) => println!(
                        "    segment {}: switched to page {}, readback failed: {}",
                        segment, target, error
                    ),
                }
            }
            Err(error) => println!(
                "    segment {}: SET_CAL_PAGE({}) failed: {}",
                segment, target, error
            ),
        }

        // Restore the page we found at the beginning.
        if original != target {
            let restore = {
                let mut slave = slave_handle.lock().await;
                slave.set_cal_page(segment, original, ACCESS_BOTH).await
            };
            match restore {
                Ok(()) => println!("    segment {}: restored page {}", segment, original),
                Err(error) => println!(
                    "    segment {}: restoring page {} failed: {}",
                    segment, original, error
                ),
            }
        }
    }

    // High-level helpers built on the same commands.
    {
        let mut slave = slave_handle.lock().await;
        match slave.update_segment_page_state().await {
            Ok(states) => {
                let summary: Vec<String> = states
                    .iter()
                    .map(|s| format!("seg{}={}", s.segment, s.xcp_page))
                    .collect();
                println!(
                    "    update_segment_page_state: [{}] -> mode {:?}",
                    summary.join(" "),
                    slave.get_current_page_mode()
                );
            }
            Err(error) => println!("    update_segment_page_state failed: {}", error),
        }

        match slave.switch_all_pages(PageMode::Working).await {
            Ok(switched) => {
                println!("    switch_all_pages(Working): {:?}", switched);
                let after_working: Vec<(u8, u8)> = slave
                    .get_segment_pages()
                    .iter()
                    .map(|s| (s.segment, s.xcp_page))
                    .collect();
                println!("      pages now: {:?}", after_working);
                match slave.switch_all_pages(PageMode::Reference).await {
                    Ok(restored) => println!("    switch_all_pages(Reference): {:?}", restored),
                    Err(error) => println!("    switch_all_pages(Reference) failed: {}", error),
                }
            }
            Err(error) => println!("    switch_all_pages(Working) failed: {}", error),
        }
    }
    println!();

    // -----------------------------------------------------------------------
    // 6. memory access: SET_MTA + UPLOAD
    // -----------------------------------------------------------------------
    println!("[6/7] SET_MTA + UPLOAD ({} bytes)", read_bytes);
    let upload_target = segments.iter().find(|s| s.xcp_info.is_some()).map(|s| {
        let ext = s
            .xcp_info
            .as_ref()
            .map(|i| i.address_extension)
            .unwrap_or(0);
        (s.name.clone(), Mta::new(s.address, ext))
    });
    let upload_target = match upload_target {
        Some(target) => Some(target),
        None => {
            let slave = slave_handle.lock().await;
            slave
                .cal_manager
                .params()
                .first()
                .map(|p| (p.name.clone(), Mta::new(p.address, p.address_ext)))
        }
    };

    match upload_target {
        Some((label, mta)) => {
            println!(
                "    target '{}' @0x{:08X} (ext {})",
                label, mta.address, mta.extension
            );
            let slave = slave_handle.lock().await;
            match slave.set_mta(mta).await {
                Ok(()) => {
                    let mut offset = 0usize;
                    let mut all = Vec::new();
                    while offset < read_bytes {
                        let chunk =
                            (read_bytes - offset).min(slave.max_upload_size()).min(255) as u8;
                        if chunk == 0 {
                            break;
                        }
                        match slave.upload(chunk).await {
                            Ok(data) => {
                                println!("    +0x{:02X}: [ {} ]", offset, hex(&data));
                                offset += data.len();
                                all.extend_from_slice(&data);
                                if data.len() < chunk as usize {
                                    break;
                                }
                            }
                            Err(error) => {
                                println!("    UPLOAD failed at +0x{:02X}: {}", offset, error);
                                break;
                            }
                        }
                    }
                    println!("    read {} byte(s): [ {} ]", all.len(), hex(&all));
                }
                Err(error) => println!("    SET_MTA failed: {}", error),
            }
        }
        None => println!("    no address available in the A2L"),
    }
    println!();

    // -----------------------------------------------------------------------
    // 7. DISCONNECT
    // -----------------------------------------------------------------------
    println!("[7/7] DISCONNECT");
    // Give the last response a moment before tearing the transport down.
    tokio::time::sleep(Duration::from_millis(20)).await;
    let disconnect_start = Instant::now();
    match client.disconnect_slave(slave_id).await {
        Ok(()) => println!(
            "    OK: disconnected ({:.0} ms)",
            disconnect_start.elapsed().as_secs_f64() * 1000.0
        ),
        Err(error) => println!("    WARN: disconnect failed: {}", error),
    }
    println!();
    println!("=== done ===");
}
