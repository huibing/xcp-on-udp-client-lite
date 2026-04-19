use std::io;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;

use tokio::net::UdpSocket;
use tokio::sync::{watch, Mutex};
use tokio::task::JoinHandle;
use xcp_client::protocol::{CommandCode, ErrorCode, Mta};

use super::fixture::{MockFixture, MOCK_EVENT_CHANNEL};
use super::protocol::{build_frame, negative_response, parse_frame, positive_response};
use super::state::MockXcpState;
use super::fixture::MOCK_SLAVE_ADDR;

#[derive(Debug)]
pub struct MockXcpSlaveServer {
    stop_tx: watch::Sender<bool>,
    task: JoinHandle<()>,
}

impl MockXcpSlaveServer {
    pub async fn start() -> io::Result<Self> {
        let socket = Arc::new(UdpSocket::bind(MOCK_SLAVE_ADDR).await?);
        let state = Arc::new(Mutex::new(MockXcpState::new(MockFixture::default())));
        let runtime = Arc::new(RuntimeState::new(socket.clone(), state));
        let (stop_tx, mut stop_rx) = watch::channel(false);

        let task = tokio::spawn(async move {
            let mut buf = [0u8; 2048];
            loop {
                tokio::select! {
                    _ = stop_rx.changed() => {
                        if *stop_rx.borrow() {
                            let _ = runtime.stop_daq_task().await;
                            break;
                        }
                    }
                    recv = socket.recv_from(&mut buf) => {
                        let Ok((len, peer)) = recv else {
                            break;
                        };
                        runtime.set_peer(peer).await;
                        let Ok((_rx_counter, payload)) = parse_frame(&buf[..len]) else {
                            continue;
                        };
                        if let Some(response_payload) = runtime.handle_command(payload).await {
                            let counter = runtime.next_response_counter();
                            let frame = build_frame(&response_payload, counter);
                            let _ = socket.send_to(&frame, peer).await;
                        }
                    }
                }
            }
        });

        Ok(Self { stop_tx, task })
    }

    pub async fn stop(self) -> io::Result<()> {
        let _ = self.stop_tx.send(true);
        self.task
            .await
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))?;
        Ok(())
    }
}

#[derive(Debug)]
struct RuntimeState {
    socket: Arc<UdpSocket>,
    state: Arc<Mutex<MockXcpState>>,
    peer: Arc<Mutex<Option<SocketAddr>>>,
    response_counter: Arc<AtomicU16>,
    daq_task: Mutex<Option<JoinHandle<()>>>,
    daq_stop_tx: Mutex<Option<watch::Sender<bool>>>,
}

impl RuntimeState {
    fn new(socket: Arc<UdpSocket>, state: Arc<Mutex<MockXcpState>>) -> Self {
        Self {
            socket,
            state,
            peer: Arc::new(Mutex::new(None)),
            response_counter: Arc::new(AtomicU16::new(0)),
            daq_task: Mutex::new(None),
            daq_stop_tx: Mutex::new(None),
        }
    }

    fn next_response_counter(&self) -> u16 {
        self.response_counter.fetch_add(1, Ordering::SeqCst)
    }

    async fn set_peer(&self, peer: SocketAddr) {
        *self.peer.lock().await = Some(peer);
    }

