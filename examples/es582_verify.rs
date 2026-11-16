//! ETAS ES582 (BOA / OCI) driver smoke test.
//!
//! Verifies `src/transport/can/es582.rs` against real hardware: BOA framework
//! load, CSI search, channel enumeration, controller bind, queue setup, frame
//! reception and (optionally) transmission.
//!
//! Usage:
//!   cargo run --example es582_verify
//!
//! Environment overrides (all optional):
//!   ES582_VERIFY_CHANNEL  channel URI or controller index, default: first found
//!   ES582_VERIFY_BITRATE  nominal bit rate in bit/s, default `500000`
//!   ES582_VERIFY_FD_BITRATE  data phase bit rate; presence enables CAN-FD
//!   ES582_VERIFY_TX       `1` sends one test frame (default: listen only)
//!   ES582_VERIFY_TX_ID    test frame CAN id (hex `0x..` or decimal), default `0x123`
//!   ES582_VERIFY_READ_SECS seconds to listen for frames, default `5`
//!   RUST_LOG              tracing filter, e.g. `info` or `debug`

use std::time::{Duration, Instant};

use xcp_client::transport::{
    es582::Es582Driver, find_driver, list_can_devices, CanDevice, CanDeviceConfig, CanDeviceError,
    CanFrame,
};

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn env_u32(key: &str) -> Option<u32> {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .map(|v| {
            v.parse::<u32>()
                .unwrap_or_else(|_| panic!("{key} must be a number"))
        })
}

fn parse_id(s: &str) -> u32 {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16).unwrap_or_else(|_| panic!("bad id: {s}"))
    } else {
        s.parse::<u32>().unwrap_or_else(|_| panic!("bad id: {s}"))
    }
}

