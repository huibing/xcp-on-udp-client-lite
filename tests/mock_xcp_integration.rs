mod support;

use std::path::PathBuf;
use std::time::Duration;

use once_cell::sync::Lazy;
use tokio::sync::Mutex;
use xcp_client::protocol::Mta;
use xcp_client::transport::TransportType;
use xcp_client::XcpClient;

use support::mock_slave::{
    build_frame, parse_frame, MockXcpSlaveServer, MOCK_EVENT_CHANNEL, MOCK_EVENT_NAME,
    MOCK_ID_STRING, MOCK_SLAVE_ADDR, PARAM_COUNTER_MAX_ADDR, PARAM_MATRIX_ADDR, PARAM_MATRIX_COLS,
    PARAM_MATRIX_LEN, PARAM_MATRIX_ROWS,
};

static PORT_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn udp_server_speaks_xcp_on_udp_frames() {
    let _guard = PORT_LOCK.lock().await;
    let server = MockXcpSlaveServer::start()
        .await
        .expect("start mock server");

    let socket = tokio::net::UdpSocket::bind("127.0.0.1:0")
        .await
        .expect("bind client");
    socket
        .connect(MOCK_SLAVE_ADDR)
        .await
        .expect("connect client socket");

    let connect_cmd = vec![0xFF, 0x00];
    let frame = build_frame(&connect_cmd, 41);
    socket.send(&frame).await.expect("send connect frame");

    let mut buf = [0u8; 128];
    let len = socket.recv(&mut buf).await.expect("receive response");
    let (counter, payload) = parse_frame(&buf[..len]).expect("parse response frame");
    assert_eq!(counter, 0);
    assert_eq!(payload[0], 0xFF);

    server.stop().await.expect("stop mock server");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn client_connects_and_reads_identity() {
    let _guard = PORT_LOCK.lock().await;
    let server = MockXcpSlaveServer::start()
        .await
        .expect("start mock server");

    let mut client = XcpClient::new();
    let slave_id = client
        .connect_slave_udp("mock", MOCK_SLAVE_ADDR)
        .await
        .expect("connect slave");
    let slave = client.get_slave(slave_id).expect("get slave");
    let slave = slave.lock().await;

    assert!(slave.is_connected());
    assert_eq!(slave.transport_type(), TransportType::Udp);
    assert_eq!(slave.get_id(0).await.expect("get id"), MOCK_ID_STRING);

    drop(slave);
    client.disconnect_all().await.expect("disconnect");
    server.stop().await.expect("stop mock server");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn send_command_and_wait_skips_unsolicited_ev_and_serv_packets() {
    // A slave may send asynchronous EV (0xFD) / SERV (0xFC) packets while a
    // command is in flight. Those land in the router's CTO channel but are
    // NOT responses — send_command_and_wait must skip them and still return
    // the real RES for the command (otherwise the RES would be misattributed
    // to the NEXT command).
    let _guard = PORT_LOCK.lock().await;
    let server = MockXcpSlaveServer::start()
        .await
        .expect("start mock server");

    let mut client = XcpClient::new();
    let slave_id = client
        .connect_slave_udp("mock", MOCK_SLAVE_ADDR)
        .await
        .expect("connect slave");
    let slave = client.get_slave(slave_id).expect("get slave");
    let slave = slave.lock().await;

    // Queue an EV packet (PID 0xFD: daq_list u16 + odt u8 + data) and a SERV
    // packet (PID 0xFC) before issuing the command, so both are already in
    // the CTO channel ahead of the response.
    server
        .send_unsolicited(&[0xFD, 0x00, 0x00, 0x00, 0xAA])
        .await
        .expect("send EV");
    server
        .send_unsolicited(&[0xFC, 0x01])
        .await
        .expect("send SERV");
    // Give the client's router a moment to route both into the CTO channel.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // The command must still succeed and return the real GET_ID response.
    assert_eq!(
        slave.get_id(0).await.expect("get id despite EV/SERV"),
        MOCK_ID_STRING
    );

    // And the next command must not receive the stale GET_ID response
    // misattributed from the previous exchange.
    assert_eq!(slave.get_id(0).await.expect("next get id"), MOCK_ID_STRING);

    drop(slave);
    client.disconnect_all().await.expect("disconnect");
    server.stop().await.expect("stop mock server");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn calibration_pages_chunked_transfers_and_checksum_work() {
    let _guard = PORT_LOCK.lock().await;
    let server = MockXcpSlaveServer::start()
        .await
        .expect("start mock server");

    let mut client = XcpClient::new();
    let slave_id = client
        .connect_slave_udp("mock", MOCK_SLAVE_ADDR)
        .await
        .expect("connect slave");
    client
        .load_a2l_for_slave(slave_id, "hello_xcp.a2l")
        .await
        .expect("load a2l");

    let slave = client.get_slave(slave_id).expect("get slave");
    let mut slave = slave.lock().await;

    assert_eq!(slave.get_cal_page(1, 0x02).await.expect("get page"), 0);

    let page_zero = slave
        .read_memory(Mta::new(PARAM_COUNTER_MAX_ADDR, 0), 4)
        .await
        .expect("read page zero");
    assert_eq!(u32::from_le_bytes(page_zero.try_into().unwrap()), 100);

    slave
        .set_cal_page(1, 1, 0x03)
        .await
        .expect("switch to page one");
    assert_eq!(slave.get_cal_page(1, 0x02).await.expect("get page one"), 1);

    let page_one = slave
        .read_memory(Mta::new(PARAM_COUNTER_MAX_ADDR, 0), 4)
        .await
        .expect("read page one");
    assert_eq!(u32::from_le_bytes(page_one.try_into().unwrap()), 42);

    let matrix: Vec<Vec<u8>> = (0..PARAM_MATRIX_ROWS)
        .map(|row| {
            (0..PARAM_MATRIX_COLS)
                .map(|col| (row * PARAM_MATRIX_COLS + col) as u8 + 10)
                .collect()
        })
        .collect();

    let raw = slave
        .read_u8_array(Mta::new(PARAM_MATRIX_ADDR, 0), PARAM_MATRIX_LEN)
        .await
        .expect("read matrix before write");
    assert!(raw.iter().all(|byte| *byte >= 200));

    let expected: Vec<u8> = matrix.into_iter().flatten().collect();
    slave
        .write_u8_array(Mta::new(PARAM_MATRIX_ADDR, 0), &expected)
        .await
        .expect("write matrix");
    let raw = slave
        .read_u8_array(Mta::new(PARAM_MATRIX_ADDR, 0), PARAM_MATRIX_LEN)
        .await
        .expect("read matrix");
    assert_eq!(raw, expected);

    let (checksum_type, checksum) = slave
        .build_checksum(Mta::new(PARAM_MATRIX_ADDR, 0), PARAM_MATRIX_LEN as u32)
        .await
        .expect("checksum matrix");
    let expected_checksum = expected
        .iter()
        .fold(0u32, |acc, value| acc.wrapping_add(*value as u32));
    assert_eq!(checksum_type, 0x01);
    assert_eq!(checksum, expected_checksum);

    drop(slave);
    client.disconnect_all().await.expect("disconnect");
    server.stop().await.expect("stop mock server");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn daq_streaming_reaches_latest_sample_and_event_metadata() {
    let _guard = PORT_LOCK.lock().await;
    let server = MockXcpSlaveServer::start()
        .await
        .expect("start mock server");

    let csv_path: PathBuf =
        std::env::temp_dir().join(format!("mock-xcp-{}.csv", std::process::id()));

    let mut client = XcpClient::new();
    let slave_id = client
        .connect_slave_udp("mock", MOCK_SLAVE_ADDR)
        .await
        .expect("connect slave");
    client
        .load_a2l_for_slave(slave_id, "hello_xcp.a2l")
        .await
        .expect("load a2l");

    let slave = client.get_slave(slave_id).expect("get slave");
    let slave = slave.lock().await;

    let daq_info = slave
        .get_daq_processor_info()
        .await
        .expect("daq processor info");
    assert_eq!(daq_info.max_event_channel, 1);

    let events = slave
        .get_daq_events(daq_info.max_event_channel)
        .await
        .expect("daq events");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].channel, MOCK_EVENT_CHANNEL);
    assert_eq!(events[0].name, MOCK_EVENT_NAME);

    {
        let mut daq = slave.daq_manager.lock().await;
        daq.set_available_events(events);
        daq.set_csv_enabled(false);
        daq.set_mf4_enabled(false);
        daq.set_csv_path(&csv_path);
        daq.start(&slave, 10).await.expect("start daq");
    }

    tokio::time::sleep(Duration::from_millis(150)).await;

    let latest = {
        let daq = slave.daq_manager.lock().await;
        let latest = daq.get_latest_sample().await.expect("latest sample");
        daq.stop(&slave).await.expect("stop daq");
        latest
    };

    assert!(latest.timestamp_s >= 0.0);
    assert!(latest.get_value("counter").is_some());
    assert_eq!(latest.raw_values["counter"].len(), 4);

    drop(slave);
    client.disconnect_all().await.expect("disconnect");
    server.stop().await.expect("stop mock server");
}

/// The `daq-print` CLI relies on `DaqManager` live snapshots: per-DAQ-list
/// values merged across ODTs plus a sample counter that decimation uses.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn daq_live_snapshots_track_values_and_sample_count() {
    let _guard = PORT_LOCK.lock().await;
    let server = MockXcpSlaveServer::start()
        .await
        .expect("start mock server");

    let mut client = XcpClient::new();
    let slave_id = client
        .connect_slave_udp("mock", MOCK_SLAVE_ADDR)
        .await
        .expect("connect slave");
    client
        .load_a2l_for_slave(slave_id, "hello_xcp.a2l")
        .await
        .expect("load a2l");

    let slave = client.get_slave(slave_id).expect("get slave");
    let slave = slave.lock().await;

    let events = slave.get_daq_events(1).await.expect("daq events");

    {
        let mut daq = slave.daq_manager.lock().await;
        daq.set_available_events(events);
        daq.set_csv_enabled(false);
        daq.set_mf4_enabled(false);
        daq.set_live_snapshots_enabled(true);
        daq.start(&slave, 10).await.expect("start daq");
    }

    // Wait until a couple of samples have been accumulated.
    let mut snapshots = std::collections::HashMap::new();
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(20)).await;
        snapshots = slave.daq_manager.lock().await.live_snapshots().await;
        if snapshots.values().any(|snapshot| snapshot.samples >= 2) {
            break;
        }
    }

    {
        let daq = slave.daq_manager.lock().await;
        daq.stop(&slave).await.expect("stop daq");
    }

    let snapshot = snapshots
        .values()
        .max_by_key(|snapshot| snapshot.samples)
        .expect("at least one live snapshot");
    assert!(
        snapshot.samples >= 2,
        "expected multiple samples, got {}",
        snapshot.samples
    );
    assert!(
        snapshot.values.contains_key("counter"),
        "snapshot values: {:?}",
        snapshot.values.keys().collect::<Vec<_>>()
    );
    assert!(snapshot.timestamp_s >= 0.0);

    drop(slave);
    client.disconnect_all().await.expect("disconnect");
    server.stop().await.expect("stop mock server");
}

