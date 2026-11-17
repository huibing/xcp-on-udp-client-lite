//! Transport factory: turns a [`TransportConfig`] into the two halves of a link.

use super::can::{self, CanConfig};
use super::config::TransportConfig;
use super::error::TransportError;
use super::ethernet;
use super::link::TransportLink;

/// Creates transport links from configuration.
pub struct TransportFactory;

impl TransportFactory {
    /// Open a link and return its send and receive halves as independent objects.
    pub async fn open(config: &TransportConfig) -> Result<TransportLink, TransportError> {
        match config {
            TransportConfig::Udp { addr } => ethernet::udp::open(addr).await,
            TransportConfig::Tcp { addr } => ethernet::tcp::open(addr).await,
            TransportConfig::Can {
                driver,
                channel,
                device_id,
                controller,
                can_id,
                baudrate,
                data_baudrate,
                slave_id,
                daq_ids,
                max_dlc_required,
            } => {
                let can_config = CanConfig {
                    driver: driver.clone(),
                    channel: channel.clone(),
                    device_id: device_id.clone(),
                    controller: *controller,
                    can_id: *can_id,
                    baudrate: *baudrate,
                    data_baudrate: *data_baudrate,
                    slave_id: *slave_id,
                    daq_ids: daq_ids.clone(),
                    max_dlc_required: *max_dlc_required,
                };
                can::open(&can_config).await
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::TransportType;

    #[tokio::test]
    async fn test_open_returns_two_independent_halves() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (_socket, _) = listener.accept().await.unwrap();
        });

        let config = TransportConfig::Tcp {
            addr: addr.to_string(),
        };

        let link = TransportFactory::open(&config).await.expect("tcp link");
        assert_eq!(link.sender.transport_type(), TransportType::Tcp);
        assert_eq!(link.sender.target_addr(), addr.to_string());

        // The receive half is a separate value: move it out, keep using the sender.
        let TransportLink { sender, receiver } = link;
        drop(receiver);
        sender.close().await.unwrap();
        server.await.unwrap();
    }
}