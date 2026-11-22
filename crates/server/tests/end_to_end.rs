//! End-to-end: real server engine + real client over TLS on localhost.

use proto::Protocol;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use exposr_server::engine::{ClientCfg, Engine, ListenerState, ServiceCfg, Snapshot};

const TOKEN: &str = "test-token-123";

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

async fn tcp_echo() -> SocketAddr {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (mut s, _) = l.accept().await.unwrap();
            tokio::spawn(async move {
                let (mut r, mut w) = s.split();
                let _ = tokio::io::copy(&mut r, &mut w).await;
            });
        }
    });
    addr
}

async fn udp_echo() -> SocketAddr {
    let s = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = s.local_addr().unwrap();
    tokio::spawn(async move {
        let mut buf = [0u8; 2048];
        loop {
            let (n, peer) = s.recv_from(&mut buf).await.unwrap();
            let mut reply = b"echo:".to_vec();
            reply.extend_from_slice(&buf[..n]);
            s.send_to(&reply, peer).await.unwrap();
        }
    });
    addr
}

async fn eventually<F, Fut, T>(what: &str, mut f: F) -> T
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Option<T>>,
{
    for _ in 0..100 {
        if let Some(v) = f().await {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("timed out waiting for {what}");
}

async fn tcp_roundtrip(port: u16, msg: &[u8]) -> Option<Vec<u8>> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).await.ok()?;
    s.write_all(msg).await.ok()?;
    let mut buf = vec![0; msg.len()];
    tokio::time::timeout(Duration::from_secs(2), s.read_exact(&mut buf)).await.ok()?.ok()?;
    Some(buf)
}

async fn udp_roundtrip(port: u16, msg: &[u8]) -> Option<Vec<u8>> {
    let s = UdpSocket::bind("127.0.0.1:0").await.ok()?;
    s.send_to(msg, ("127.0.0.1", port)).await.ok()?;
    let mut buf = [0u8; 2048];
    let n = tokio::time::timeout(Duration::from_millis(300), s.recv(&mut buf)).await.ok()?.ok()?;
    Some(buf[..n].to_vec())
}

#[tokio::test(flavor = "multi_thread")]
async fn tcp_and_udp_end_to_end() {
    let dir = std::env::temp_dir().join(format!("exposr-test-{}", rand::random::<u32>()));
    let tls = exposr_server::tls::load(None, None, &dir).unwrap();

    let engine = Engine::new("127.0.0.1".parse().unwrap());
    let control = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let control_addr = control.local_addr().unwrap();
    tokio::spawn(engine.clone().run_control(control, tls.acceptor.clone()));

    let (tcp_local, udp_local) = (tcp_echo().await, udp_echo().await);
    let (tcp_port, udp_port) = (free_port(), free_port());
    let client = ClientCfg {
        id: "c1".into(),
        name: "home".into(),
        token_hash: proto::sha256_hex(TOKEN.as_bytes()),
        enabled: true,
    };
    let mut tcp = ServiceCfg {
        id: "s-tcp".into(),
        name: "echo-tcp".into(),
        client_id: "c1".into(),
        protocol: Protocol::Tcp,
        bind_port: tcp_port,
        local_addr: tcp_local.to_string(),
        enabled: true,
    };
    let udp = ServiceCfg {
        id: "s-udp".into(),
        name: "echo-udp".into(),
        protocol: Protocol::Udp,
        bind_port: udp_port,
        local_addr: udp_local.to_string(),
        ..tcp.clone()
    };
    let snap = |services: Vec<ServiceCfg>| Snapshot { clients: vec![client.clone()], services };
    engine.apply(snap(vec![tcp.clone(), udp.clone()])).await;
    assert_eq!(engine.service_runtime("s-tcp").state, ListenerState::Listening);
    assert_eq!(engine.service_runtime("s-udp").state, ListenerState::Listening);

    let client_cfg = |token: &str| exposr_client::Config {
        server: control_addr.to_string(),
        server_name: Some("exposr-server".into()),
        token: token.into(),
        tls: proto::tls::client_config(proto::tls::Trust::Fingerprint(tls.fingerprint.clone())).unwrap(),
    };

    // A wrong token is rejected.
    let mut events = engine.subscribe();
    let bad = tokio::spawn(exposr_client::run(client_cfg("nope")));
    let ev = tokio::time::timeout(Duration::from_secs(5), events.recv()).await.unwrap().unwrap();
    assert!(matches!(ev, exposr_server::engine::EngineEvent::ClientRejected { .. }), "{ev:?}");
    bad.abort();

    // The real client connects.
    tokio::spawn(exposr_client::run(client_cfg(TOKEN)));
    eventually("client online", || async { (!engine.online_clients().is_empty()).then_some(()) }).await;

    // TCP, including several concurrent connections and a larger payload.
    assert_eq!(tcp_roundtrip(tcp_port, b"hello").await.unwrap(), b"hello");
    let big: Vec<u8> = (0..500_000u32).map(|i| i as u8).collect();
    let tasks: Vec<_> = (0..8).map(|_| tokio::spawn(tcp_roundtrip(tcp_port, Box::leak(big.clone().into_boxed_slice())))).collect();
    for t in tasks {
        assert_eq!(t.await.unwrap().unwrap(), big);
    }
    let rt = engine.service_runtime("s-tcp");
    assert_eq!(rt.total_connections, 9);
    assert_eq!(rt.bytes_in, 5 + 8 * 500_000);
    assert_eq!(rt.bytes_out, rt.bytes_in);

    // UDP (the client opens its channel asynchronously, so retry).
    let reply = eventually("udp echo", || udp_roundtrip(udp_port, b"ping")).await;
    assert_eq!(reply, b"echo:ping");

    // Hot reload: move the TCP service to another port.
    let old_port = tcp.bind_port;
    tcp.bind_port = free_port();
    engine.apply(snap(vec![tcp.clone(), udp.clone()])).await;
    assert!(TcpStream::connect(("127.0.0.1", old_port)).await.is_err(), "old port still open");
    assert_eq!(tcp_roundtrip(tcp.bind_port, b"moved").await.unwrap(), b"moved");
    // Counters survive the listener restart.
    assert_eq!(engine.service_runtime("s-tcp").total_connections, 10);

    // Disabling a service closes its port; UDP keeps working.
    tcp.enabled = false;
    engine.apply(snap(vec![tcp.clone(), udp.clone()])).await;
    assert_eq!(engine.service_runtime("s-tcp").state, ListenerState::Inactive);
    assert!(TcpStream::connect(("127.0.0.1", tcp.bind_port)).await.is_err());
    assert_eq!(eventually("udp after reload", || udp_roundtrip(udp_port, b"x")).await, b"echo:x");

    // A port that is already taken is reported, not fatal.
    let squatter = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let mut clash = tcp.clone();
    clash.id = "s-clash".into();
    clash.enabled = true;
    clash.bind_port = squatter.local_addr().unwrap().port();
    engine.apply(snap(vec![clash, udp.clone()])).await;
    let rt = engine.service_runtime("s-clash");
    assert_eq!(rt.state, ListenerState::Failed);
    assert!(rt.error.unwrap().contains("cannot bind"));

    // Disabling the client kicks it off.
    let mut disabled = client.clone();
    disabled.enabled = false;
    engine.apply(Snapshot { clients: vec![disabled], services: vec![udp] }).await;
    eventually("client kicked", || async { engine.online_clients().is_empty().then_some(()) }).await;

    let _ = std::fs::remove_dir_all(dir);
}
