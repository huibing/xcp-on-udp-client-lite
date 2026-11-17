//! XCP transport layer.
//!
//! A link is created as **two independent objects** ([`TransportLink`]): a
//! [`TransportSender`] (shared, `&self`, `Sync`) and a [`TransportReceiver`] (owned by exactly one
//! consumer, `&mut self`, no `Sync`). The receive half is handed to the [`router`], which
//! classifies packets by PID and forwards them over two independent channels.
//!
//! Layout:
//! - [`link`] — the two traits, [`ReceivedPacket`], [`TransportLink`]
//! - [`error`] — [`TransportError`]
//! - [`config`] — [`TransportConfig`] / [`TransportType`]
//! - [`factory`] — [`TransportFactory::open`]
//! - [`router`] — PID classification, [`CtoChannel`] / [`DtoChannel`]
//! - `ethernet` — UDP and TCP halves sharing one frame codec
//! - `can` — XCP-on-CAN halves on top of a [`CanDevice`]

mod can;
mod config;
mod error;
mod ethernet;
mod factory;
mod link;
mod resolve;
mod router;

pub use can::es582;
pub use can::invalidate_hardware_cache;
pub use can::{
    driver_names, drivers, find_driver, list_can_devices, CanConfig, CanDevice, CanDeviceConfig,
    CanDeviceError, CanDeviceInfo, CanDriver, CanFrame,
};
pub use config::{TransportConfig, TransportType};
pub use error::TransportError;
pub use factory::TransportFactory;
pub use link::{ReceivedPacket, TransportLink, TransportReceiver, TransportSender};
pub use resolve::{
    parse_transport_type, resolve_transport_config, transport_display, TransportOverrides,
};
pub use router::{
    spawn_router, ChannelError, CtoChannel, DtoChannel, PacketType, RouterHandle, XcpMessage,
};