    async fn handle_command(&self, payload: Vec<u8>) -> Option<Vec<u8>> {
        let command = *payload.first()?;
        match command {
            x if x == CommandCode::Connect as u8 => Some(self.handle_connect().await),
            x if x == CommandCode::Disconnect as u8 => Some(self.handle_disconnect().await),
            x if x == CommandCode::GetId as u8 => Some(self.handle_get_id().await),
            x if x == CommandCode::SetMta as u8 => Some(self.handle_set_mta(&payload).await),
            x if x == CommandCode::Upload as u8 => Some(self.handle_upload(&payload).await),
            x if x == CommandCode::ShortUpload as u8 => Some(self.handle_short_upload(&payload).await),
            x if x == CommandCode::ShortDownload as u8 => {
                Some(self.handle_short_download(&payload).await)
            }
            x if x == CommandCode::Download as u8 => Some(self.handle_download(&payload).await),
            x if x == CommandCode::DownloadNext as u8 => {
                Some(self.handle_download_next(&payload).await)
            }
            x if x == CommandCode::BuildChecksum as u8 => {
                Some(self.handle_build_checksum(&payload).await)
            }
            x if x == CommandCode::GetCalPage as u8 => Some(self.handle_get_cal_page(&payload).await),
            x if x == CommandCode::SetCalPage as u8 => Some(self.handle_set_cal_page(&payload).await),
            x if x == CommandCode::GetDaqProcessorInfo as u8 => {
                Some(self.handle_get_daq_processor_info().await)
            }
            x if x == CommandCode::GetDaqResolutionInfo as u8 => {
                Some(self.handle_get_daq_resolution_info().await)
            }
            x if x == CommandCode::GetDaqEventInfo as u8 => {
                Some(self.handle_get_daq_event_info(&payload).await)
            }
            x if x == CommandCode::GetDaqClock as u8 => Some(self.handle_get_daq_clock().await),
            x if x == CommandCode::FreeDaq as u8 => Some(self.handle_free_daq().await),
            x if x == CommandCode::AllocDaq as u8 => Some(self.handle_alloc_daq(&payload).await),
            x if x == CommandCode::AllocOdt as u8 => Some(self.handle_alloc_odt(&payload).await),
            x if x == CommandCode::AllocOdtEntry as u8 => {
                Some(self.handle_alloc_odt_entry(&payload).await)
            }
            x if x == CommandCode::SetDaqPtr as u8 => Some(self.handle_set_daq_ptr(&payload).await),
            x if x == CommandCode::WriteDaq as u8 => Some(self.handle_write_daq(&payload).await),
            x if x == CommandCode::SetDaqListMode as u8 => {
                Some(self.handle_set_daq_list_mode(&payload).await)
            }
            x if x == CommandCode::StartStopDaqList as u8 => {
                Some(self.handle_start_stop_daq_list(&payload).await)
            }
            x if x == CommandCode::StartStopSynch as u8 => {
                Some(self.handle_start_stop_synch(&payload).await)
            }
            _ => Some(negative_response(ErrorCode::UnknownCommand, &[])),
        }
    }

    async fn handle_connect(&self) -> Vec<u8> {
        let state = self.state.lock().await;
        positive_response(&[
            state.fixture.connect_resource,
            0,
            state.fixture.max_cto,
            (state.fixture.max_dto & 0xFF) as u8,
            (state.fixture.max_dto >> 8) as u8,
            state.fixture.protocol_version,
            state.fixture.transport_version,
        ])
    }

    async fn handle_disconnect(&self) -> Vec<u8> {
        let _ = self.stop_daq_task().await;
        positive_response(&[])
    }

    async fn handle_get_id(&self) -> Vec<u8> {
        let state = self.state.lock().await;
        let id = state.fixture.id_string.as_bytes();
        let len = id.len() as u32;
        let mut data = vec![1, 0, 0, len as u8, (len >> 8) as u8, (len >> 16) as u8];
        data.extend_from_slice(id);
        positive_response(&data)
    }

    async fn handle_set_mta(&self, payload: &[u8]) -> Vec<u8> {
        if payload.len() < 8 {
            return negative_response(ErrorCode::CommandSyntax, &[]);
        }
        let extension = payload[3];
        let address = u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
        self.state.lock().await.set_mta(Mta::new(address, extension));
        positive_response(&[])
    }

    async fn handle_upload(&self, payload: &[u8]) -> Vec<u8> {
        if payload.len() < 2 {
            return negative_response(ErrorCode::CommandSyntax, &[]);
        }
        match self.state.lock().await.upload(payload[1] as usize) {
            Ok(data) => positive_response(&data),
            Err(_) => negative_response(ErrorCode::AccessDenied, &[]),
        }
    }

    async fn handle_short_upload(&self, payload: &[u8]) -> Vec<u8> {
        if payload.len() < 8 {
            return negative_response(ErrorCode::CommandSyntax, &[]);
        }
        let length = payload[1] as usize;
        let extension = payload[3];
        let address = u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
        match self.state.lock().await.read_memory(Mta::new(address, extension), length) {
            Ok(data) => positive_response(&data),
            Err(_) => negative_response(ErrorCode::AccessDenied, &[]),
        }
    }

    async fn handle_short_download(&self, payload: &[u8]) -> Vec<u8> {
        if payload.len() < 8 {
            return negative_response(ErrorCode::CommandSyntax, &[]);
        }
        let extension = payload[3];
        let address = u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
        match self
            .state
            .lock()
            .await
            .write_memory(Mta::new(address, extension), &payload[8..])
        {
            Ok(()) => positive_response(&[]),
            Err(_) => negative_response(ErrorCode::AccessDenied, &[]),
        }
    }

    async fn handle_download(&self, payload: &[u8]) -> Vec<u8> {
        if payload.len() < 2 {
            return negative_response(ErrorCode::CommandSyntax, &[]);
        }
        let total_size = payload[1] as usize;
        match self
            .state
            .lock()
            .await
            .download_first(total_size, &payload[2..])
        {
            Ok(()) => positive_response(&[]),
            Err(_) => negative_response(ErrorCode::AccessDenied, &[]),
        }
    }

