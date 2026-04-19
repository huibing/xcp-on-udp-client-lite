mod support;

use std::path::PathBuf;
use std::time::Duration;

use once_cell::sync::Lazy;
use tokio::sync::Mutex;
use xcp_client::XcpClient;
use xcp_client::protocol::Mta;
use xcp_client::transport::TransportType;

use support::mock_slave::{
    build_frame, parse_frame, MockXcpSlaveServer, MOCK_EVENT_CHANNEL, MOCK_EVENT_NAME,
    MOCK_ID_STRING, MOCK_SLAVE_ADDR, PARAM_COUNTER_MAX_ADDR, PARAM_MATRIX_ADDR, PARAM_MATRIX_COLS,
    PARAM_MATRIX_LEN, PARAM_MATRIX_ROWS,
};

static PORT_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn udp_server_speaks_xcp_on_udp_frames() {
    let _guard = PORT_LOCK.lock().await;
    let server = MockXcpSlaveServer::start().await.expect("start mock server");

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
    let server = MockXcpSlaveServer::start().await.expect("start mock server");

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
async fn calibration_pages_chunked_transfers_and_checksum_work() {
    let _guard = PORT_LOCK.lock().await;
    let server = MockXcpSlaveServer::start().await.expect("start mock server");

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
    let server = MockXcpSlaveServer::start().await.expect("start mock server");

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
