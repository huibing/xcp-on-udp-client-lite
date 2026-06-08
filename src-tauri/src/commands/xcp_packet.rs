use std::sync::Arc;
use xcp_client::xcp_packet_log::{XcpPacketEntry, XcpPacketLog};

#[tauri::command]
pub fn xcp_packet_get(
    state: tauri::State<'_, Arc<XcpPacketLog>>,
    since_id: u64,
) -> Vec<XcpPacketEntry> {
    state.entries_since(since_id)
}

#[tauri::command]
pub fn xcp_packet_clear(state: tauri::State<'_, Arc<XcpPacketLog>>) {
    state.clear();
}

#[tauri::command]
pub fn xcp_packet_set_enabled(state: tauri::State<'_, Arc<XcpPacketLog>>, enabled: bool) {
    state.set_enabled(enabled);
}
