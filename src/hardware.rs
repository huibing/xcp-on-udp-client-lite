//! Host hardware discovery for the desktop app's "scan hardware" action.
//!
//! A scan reports two groups of hardware:
//!
//! - **Network adapters** — every NIC known to the operating system with its
//!   addresses, link state and speed ([`scan_network_interfaces`]).
//! - **CAN interfaces** — every channel exposed by the compiled-in CAN drivers.
//!   Only PCAN (PEAK-System PCAN-Basic) and ES582 (ETAS BOA V2/OCI) are built
//!   in today, see [`crate::transport::can::drivers`].
//!
//! [`scan`] combines both into a single [`HardwareScanReport`]. Driver runtime
//! problems (missing `PCANBasic.dll`, missing BOA framework) are reported per
//! driver so the UI can distinguish "not installed" from "nothing plugged in".

use std::time::Instant;

use serde::Serialize;

use crate::transport::{drivers, invalidate_hardware_cache, list_can_devices, CanDeviceInfo};

/// One network adapter of the host PC.
#[derive(Debug, Clone, Serialize)]
pub struct NetworkInterface {
    /// Adapter name reported by the OS (Windows: friendly name, e.g. `Wi-Fi`).
    pub name: String,
    /// Hardware/driver description.
    pub description: String,
    /// OS-level adapter identifier (Windows: `AdapterName`, e.g. the NPF GUID path).
    pub id: String,
    /// `ethernet`, `wifi`, `loopback`, `tunnel`, `ppp`, `wwan` or `other`.
    pub kind: String,
    /// Raw OS interface type (Windows `IfType`, Linux `ARPHRD_*`, 0 when unknown).
    pub if_type: u32,
    /// `up`, `down`, `testing`, `dormant`, `not_present`, `lower_layer_down` or `unknown`.
    pub status: String,
    /// True when the link is up.
    pub up: bool,
    /// MAC address formatted `aa:bb:cc:dd:ee:ff` when the OS reports one.
    pub mac: Option<String>,
    /// IPv4 addresses assigned to the adapter.
    pub ipv4: Vec<String>,
    /// IPv6 addresses assigned to the adapter.
    pub ipv6: Vec<String>,
    /// Link speed in Mbit/s when known.
    pub speed_mbps: Option<u64>,
    /// MTU in bytes when known.
    pub mtu: Option<u32>,
}

impl NetworkInterface {
    /// Display label: `name` plus the primary IPv4 address when available.
    pub fn display_label(&self) -> String {
        match self.ipv4.first() {
            Some(ip) => format!("{} ({})", self.name, ip),
            None => self.name.clone(),
        }
    }
}

/// Status of one compiled-in CAN driver.
#[derive(Debug, Clone, Serialize)]
pub struct CanDriverStatus {
    /// Driver id, e.g. `"pcan"`.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// True when the driver runtime (vendor DLL / framework) could be loaded.
    pub available: bool,
    /// Load error when `available` is false.
    pub error: Option<String>,
}

/// One CAN channel found by the drivers.
#[derive(Debug, Clone, Serialize)]
pub struct CanDeviceSummary {
    pub driver: String,
    pub device_id: String,
    pub device_type: String,
    pub controller: u8,
    pub channel: String,
    pub name: String,
    /// Ready-to-display label (device + channel).
    pub label: String,
    /// Whether the channel is free to open.
    pub available: bool,
    pub fd_capable: bool,
    /// Selection token used by the UI (`driver:device_id:controller`).
    pub value: String,
}

impl From<&CanDeviceInfo> for CanDeviceSummary {
    fn from(info: &CanDeviceInfo) -> Self {
        Self {
            driver: info.driver.clone(),
            device_id: info.device_id.clone(),
            device_type: info.device_type.clone(),
            controller: info.controller,
            channel: info.channel.clone(),
            name: info.name.clone(),
            label: info.display_label(),
            available: info.available,
            fd_capable: info.fd_capable,
            value: info.selection_value(),
        }
    }
}

