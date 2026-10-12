//! PCAN driver smoke test.
//!
//! Verifies the basic functionality of `src/transport/can/pcan.rs` against real
//! hardware: DLL load, channel enumeration, open/init, receive, transmit and
//! close. Requires a PCAN-USB (pro) device with traffic on the channel.
//!
//! Usage:
//!   cargo run --example pcan_verify
//!
//! Environment overrides (all optional):
//!   PCAN_VERIFY_CHANNEL   channel name/handle, default `PCAN_USBBUS1`
//!   PCAN_VERIFY_BITRATE   nominal bit rate in bit/s, default `500000`
//!   PCAN_VERIFY_TX_ID     test frame CAN id (hex `0x..` or decimal), default `0x123`
//!   PCAN_VERIFY_READ_SECS seconds to listen for frames, default `5`
//!   RUST_LOG              tracing filter, e.g. `info` or `debug`

use std::time::{Duration, Instant};

use xcp_client::transport::{
    find_driver, list_can_devices, CanDeviceConfig, CanDeviceError, CanFrame,
};

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn parse_id(s: &str) -> u32 {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16).unwrap_or_else(|_| panic!("bad id: {s}"))
    } else {
        s.parse::<u32>().unwrap_or_else(|_| panic!("bad id: {s}"))
    }
}

struct Check {
    name: &'static str,
    ok: bool,
    detail: String,
}

impl Check {
    fn pass(name: &'static str, detail: impl Into<String>) -> Self {
        Self { name, ok: true, detail: detail.into() }
    }
    fn fail(name: &'static str, detail: impl Into<String>) -> Self {
        Self { name, ok: false, detail: detail.into() }
    }
}

fn hex(data: &[u8]) -> String {
    data.iter().map(|b| format!("{:02X}", b)).collect::<Vec<_>>().join(" ")
}

