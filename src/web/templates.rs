//! HTML Template Engine
//!
//! Uses Handlebars for template rendering with embedded template files.
//! All templates are compiled into the binary using rust-embed.

use handlebars::Handlebars;
use rust_embed::RustEmbed;
use serde_json::json;
use std::collections::HashMap;

/// Embedded templates - automatically includes all files in templates/ directory
#[derive(RustEmbed)]
#[folder = "src/web/templates"]
#[include = "*.hbs"]
pub struct Templates;

/// Template engine for rendering HTML pages
pub struct TemplateEngine {
    hbs: Handlebars<'static>,
}

impl TemplateEngine {
    /// Create a new template engine with all templates registered
    pub fn new() -> Self {
        let mut hbs = Handlebars::new();

        // Register all embedded templates
        let mut template_count = 0;
        for file in Templates::iter() {
            let filename = file.as_ref();
            // Extract template name from filename (e.g., "base.hbs" -> "base")
            if let Some(name) = filename.strip_suffix(".hbs") {
                if let Some(content) = Templates::get(filename) {
                    let content_str = std::str::from_utf8(&content.data)
                        .expect("Template is not valid UTF-8");
                    hbs.register_template_string(name, content_str)
                        .unwrap_or_else(|e| panic!("Failed to register {} template: {}", name, e));
                    template_count += 1;
                }
            }
        }

        // Verify critical templates exist
        if template_count == 0 {
            panic!("No templates found! The binary may be corrupted or templates were not embedded correctly.");
        }

        // Verify essential templates
        for essential in &["base", "login", "dashboard"] {
            if hbs.get_template(essential).is_none() {
                panic!("Essential template '{}' not found! Template embedding may have failed.", essential);
            }
        }

        tracing::info!("Loaded {} embedded templates", template_count);

        Self { hbs }
    }

    /// Render a template with data
    fn render(&self, template_name: &str, data: &serde_json::Value) -> String {
        self.hbs
            .render(template_name, data)
            .unwrap_or_else(|e| format!("<!-- Template error: {} -->", e))
    }

    /// Render a full page with base layout
    fn _render_page(
        &self,
        title: &str,
        content_template: &str,
        content_data: serde_json::Value,
        status: Option<&str>,
        error: Option<&str>,
    ) -> String {
        let content = self.render(content_template, &content_data);

        let base_data = json!({
            "title": title,
            "content": content,
            "status_message": status,
            "error_message": error,
            "active_dashboard": title == "Dashboard",
            "active_calibration": title == "Calibration",
            "active_daq": title == "DAQ",
            "active_a2l": title == "A2L",
        });

        self.render("base", &base_data)
    }

    /// Render login page
    pub fn login_page(&self, error: Option<&str>) -> String {
        let data = json!({
            "error": error,
        });
        self.render("login", &data)
    }

    /// Render dashboard page
    #[allow(clippy::too_many_arguments)]
    pub fn dashboard_page(
        &self,
        slaves: &[(String, String, bool, usize, usize)], // (id, name, connected, cal_count, meas_count)
        current_slave: Option<&(String, String)>,        // (id, name)
        connected_count: usize,
        total_cal: usize,
        total_meas: usize,
        daq_running: bool,
    ) -> String {
        let slave_count = slaves.len();

        // Transform slaves for template
        let slaves_data: Vec<serde_json::Value> = slaves
            .iter()
            .map(|(id, name, connected, cal_count, meas_count)| {
                let is_current = current_slave.map_or(false, |s| s.0 == *id);
                json!({
                    "id": id,
                    "name": name,
                    "connected": connected,
                    "cal_count": cal_count,
                    "meas_count": meas_count,
                    "is_current": is_current,
                })
            })
            .collect();

        let data = json!({
            "slave_count": slave_count,
            "slaves": slaves_data,
            "has_multiple_slaves": slave_count > 1,
            "connected_count": connected_count,
            "total_cal": total_cal,
            "total_meas": total_meas,
            "daq_running": daq_running,
        });

        // Note: Only render the content template, NOT wrapped in base template
        // The caller (routes.rs) will wrap it with templates::page()
        self.render("dashboard", &data)
    }

