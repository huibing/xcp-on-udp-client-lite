//! XCP on Ethernet (UDP and TCP) transport halves.
//!
//! Both share the 4-byte LEN/COUNTER header ([`codec`]) and differ only in how bytes are
//! delivered: datagrams (`udp`) vs a byte stream (`tcp`).

pub mod codec;