/// Result of one hardware scan.
#[derive(Debug, Clone, Serialize)]
pub struct HardwareScanReport {
    /// Local timestamp of the scan (RFC 3339).
    pub scanned_at: String,
    /// Wall-clock scan duration.
    pub duration_ms: u64,
    /// Network adapters found on the host.
    pub network: Vec<NetworkInterface>,
    /// Status of every compiled-in CAN driver.
    pub can_drivers: Vec<CanDriverStatus>,
    /// CAN channels found by the drivers.
    pub can_devices: Vec<CanDeviceSummary>,
    /// Problems worth surfacing in the UI.
    pub warnings: Vec<String>,
}

impl HardwareScanReport {
    /// Number of network adapters found.
    pub fn network_count(&self) -> usize {
        self.network.len()
    }

    /// Number of CAN channels found.
    pub fn can_count(&self) -> usize {
        self.can_devices.len()
    }
}

/// Scan the host: network adapters + CAN interfaces (PCAN, ES582).
///
/// Blocking — callers on async runtimes should use `spawn_blocking` (driver
/// enumeration loads vendor DLLs and can take a couple of seconds).
pub fn scan() -> HardwareScanReport {
    let started = Instant::now();
    let mut warnings = Vec::new();

    // A manual scan must reflect what is attached right now, not a cached list.
    invalidate_hardware_cache();

    let mut can_drivers = Vec::with_capacity(2);
    for driver in drivers() {
        let status = driver.runtime_status();
        let (available, error) = match status {
            Ok(()) => (true, None),
            Err(error) => (false, Some(error)),
        };
        if let Some(error) = &error {
            warnings.push(format!("{} runtime unavailable: {}", driver.name(), error));
        }
        can_drivers.push(CanDriverStatus {
            name: driver.name().to_string(),
            description: driver.description().to_string(),
            available,
            error,
        });
    }

    let can_devices: Vec<CanDeviceSummary> = list_can_devices()
        .iter()
        .map(CanDeviceSummary::from)
        .collect();

    if can_devices.is_empty() && can_drivers.iter().all(|driver| driver.available) {
        warnings.push("CAN drivers loaded but no PCAN/ES582 hardware was found".to_string());
    }

    let network = scan_network_interfaces();
    if network.is_empty() {
        warnings.push("The operating system reported no network adapters".to_string());
    }

    HardwareScanReport {
        scanned_at: chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
        duration_ms: started.elapsed().as_millis() as u64,
        network,
        can_drivers,
        can_devices,
        warnings,
    }
}

/// Sort adapters into a stable, human-friendly order: up links first, then
/// virtual/loopback adapters last, then by name.
fn sort_interfaces(interfaces: &mut [NetworkInterface]) {
    interfaces.sort_by(|a, b| {
        b.up.cmp(&a.up)
            .then_with(|| a.kind.eq("loopback").cmp(&b.kind.eq("loopback")))
            .then_with(|| a.name.cmp(&b.name))
    });
}

/// Map an OS interface type to a coarse adapter kind.
#[allow(dead_code)]
fn interface_kind(if_type: u32) -> &'static str {
    match if_type {
        // Windows IF_TYPE_* / Linux ARPHRD_*
        6 | 62 | 117 => "ethernet",
        71 => "wifi",
        772 | 24 => "loopback",
        131 | 23 => "tunnel",
        243 => "wwan",
        _ => "other",
    }
}

/// Map Windows `IF_OPER_STATUS` to a short status string.
#[cfg(windows)]
fn interface_status(oper_status: i32) -> &'static str {
    match oper_status {
        1 => "up",
        2 => "down",
        3 => "testing",
        5 => "dormant",
        6 => "not_present",
        7 => "lower_layer_down",
        _ => "unknown",
    }
}

// ---------------------------------------------------------------------------
// Windows: IP Helper API (`GetAdaptersAddresses`)
// ---------------------------------------------------------------------------