    /// Render connect page
    /// addresses: Vec of (address, Option<a2l_filename>)
    pub fn connect_page(&self, addresses: &[(String, Option<String>)], default_name: &str) -> String {
        // Build address list with optional A2L suffix
        let addresses_data: Vec<serde_json::Value> = if addresses.is_empty() {
            vec![json!({"value": "127.0.0.1:5555", "label": "127.0.0.1:5555 (default)"})]
        } else {
            addresses
                .iter()
                .map(|(addr, a2l_name)| {
                    let label = if let Some(name) = a2l_name {
                        format!("{} [{}]", addr, name)
                    } else {
                        addr.clone()
                    };
                    json!({"value": addr, "label": label})
                })
                .collect()
        };

        let data = json!({
            "addresses": addresses_data,
            "default_name": default_name,
        });

        // Note: Only render the content template, NOT wrapped in base template
        // The caller (routes.rs) will wrap it with templates::page()
        self.render("connect", &data)
    }

    /// Render connect page (device-based version)
    pub fn connect_page_devices(&self, devices: &[serde_json::Value], default_name: &str) -> String {
        let data = json!({
            "devices": devices,
            "default_name": default_name,
        });

        // Note: Only render the content template, NOT wrapped in base template
        // The caller (routes.rs) will wrap it with templates::page()
        self.render("connect_devices", &data)
    }

    /// Render calibration page
    pub fn calibration_page(
        &self,
        params: &[(String, String, usize, String, String, u32, Option<String>)], // (slave_id, slave_name, idx, name, type_name, address, value)
        _selected: usize,
    ) -> String {
        let params_data: Vec<serde_json::Value> = params
            .iter()
            .map(|(slave_id, slave_name, idx, name, type_name, address, value)| {
                json!({
                    "slave_id": escape_html(slave_id),
                    "slave_name": escape_html(slave_name),
                    "idx": idx,
                    "name": escape_html(name),
                    "type_name": type_name,
                    "address_hex": format!("{:08X}", address),
                    "value": value.as_ref().map(|v| escape_html(v)),
                })
            })
            .collect();

        let data = json!({
            "params": params_data,
        });

        // Note: Only render the content template, NOT wrapped in base template
        // The caller (routes.rs) will wrap it with templates::page()
        self.render("calibration", &data)
    }

    /// Render calibration write page
    pub fn cal_write_page(
        &self,
        slave_id: &str,
        idx: usize,
        name: &str,
        current_value: Option<&str>,
        is_enum: bool,
        enum_options: &[(i64, String)],
    ) -> String {
        let enum_options_json: Vec<serde_json::Value> = enum_options
            .iter()
            .map(|(raw, text)| json!({"raw": raw, "text": text}))
            .collect();

        let data = json!({
            "slave_id": escape_html(slave_id),
            "idx": idx,
            "name": escape_html(name),
            "current_value": current_value.unwrap_or("Not read yet"),
            "is_enum": is_enum,
            "enum_options": enum_options_json,
        });

        // Note: Only render the content template, NOT wrapped in base template
        // The caller (routes.rs) will wrap it with templates::page()
        self.render("cal_write", &data)
    }

