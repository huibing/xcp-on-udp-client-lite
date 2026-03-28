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
    connected: bool,
    slave_addr: &str,
    a2l_path: &str,
    cal_count: usize,
    meas_count: usize,
    daq_running: bool,
) -> String {
    let conn_status = if connected {
        format!(
            r#"<span class="tag connected">Connected</span> to {}"#,
            escape_html(slave_addr)
        )
    } else {
        r#"<span class="tag disconnected">Disconnected</span>"#.to_string()
    };

    let daq_status = if daq_running {
        r#"<span class="tag running">Running</span>"#
    } else {
        r#"<span class="tag stopped">Stopped</span>"#
    };

    let content = format!(
        r#"
        <div class="grid">
            <div class="card">
                <h2>Connection Status</h2>
                <p>{}</p>
                <br>
                <div class="btn-group">
                    {}
                </div>
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
                <h2>A2L File</h2>
                <p>{}</p>
                <br>
                <p>{} calibration parameters</p>
                <p>{} measurements</p>
            </div>
        </div>
        "#,
        conn_status,
        if connected {
            r#"<button onclick="location.href='/api/disconnect'" class="danger">Disconnect</button>"#
        } else {
            r#"<button onclick="location.href='/connect'">Connect</button>"#
        },
        daq_status,
        if daq_running {
            r#"<button onclick="location.href='/api/daq/stop'" class="danger">Stop DAQ</button>"#
        } else {
            r#"<button onclick="location.href='/api/daq/start'">Start DAQ</button>"#
        },
        escape_html(a2l_path),
        cal_count,
        meas_count,
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
pub fn calibration_page(
    params: &[(usize, String, String, u32, Option<String>)],
    selected: usize,
) -> String {
    let rows: String = params
        .iter()
        .map(|(idx, name, type_name, address, value)| {
            let value_str = value.as_ref()
                .map(|v| format!(r#"<span class="value-display">{}</span>"#, escape_html(v)))
                .unwrap_or_else(|| "-".to_string());

            format!(
                r#"
                <tr {}>
                    <td>{}</td>
                    <td>{}</td>
                    <td>0x{:08X}</td>
                    <td>{}</td>
                    <td>
                        <div class="btn-group">
                            <button onclick="location.href='/api/cal/read/{}'" title="Read">R</button>
                            <button onclick="location.href='/cal/write/{}'" title="Write">W</button>
                        </div>
                    </td>
                </tr>
                "#,
                if *idx == selected { r#"style="background: #0f3460;""# } else { "" },
                escape_html(name),
                escape_html(type_name),
                address,
                value_str,
                idx,
                idx
            )
        })
        .collect();

    let content = format!(
        r#"
        <div class="card">
            <h2>Calibration Parameters</h2>
            <form method="GET" action="/calibration" class="search-box">
                <input type="text" name="search" placeholder="Search parameters..." autofocus>
                <button type="submit" style="margin-top: 0.5rem;">Search</button>
            </form>
            <table>
                <thead>
                    <tr>
                        <th>Name</th>
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
            r#"<tr><td colspan="5" style="text-align: center; color: #666;">No parameters loaded</td></tr>"#.to_string()
        } else {
            rows
        }
    );

    page("Calibration", &content, None, None)
}

/// Calibration write page template
pub fn cal_write_page(idx: usize, name: &str, current_value: Option<&str>) -> String {
    let current = current_value.unwrap_or("Not read yet");

    let content = format!(
        r#"
        <div class="card" style="max-width: 500px;">
            <h2>Write Calibration Parameter</h2>
            <p><strong>Parameter:</strong> {}</p>
            <p><strong>Current Value:</strong> {}</p>
            <br>
            <form method="POST" action="/api/cal/write/{}">
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
        escape_html(name),
        escape_html(current),
        idx
    );

    page("Write Calibration", &content, None, None)
}

/// DAQ page template
pub fn daq_page(
    measurements: &[(usize, String, bool)],
    selected_indices: &[usize],
    daq_running: bool,
    latest_values: &[(String, f64)],
) -> String {
    let meas_list: String = measurements
        .iter()
        .map(|(idx, name, is_selected)| {
            format!(
                r#"
                <div class="daq-value">
                    <label class="checkbox">
                        <input type="checkbox" name="meas" value="{}" {} onchange="toggleMeas({})">
                        <span>{}</span>
                    </label>
                </div>
                "#,
                idx,
                if *is_selected { "checked" } else { "" },
                idx,
                escape_html(name)
            )
        })
        .collect();

    let values_html: String = if daq_running && !latest_values.is_empty() {
        latest_values
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
    } else {
        r#"<p style="color: #666; text-align: center;">No data (DAQ not running)</p>"#.to_string()
    };

    let content = format!(
        r#"
        <div class="grid">
            <div class="card">
                <h2>Measurements ({})</h2>
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
            function toggleMeas(idx) {{
                const form = document.getElementById('measForm');
                const formData = new FormData(form);
                fetch('/api/daq/toggle', {{
                    method: 'POST',
                    headers: {{'Content-Type': 'application/x-www-form-urlencoded'}},
                    body: 'idx=' + idx
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
        if meas_list.is_empty() {
            r#"<p style="color: #666;">No measurements loaded</p>"#.to_string()
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