/// End-to-end smoke test of `daq-print`: the CLI must print header lines plus
/// at least one data line carrying a live measurement value to stdout.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn daq_print_cli_prints_live_values_to_stdout() {
    let _guard = PORT_LOCK.lock().await;
    let server = MockXcpSlaveServer::start()
        .await
        .expect("start mock server");

    let dir = std::env::temp_dir().join(format!("xcp-daq-print-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let config_path = dir.join("daq_print.toml");
    std::fs::write(
        &config_path,
        format!(
            r#"
a2l = "hello_xcp.a2l"

[connection]
addr = "{MOCK_SLAVE_ADDR}"

[print]
decimation = 1
refresh_ms = 10

[[events]]
channel = 0
measurements = ["counter"]
"#
        ),
    )
    .expect("write config");

    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_xcp_client"))
        .arg("daq-print")
        .arg("--config")
        .arg(&config_path)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("spawn daq-print");

    // Give the CLI time to connect, start DAQ and print a few samples.
    tokio::time::sleep(Duration::from_millis(1500)).await;

    child.kill().await.expect("kill daq-print");
    let output = child.wait_with_output().await.expect("collect output");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("# xcp-client daq-print"),
        "missing header in stdout: {}",
        stdout
    );
    let data_line = stdout
        .lines()
        .find(|line| !line.starts_with('#'))
        .unwrap_or("");
    assert!(
        data_line.contains("counter="),
        "no data line with 'counter=' in stdout: {}",
        stdout
    );
    assert!(
        data_line.contains("ev0"),
        "data line is not tagged with the event: {}",
        data_line
    );

    let _ = std::fs::remove_dir_all(&dir);
    server.stop().await.expect("stop mock server");
}

