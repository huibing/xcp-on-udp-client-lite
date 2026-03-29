//! HTML Templates
//!
//! Pure Rust HTML templates - no frontend framework.
//! All pages are server-rendered HTML with minimal JavaScript.

/// HTML page wrapper with common layout
pub fn page(title: &str, content: &str, status: Option<&str>, error: Option<&str>) -> String {
    let status_html = status.map(|s| {
        format!(r#"<div class="status success">{}</div>"#, escape_html(s))
    }).unwrap_or_default();

    let error_html = error.map(|e| {
        format!(r#"<div class="status error">{}</div>"#, escape_html(e))
    }).unwrap_or_default();

    format!(
        r##"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{} - XCP Web Client</title>
    <style>
        * {{
            margin: 0;
            padding: 0;
            box-sizing: border-box;
        }}
        body {{
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            background: #1a1a2e;
            color: #eee;
            min-height: 100vh;
        }}
        .header {{
            background: #16213e;
            padding: 1rem 2rem;
            border-bottom: 2px solid #0f3460;
            display: flex;
            justify-content: space-between;
            align-items: center;
        }}
        .header h1 {{
            color: #00ff88;
            font-size: 1.5rem;
        }}
        .nav {{
            display: flex;
            gap: 1rem;
        }}
        .nav a {{
            color: #eee;
            text-decoration: none;
            padding: 0.5rem 1rem;
            border-radius: 4px;
            transition: background 0.2s;
        }}
        .nav a:hover {{
            background: #0f3460;
        }}
        .nav a.active {{
            background: #00ff88;
            color: #1a1a2e;
        }}
        .container {{
            max-width: 1400px;
            margin: 0 auto;
            padding: 2rem;
        }}
        .status {{
            padding: 1rem;
            margin-bottom: 1rem;
            border-radius: 4px;
        }}
        .status.success {{
            background: #0f3d0f;
            border: 1px solid #00ff88;
            color: #00ff88;
        }}
        .status.error {{
            background: #3d0f0f;
            border: 1px solid #ff4444;
            color: #ff4444;
        }}
        .card {{
            background: #16213e;
            border-radius: 8px;
            padding: 1.5rem;
            margin-bottom: 1rem;
            border: 1px solid #0f3460;
        }}
        .card h2 {{
            color: #00ff88;
            margin-bottom: 1rem;
            font-size: 1.2rem;
        }}
        .form-group {{
            margin-bottom: 1rem;
        }}
        .form-group label {{
            display: block;
            margin-bottom: 0.5rem;
            color: #aaa;
        }}
        input[type="text"],
        input[type="password"],
        input[type="number"],
        select {{
            width: 100%;
            padding: 0.75rem;
            background: #1a1a2e;
            border: 1px solid #0f3460;
            border-radius: 4px;
            color: #eee;
            font-size: 1rem;
        }}
        input:focus, select:focus {{
            outline: none;
            border-color: #00ff88;
        }}
        button {{
            background: #00ff88;
            color: #1a1a2e;
            border: none;
            padding: 0.75rem 1.5rem;
            border-radius: 4px;
            cursor: pointer;
            font-size: 1rem;
            font-weight: bold;
            transition: opacity 0.2s;
        }}
        button:hover {{
            opacity: 0.9;
        }}
        button.secondary {{
            background: #0f3460;
            color: #eee;
        }}
        button.danger {{
            background: #ff4444;
            color: #fff;
        }}
        .btn-group {{
            display: flex;
            gap: 0.5rem;
            flex-wrap: wrap;
        }}
        table {{
            width: 100%;
            border-collapse: collapse;
        }}
        th, td {{
            padding: 0.75rem;
            text-align: left;
            border-bottom: 1px solid #0f3460;
        }}
        th {{
            color: #00ff88;
            font-weight: 600;
        }}
        tr:hover {{
            background: #1a1a2e;
        }}
        .tag {{
            display: inline-block;
            padding: 0.25rem 0.5rem;
            border-radius: 4px;
            font-size: 0.8rem;
            font-weight: bold;
        }}
        .tag.connected {{
            background: #0f3d0f;
            color: #00ff88;
        }}
        .tag.disconnected {{
            background: #3d0f0f;
            color: #ff4444;
        }}
        .tag.running {{
            background: #0f3d0f;
            color: #00ff88;
        }}
        .tag.stopped {{
            background: #3d3d0f;
            color: #ffaa00;
        }}
        .grid {{
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
            gap: 1rem;
        }}
        .login-box {{
            max-width: 400px;
            margin: 5rem auto;
        }}
        .login-box h2 {{
            text-align: center;
            color: #00ff88;
            margin-bottom: 2rem;
        }}
        .value-display {{
            font-family: 'Courier New', monospace;
            background: #1a1a2e;
            padding: 0.5rem;
            border-radius: 4px;
            color: #00ff88;
        }}
        .search-box {{
            margin-bottom: 1rem;
        }}
        .checkbox {{
            display: flex;
            align-items: center;
            gap: 0.5rem;
            cursor: pointer;
        }}
        .checkbox input {{
            width: auto;
        }}
        .daq-value {{
            display: flex;
            justify-content: space-between;
            padding: 0.5rem 0;
            border-bottom: 1px solid #0f3460;
        }}
        .daq-value .name {{
            color: #aaa;
        }}
        .daq-value .value {{
            color: #00ff88;
            font-family: 'Courier New', monospace;
        }}
        pre {{
            background: #1a1a2e;
            padding: 1rem;
            border-radius: 4px;
            overflow-x: auto;
            font-size: 0.9rem;
        }}
    </style>
</head>
<body>
    <div class="header">
        <h1>XCP Web Client</h1>
        <nav class="nav">
            <a href="/" {}>Dashboard</a>
            <a href="/calibration" {}>Calibration</a>
            <a href="/daq" {}>DAQ</a>
            <a href="/a2l" {}>A2L</a>
            <a href="/logout">Logout</a>
        </nav>
    </div>
    <div class="container">
        {}
        {}
        {}
    </div>
</body>
</html>"##,
        escape_html(title),
        if title == "Dashboard" { "class=\"active\"" } else { "" },
        if title == "Calibration" { "class=\"active\"" } else { "" },
        if title == "DAQ" { "class=\"active\"" } else { "" },
        if title == "A2L" { "class=\"active\"" } else { "" },
        status_html,
        error_html,
        content
    )
}

/// Login page template
pub fn login_page(error: Option<&str>) -> String {
    let error_html = error.map(|e| {
        format!(r#"<div class="status error">{}</div>"#, escape_html(e))
    }).unwrap_or_default();

    format!(
        r##"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Login - XCP Web Client</title>
    <style>
        * {{
            margin: 0;
            padding: 0;
            box-sizing: border-box;
        }}
        body {{
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            background: #1a1a2e;
            color: #eee;
            min-height: 100vh;
            display: flex;
            align-items: center;
            justify-content: center;
        }}
        .login-box {{
            background: #16213e;
            padding: 2rem;
            border-radius: 8px;
            border: 1px solid #0f3460;
            width: 100%;
            max-width: 400px;
        }}
        .login-box h1 {{
            color: #00ff88;
            text-align: center;
            margin-bottom: 2rem;
        }}
        .form-group {{
            margin-bottom: 1rem;
        }}
        .form-group label {{
            display: block;
            margin-bottom: 0.5rem;
            color: #aaa;
        }}
        input[type="text"],
        input[type="password"] {{
            width: 100%;
            padding: 0.75rem;
            background: #1a1a2e;
            border: 1px solid #0f3460;
            border-radius: 4px;
            color: #eee;
            font-size: 1rem;
        }}
        input:focus {{
            outline: none;
            border-color: #00ff88;
        }}
        button {{
            width: 100%;
            background: #00ff88;
            color: #1a1a2e;
            border: none;
            padding: 0.75rem;
            border-radius: 4px;
            cursor: pointer;
            font-size: 1rem;
            font-weight: bold;
            margin-top: 1rem;
        }}
        button:hover {{
            opacity: 0.9;
        }}
        .status.error {{
            background: #3d0f0f;
            border: 1px solid #ff4444;
            color: #ff4444;
            padding: 1rem;
            margin-bottom: 1rem;
            border-radius: 4px;
        }}
        .hint {{
            text-align: center;
            color: #666;
            margin-top: 1rem;
            font-size: 0.9rem;
        }}
    </style>
</head>
<body>
    <div class="login-box">
        <h1>XCP Web Client</h1>
        {}
        <form method="POST" action="/api/login">
            <div class="form-group">
                <label for="username">Username</label>
                <input type="text" id="username" name="username" required autofocus>
            </div>
            <div class="form-group">
                <label for="password">Password</label>
                <input type="password" id="password" name="password" required>
            </div>
            <button type="submit">Login</button>
        </form>
        <p class="hint">Default: admin / admin</p>
    </div>
</body>
</html>"##,
        error_html
    )
}

/// Dashboard page template
pub fn dashboard_page(
    slaves: &[(String, String, bool, usize, usize)], // (id, name, connected, cal_count, meas_count)
    current_slave: Option<&(String, String)>,        // (id, name)
    connected_count: usize,
    total_cal: usize,
    total_meas: usize,
    daq_running: bool,
) -> String {
    let daq_status = if daq_running {
        r#"<span class="tag running">Running</span>"#
    } else {
        r#"<span class="tag stopped">Stopped</span>"#
    };

    // Build slave list
    let slaves_html: String = if slaves.is_empty() {
        r#"<p style="color: #666;">No slaves connected</p>"#.to_string()
    } else {
        slaves.iter().map(|(id, name, connected, cal_count, meas_count)| {
            let status_class = if *connected { "connected" } else { "disconnected" };
            let status_text = if *connected { "Connected" } else { "Disconnected" };
            let is_current = current_slave.map_or(false, |s| s.0 == *id);

            format!(
                r#"
                <div style="display: flex; justify-content: space-between; align-items: center; padding: 0.5rem; background: {}; margin-bottom: 0.5rem; border-radius: 4px;">
                    <div>
                        <strong>{}</strong> {}
                        <span class="tag {}">{}</span>
                        <br>
                        <small style="color: #888;">{} cal, {} meas</small>
                    </div>
                    <div class="btn-group">
                        {}
                    </div>
                </div>
                "#,
                if is_current { "#0f3460" } else { "#1a1a2e" },
                escape_html(name),
                if is_current { "<span style='color: #00ff88;'>(current)</span>" } else { "" },
                status_class,
                status_text,
                cal_count,
                meas_count,
                if is_current {
                    r#"<button class="secondary" onclick="location.href='/api/disconnect'">Disconnect</button>"#.to_string()
                } else {
                    format!(
                        r#"<button onclick="location.href='/api/slave/select/{}'">Select</button>
                           <button class="secondary" onclick="location.href='/api/disconnect/{}'">Disconnect</button>"#,
                        id, id
                    )
                }
            )
        }).collect()
    };

    let content = format!(
        r#"
        <div class="grid">
            <div class="card">
                <h2>Slaves ({})</h2>
                {}
                <br>
                <button onclick="location.href='/connect'">Connect New Slave</button>
                {}
            </div>
            <div class="card">
                <h2>DAQ Status</h2>
                <p>{}</p>
                <br>
                <div class="btn-group">
                    {}
                </div>
            </div>
            <div class="card">
                <h2>Summary</h2>
                <p><span class="tag connected">{}</span> connected</p>
                <p>{} calibration parameters</p>
                <p>{} measurements</p>
            </div>
        </div>
        "#,
        slaves.len(),
        slaves_html,
        if slaves.len() > 1 {
            r#"<br><button class="secondary" onclick="location.href='/slaves'">Manage Slaves</button>"#
        } else {
            ""
        },
        daq_status,
        if daq_running {
            r#"<button onclick="location.href='/api/daq/stop'" class="danger">Stop DAQ</button>"#
        } else {
            r#"<button onclick="location.href='/api/daq/start'">Start DAQ</button>"#
        },
        connected_count,
        total_cal,
        total_meas,
    );

    page("Dashboard", &content, None, None)
}

/// Connect page template
pub fn connect_page(addresses: &[String]) -> String {
    let address_options: String = addresses
        .iter()
        .map(|addr| {
            format!(
                r#"<option value="{}">{}</option>"#,
                escape_html(addr),
                escape_html(addr)
            )
        })
        .collect();

    let content = format!(
        r#"
        <div class="card" style="max-width: 500px;">
            <h2>Connect to XCP Slave</h2>
            <form method="POST" action="/api/connect">
                <div class="form-group">
                    <label for="name">Slave Name</label>
                    <input type="text" name="name" id="name" placeholder="ECU1" required>
                </div>
                <div class="form-group">
                    <label for="address">Slave Address</label>
                    <select name="address" id="address">
                        {}
                    </select>
                </div>
                <div class="form-group">
                    <label for="custom">Or enter custom address</label>
                    <input type="text" name="custom" id="custom" placeholder="127.0.0.1:5555">
                </div>
                <div class="btn-group">
                    <button type="submit">Connect</button>
                    <button type="button" class="secondary" onclick="location.href='/'">Cancel</button>
                </div>
            </form>
        </div>
        "#,
        if address_options.is_empty() {
            r#"<option value="127.0.0.1:5555">127.0.0.1:5555 (default)</option>"#.to_string()
        } else {
            address_options
        }
    );

    page("Connect", &content, None, None)
}

/// Calibration list page template
/// params: (slave_name, idx, param_name, type_name, address, value)
pub fn calibration_page(
    params: &[(String, usize, String, String, u32, Option<String>)],
    _selected: usize,
) -> String {
    let rows: String = params
        .iter()
        .map(|(slave_name, idx, name, type_name, address, value)| {
            let value_str = value.as_ref()
                .map(|v| format!(r#"<span class="value-display">{}</span>"#, escape_html(v)))
                .unwrap_or_else(|| "-".to_string());

            format!(
                r#"
                <tr>
                    <td><span style="color: #888; font-size: 0.85em;">[{}]</span><br>{}</td>
                    <td>{}</td>
                    <td>0x{:08X}</td>
                    <td>{}</td>
                    <td>
                        <div class="btn-group">
                            <button onclick="location.href='/api/cal/read/{}/{}'" title="Read">R</button>
                            <button onclick="location.href='/cal/write/{}/{}'" title="Write">W</button>
                        </div>
                    </td>
                </tr>
                "#,
                escape_html(slave_name),
                escape_html(name),
                escape_html(type_name),
                address,
                value_str,
                slave_name.split(':').next().unwrap_or(slave_name), // Use slave ID prefix
                idx,
                slave_name.split(':').next().unwrap_or(slave_name),
                idx
            )
        })
        .collect();

    let content = format!(
        r#"
        <div class="card">
            <h2>Calibration Parameters</h2>
            <form method="GET" action="/calibration" class="search-box">
                <div style="display: flex; gap: 0.5rem;">
                    <input type="text" name="search" placeholder="Search parameters..." autofocus style="flex: 2;">
                    <input type="text" name="slave" placeholder="Filter by slave..." style="flex: 1;">
                </div>
                <button type="submit" style="margin-top: 0.5rem;">Search</button>
            </form>
            <table>
                <thead>
                    <tr>
                        <th>Device / Name</th>
                        <th>Type</th>
                        <th>Address</th>
                        <th>Value</th>
                        <th>Actions</th>
                    </tr>
                </thead>
                <tbody>
                    {}
                </tbody>
            </table>
        </div>
        "#,
        if rows.is_empty() {
            r#"<tr><td colspan="5" style="text-align: center; color: #666;">No parameters loaded. Load an A2L file first.</td></tr>"#.to_string()
        } else {
            rows
        }
    );

    page("Calibration", &content, None, None)
}

/// Calibration write page template
pub fn cal_write_page(slave_id: &str, idx: usize, name: &str, current_value: Option<&str>) -> String {
    let current = current_value.unwrap_or("Not read yet");

    let content = format!(
        r#"
        <div class="card" style="max-width: 500px;">
            <h2>Write Calibration Parameter</h2>
            <p><strong>Device:</strong> {}</p>
            <p><strong>Parameter:</strong> {}</p>
            <p><strong>Current Value:</strong> {}</p>
            <br>
            <form method="POST" action="/api/cal/write/{}/{}">
                <div class="form-group">
                    <label for="value">New Value</label>
                    <input type="text" name="value" id="value" required autofocus>
                </div>
                <div class="btn-group">
                    <button type="submit">Write</button>
                    <button type="button" class="secondary" onclick="location.href='/calibration'">Cancel</button>
                </div>
            </form>
        </div>
        "#,
        escape_html(slave_id),
        escape_html(name),
        escape_html(current),
        escape_html(slave_id),
        idx
    );

    page("Write Calibration", &content, None, None)
}

/// DAQ page template
/// measurements: (slave_id, idx, name, is_selected)
pub fn daq_page(
    measurements: &[(String, usize, String, bool)],
    selected_indices: &[usize],
    daq_running: bool,
    latest_values: &[(String, f64)],
    slaves: &[(String, String, bool, usize, usize)],
    current_slave_id: Option<&str>,
) -> String {
    // Group measurements by slave
    let mut slave_measurements: std::collections::HashMap<&str, Vec<&(String, usize, String, bool)>> = std::collections::HashMap::new();
    for m in measurements {
        slave_measurements.entry(&m.0).or_default().push(m);
    }

    let meas_list: String = slave_measurements.iter()
        .map(|(slave_id, meas)| {
            let slave_name = slaves.iter()
                .find(|(id, _, _, _, _)| id == *slave_id)
                .map(|(_, name, _, _, _)| name.as_str())
                .unwrap_or(slave_id);

            let slave_section = format!(
                r#"<div style="background: #0f3460; padding: 0.5rem; margin-bottom: 0.5rem; border-radius: 4px;">
                    <strong style="color: #00ff88;">[{}]</strong>
                </div>"#,
                escape_html(slave_name)
            );

            let meas_html: String = meas.iter().map(|(slave_id, idx, name, is_selected)| {
                format!(
                    r#"
                    <div class="daq-value">
                        <label class="checkbox">
                            <input type="checkbox" name="meas_{}_{}" {} onchange="toggleMeas('{}', {})">
                            <span>{}</span>
                        </label>
                    </div>
                    "#,
                    escape_html(slave_id),
                    idx,
                    if *is_selected { "checked" } else { "" },
                    escape_html(slave_id),
                    idx,
                    escape_html(name)
                )
            }).collect();

            format!("{}{}", slave_section, meas_html)
        })
        .collect();

    let values_html: String = if daq_running && !latest_values.is_empty() {
        latest_values
            .iter()
            .map(|(name, value)| {
                // Parse device prefix if present
                let display_name = if let Some(end) = name.find(']') {
                    let device = &name[1..end];
                    let var_name = &name[end+2..];
                    format!(
                        r#"<span style="color: #888; font-size: 0.85em;">[{}]</span> {}"#,
                        escape_html(device),
                        escape_html(var_name)
                    )
                } else {
                    escape_html(name)
                };

                format!(
                    r#"
                    <div class="daq-value">
                        <span class="name">{}</span>
                        <span class="value">{:.6}</span>
                    </div>
                    "#,
                    display_name,
                    value
                )
            })
            .collect()
    } else {
        r#"<p style="color: #666; text-align: center;">No data (DAQ not running)</p>"#.to_string()
    };

    // Slave selector
    let slave_selector = if slaves.len() > 1 {
        let options: String = slaves.iter().map(|(id, name, connected, _, _)| {
            let status = if *connected { "●" } else { "○" };
            let selected = current_slave_id.map_or(false, |cid| cid == id);
            format!(
                r#"<option value="{}" {}>{} {}</option>"#,
                id,
                if selected { "selected" } else { "" },
                status,
                escape_html(name)
            )
        }).collect();

        format!(
            r#"
            <div class="form-group" style="margin-bottom: 1rem;">
                <label>Current Slave</label>
                <select onchange="selectSlave(this.value)">
                    <option value="">All Slaves</option>
                    {}
                </select>
            </div>
            <script>
                function selectSlave(id) {{
                    if (id) {{
                        location.href = '/api/slave/select/' + id;
                    }}
                }}
            </script>
            "#,
            options
        )
    } else {
        String::new()
    };

    let content = format!(
        r#"
        <div class="grid">
            <div class="card">
                <h2>Measurements ({})</h2>
                {}
                <form id="measForm" method="POST" action="/api/daq/select">
                    <div style="max-height: 400px; overflow-y: auto;">
                        {}
                    </div>
                </form>
            </div>
            <div class="card">
                <h2>Live Data {}</h2>
                <div id="liveData" style="max-height: 400px; overflow-y: auto;">
                    {}
                </div>
                <br>
                <div class="btn-group">
                    {}
                    <button onclick="location.href='/api/daq/clear'" class="secondary">Clear</button>
                </div>
            </div>
        </div>
        <script>
            function toggleMeas(slaveId, idx) {{
                fetch('/api/daq/toggle', {{
                    method: 'POST',
                    headers: {{'Content-Type': 'application/x-www-form-urlencoded'}},
                    body: 'slave_id=' + encodeURIComponent(slaveId) + '&idx=' + idx
                }});
            }}
            // Auto-refresh live data every 500ms
            setInterval(function() {{
                fetch('/api/daq/data')
                    .then(r => r.text())
                    .then(html => {{
                        document.getElementById('liveData').innerHTML = html;
                    }});
            }}, 500);
        </script>
        "#,
        selected_indices.len(),
        slave_selector,
        if meas_list.is_empty() {
            r#"<p style="color: #666;">No measurements loaded. Load A2L files first.</p>"#.to_string()
        } else {
            meas_list
        },
        if daq_running {
            r#"<span class="tag running">RUNNING</span>"#
        } else {
            r#"<span class="tag stopped">STOPPED</span>"#
        },
        values_html,
        if daq_running {
            r#"<button onclick="location.href='/api/daq/stop'" class="danger">Stop</button>"#.to_string()
        } else {
            r#"<button onclick="location.href='/api/daq/start'">Start</button>"#.to_string()
        }
    );

    page("DAQ", &content, None, None)
}

/// A2L file management page
pub fn a2l_page(current_path: &str) -> String {
    let content = format!(
        r#"
        <div class="card" style="max-width: 600px;">
            <h2>A2L File Management</h2>
            <p><strong>Current File:</strong></p>
            <pre>{}</pre>
            <br>
            <form method="POST" action="/api/a2l/load" enctype="multipart/form-data">
                <div class="form-group">
                    <label for="slave_id">Target Slave (optional)</label>
                    <input type="text" name="slave_id" id="slave_id" placeholder="Slave ID (leave empty for current)">
                </div>
                <div class="form-group">
                    <label for="path">Load A2L File</label>
                    <input type="text" name="path" id="path" placeholder="path/to/file.a2l" required>
                </div>
                <button type="submit">Load</button>
            </form>
        </div>
        "#,
        escape_html(current_path)
    );

    page("A2L", &content, None, None)
}

/// Slaves management page
pub fn slaves_page(
    slaves: &[(String, String, bool, usize, usize)], // (id, name, connected, cal_count, meas_count)
    current_id: Option<crate::slave::SlaveId>,
) -> String {
    let rows: String = slaves
        .iter()
        .map(|(id, name, connected, cal_count, meas_count)| {
            let status_class = if *connected { "connected" } else { "disconnected" };
            let status_text = if *connected { "Connected" } else { "Disconnected" };
            let is_current = current_id.map_or(false, |cid| cid.0.to_string() == *id);

            format!(
                r#"
                <tr>
                    <td>
                        {}
                        <span class="tag {}">{}</span>
                    </td>
                    <td>{}</td>
                    <td>{}</td>
                    <td>{}</td>
                    <td>
                        <div class="btn-group">
                            {}
                            <button class="secondary" onclick="location.href='/api/disconnect/{}'">Disconnect</button>
                        </div>
                    </td>
                </tr>
                "#,
                if is_current {
                    format!("<strong>{}</strong> <span style='color: #00ff88;'>(current)</span>", escape_html(name))
                } else {
                    escape_html(name)
                },
                status_class,
                status_text,
                cal_count,
                meas_count,
                escape_html(&id[..8.min(id.len())]),
                if is_current {
                    r#"<button disabled>Selected</button>"#.to_string()
                } else {
                    format!(r#"<button onclick="location.href='/api/slave/select/{}'">Select</button>"#, id)
                },
                id
            )
        })
        .collect();

    let content = format!(
        r#"
        <div class="card">
            <h2>Connected Slaves ({})</h2>
            <table>
                <thead>
                    <tr>
                        <th>Name</th>
                        <th>Cal Params</th>
                        <th>Measurements</th>
                        <th>ID</th>
                        <th>Actions</th>
                    </tr>
                </thead>
                <tbody>
                    {}
                </tbody>
            </table>
            <br>
            <button onclick="location.href='/connect'">Connect New Slave</button>
        </div>
        "#,
        slaves.len(),
        if rows.is_empty() {
            r#"<tr><td colspan="5" style="text-align: center; color: #666;">No slaves connected</td></tr>"#.to_string()
        } else {
            rows
        }
    );

    page("Slaves", &content, None, None)
}

/// DAQ data partial (for AJAX updates)
pub fn daq_data_partial(values: &[(String, f64)]) -> String {
    if values.is_empty() {
        return r#"<p style="color: #666; text-align: center;">No data</p>"#.to_string();
    }

    values
        .iter()
        .map(|(name, value)| {
            format!(
                r#"
                <div class="daq-value">
                    <span class="name">{}</span>
                    <span class="value">{:.6}</span>
                </div>
                "#,
                escape_html(name),
                value
            )
        })
        .collect()
}

/// Escape HTML special characters
fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}
