//! Wire protocol shared by the tunnel server and client.
//!
//! Every connection from a client to the server is a TLS stream that starts with
//! one length-prefixed JSON [`Hello`] frame, which decides what the stream is:
//!
//! * `Control` — long-lived session; then JSON [`ServerMsg`] / [`ClientMsg`] frames.
//! * `Data`    — one TCP visitor connection; raw bytes follow the hello.
//! * `Udp`     — the datagram channel of one UDP service; binary [`udp`] frames follow.

pub mod frame;
pub mod msg;
pub mod pump;
pub mod tls;
pub mod udp;

pub use msg::*;

/// Bumped on incompatible wire changes.
pub const PROTOCOL_VERSION: u32 = 1;

/// Server pings the client this often; either side gives up after `IDLE_TIMEOUT`.
pub const PING_INTERVAL: std::time::Duration = std::time::Duration::from_secs(15);
pub const IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(45);

/// SHA-256 hex digest — used for token hashes and certificate fingerprints.
pub fn sha256_hex(data: &[u8]) -> String {
    use sha2::Digest;
    hex::encode(sha2::Sha256::digest(data))
}
