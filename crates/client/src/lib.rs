//! Tunnel client: keeps a control session to the server and forwards visitor
//! traffic to local services. All configuration (which services, which local
//! addresses) comes from the server, so the client only needs a token.

use anyhow::{Context, Result, bail};
use proto::frame::{read_json, write_json};
use proto::{ClientMsg, Hello, PROTOCOL_VERSION, Protocol, ServerMsg, ServiceSpec};
use rustls_pki_types::ServerName;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinHandle;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;
use tracing::{debug, info, warn};

const UDP_IDLE: Duration = Duration::from_secs(120);
const DIAL_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub struct Config {
    /// `host:port` of the server's control port.
    pub server: String,
    /// TLS server name; defaults to the host part of `server`.
    pub server_name: Option<String>,
    pub token: String,
    pub tls: Arc<rustls::ClientConfig>,
}

#[derive(Clone)]
struct Conn {
    server: String,
    name: ServerName<'static>,
    connector: TlsConnector,
}

impl Conn {
    async fn open(&self) -> Result<TlsStream<TcpStream>> {
        let tcp = tokio::time::timeout(DIAL_TIMEOUT, TcpStream::connect(&self.server))
            .await
            .context("connect timeout")?
            .with_context(|| format!("cannot reach {}", self.server))?;
        tcp.set_nodelay(true)?;
        self.connector.connect(self.name.clone(), tcp).await.context("TLS handshake failed")
    }
}

/// Runs forever, reconnecting with exponential backoff.
pub async fn run(cfg: Config) -> ! {
    let mut backoff = Duration::from_secs(1);
    loop {
        let started = Instant::now();
        match session(&cfg).await {
            Ok(()) => {}
            Err(e) => warn!("session ended: {e:#}"),
        }
        if started.elapsed() > Duration::from_secs(60) {
            backoff = Duration::from_secs(1);
        }
        info!("reconnecting in {}s", backoff.as_secs());
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(30));
    }
}

fn server_name(cfg: &Config) -> Result<ServerName<'static>> {
    let host = match &cfg.server_name {
        Some(n) => n.clone(),
        None => {
            let host = cfg.server.rsplit_once(':').map_or(cfg.server.as_str(), |(h, _)| h);
            host.trim_start_matches('[').trim_end_matches(']').to_owned()
        }
    };
    ServerName::try_from(host).context("invalid server name")
}

async fn session(cfg: &Config) -> Result<()> {
    let conn = Conn {
        server: cfg.server.clone(),
        name: server_name(cfg)?,
        connector: TlsConnector::from(cfg.tls.clone()),
    };
    let mut stream = conn.open().await?;
    write_json(
        &mut stream,
        &Hello::Control {
            token: cfg.token.clone(),
            version: PROTOCOL_VERSION,
            client_version: env!("CARGO_PKG_VERSION").to_owned(),
        },
    )
    .await?;

    let key = match read_json::<ServerMsg, _>(&mut stream).await? {
        ServerMsg::Welcome { session, client_name } => {
            info!("connected to {} as \"{client_name}\"", cfg.server);
            session
        }
        ServerMsg::Rejected { reason } => bail!("server rejected us: {reason}"),
        other => bail!("unexpected message {other:?}"),
    };

    let (mut reader, writer) = tokio::io::split(stream);
    let writer = Arc::new(Mutex::new(writer));
    let mut services: HashMap<String, ServiceSpec> = HashMap::new();
    let mut udp_tasks: HashMap<String, (ServiceSpec, JoinHandle<()>)> = HashMap::new();

    let result = async {
        loop {
            let msg = tokio::time::timeout(proto::IDLE_TIMEOUT, read_json::<ServerMsg, _>(&mut reader))
                .await
                .context("server timed out")??;
            match msg {
                ServerMsg::Ping => write_json(&mut *writer.lock().await, &ClientMsg::Pong).await?,
                ServerMsg::Services { services: list } => {
                    services = list.into_iter().map(|s| (s.id.clone(), s)).collect();
                    let names: Vec<_> = services.values().map(|s| format!("{} ({})", s.name, s.protocol)).collect();
                    info!("services: [{}]", names.join(", "));
                    sync_udp(&mut udp_tasks, &services, &conn, &key);
                }
                ServerMsg::Connect { conn_id, service_id } => {
                    let Some(spec) = services.get(&service_id).cloned() else {
                        continue;
                    };
                    let (conn, key, writer) = (conn.clone(), key.clone(), writer.clone());
                    tokio::spawn(async move {
                        if let Err(e) = forward_tcp(&conn, &key, conn_id, &spec).await {
                            debug!(service = %spec.name, "forward failed: {e:#}");
                            let msg = ClientMsg::ConnectFailed { conn_id, error: format!("{e:#}") };
                            let _ = write_json(&mut *writer.lock().await, &msg).await;
                        }
                    });
                }
                ServerMsg::Rejected { reason } => bail!("server rejected us: {reason}"),
                ServerMsg::Welcome { .. } => {}
            }
        }
    }
    .await;

    for (_, (_, task)) in udp_tasks {
        task.abort();
    }
    result
}