/// `single-line` mode must fall back to appended lines when stdout is not a
/// terminal (piped), so captures never contain raw ANSI escape sequences.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn daq_print_cli_single_line_mode_falls_back_when_piped() {
    let _guard = PORT_LOCK.lock().await;
    let server = MockXcpSlaveServer::start()
        .await
        .expect("start mock server");

    let dir = std::env::temp_dir().join(format!("xcp-daq-print-inline-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let config_path = dir.join("daq_print_inline.toml");
    std::fs::write(
        &config_path,
        format!(
            r#"
a2l = "hello_xcp.a2l"

[connection]
addr = "{MOCK_SLAVE_ADDR}"

[print]
mode = "single-line"
decimation = 1
refresh_ms = 10

[[events]]
channel = 0
measurements = ["counter"]
"#
        ),
    )
    .expect("write config");

    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_xcp_client"))
        .arg("daq-print")
        .arg("--config")
        .arg(&config_path)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("spawn daq-print");

    tokio::time::sleep(Duration::from_millis(1500)).await;

    child.kill().await.expect("kill daq-print");
    let output = child.wait_with_output().await.expect("collect output");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("mode line"),
        "header should report the fallback mode: {}",
        stdout
    );
    assert!(
        !stdout.contains('\u{1b}'),
        "piped output must not contain ANSI escapes: {:?}",
        stdout
    );
    let data_lines = stdout.lines().filter(|line| !line.starts_with('#')).count();
    assert!(
        data_lines > 1,
        "expected appended data lines, got {} in: {}",
        data_lines,
        stdout
    );

    let _ = std::fs::remove_dir_all(&dir);
    server.stop().await.expect("stop mock server");
}