fn hex(data: &[u8]) -> String {
    data.iter()
        .map(|b| format!("{:02X}", b))
        .collect::<Vec<_>>()
        .join(" ")
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    let bitrate = env_u32("ES582_VERIFY_BITRATE").unwrap_or(500_000);
    let fd_bitrate = env_u32("ES582_VERIFY_FD_BITRATE");
    let read_secs = env_u32("ES582_VERIFY_READ_SECS").unwrap_or(5) as u64;
    let send = env_or("ES582_VERIFY_TX", "0") == "1";
    let tx_id = parse_id(&env_or("ES582_VERIFY_TX_ID", "0x123"));
    let channel = std::env::var("ES582_VERIFY_CHANNEL")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());

    println!("=== ES582 (BOA/OCI) driver verification ===");
    println!(
        "bitrate={} bit/s fd={:?} read_secs={} tx={} channel={:?}",
        bitrate, fd_bitrate, read_secs, send, channel
    );
    println!();

    // -----------------------------------------------------------------------
    // 1. BOA framework load + CSI search + channel enumeration
    // -----------------------------------------------------------------------
    println!("[1/5] enumerate OCI CAN channels");
    let driver = Es582Driver::new();
    let channels = driver.probe_channels();
    if channels.is_empty() {
        println!("    FAIL: no OCI CAN channel found");
        println!();
        println!("    Checks:");
        println!("      - is EBI-IP / INCA (BOA V2) installed?");
        println!("      - is the ES582 plugged in and its OCD (ES582_1_OCD_CAN.dll) present?");
        println!("      - try RUST_LOG=debug and BOA_DLL_DIR / BOA_OCD_DIR overrides");
        std::process::exit(1);
    }
    for channel in &channels {
        let info = &channel.info;
        let details = &channel.details;
        println!(
            "      - {} | dev={} ctrl={} | uri={} | available={} fd={}",
            info.name,
            info.device_id,
            info.controller,
            info.channel,
            info.available,
            info.fd_capable
        );
        println!(
            "        api=OCI_CAN v{} state={}(0x{:X}) tick={} Hz",
            details.api_version,
            details.state_label(),
            details.state_code,
            details.timer_tick_hz
        );
        println!(
            "        samplesPerBit=0x{:X} media=0x{:X} busEvents=0x{:X} msgFlags=0x{:X}",
            details.samples_per_bit,
            details.physical_media,
            details.bus_events,
            details.message_flags
        );
        if details.fd_capable {
            println!(
                "        CAN-FD: maxDataSize={} maxRate={} qualifiedRate={}",
                details.fd_max_data_size,
                details.fd_max_data_rate,
                details.fd_max_qualified_data_rate
            );
        }
    }
    println!();

    // -----------------------------------------------------------------------
    // 2. registry integration (what the CLI / UI use)
    // -----------------------------------------------------------------------
    println!("[2/5] registry view (find_driver + list_can_devices)");
    match find_driver("es582") {
        Some(registry_driver) => println!(
            "    driver registered: {} ({})",
            registry_driver.name(),
            registry_driver.description()
        ),
        None => println!("    FAIL: es582 driver is not registered"),
    }
    for device in list_can_devices() {
        println!(
            "    - [{}] {} | dev={} ctrl={} ch={} avail={} fd={} -> selection '{}'",
            device.driver,
            device.name,
            device.device_id,
            device.controller,
            device.channel,
            device.available,
            device.fd_capable,
            device.selection_value()
        );
    }
    println!();

    // -----------------------------------------------------------------------
    // 3. open the channel
    // -----------------------------------------------------------------------
    let channel_value = channel.unwrap_or_else(|| {
        channels
            .iter()
            .find(|c| c.info.available)
            .map(|c| c.info.channel.clone())
            .unwrap_or_else(|| channels[0].info.channel.clone())
    });
    println!(
        "[3/5] open {} @ {} bit/s{}",
        channel_value,
        bitrate,
        fd_bitrate
            .map(|d| format!(" / {} bit/s data phase (CAN-FD)", d))
            .unwrap_or_default()
    );

    let driver = match find_driver("es582") {
        Some(driver) => driver,
        None => {
            println!("    FAIL: es582 driver is not registered");
            std::process::exit(1);
        }
    };
    let config = CanDeviceConfig {
        channel: channel_value.clone(),
        device_id: None,
        controller: None,
        baudrate: Some(bitrate),
        data_baudrate: fd_bitrate,
        accept_ids: Vec::new(),
    };
    // The concrete device type exposes the receive-path diagnostics
    // (`rx_mode`, `dropped_frames`) that the boxed trait object hides.
    let mut device = match Es582Driver::new().open_device(&config) {
        Ok(device) => device,
        Err(error) => {
            println!("    FAIL: {}", error);
            std::process::exit(1);
        }
    };
    let _ = &driver;
    let info = device.info();
    println!(
        "    OK: {} device {} channel {} (ctrl {}, fd={}, bitrate={}, rx={})",
        info.device_type,
        info.device_id,
        info.channel,
        info.controller,
        device.is_fd(),
        device.bitrate_description(),
        device.rx_mode()
    );
    println!();

    // -----------------------------------------------------------------------
    // 4. receive frames
    // -----------------------------------------------------------------------
    println!("[4/5] receive frames for {}s", read_secs);
    let deadline = Instant::now() + Duration::from_secs(read_secs);
    let mut received = 0u64;
    let mut data_frames = 0u64;
    let mut status_frames = 0u64;
    let mut per_id: std::collections::HashMap<u32, u64> = std::collections::HashMap::new();
    let mut first_ts: Option<u64> = None;
    let mut last_ts: Option<u64> = None;

    while Instant::now() < deadline {
        match device.recv_frame(Some(Duration::from_millis(500))) {
            Ok(Some(frame)) => {
                received += 1;
                if let Some(ts) = frame.timestamp_us {
                    first_ts.get_or_insert(ts);
                    last_ts = Some(ts);
                }
                if frame.data.is_empty() {
                    status_frames += 1;
                    println!(
                        "    [{:>5}] STATUS id=0x{:08X} ext={}",
                        received, frame.id, frame.extended
                    );
                } else {
                    data_frames += 1;
                    *per_id.entry(frame.id).or_insert(0) += 1;
                    println!(
                        "    [{:>5}] DATA   id=0x{:08X} ext={} len={:<2} ts={:?} [ {} ]",
                        received,
                        frame.id,
                        frame.extended,
                        frame.data.len(),
                        frame.timestamp_us,
                        hex(&frame.data)
                    );
                }
            }
            Ok(None) => {}
            Err(error) => {
                println!("    FAIL: {}", error);
                break;
            }
        }
    }

    if data_frames > 0 {
        println!(
            "    OK: {} frame(s) ({} data, {} status) on {} identifier(s)",
            received,
            data_frames,
            status_frames,
            per_id.len()
        );
        let mut ids: Vec<(u32, u64)> = per_id.into_iter().collect();
        ids.sort_by_key(|a| std::cmp::Reverse(a.1));
        for (id, count) in ids.iter().take(10) {
            println!("      0x{:08X}: {} frame(s)", id, count);
        }
        if let (Some(first), Some(last)) = (first_ts, last_ts) {
            if last > first {
                let secs = (last - first) as f64 / 1_000_000.0;
                println!(
                    "      hardware timestamps span {:.3}s ({:.1} frames/s)",
                    secs,
                    data_frames as f64 / secs
                );
            }
        }
    } else if status_frames > 0 {
        println!(
            "    WARN: only {} status/error frame(s), no data frames",
            status_frames
        );
    } else {
        println!("    WARN: no frames received (is anything transmitting on the bus?)");
    }
    println!();

    // -----------------------------------------------------------------------
    // 5. transmit (opt-in) and close
    // -----------------------------------------------------------------------
    if send {
        println!("[5/5] send one test frame id=0x{:X}", tx_id);
        let payload = [0xDEu8, 0xAD, 0xBE, 0xEF, 0x01, 0x02, 0x03, 0x04];
        let frame = CanFrame::new(tx_id, payload.to_vec());
        match device.send_frame(&frame) {
            Ok(()) => println!(
                "    OK: frame queued (id=0x{:X} [ {} ])",
                tx_id,
                hex(&payload)
            ),
            Err(CanDeviceError::SendFailed(error)) => println!("    FAIL: {}", error),
            Err(error) => println!("    FAIL: {}", error),
        }
    } else {
        println!("[5/5] transmit skipped (set ES582_VERIFY_TX=1 to send a test frame)");
    }

    match device.close() {
        Ok(()) => println!(
            "    OK: channel closed (rx={}, dropped={})",
            device.rx_mode(),
            device.dropped_frames()
        ),
        Err(error) => println!("    FAIL: close: {}", error),
    }
    drop(device);
    println!();
    println!("=== done ===");
}