#[cfg(windows)]
pub fn scan_network_interfaces() -> Vec<NetworkInterface> {
    use windows_sys::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, NO_ERROR};
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GetAdaptersAddresses, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER,
        GAA_FLAG_SKIP_MULTICAST, IP_ADAPTER_ADDRESSES_LH,
    };
    use windows_sys::Win32::Networking::WinSock::AF_UNSPEC;

    const MAX_BUFFER_BYTES: usize = 4 * 1024 * 1024;

    let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;

    // The API writes an `IP_ADAPTER_ADDRESSES_LH` array into the buffer, so it
    // has to be pointer-aligned: back it with `u64` words instead of bytes.
    let mut capacity_bytes: usize = 16 * 1024;
    let mut buffer: Vec<u64> = Vec::new();

    let head: *const IP_ADAPTER_ADDRESSES_LH = loop {
        let words = capacity_bytes.div_ceil(8);
        buffer.clear();
        buffer.resize(words, 0);

        let mut size = (words * 8) as u32;
        let code = unsafe {
            GetAdaptersAddresses(
                AF_UNSPEC as u32,
                flags,
                std::ptr::null(),
                buffer.as_mut_ptr().cast(),
                &mut size,
            )
        };

        if code == NO_ERROR {
            break buffer.as_ptr().cast();
        }
        if code != ERROR_BUFFER_OVERFLOW {
            return Vec::new();
        }
        capacity_bytes = (size as usize).max(capacity_bytes + 1024);
        if capacity_bytes > MAX_BUFFER_BYTES {
            return Vec::new();
        }
    };

    let mut interfaces = unsafe { collect_adapters(head) };

    sort_interfaces(&mut interfaces);
    interfaces
}

/// Walk the adapter linked list returned by `GetAdaptersAddresses`.
#[cfg(windows)]
unsafe fn collect_adapters(
    head: *const windows_sys::Win32::NetworkManagement::IpHelper::IP_ADAPTER_ADDRESSES_LH,
) -> Vec<NetworkInterface> {
    use windows_sys::Win32::Networking::WinSock::{AF_INET, AF_INET6, SOCKADDR_IN, SOCKADDR_IN6};

    let mut interfaces = Vec::new();
    let mut adapter = head;

    while !adapter.is_null() {
        let entry = &*adapter;

        let name = wide_to_string(entry.FriendlyName);
        let description = wide_to_string(entry.Description);
        let id = ansi_to_string(entry.AdapterName);

        let mut mac = None;
        if entry.PhysicalAddressLength > 0 {
            let length = (entry.PhysicalAddressLength as usize).min(entry.PhysicalAddress.len());
            if length > 0 {
                let octets: Vec<String> = entry.PhysicalAddress[..length]
                    .iter()
                    .map(|byte| format!("{:02x}", byte))
                    .collect();
                mac = Some(octets.join(":"));
            }
        }

        let mut ipv4 = Vec::new();
        let mut ipv6 = Vec::new();
        let mut unicast = entry.FirstUnicastAddress;
        while !unicast.is_null() {
            let address = &*unicast;
            let sockaddr = address.Address.lpSockaddr;
            if !sockaddr.is_null() {
                match (*sockaddr).sa_family {
                    AF_INET => {
                        let sin = &*(sockaddr as *const SOCKADDR_IN);
                        // `sin_addr.S_un.S_addr` is in network byte order, i.e.
                        // the in-memory byte order already is the octet order.
                        let octets = sin.sin_addr.S_un.S_addr.to_ne_bytes();
                        ipv4.push(std::net::Ipv4Addr::from(octets).to_string());
                    }
                    AF_INET6 => {
                        let sin6 = &*(sockaddr as *const SOCKADDR_IN6);
                        let octets = sin6.sin6_addr.u.Byte;
                        ipv6.push(std::net::Ipv6Addr::from(octets).to_string());
                    }
                    _ => {}
                }
            }
            unicast = address.Next;
        }

        let up = entry.OperStatus == 1; // IfOperStatusUp
                                        // Adapters that do not report a link speed return 0 or u64::MAX.
        let speed_mbps = match entry.TransmitLinkSpeed {
            0 | u64::MAX => None,
            speed => Some(speed / 1_000_000).filter(|mbps| *mbps > 0),
        };

        interfaces.push(NetworkInterface {
            name,
            description,
            id,
            kind: interface_kind(entry.IfType).to_string(),
            if_type: entry.IfType,
            status: interface_status(entry.OperStatus).to_string(),
            up,
            mac,
            ipv4,
            ipv6,
            speed_mbps,
            // The pseudo loopback adapter reports MTU 0xFFFFFFFF.
            mtu: (entry.Mtu != 0 && entry.Mtu != u32::MAX).then_some(entry.Mtu),
        });

        adapter = entry.Next;
    }

    interfaces
}

