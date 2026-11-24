//! The tunnel engine: accepts client connections on the control port, binds one
//! public listener per enabled service, and splices visitors to clients.
//!
//! It knows nothing about MongoDB: the app layer feeds it a [`Snapshot`] of the
//! desired configuration via [`Engine::apply`], which hot-reconciles listeners
//! and pushes updated service lists to connected clients.

mod udp;

use anyhow::{Context, Result, bail};
use proto::frame::{read_json, write_json};
use proto::pump::{Counters, pump};
use proto::{ClientMsg, Hello, PROTOCOL_VERSION, Protocol, ServerMsg, ServiceSpec};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};
use tokio::net::{TcpListener, TcpSocket, TcpStream, UdpSocket};
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::server::TlsStream;
use tracing::{debug, info, warn};

pub use udp::UdpHub;

pub type Tls = TlsStream<TcpStream>;

const HELLO_TIMEOUT: Duration = Duration::from_secs(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientCfg {
    pub id: String,
    pub name: String,
    pub token_hash: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceCfg {
    pub id: String,
    pub name: String,
    pub client_id: String,
    pub protocol: Protocol,
    pub bind_port: u16,
    pub local_addr: String,
    pub enabled: bool,
    /// `http` services: SHA-256 of their URL secret.
    pub http_key: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub clients: Vec<ClientCfg>,
    pub services: Vec<ServiceCfg>,
}

#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub addr: SocketAddr,
    pub client_version: String,
    pub connected_at: SystemTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListenerState {
    /// Disabled, or its client is disabled.
    Inactive,
    Listening,
    Failed,
}

#[derive(Debug, Clone)]
pub struct ServiceRuntime {
    pub state: ListenerState,
    pub error: Option<String>,
    pub active_connections: u64,
    pub total_connections: u64,
    pub bytes_in: u64,
    pub bytes_out: u64,
}

#[derive(Debug, Clone)]
pub enum EngineEvent {
    ClientConnected { client_id: String, name: String, addr: SocketAddr },
    ClientDisconnected { client_id: String, name: String },
    ClientRejected { addr: SocketAddr, reason: String },
    ListenerFailed { service_id: String, name: String, error: String },
}

#[derive(Debug, Default)]
pub struct ServiceStats {
    pub traffic: Arc<Counters>,
    pub active: AtomicU64,
    pub total: AtomicU64,
}

struct Session {
    key: String,
    tx: mpsc::Sender<ServerMsg>,
    info: SessionInfo,
}

struct Pending {
    client_id: String,
    tx: oneshot::Sender<Tls>,
}

struct Listener {
    cfg: ServiceCfg,
    task: Option<JoinHandle<()>>,
    error: Option<String>,
    udp: Option<Arc<UdpHub>>,
}

#[derive(Default)]
struct State {
    clients: HashMap<String, ClientCfg>,
    services: HashMap<String, ServiceCfg>,
    sessions: HashMap<String, Session>,
    pending: HashMap<u64, Pending>,
    listeners: HashMap<String, Listener>,
    stats: HashMap<String, Arc<ServiceStats>>,
}

struct Inner {
    bind_ip: IpAddr,
    state: Mutex<State>,
    events: broadcast::Sender<EngineEvent>,
}

#[derive(Clone)]
pub struct Engine(Arc<Inner>);

fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::fill(&mut buf[..]);
    hex::encode(buf)
}

impl Engine {
    /// `bind_ip` is the address public service ports are bound on (usually `0.0.0.0`).
    pub fn new(bind_ip: IpAddr) -> Self {
        let (events, _) = broadcast::channel(256);
        Engine(Arc::new(Inner { bind_ip, state: Mutex::default(), events }))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.0.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn emit(&self, ev: EngineEvent) {
        let _ = self.0.events.send(ev);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<EngineEvent> {
        self.0.events.subscribe()
    }

    // ---------------------------------------------------------------- config

    /// Reconciles the running state with `snap`: kicks removed/disabled clients,
    /// (re)binds listeners whose port/protocol changed, retries failed binds,
    /// and pushes the new service lists to connected clients.
    pub async fn apply(&self, snap: Snapshot) {
        let mut to_stop = Vec::new();
        {
            let mut st = self.lock();
            st.clients = snap.clients.into_iter().map(|c| (c.id.clone(), c)).collect();
            st.services = snap.services.into_iter().map(|s| (s.id.clone(), s)).collect();

            let State { clients, services, sessions, listeners, stats, .. } = &mut *st;
            sessions.retain(|cid, _| clients.get(cid).is_some_and(|c| c.enabled));
            stats.retain(|sid, _| services.contains_key(sid));

            let wanted = |s: &ServiceCfg| s.enabled && clients.get(&s.client_id).is_some_and(|c| c.enabled);
            let ids: Vec<String> = listeners.keys().cloned().collect();
            for id in ids {
                let keep = services.get(&id).filter(|s| wanted(s)).is_some_and(|s| {
                    let l = &listeners[&id];
                    l.error.is_none() && l.cfg.protocol == s.protocol && l.cfg.bind_port == s.bind_port
                });
                if keep {
                    listeners.get_mut(&id).unwrap().cfg = services[&id].clone();
                } else {
                    to_stop.push(listeners.remove(&id).unwrap());
                }
            }
        }

        // Wait for stopped listeners to actually release their sockets so that
        // a port can be re-bound right away.
        for mut l in to_stop {
            if let Some(task) = l.task.take() {
                task.abort();
                let _ = task.await;
            }
        }

        let mut st = self.lock();
        let to_start: Vec<ServiceCfg> = st
            .services
            .values()
            .filter(|s| {
                s.enabled
                    && !st.listeners.contains_key(&s.id)
                    && st.clients.get(&s.client_id).is_some_and(|c| c.enabled)
            })
            .cloned()
            .collect();
        for cfg in to_start {
            let stats = st.stats.entry(cfg.id.clone()).or_default().clone();
            let listener = self.start_listener(cfg.clone(), stats);
            if let Some(err) = &listener.error {
                warn!(service = %cfg.name, port = cfg.bind_port, "listener failed: {err}");
                self.emit(EngineEvent::ListenerFailed {
                    service_id: cfg.id.clone(),
                    name: cfg.name.clone(),
                    error: err.clone(),
                });
            } else {
                info!(service = %cfg.name, "listening on {}/{}", cfg.bind_port, cfg.protocol);
            }
            st.listeners.insert(cfg.id.clone(), listener);
        }

        for (cid, session) in &st.sessions {
            let _ = session.tx.try_send(ServerMsg::Services { services: services_for(&st, cid) });
        }
    }

    fn start_listener(&self, cfg: ServiceCfg, stats: Arc<ServiceStats>) -> Listener {
        let addr = SocketAddr::new(self.0.bind_ip, cfg.bind_port);
        let mut listener = Listener { cfg: cfg.clone(), task: None, error: None, udp: None };
        match cfg.protocol {
            Protocol::Tcp => match bind_tcp(addr) {
                Ok(l) => {
                    let engine = self.clone();
                    listener.task = Some(tokio::spawn(engine.accept_visitors(cfg.id, l, stats)));
                }
                Err(e) => listener.error = Some(format!("{e:#}")),
            },
            Protocol::Udp => match bind_udp(addr) {
                Ok(sock) => {
                    let hub = Arc::new(UdpHub::new(sock, stats));
                    listener.task = Some(tokio::spawn(hub.clone().recv_loop()));
                    listener.udp = Some(hub);
                }
                Err(e) => listener.error = Some(format!("{e:#}")),
            },
            // Served by the admin HTTP listener through `open_http`.
            Protocol::Http => {}
        }
        listener
    }

    /// Disconnects a client's current session (it may reconnect if still allowed).
    pub fn kick(&self, client_id: &str) {
        self.lock().sessions.remove(client_id);
    }

    // ---------------------------------------------------------------- status

    pub fn session_info(&self, client_id: &str) -> Option<SessionInfo> {
        self.lock().sessions.get(client_id).map(|s| s.info.clone())
    }

    pub fn online_clients(&self) -> Vec<String> {
        self.lock().sessions.keys().cloned().collect()
    }

    pub fn service_runtime(&self, service_id: &str) -> ServiceRuntime {
        let st = self.lock();
        let (state, error) = match st.listeners.get(service_id) {
            None => (ListenerState::Inactive, None),
            Some(l) if l.error.is_some() => (ListenerState::Failed, l.error.clone()),
            Some(_) => (ListenerState::Listening, None),
        };
        let s = st.stats.get(service_id);
        let get = |f: fn(&ServiceStats) -> &AtomicU64| s.map_or(0, |s| f(s).load(Ordering::Relaxed));
        ServiceRuntime {
            state,
            error,
            active_connections: get(|s| &s.active),
            total_connections: get(|s| &s.total),
            bytes_in: get(|s| &s.traffic.bytes_in),
            bytes_out: get(|s| &s.traffic.bytes_out),
        }
    }

    /// Cumulative `(bytes_in, bytes_out)` per service since the process started.
    pub fn traffic_totals(&self) -> HashMap<String, (u64, u64)> {
        self.lock()
            .stats
            .iter()
            .map(|(id, s)| {
                let t = &s.traffic;
                (id.clone(), (t.bytes_in.load(Ordering::Relaxed), t.bytes_out.load(Ordering::Relaxed)))
            })
            .collect()
    }

    // --------------------------------------------------------------- control

    /// Accepts client connections (TLS) on the control port. Runs forever.
    pub async fn run_control(self, listener: TcpListener, acceptor: TlsAcceptor) -> Result<()> {
        info!("control port listening on {}", listener.local_addr()?);
        loop {
            let (sock, addr) = listener.accept().await?;
            let _ = sock.set_nodelay(true);
            let (engine, acceptor) = (self.clone(), acceptor.clone());
            tokio::spawn(async move {
                if let Err(e) = engine.handle_conn(sock, addr, acceptor).await {
                    debug!(%addr, "connection closed: {e:#}");
                }
            });
        }
    }

    async fn handle_conn(self, sock: TcpStream, addr: SocketAddr, acceptor: TlsAcceptor) -> Result<()> {
        let mut stream = tokio::time::timeout(HELLO_TIMEOUT, acceptor.accept(sock)).await.context("tls timeout")??;
        let hello: Hello = tokio::time::timeout(HELLO_TIMEOUT, read_json(&mut stream)).await.context("hello timeout")??;
        match hello {
            Hello::Control { token, version, client_version } => {
                self.run_session(stream, addr, &token, version, client_version).await
            }
            Hello::Data { session, conn_id } => {
                let mut st = self.lock();
                let client_id = client_for_key(&st, &session).context("unknown session")?;
                match st.pending.remove(&conn_id) {
                    Some(p) if p.client_id == client_id => {
                        let _ = p.tx.send(stream);
                        Ok(())
                    }
                    Some(p) => {
                        st.pending.insert(conn_id, p);
                        bail!("connection {conn_id} belongs to another client")
                    }
                    None => bail!("unknown or expired connection {conn_id}"),
                }
            }
            Hello::Udp { session, service_id } => {
                let hub = {
                    let st = self.lock();
                    let client_id = client_for_key(&st, &session).context("unknown session")?;
                    let l = st.listeners.get(&service_id).context("unknown service")?;
                    if l.cfg.client_id != client_id {
                        bail!("service belongs to another client");
                    }
                    l.udp.clone().context("not a running udp service")?
                };
                hub.attach(stream).await;
                Ok(())
            }
        }
    }

    async fn run_session(
        &self,
        mut stream: Tls,
        addr: SocketAddr,
        token: &str,
        version: u32,
        client_version: String,
    ) -> Result<()> {
        let token_hash = proto::sha256_hex(token.as_bytes());
        let found = {
            let st = self.lock();
            st.clients.values().find(|c| c.token_hash == token_hash).cloned()
        };
        let reason = match &found {
            _ if version != PROTOCOL_VERSION => {
                Some(format!("protocol version {version} unsupported, server speaks {PROTOCOL_VERSION}"))
            }
            None => Some("invalid token".to_owned()),
            Some(c) if !c.enabled => Some("client is disabled".to_owned()),
            Some(_) => None,
        };
        if let Some(reason) = reason {
            warn!(%addr, "client rejected: {reason}");
            self.emit(EngineEvent::ClientRejected { addr, reason: reason.clone() });
            write_json(&mut stream, &ServerMsg::Rejected { reason: reason.clone() }).await?;
            bail!(reason);
        }
        let client = found.unwrap();

        let key = random_hex(32);
        let (tx, mut rx) = mpsc::channel::<ServerMsg>(256);
        let services = {
            let mut st = self.lock();
            let info = SessionInfo { addr, client_version, connected_at: SystemTime::now() };
            // Replacing an existing session drops its sender, which ends it.
            st.sessions.insert(
                client.id.clone(),
                Session { key: key.clone(), tx, info },
            );
            services_for(&st, &client.id)
        };
        info!(client = %client.name, %addr, "client connected");
        self.emit(EngineEvent::ClientConnected { client_id: client.id.clone(), name: client.name.clone(), addr });

        let result = async {
            write_json(&mut stream, &ServerMsg::Welcome { session: key.clone(), client_name: client.name.clone() })
                .await?;
            write_json(&mut stream, &ServerMsg::Services { services }).await?;

            let (mut reader, mut writer) = tokio::io::split(stream);
            let (in_tx, mut in_rx) = mpsc::channel::<ClientMsg>(64);
            let _reader_task = AbortOnDrop(tokio::spawn(async move {
                while let Ok(msg) = read_json::<ClientMsg, _>(&mut reader).await {
                    if in_tx.send(msg).await.is_err() {
                        break;
                    }
                }
            }));

            let mut ping = tokio::time::interval(proto::PING_INTERVAL);
            let mut last_seen = Instant::now();
            loop {
                tokio::select! {
                    msg = rx.recv() => match msg {
                        Some(msg) => tokio::time::timeout(HELLO_TIMEOUT, write_json(&mut writer, &msg))
                            .await
                            .context("write timeout")??,
                        None => bail!("session replaced or client removed"),
                    },
                    msg = in_rx.recv() => match msg {
                        Some(ClientMsg::Pong) => last_seen = Instant::now(),
                        Some(ClientMsg::ConnectFailed { conn_id, error }) => {
                            last_seen = Instant::now();
                            debug!(client = %client.name, conn_id, "client could not reach local service: {error}");
                            self.lock().pending.remove(&conn_id);
                        }
                        None => bail!("client closed the connection"),
                    },
                    _ = ping.tick() => {
                        if last_seen.elapsed() > proto::IDLE_TIMEOUT {
                            bail!("client timed out");
                        }
                        write_json(&mut writer, &ServerMsg::Ping).await?;
                    }
                }
            }
            #[allow(unreachable_code)]
            Ok::<(), anyhow::Error>(())
        }
        .await;

        let mut st = self.lock();
        if st.sessions.get(&client.id).is_some_and(|s| s.key == key) {
            st.sessions.remove(&client.id);
        }
        st.pending.retain(|_, p| p.client_id != client.id || !p.tx.is_closed());
        drop(st);
        info!(client = %client.name, "client disconnected: {:#}", result.as_ref().err().unwrap());
        self.emit(EngineEvent::ClientDisconnected { client_id: client.id.clone(), name: client.name.clone() });
        result
    }

    // ----------------------------------------------------------------- data

    async fn accept_visitors(self, service_id: String, listener: TcpListener, stats: Arc<ServiceStats>) {
        loop {
            match listener.accept().await {
                Ok((sock, peer)) => {
                    let _ = sock.set_nodelay(true);
                    let (engine, sid, stats) = (self.clone(), service_id.clone(), stats.clone());
                    tokio::spawn(async move {
                        if let Err(e) = engine.handle_visitor(&sid, sock, stats).await {
                            debug!(%peer, "visitor dropped: {e:#}");
                        }
                    });
                }
                Err(e) => {
                    warn!("accept failed: {e}");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
    }

    async fn handle_visitor(&self, service_id: &str, sock: TcpStream, stats: Arc<ServiceStats>) -> Result<()> {
        let data = self.open_data(service_id).await?;
        stats.total.fetch_add(1, Ordering::Relaxed);
        stats.active.fetch_add(1, Ordering::Relaxed);
        let res = pump(sock, data, Some(stats.traffic.clone())).await;
        stats.active.fetch_sub(1, Ordering::Relaxed);
        res.map_err(Into::into)
    }

    /// A stream to the local service of the running `http` service whose secret
    /// hashes to `key_hash`. `None` when no such service is running.
    pub async fn open_http(&self, key_hash: &str) -> Option<Result<Tls>> {
        let service_id = {
            let st = self.lock();
            let id = st.listeners.values().find(|l| l.cfg.http_key.as_deref() == Some(key_hash))?.cfg.id.clone();
            if let Some(stats) = st.stats.get(&id) {
                stats.total.fetch_add(1, Ordering::Relaxed);
            }
            id
        };
        Some(self.open_data(&service_id).await)
    }

    /// Asks the service's client to open a data stream to its local address.
    async fn open_data(&self, service_id: &str) -> Result<Tls> {
        let conn_id = rand::random::<u64>();
        let (otx, orx) = oneshot::channel();
        let tx = {
            let mut st = self.lock();
            let svc = st.services.get(service_id).context("service removed")?;
            let client_id = svc.client_id.clone();
            let session = st.sessions.get(&client_id).context("client offline")?;
            let tx = session.tx.clone();
            st.pending.insert(conn_id, Pending { client_id, tx: otx });
            tx
        };
        if tx.send(ServerMsg::Connect { conn_id, service_id: service_id.to_owned() }).await.is_err() {
            self.lock().pending.remove(&conn_id);
            bail!("client went away");
        }
        let data = match tokio::time::timeout(CONNECT_TIMEOUT, orx).await {
            Ok(Ok(data)) => data,
            _ => {
                self.lock().pending.remove(&conn_id);
                bail!("client did not open the data connection");
            }
        };
        Ok(data)
    }
}

fn client_for_key(st: &State, key: &str) -> Option<String> {
    st.sessions.iter().find(|(_, s)| s.key == key).map(|(id, _)| id.clone())
}

fn services_for(st: &State, client_id: &str) -> Vec<ServiceSpec> {
    let mut v: Vec<ServiceSpec> = st
        .services
        .values()
        .filter(|s| s.client_id == client_id && s.enabled)
        .map(|s| ServiceSpec {
            id: s.id.clone(),
            name: s.name.clone(),
            protocol: s.protocol,
            local_addr: s.local_addr.clone(),
        })
        .collect();
    v.sort_by(|a, b| a.id.cmp(&b.id));
    v
}

fn bind_tcp(addr: SocketAddr) -> Result<TcpListener> {
    let sock = if addr.is_ipv4() { TcpSocket::new_v4()? } else { TcpSocket::new_v6()? };
    sock.set_reuseaddr(true)?;
    sock.bind(addr).with_context(|| format!("cannot bind tcp {addr}"))?;
    Ok(sock.listen(1024)?)
}

fn bind_udp(addr: SocketAddr) -> Result<UdpSocket> {
    let sock = std::net::UdpSocket::bind(addr).with_context(|| format!("cannot bind udp {addr}"))?;
    sock.set_nonblocking(true)?;
    Ok(UdpSocket::from_std(sock)?)
}

pub(crate) struct AbortOnDrop<T>(pub JoinHandle<T>);

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Engine")
    }
}
