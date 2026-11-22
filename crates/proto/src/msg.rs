use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Hello {
    Control { token: String, version: u32, client_version: String },
    Data { session: String, conn_id: u64 },
    Udp { session: String, service_id: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Tcp,
    Udp,
}

impl std::fmt::Display for Protocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Protocol::Tcp => "tcp",
            Protocol::Udp => "udp",
        })
    }
}

/// What the client needs to know about a service it serves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceSpec {
    pub id: String,
    pub name: String,
    pub protocol: Protocol,
    /// Address the client forwards to, e.g. `127.0.0.1:8080`.
    pub local_addr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    Welcome { session: String, client_name: String },
    Rejected { reason: String },
    /// Full list of services for this client; sent on connect and on every change.
    Services { services: Vec<ServiceSpec> },
    /// A visitor is waiting: open a `Data` stream for `conn_id`.
    Connect { conn_id: u64, service_id: String },
    Ping,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    Pong,
    ConnectFailed { conn_id: u64, error: String },
}
