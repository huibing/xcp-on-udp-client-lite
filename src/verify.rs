//! Non-interactive checksum verification CLI subcommand.
//!
//! Verifies ECU flash memory by comparing master-computed checksums (from HEX file)
//! against slave-reported checksums (via XCP BUILD_CHECKSUM command).
//!
//! Usage:
//!   cargo run -- verify --a2l abc.a2l --hex firmware.hex
//!   cargo run -- verify --a2l abc.a2l --hex firmware.hex --addr 192.168.1.100:5555

use xcp_client::checksum::{verify_segments, SegmentStatus, VerificationReport};
use xcp_client::hex::HexImage;
use xcp_client::transport::TransportConfig;
use xcp_client::{A2lReader, XcpClient};

/// Run the verify subcommand
pub async fn run_verify(
    a2l_path: &str,
    hex_path: &str,
    addr_override: Option<String>,
    max_block_size: Option<u32>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Load A2L file
    println!("Loading A2L file: {}", a2l_path);
    let reader = A2lReader::load(a2l_path).map_err(|e| format!("Failed to load A2L: {}", e))?;

    let segments = reader.get_memory_segments();
    if segments.is_empty() {
        eprintln!("Error: no memory segments found in A2L file");
        std::process::exit(1);
    }
    println!("  {} memory segment(s) found", segments.len());
    for seg in &segments {
        let algo = seg
            .xcp_info
            .as_ref()
            .and_then(|info| info.checksum.as_ref())
            .map(|c| c.name().to_string())
            .unwrap_or_else(|| "N/A".to_string());
        println!(
            "    {} @ 0x{:08X} ({} bytes) [{}]",
            seg.name, seg.address, seg.size, algo
        );
    }

    // Load HEX file
    println!("\nLoading HEX file: {}", hex_path);
    let hex_image =
        HexImage::from_file(hex_path).map_err(|e| format!("Failed to load HEX file: {}", e))?;
    if let Some((range_start, range_end)) = hex_image.address_range() {
        let byte_count = hex_image.byte_count_in_range(range_start, range_end - range_start);
        println!(
            "  {} bytes, address range 0x{:08X}..0x{:08X}",
            byte_count, range_start, range_end
        );
    } else {
        eprintln!("Error: HEX file contains no data");
        std::process::exit(1);
    }

    // Get slave address
    let slave_addr = if let Some(addr) = addr_override {
        addr
    } else {
        let addrs = reader.get_slave_addresses();
        addrs
            .into_iter()
            .next()
            .ok_or("No slave address found in A2L file. Use --addr to specify one.")?
    };

    // Connect to slave
    println!("\nConnecting to slave at {}...", slave_addr);
    let mut client = XcpClient::new();
    let transport_config = TransportConfig::Udp {
        addr: slave_addr.clone(),
    };
    let slave_id = client
        .connect_slave("verify-target", transport_config)
        .await
        .map_err(|e| format!("Failed to connect: {}", e))?;
    println!("  Connected (id: {})", slave_id);

    // Set memory segments on slave
    {
        let xcp_slave = client
            .get_slave(slave_id)
            .ok_or("Failed to get slave handle")?;
        let mut guard = xcp_slave.lock().await;
        guard.set_memory_segments(segments.clone(), Some(std::path::PathBuf::from(a2l_path)));
    }

    // Run verification
    println!("\nVerifying checksums...");
    let report = {
        let xcp_slave = client
            .get_slave(slave_id)
            .ok_or("Failed to get slave handle")?;
        let mut guard = xcp_slave.lock().await;
        verify_segments(&mut guard, &hex_image, &segments, max_block_size)
            .await
            .map_err(|e| format!("Verification error: {}", e))?
    };

    // Disconnect
    println!("\nDisconnecting...");
    if let Err(e) = client.disconnect_slave(slave_id).await {
        eprintln!("Warning: disconnect failed: {}", e);
    }

    // Print results
    print_report(&report);

    if !report.all_passed() {
        std::process::exit(1);
    }

    Ok(())
}

/// Print a formatted verification report to stdout
fn print_report(report: &VerificationReport) {
    println!("\n╔══════════════════════════════════════════════════════════════════╗");
    println!("║                   Checksum Verification Report                  ║");
    println!("╠══════════════════════════════════════════════════════════════════╣");

    for result in &report.results {
        let status_icon = match &result.status {
            SegmentStatus::Pass => "✓",
            SegmentStatus::Fail => "✗",
            SegmentStatus::Skipped(_) => "⊘",
            SegmentStatus::Error(_) => "!",
        };

        let page_info = result
            .page
            .map(|p| format!(" [page {}]", p))
            .unwrap_or_default();

        println!("║ {} {}{}", status_icon, result.name, page_info);
        println!(
            "║   Address: 0x{:08X}  Size: {} bytes  Algorithm: {}",
            result.address, result.size, result.algorithm
        );

        match &result.status {
            SegmentStatus::Pass => {
                println!(
                    "║   Master: 0x{:08X}  Slave: 0x{:08X}  → PASS",
                    result.master_checksum, result.slave_checksum
                );
            }
            SegmentStatus::Fail => {
                println!(
                    "║   Master: 0x{:08X}  Slave: 0x{:08X}  → FAIL ← MISMATCH",
                    result.master_checksum, result.slave_checksum
                );
            }
            SegmentStatus::Skipped(reason) => {
                println!("║   SKIPPED: {}", reason);
            }
            SegmentStatus::Error(msg) => {
                println!("║   ERROR: {}", msg);
            }
        }

        // Print chunk details if present
        if let Some(chunks) = &result.chunks {
            for chunk in chunks {
                let chunk_icon = if chunk.passed { "·" } else { "✗" };
                println!(
                    "║     {} Chunk +0x{:08X} ({} bytes): master=0x{:08X} slave=0x{:08X}{}",
                    chunk_icon,
                    chunk.offset,
                    chunk.size,
                    chunk.master_checksum,
                    chunk.slave_checksum,
                    if !chunk.passed { " ← MISMATCH" } else { "" }
                );
            }
        }

        println!("║");
    }

    println!("╠══════════════════════════════════════════════════════════════════╣");
    println!("║ {}", report.summary());
    if report.all_passed() {
        println!("║ Result: ALL PASSED ✓");
    } else {
        println!("║ Result: VERIFICATION FAILED ✗");
    }
    println!("╚══════════════════════════════════════════════════════════════════╝");
}