    /// Render DAQ page
    #[allow(clippy::too_many_arguments)]
    pub fn daq_page(
        &self,
        measurements: &[(String, usize, String, bool)], // (slave_id, idx, name, is_selected)
        selected_indices: &[usize],
        daq_running: bool,
        latest_values: &[(String, f64)],
        slaves: &[(String, String, bool, usize, usize)], // (id, name, connected, _, _)
        current_slave_id: Option<&str>,
    ) -> String {
        // Group measurements by slave
        let mut slave_meas_map: HashMap<&str, Vec<serde_json::Value>> = HashMap::new();
        for (slave_id, idx, name, is_selected) in measurements {
            let entry = json!({
                "slave_id": slave_id,
                "idx": idx,
                "name": escape_html(name),
                "is_selected": is_selected,
            });
            slave_meas_map.entry(slave_id).or_default().push(entry);
        }

        // Get slave names
        let slave_names: HashMap<&str, &str> = slaves
            .iter()
            .map(|(id, name, _, _, _)| (id.as_str(), name.as_str()))
            .collect();

        // Build measurements by slave for template
        let measurements_by_slave: Vec<serde_json::Value> = slave_meas_map
            .iter()
            .map(|(slave_id, measurements)| {
                let slave_name = slave_names.get(slave_id).unwrap_or(slave_id);
                json!({
                    "slave_name": escape_html(slave_name),
                    "measurements": measurements,
                })
            })
            .collect();

        // Build slave selector options
        let has_slave_selector = slaves.len() > 1;
        let slave_options: Vec<serde_json::Value> = slaves
            .iter()
            .map(|(id, name, connected, _, _)| {
                json!({
                    "id": id,
                    "name": name,
                    "status": if *connected { "●" } else { "○" },
                    "selected": current_slave_id.map_or(false, |cid| cid == id),
                })
            })
            .collect();

        // Transform latest values
        let latest_values_data: Vec<serde_json::Value> = latest_values
            .iter()
            .map(|(name, value)| {
                let (device, var_name) = if let Some(end) = name.find(']') {
                    let device = &name[1..end];
                    let var_name = &name[end + 2..];
                    (Some(device.to_string()), var_name.to_string())
                } else {
                    (None, name.clone())
                };

                json!({
                    "name": escape_html(name),
                    "device": device.map(|d| escape_html(&d)),
                    "var_name": escape_html(&var_name),
                    "value": format!("{:.6}", value),
                })
            })
            .collect();

        let data = json!({
            "selected_count": selected_indices.len(),
            "has_slave_selector": has_slave_selector,
            "slave_options": slave_options,
            "measurements_by_slave": measurements_by_slave,
            "daq_running": daq_running,
            "latest_values": latest_values_data,
        });

        // Note: Only render the content template, NOT wrapped in base template
        // The caller (routes.rs) will wrap it with templates::page()
        self.render("daq", &data)
    }

    /// Render A2L page
    pub fn a2l_page(
        &self,
        current_path: &str,
        current_slave: Option<&(String, String)>,
        slave_addresses: &[String],
        cal_count: usize,
        meas_count: usize,
    ) -> String {
        let data = json!({
            "current_path": escape_html(current_path),
            "current_slave": current_slave.map(|(id, name)| json!({
                "id": id,
                "name": name,
            })),
            "slave_addresses": slave_addresses,
            "cal_count": cal_count,
            "meas_count": meas_count,
        });

        // Note: Only render the content template, NOT wrapped in base template
        // The caller (routes.rs) will wrap it with templates::page()
        self.render("a2l", &data)
    }

    /// Render slaves management page
    pub fn slaves_page(
        &self,
        slaves: &[(String, String, bool, usize, usize)], // (id, name, connected, cal_count, meas_count)
        current_id: Option<crate::slave::SlaveId>,
    ) -> String {
        let slaves_data: Vec<serde_json::Value> = slaves
            .iter()
            .map(|(id, name, connected, cal_count, meas_count)| {
                let is_current = current_id.map_or(false, |cid| cid.0.to_string() == *id);
                json!({
                    "id": id,
                    "short_id": &id[..8.min(id.len())],
                    "name": name,
                    "connected": connected,
                    "cal_count": cal_count,
                    "meas_count": meas_count,
                    "is_current": is_current,
                })
            })
            .collect();

        let data = json!({
            "slave_count": slaves.len(),
            "slaves": slaves_data,
        });

        // Note: Only render the content template, NOT wrapped in base template
        // The caller (routes.rs) will wrap it with templates::page()
        self.render("slaves", &data)
    }

    /// Render checksum verification page
    pub fn checksum_page(&self, data: &serde_json::Value) -> String {
        self.render("checksum", data)
    }
}

impl Default for TemplateEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Render a full page with base layout
pub fn page(title: &str, content: &str, status: Option<&str>, error: Option<&str>) -> String {
    let engine = TemplateEngine::new();
    let data = json!({
        "title": title,
        "content": content,
        "status_message": status,
        "error_message": error,
        "active_dashboard": title == "Dashboard",
        "active_calibration": title == "Calibration",
        "active_daq": title == "DAQ",
        "active_a2l": title == "A2L",
        "active_checksum": title == "Checksum",
    });
    engine.render("base", &data)
}

/// Render dashboard page
#[allow(clippy::too_many_arguments)]
pub fn dashboard_page(
    slaves: &[(String, String, bool, usize, usize)],
    current_slave: Option<&(String, String)>,
    connected_count: usize,
    total_cal: usize,
    total_meas: usize,
    daq_running: bool,
) -> String {
    let engine = TemplateEngine::new();
    engine.dashboard_page(slaves, current_slave, connected_count, total_cal, total_meas, daq_running)
}