    async fn handle_download_next(&self, payload: &[u8]) -> Vec<u8> {
        if payload.len() < 2 {
            return negative_response(ErrorCode::CommandSyntax, &[]);
        }
        let remaining_after = payload[1] as usize;
        match self
            .state
            .lock()
            .await
            .download_next(remaining_after, &payload[2..])
        {
            Ok(()) => positive_response(&[]),
            Err(_) => negative_response(ErrorCode::AccessDenied, &[]),
        }
    }

    async fn handle_build_checksum(&self, payload: &[u8]) -> Vec<u8> {
        if payload.len() < 8 {
            return negative_response(ErrorCode::CommandSyntax, &[]);
        }
        let block_size = u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
        let state = self.state.lock().await;
        match state.checksum(state.current_mta, block_size) {
            Ok((checksum_type, checksum)) => {
                let mut data = vec![checksum_type, 0, 0];
                data.extend_from_slice(&checksum.to_le_bytes());
                positive_response(&data)
            }
            Err(limit) => {
                let mut params = Vec::with_capacity(6);
                params.extend_from_slice(&1u16.to_le_bytes());
                params.extend_from_slice(&limit.to_le_bytes());
                negative_response(ErrorCode::ParameterOutOfRange, &params)
            }
        }
    }

    async fn handle_get_cal_page(&self, payload: &[u8]) -> Vec<u8> {
        if payload.len() < 3 {
            return negative_response(ErrorCode::CommandSyntax, &[]);
        }
        let segment = payload[2];
        let page = self.state.lock().await.get_cal_page(segment).unwrap_or(0);
        positive_response(&[0, 0, page])
    }

    async fn handle_set_cal_page(&self, payload: &[u8]) -> Vec<u8> {
        if payload.len() < 4 {
            return negative_response(ErrorCode::CommandSyntax, &[]);
        }
        let segment = payload[2];
        let page = payload[3];
        self.state.lock().await.set_cal_page(segment, page);
        positive_response(&[])
    }

    async fn handle_get_daq_processor_info(&self) -> Vec<u8> {
        let state = self.state.lock().await;
        let max_daq = state.fixture.max_daq.to_le_bytes();
        let max_event = state.fixture.max_event_channel.to_le_bytes();
        positive_response(&[
            state.fixture.daq_properties,
            max_daq[0],
            max_daq[1],
            max_event[0],
            max_event[1],
            state.fixture.min_daq,
            state.fixture.daq_key_byte,
        ])
    }

    async fn handle_get_daq_resolution_info(&self) -> Vec<u8> {
        let state = self.state.lock().await;
        let ticks = state.fixture.timestamp_ticks.to_le_bytes();
        positive_response(&[
            1,
            4,
            1,
            4,
            state.fixture.timestamp_mode,
            ticks[0],
            ticks[1],
        ])
    }

    async fn handle_get_daq_event_info(&self, payload: &[u8]) -> Vec<u8> {
        if payload.len() < 4 {
            return negative_response(ErrorCode::CommandSyntax, &[]);
        }
        let channel = u16::from_le_bytes([payload[2], payload[3]]);
        if channel != MOCK_EVENT_CHANNEL {
            return negative_response(ErrorCode::ParameterOutOfRange, &[]);
        }
        let mut state = self.state.lock().await;
        let event_name = state.fixture.event_name.clone().into_bytes();
        let name_len = event_name.len() as u8;
        state.prepare_upload_buffer(event_name);
        positive_response(&[0, 0xFF, name_len, 1, 6, 0])
    }

    async fn handle_get_daq_clock(&self) -> Vec<u8> {
        let state = self.state.lock().await;
        let clock = state.current_clock().to_le_bytes();
        positive_response(&[0, 0, 0, clock[0], clock[1], clock[2], clock[3]])
    }

    async fn handle_free_daq(&self) -> Vec<u8> {
        self.state.lock().await.free_daq();
        let _ = self.stop_daq_task().await;
        positive_response(&[])
    }

    async fn handle_alloc_daq(&self, payload: &[u8]) -> Vec<u8> {
        if payload.len() < 4 {
            return negative_response(ErrorCode::CommandSyntax, &[]);
        }
        let daq_count = u16::from_le_bytes([payload[2], payload[3]]);
        self.state.lock().await.alloc_daq(daq_count);
        positive_response(&[])
    }

