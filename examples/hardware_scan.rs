//! Hardware scan smoke test.
//!
//! Prints the JSON report produced by `xcp_client::hardware::scan()` — the same
//! payload the desktop app's Devices page receives from the `scan_hardware`
//! command: host network adapters plus PCAN/ES582 CAN channels.
//!
//! Usage:
//!   cargo run --example hardware_scan
//!
//! Environment overrides (all optional, see the CAN drivers):
//!   PCAN_BASIC_DLL   path to `PCANBasic.dll`
//!   BOA_DLL_DIR      directory holding the ETAS BOA V2 framework DLLs
//!   RUST_LOG         tracing filter, e.g. `info` or `debug`

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let filter = std::env::var("RUST_LOG").unwrap_or_else(|_| "warn".to_string());
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();

    let report = xcp_client::hardware::scan();
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