fn main() {
    // Show the driver's own tracing::info!/debug! output.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    let channel = env_or("PCAN_VERIFY_CHANNEL", "PCAN_USBBUS1");
    let bitrate: u32 = env_or("PCAN_VERIFY_BITRATE", "500000")
        .parse()
        .expect("PCAN_VERIFY_BITRATE must be a number");
    let tx_id = parse_id(&env_or("PCAN_VERIFY_TX_ID", "0x123"));
    let read_secs: u64 = env_or("PCAN_VERIFY_READ_SECS", "5")
        .parse()
        .expect("PCAN_VERIFY_READ_SECS must be a number");

    println!("=== PCAN driver verification ===");
    println!(
        "channel={} bitrate={} bit/s tx_id=0x{:X} read_secs={}",
        channel, bitrate, tx_id, read_secs
    );
    println!();

    let mut checks: Vec<Check> = Vec::new();

    // -----------------------------------------------------------------------
    // 1. list_channels  (exercises DLL load + CAN_GetValue attached channels)
    // -----------------------------------------------------------------------
    println!("[1/5] list_can_devices()");
    let devices = list_can_devices();
    if devices.is_empty() {
        checks.push(Check::fail(
            "list_channels",
            "no PCAN channels enumerated (DLL load or device detection failed)",
        ));
        println!("    FAIL: no channels found");
    } else {
        println!("    found {} channel(s):", devices.len());
        for d in &devices {
            println!(
                "      - {} | {} | dev={} ctrl={} ch={} avail={} fd={}",
                d.driver, d.name, d.device_id, d.controller, d.channel, d.available, d.fd_capable,
            );
        }
        checks.push(Check::pass(
            "list_channels",
            format!("{} channel(s) enumerated", devices.len()),
        ));
    }
    println!();

    // -----------------------------------------------------------------------
    // 2. open  (exercises resolve_handle + resolve_bitrate + CAN_Initialize)
    // -----------------------------------------------------------------------
    println!("[2/5] open channel {} @ {} bit/s", channel, bitrate);
    let driver = match find_driver("pcan") {
        Some(d) => d,
        None => {
            checks.push(Check::fail("open", "pcan driver not registered"));
            println!("    FAIL: pcan driver not found");
            print_summary(&checks);
            return;
        }
    };
    println!("    driver: {} ({})", driver.name(), driver.description());

    let config = CanDeviceConfig {
        channel: channel.clone(),
        device_id: None,
        controller: None,
        baudrate: Some(bitrate),
        data_baudrate: None,
    };

    let mut device = match driver.open(&config) {
        Ok(dev) => {
            let info = dev.info();
            println!(
                "    OK: opened {} device {} channel {} (ctrl {}, fd={}, bitrate={})",
                info.device_type, info.device_id, info.channel, info.controller, dev.is_fd(),
                dev.bitrate_description()
            );
            checks.push(Check::pass(
                "open",
                format!("{} {} {}", info.device_type, info.device_id, info.channel),
            ));
            dev
        }
        Err(e) => {
            checks.push(Check::fail("open", e.to_string()));
            println!("    FAIL: {e}");
            print_summary(&checks);
            return;
        }
    };
    println!();

    // -----------------------------------------------------------------------
    // 3. recv  (exercises CAN_Read polling + frame decoding)
    // -----------------------------------------------------------------------
    println!("[3/5] recv frames for {}s", read_secs);
    let deadline = Instant::now() + Duration::from_secs(read_secs);
    let mut received = 0u64;
    let mut data_frames = 0u64;
    let mut status_frames = 0u64;
    let mut first_data: Option<CanFrame> = None;
    while Instant::now() < deadline {
        match device.recv_frame(Some(Duration::from_millis(500))) {
            Ok(Some(frame)) => {
                received += 1;
                if frame.data.is_empty() {
                    status_frames += 1;
                    println!(
                        "    [{:>5}] STATUS/ERR id=0x{:08X} ext={}",
                        received, frame.id, frame.extended
                    );
                } else {
                    data_frames += 1;
                    if first_data.is_none() {
                        first_data = Some(frame.clone());
                    }
                    println!(
                        "    [{:>5}] DATA    id=0x{:08X} ext={} dlc={} [ {} ]",
                        received,
                        frame.id,
                        frame.extended,
                        frame.data.len(),
                        hex(&frame.data)
                    );
                }
            }
            Ok(None) => { /* timeout, keep waiting until deadline */ }
            Err(e) => {
                checks.push(Check::fail("recv", e.to_string()));
                println!("    FAIL: {e}");
                break;
            }
        }
    }
    if checks.iter().all(|c| c.name != "recv") {
        if data_frames > 0 {
            checks.push(Check::pass(
                "recv",
                format!("{} data frame(s), {} status frame(s)", data_frames, status_frames),
            ));
            println!(
                "    OK: received {} frame(s) ({} data, {} status)",
                received, data_frames, status_frames
            );
        } else if status_frames > 0 {
            checks.push(Check::fail(
                "recv",
                "only status/error frames, no data frames (bit rate mismatch or no traffic?)",
            ));
            println!("    WARN: no data frames, only status/error frames");
        } else {
            checks.push(Check::fail("recv", "no frames received at all"));
            println!("    FAIL: no frames received");
        }
    }
    println!();

    // -----------------------------------------------------------------------
    // 4. send  (exercises CAN_Write)
    // -----------------------------------------------------------------------
    println!("[4/5] send one test frame id=0x{:X}", tx_id);
    let payload = [0xDEu8, 0xAD, 0xBE, 0xEF, 0x01, 0x02, 0x03, 0x04];
    let frame = CanFrame::new(tx_id, payload.to_vec());
    match device.send_frame(&frame) {
        Ok(()) => {
            checks.push(Check::pass("send", format!("tx id=0x{:X} dlc=8", tx_id)));
            println!("    OK: frame written (id=0x{:X} [ {} ])", tx_id, hex(&payload));
        }
        Err(CanDeviceError::SendFailed(e)) => {
            checks.push(Check::fail("send", e));
            println!("    FAIL: send failed");
        }
        Err(e) => {
            checks.push(Check::fail("send", e.to_string()));
            println!("    FAIL: {e}");
        }
    }
    println!();

    // -----------------------------------------------------------------------
    // 5. close  (exercises CAN_Uninitialize + Drop)
    // -----------------------------------------------------------------------
    println!("[5/5] close channel");
    match device.close() {
        Ok(()) => {
            checks.push(Check::pass("close", "channel uninitialized"));
            println!("    OK: channel closed");
        }
        Err(e) => {
            checks.push(Check::fail("close", e.to_string()));
            println!("    FAIL: {e}");
        }
    }
    // Drop should be a no-op now (closed flag set).
    drop(device);
    println!();

    print_summary(&checks);

    // Exit code reflects overall result.
    let all_ok = checks.iter().all(|c| c.ok);
    std::process::exit(if all_ok { 0 } else { 1 });
}

fn print_summary(checks: &[Check]) {
    println!("=== summary ===");
    let mut pass = 0;
    let mut fail = 0;
    for c in checks {
        let tag = if c.ok { "PASS" } else { "FAIL" };
        println!("  [{tag}] {:<14} {}", c.name, c.detail);
        if c.ok { pass += 1 } else { fail += 1 };
    }
    println!();
    println!("{} passed, {} failed, {} total", pass, fail, pass + fail);
}