/// Read a NUL-terminated UTF-16 string.
#[cfg(windows)]
unsafe fn wide_to_string(ptr: *const u16) -> String {
    if ptr.is_null() {
        return String::new();
    }
    let mut length = 0usize;
    while *ptr.add(length) != 0 && length < 4096 {
        length += 1;
    }
    String::from_utf16_lossy(std::slice::from_raw_parts(ptr, length))
}

/// Read a NUL-terminated ANSI string.
#[cfg(windows)]
unsafe fn ansi_to_string(ptr: *const u8) -> String {
    if ptr.is_null() {
        return String::new();
    }
    std::ffi::CStr::from_ptr(ptr.cast())
        .to_string_lossy()
        .into_owned()
}

// ---------------------------------------------------------------------------
// Linux: `/sys/class/net` + `ip addr`
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
pub fn scan_network_interfaces() -> Vec<NetworkInterface> {
    use std::collections::HashMap;
    use std::fs;

    let mut addresses: HashMap<String, (Vec<String>, Vec<String>)> = HashMap::new();
    if let Ok(output) = std::process::Command::new("ip")
        .args(["-o", "addr", "show"])
        .output()
    {
        if output.status.success() {
            for line in String::from_utf8_lossy(&output.stdout).lines() {
                collect_ip_line(line, &mut addresses);
            }
        }
    }

    let entries = match fs::read_dir("/sys/class/net") {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };

    let mut interfaces = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let read = |file: &str| {
            fs::read_to_string(path.join(file))
                .ok()
                .map(|value| value.trim().to_string())
        };

        let if_type: u32 = read("type")
            .and_then(|value| value.parse().ok())
            .unwrap_or(0);
        let wireless = path.join("wireless").exists();
        let kind = if if_type == 772 || name == "lo" {
            "loopback"
        } else if wireless {
            "wifi"
        } else {
            interface_kind(if_type)
        };
        let status = read("operstate").unwrap_or_else(|| "unknown".to_string());
        let mac = read("address").filter(|mac| mac != "00:00:00:00:00:00");
        let mtu = read("mtu").and_then(|value| value.parse().ok());
        let speed_mbps = read("speed")
            .and_then(|value| value.parse::<i64>().ok())
            .filter(|speed| *speed > 0)
            .map(|speed| speed as u64);
        let description = fs::read_link(path.join("device/driver"))
            .ok()
            .and_then(|target| {
                target
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .unwrap_or_default();
        let (ipv4, ipv6) = addresses.remove(&name).unwrap_or_default();

        interfaces.push(NetworkInterface {
            name: name.clone(),
            description,
            id: name,
            kind: kind.to_string(),
            if_type,
            status: status.clone(),
            up: status == "up",
            mac,
            ipv4,
            ipv6,
            speed_mbps,
            mtu,
        });
    }

    sort_interfaces(&mut interfaces);
    interfaces
}

/// Parse one `ip -o addr show` line into the address map.
#[cfg(target_os = "linux")]
fn collect_ip_line(
    line: &str,
    addresses: &mut std::collections::HashMap<String, (Vec<String>, Vec<String>)>,
) {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    // "2: eth0    inet 192.168.1.5/24 brd ... scope global eth0"
    let Some(name) = tokens.get(1).map(|name| name.trim_end_matches(':')) else {
        return;
    };
    for index in 0..tokens.len() {
        let family = match tokens[index] {
            "inet" => 4,
            "inet6" => 6,
            _ => continue,
        };
        let Some(value) = tokens.get(index + 1) else {
            continue;
        };
        let address = value.split('/').next().unwrap_or(value);
        let entry = addresses.entry(name.to_string()).or_default();
        match family {
            4 => entry.0.push(address.to_string()),
            _ => entry.1.push(address.to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// macOS / BSD: `ifconfig -a`
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
pub fn scan_network_interfaces() -> Vec<NetworkInterface> {
    let output = match std::process::Command::new("ifconfig").arg("-a").output() {
        Ok(output) if output.status.success() => output,
        _ => return Vec::new(),
    };

    let mut interfaces: Vec<NetworkInterface> = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if !line.starts_with(char::is_whitespace) && line.contains(':') {
            let mut fields = line.split_whitespace();
            let name = fields
                .next()
                .map(|name| name.trim_end_matches(':').to_string())
                .unwrap_or_default();
            let mtu =
                fields.find_map(|field| field.strip_prefix("mtu=").and_then(|v| v.parse().ok()));
            let up = line.contains("<UP,") || line.contains("<UP>");
            let kind = if name == "lo0" {
                "loopback"
            } else if name.starts_with("utun") || name.starts_with("tun") {
                "tunnel"
            } else if name.starts_with("en") {
                "ethernet"
            } else {
                "other"
            };
            interfaces.push(NetworkInterface {
                name: name.clone(),
                description: String::new(),
                id: name,
                kind: kind.to_string(),
                if_type: 0,
                status: if up { "up" } else { "down" }.to_string(),
                up,
                mac: None,
                ipv4: Vec::new(),
                ipv6: Vec::new(),
                speed_mbps: None,
                mtu,
            });
            continue;
        }

        let Some(current) = interfaces.last_mut() else {
            continue;
        };
        let tokens: Vec<&str> = line.split_whitespace().collect();
        match tokens.first().copied() {
            Some("ether") => {
                if let Some(mac) = tokens.get(1) {
                    current.mac = Some(mac.to_string());
                }
            }
            Some("inet") => {
                if let Some(address) = tokens.get(1) {
                    current.ipv4.push(address.to_string());
                }
            }
            Some("inet6") => {
                if let Some(address) = tokens.get(1) {
                    let address = address.split('%').next().unwrap_or(address);
                    current.ipv6.push(address.to_string());
                }
            }
            _ => {}
        }
    }

    sort_interfaces(&mut interfaces);
    interfaces
}

// ---------------------------------------------------------------------------
// Other platforms
// ---------------------------------------------------------------------------

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
pub fn scan_network_interfaces() -> Vec<NetworkInterface> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_counts_match_vectors() {
        let report = HardwareScanReport {
            scanned_at: "2024-01-01T00:00:00+00:00".to_string(),
            duration_ms: 1,
            network: Vec::new(),
            can_drivers: Vec::new(),
            can_devices: Vec::new(),
            warnings: Vec::new(),
        };
        assert_eq!(report.network_count(), 0);
        assert_eq!(report.can_count(), 0);
    }

    #[test]
    fn can_device_summary_uses_selection_token() {
        let info = CanDeviceInfo {
            driver: "pcan".to_string(),
            device_id: "3".to_string(),
            name: "PCAN-USB".to_string(),
            device_type: "PCAN-USB".to_string(),
            controller: 1,
            channel: "PCAN_USBBUS2".to_string(),
            available: true,
            fd_capable: false,
        };
        let summary = CanDeviceSummary::from(&info);
        assert_eq!(summary.value, "pcan:3:1");
        assert!(summary.label.contains("PCAN_USBBUS2"));
    }

    #[test]
    fn loopback_sorts_last() {
        let mut interfaces = vec![
            NetworkInterface {
                name: "lo".to_string(),
                description: String::new(),
                id: "lo".to_string(),
                kind: "loopback".to_string(),
                if_type: 772,
                status: "unknown".to_string(),
                up: true,
                mac: None,
                ipv4: Vec::new(),
                ipv6: Vec::new(),
                speed_mbps: None,
                mtu: None,
            },
            NetworkInterface {
                name: "eth0".to_string(),
                description: String::new(),
                id: "eth0".to_string(),
                kind: "ethernet".to_string(),
                if_type: 1,
                status: "up".to_string(),
                up: true,
                mac: None,
                ipv4: vec!["10.0.0.2".to_string()],
                ipv6: Vec::new(),
                speed_mbps: None,
                mtu: None,
            },
        ];
        sort_interfaces(&mut interfaces);
        assert_eq!(interfaces[0].name, "eth0");
        assert_eq!(interfaces[0].display_label(), "eth0 (10.0.0.2)");
        assert_eq!(interfaces[1].name, "lo");
    }
}