/// `daq-record` end-to-end: records to CSV, quits on "q" + Enter, exits cleanly.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn daq_record_cli_writes_csv_and_exits_on_q() {
    let _guard = PORT_LOCK.lock().await;
    let server = MockXcpSlaveServer::start()
        .await
        .expect("start mock server");

    let dir = std::env::temp_dir().join(format!("xcp-daq-record-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let config_path = dir.join("daq_record.toml");
    let csv_path = dir.join("recording.csv");
    std::fs::write(
        &config_path,
        format!(
            r#"
a2l = "hello_xcp.a2l"

[connection]
addr = "{MOCK_SLAVE_ADDR}"

[output]
csv = '{}'

[[events]]
channel = 0
measurements = ["counter"]
"#,
            csv_path.display()
        ),
    )
    .expect("write config");

    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_xcp_client"))
        .arg("daq-record")
        .arg("--config")
        .arg(&config_path)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .expect("spawn daq-record");

    // Let it record for a moment, then quit via the stdin shortcut.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        stdin.write_all(b"q\n").await.expect("write q");
        // The child may already be exiting, so a failed flush is fine.
        let _ = stdin.flush().await;
        // Keep the pipe open while waiting so the child never sees stdin EOF.
        let _keep = stdin;
        let output = tokio::time::timeout(Duration::from_secs(15), child.wait_with_output())
            .await
            .expect("daq-record did not exit after q")
            .expect("wait");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert_eq!(output.status.code(), Some(0), "stdout: {}", stdout);
        assert!(
            stdout.contains("q received. Shutting down"),
            "stdout: {}",
            stdout
        );
        assert!(stdout.contains("Recording Summary"), "stdout: {}", stdout);
    }

    let csv = std::fs::read_to_string(&csv_path).expect("csv exists");
    let lines: Vec<&str> = csv.lines().collect();
    assert!(lines.len() >= 3, "expected header + rows, got: {:?}", lines);
    assert!(lines[0].starts_with("time_s"), "header: {}", lines[0]);
    assert!(lines[0].contains("counter"), "header: {}", lines[0]);

    let _ = std::fs::remove_dir_all(&dir);
    server.stop().await.expect("stop mock server");
}

/// `daq-record` multi-slave: when one slave fails to start, the slaves that are
/// already recording are shut down cleanly and the process reports the error.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn daq_record_cli_rolls_back_started_slaves_on_error() {
    let _guard = PORT_LOCK.lock().await;
    let server = MockXcpSlaveServer::start()
        .await
        .expect("start mock server");

    let dir = std::env::temp_dir().join(format!("xcp-daq-record-multi-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let config_path = dir.join("daq_record_multi.toml");
    std::fs::write(
        &config_path,
        format!(
            r#"
[output]
csv = '{}'

[[slaves]]
name = "ok"
a2l = "hello_xcp.a2l"

[slaves.connection]
addr = "{MOCK_SLAVE_ADDR}"

[[slaves.events]]
channel = 0
measurements = ["counter"]

[[slaves]]
name = "broken"
a2l = "this_a2l_does_not_exist.a2l"

[slaves.connection]
addr = "{MOCK_SLAVE_ADDR}"
"#,
            dir.join("{name}.csv").display()
        ),
    )
    .expect("write config");

    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_xcp_client"))
        .arg("daq-record")
        .arg("--config")
        .arg(&config_path)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .output()
        .await
        .expect("run daq-record");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_ne!(output.status.code(), Some(0), "stdout: {}", stdout);
    assert!(
        stderr.contains("this_a2l_does_not_exist.a2l"),
        "stderr: {}",
        stderr
    );
    // The first slave was recording and must have been rolled back.
    assert!(stdout.contains("[ok] recording"), "stdout: {}", stdout);
    assert!(
        stdout.contains("[ok] stopping recording"),
        "stdout: {}",
        stdout
    );
    assert!(stdout.contains("[ok] disconnecting"), "stdout: {}", stdout);

    let _ = std::fs::remove_dir_all(&dir);
    server.stop().await.expect("stop mock server");
}