    async fn handle_alloc_odt(&self, payload: &[u8]) -> Vec<u8> {
        if payload.len() < 5 {
            return negative_response(ErrorCode::CommandSyntax, &[]);
        }
        let daq_list = u16::from_le_bytes([payload[2], payload[3]]);
        let odt_count = payload[4];
        self.state.lock().await.alloc_odt(daq_list, odt_count);
        positive_response(&[])
    }

    async fn handle_alloc_odt_entry(&self, payload: &[u8]) -> Vec<u8> {
        if payload.len() < 6 {
            return negative_response(ErrorCode::CommandSyntax, &[]);
        }
        let daq_list = u16::from_le_bytes([payload[2], payload[3]]);
        let odt = payload[4];
        let entry_count = payload[5];
        self.state
            .lock()
            .await
            .alloc_odt_entry(daq_list, odt, entry_count);
        positive_response(&[])
    }

    async fn handle_set_daq_ptr(&self, payload: &[u8]) -> Vec<u8> {
        if payload.len() < 6 {
            return negative_response(ErrorCode::CommandSyntax, &[]);
        }
        let daq_list = u16::from_le_bytes([payload[2], payload[3]]);
        let odt = payload[4];
        let entry = payload[5];
        self.state.lock().await.set_daq_ptr(daq_list, odt, entry);
        positive_response(&[])
    }

    async fn handle_write_daq(&self, payload: &[u8]) -> Vec<u8> {
        if payload.len() < 8 {
            return negative_response(ErrorCode::CommandSyntax, &[]);
        }
        let size = payload[2];
        let extension = payload[3];
        let address = u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
        self.state
            .lock()
            .await
            .write_daq(Mta::new(address, extension), size);
        positive_response(&[])
    }

    async fn handle_set_daq_list_mode(&self, payload: &[u8]) -> Vec<u8> {
        if payload.len() < 8 {
            return negative_response(ErrorCode::CommandSyntax, &[]);
        }
        let mode = payload[1];
        let daq_list = u16::from_le_bytes([payload[2], payload[3]]);
        let event_channel = u16::from_le_bytes([payload[4], payload[5]]);
        let prescaler = payload[6];
        let priority = payload[7];
        self.state
            .lock()
            .await
            .set_daq_list_mode(daq_list, mode, event_channel, prescaler, priority);
        positive_response(&[])
    }

    async fn handle_start_stop_daq_list(&self, payload: &[u8]) -> Vec<u8> {
        if payload.len() < 4 {
            return negative_response(ErrorCode::CommandSyntax, &[]);
        }
        let mode = payload[1];
        let daq_list = u16::from_le_bytes([payload[2], payload[3]]);
        self.state.lock().await.start_stop_daq_list(mode, daq_list);
        positive_response(&[])
    }

    async fn handle_start_stop_synch(&self, payload: &[u8]) -> Vec<u8> {
        if payload.len() < 2 {
            return negative_response(ErrorCode::CommandSyntax, &[]);
        }
        let mode = payload[1];
        self.state.lock().await.start_stop_synch(mode);
        if mode == 0 {
            let _ = self.stop_daq_task().await;
        } else {
            let _ = self.start_daq_task().await;
        }
        positive_response(&[])
    }

    async fn start_daq_task(&self) -> io::Result<()> {
        if self.daq_task.lock().await.is_some() {
            return Ok(());
        }
        let socket = self.socket.clone();
        let state = self.state.clone();
        let peer = self.peer.clone();
        let response_counter = self.response_counter.clone();
        let (stop_tx, mut stop_rx) = watch::channel(false);
        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(20));
            loop {
                tokio::select! {
                    _ = stop_rx.changed() => {
                        if *stop_rx.borrow() {
                            break;
                        }
                    }
                    _ = interval.tick() => {
                        let target = { *peer.lock().await };
                        let Some(target) = target else {
                            continue;
                        };
                        let payload = { state.lock().await.next_dto_payload() };
                        let Some(payload) = payload else {
                            continue;
                        };
                        let counter = response_counter.fetch_add(1, Ordering::SeqCst);
                        let frame = build_frame(&payload, counter);
                        let _ = socket.send_to(&frame, target).await;
                    }
                }
            }
        });

        *self.daq_stop_tx.lock().await = Some(stop_tx);
        *self.daq_task.lock().await = Some(handle);
        Ok(())
    }

    async fn stop_daq_task(&self) -> io::Result<()> {
        if let Some(stop_tx) = self.daq_stop_tx.lock().await.take() {
            let _ = stop_tx.send(true);
        }
        if let Some(handle) = self.daq_task.lock().await.take() {
            handle
                .await
                .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))?;
        }
        Ok(())
    }
}