/// Render connect page (new device-based version)
pub fn connect_page_devices(devices: &[serde_json::Value], default_name: &str) -> String {
    let engine = TemplateEngine::new();
    engine.connect_page_devices(devices, default_name)
}

/// Render connect page (legacy version)
pub fn connect_page(addresses: &[(String, Option<String>)], default_name: &str) -> String {
    let engine = TemplateEngine::new();
    engine.connect_page(addresses, default_name)
}

/// Render slaves page
pub fn slaves_page(
    slaves: &[(String, String, bool, usize, usize)],
    current_id: Option<crate::slave::SlaveId>,
) -> String {
    let engine = TemplateEngine::new();
    engine.slaves_page(slaves, current_id)
}

/// Render calibration page
pub fn calibration_page(
    params: &[(String, String, usize, String, String, u32, Option<String>)],
    selected: usize,
) -> String {
    let engine = TemplateEngine::new();
    engine.calibration_page(params, selected)
}

/// Render calibration write page
pub fn cal_write_page(
    slave_id: &str,
    idx: usize,
    name: &str,
    current_value: Option<&str>,
    is_enum: bool,
    enum_options: &[(i64, String)],
) -> String {
    let engine = TemplateEngine::new();
    engine.cal_write_page(slave_id, idx, name, current_value, is_enum, enum_options)
}

/// Render DAQ page
#[allow(clippy::too_many_arguments)]
pub fn daq_page(
    measurements: &[(String, usize, String, bool)],
    selected_indices: &[usize],
    daq_running: bool,
    latest_values: &[(String, f64)],
    slaves: &[(String, String, bool, usize, usize)],
    current_slave_id: Option<&str>,
) -> String {
    let engine = TemplateEngine::new();
    engine.daq_page(measurements, selected_indices, daq_running, latest_values, slaves, current_slave_id)
}

/// Render A2L page
pub fn a2l_page(
    current_path: &str,
    current_slave: Option<&(String, String)>,
    slave_addresses: &[String],
    cal_count: usize,
    meas_count: usize,
) -> String {
    let engine = TemplateEngine::new();
    engine.a2l_page(current_path, current_slave, slave_addresses, cal_count, meas_count)
}

/// Escape HTML special characters
fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_template_engine_creation() {
        let engine = TemplateEngine::new();
        // Just verify it doesn't panic
        let _ = engine.login_page(None);
    }

    #[test]
    fn test_login_page_with_error() {
        let engine = TemplateEngine::new();
        let html = engine.login_page(Some("Invalid credentials"));
        assert!(html.contains("Invalid credentials"));
        assert!(html.contains("XCP Web Client"));
    }

    #[test]
    fn test_escape_html() {
        assert_eq!(escape_html("<script>"), "&lt;script&gt;");
        assert_eq!(escape_html("\"test\""), "&quot;test&quot;");
        assert_eq!(escape_html("A & B"), "A &amp; B");
    }

    #[test]
    fn test_dashboard_page() {
        let engine = TemplateEngine::new();
        let slaves = vec![(
            "test-id-123".to_string(),
            "ECU1".to_string(),
            true,
            5,
            8,
        )];
        let html = engine.dashboard_page(&slaves, None, 1, 5, 8, false);
        assert!(html.contains("ECU1"));
        assert!(html.contains("Slaves (1)"));
    }

    #[test]
    fn test_connect_page() {
        let engine = TemplateEngine::new();
        let addresses = vec![("127.0.0.1:5555".to_string(), None)];
        let html = engine.connect_page(&addresses, "ECU1");
        assert!(html.contains("Connect to XCP Slave"));
        assert!(html.contains("127.0.0.1:5555"));
    }

    #[test]
    fn test_calibration_page() {
        let engine = TemplateEngine::new();
        // New calibration template uses dynamic loading via modal
        // Test that the page renders correctly with empty params
        let html = engine.calibration_page(&[], 0);
        assert!(html.contains("Add Variables"));
        assert!(html.contains("No variables selected"));
    }

    #[test]
    fn test_a2l_page() {
        let engine = TemplateEngine::new();
        let html = engine.a2l_page("/path/to/file.a2l", None, &[], 0, 0);
        assert!(html.contains("A2L File Management"));
        assert!(html.contains("/path/to/file.a2l"));
    }
}