async fn forward_tcp(conn: &Conn, key: &str, conn_id: u64, spec: &ServiceSpec) -> Result<()> {
    let local = tokio::time::timeout(DIAL_TIMEOUT, TcpStream::connect(&spec.local_addr))
        .await
        .context("local connect timeout")?
        .with_context(|| format!("cannot reach local service {}", spec.local_addr))?;
    local.set_nodelay(true)?;
    let mut data = conn.open().await?;
    write_json(&mut data, &Hello::Data { session: key.to_owned(), conn_id }).await?;
    proto::pump::pump(data, local, None).await?;
    Ok(())
}

fn sync_udp(
    tasks: &mut HashMap<String, (ServiceSpec, JoinHandle<()>)>,
    services: &HashMap<String, ServiceSpec>,
    conn: &Conn,
    key: &str,
) {
    tasks.retain(|id, (spec, task)| {
        let keep = services.get(id) == Some(spec);
        if !keep {
            task.abort();
        }
        keep
    });
    for spec in services.values().filter(|s| s.protocol == Protocol::Udp) {
        if tasks.contains_key(&spec.id) {
            continue;
        }
        let (conn, key, s) = (conn.clone(), key.to_owned(), spec.clone());
        let task = tokio::spawn(async move {
            loop {
                if let Err(e) = udp_channel(&conn, &key, &s).await {
                    debug!(service = %s.name, "udp channel closed: {e:#}");
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        });
        tasks.insert(spec.id.clone(), (spec.clone(), task));
    }
}

/// One stream carries all datagrams of a UDP service. Each remote peer gets its
/// own local socket so replies can be routed back to it.
async fn udp_channel(conn: &Conn, key: &str, spec: &ServiceSpec) -> Result<()> {
    let local_addr: SocketAddr = tokio::net::lookup_host(&spec.local_addr)
        .await?
        .next()
        .with_context(|| format!("cannot resolve {}", spec.local_addr))?;
    let mut stream = conn.open().await?;
    write_json(&mut stream, &Hello::Udp { session: key.to_owned(), service_id: spec.id.clone() }).await?;
    let (mut reader, mut writer) = tokio::io::split(stream);

    let (back_tx, mut back_rx) = mpsc::channel::<(SocketAddr, Vec<u8>)>(1024);
    let writer_task = tokio::spawn(async move {
        while let Some((peer, data)) = back_rx.recv().await {
            if proto::udp::write(&mut writer, peer, &data).await.is_err() {
                break;
            }
        }
    });

    struct Peer {
        socket: Arc<UdpSocket>,
        task: JoinHandle<()>,
        last: Arc<std::sync::Mutex<Instant>>,
    }
    let mut peers: HashMap<SocketAddr, Peer> = HashMap::new();
    let mut gc = tokio::time::interval(Duration::from_secs(30));

    let result: Result<()> = async {
        loop {
            tokio::select! {
                frame = proto::udp::read(&mut reader) => {
                    let (peer, data) = frame?;
                    if !peers.contains_key(&peer) {
                        let bind: SocketAddr = if local_addr.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" }.parse()?;
                        let socket = Arc::new(UdpSocket::bind(bind).await?);
                        socket.connect(local_addr).await?;
                        let last = Arc::new(std::sync::Mutex::new(Instant::now()));
                        let (sock, tx, l) = (socket.clone(), back_tx.clone(), last.clone());
                        let task = tokio::spawn(async move {
                            let mut buf = vec![0u8; 65536];
                            while let Ok(n) = sock.recv(&mut buf).await {
                                *l.lock().unwrap() = Instant::now();
                                if tx.send((peer, buf[..n].to_vec())).await.is_err() {
                                    break;
                                }
                            }
                        });
                        peers.insert(peer, Peer { socket, task, last });
                    }
                    let p = &peers[&peer];
                    *p.last.lock().unwrap() = Instant::now();
                    let _ = p.socket.send(&data).await;
                }
                _ = gc.tick() => peers.retain(|_, p| {
                    let alive = p.last.lock().unwrap().elapsed() < UDP_IDLE;
                    if !alive { p.task.abort(); }
                    alive
                }),
            }
        }
    }
    .await;

    writer_task.abort();
    for p in peers.into_values() {
        p.task.abort();
    }
    result
}
