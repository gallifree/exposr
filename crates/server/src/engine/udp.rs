//! A bound public UDP socket plus (at most) one attached client channel.
//! Datagrams from any peer go to the client tagged with the peer address;
//! replies from the client are sent back to that peer.

use super::{AbortOnDrop, ServiceStats, Tls};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tracing::debug;

type Datagram = (SocketAddr, Vec<u8>);

pub struct UdpHub {
    socket: UdpSocket,
    stats: Arc<ServiceStats>,
    channel: Mutex<Option<(u64, mpsc::Sender<Datagram>)>>,
    generation: AtomicU64,
}

impl UdpHub {
    pub fn new(socket: UdpSocket, stats: Arc<ServiceStats>) -> Self {
        UdpHub { socket, stats, channel: Mutex::new(None), generation: AtomicU64::new(0) }
    }

    fn sender(&self) -> Option<mpsc::Sender<Datagram>> {
        self.channel.lock().unwrap().as_ref().map(|(_, tx)| tx.clone())
    }

    pub async fn recv_loop(self: Arc<Self>) {
        let mut buf = vec![0u8; 65536];
        loop {
            let Ok((n, peer)) = self.socket.recv_from(&mut buf).await else { continue };
            self.stats.traffic.bytes_in.fetch_add(n as u64, Ordering::Relaxed);
            if let Some(tx) = self.sender() {
                // Drop on overflow rather than stall: it's UDP.
                let _ = tx.try_send((peer, buf[..n].to_vec()));
            }
        }
    }

    /// Serves `stream` as the client channel until it closes. A newer channel
    /// replaces an older one.
    pub async fn attach(&self, stream: Tls) {
        let generation = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        let (tx, mut rx) = mpsc::channel::<Datagram>(1024);
        *self.channel.lock().unwrap() = Some((generation, tx));
        self.stats.active.store(1, Ordering::Relaxed);

        let (mut reader, mut writer) = tokio::io::split(stream);
        let _writer = AbortOnDrop(tokio::spawn(async move {
            while let Some((peer, data)) = rx.recv().await {
                if proto::udp::write(&mut writer, peer, &data).await.is_err() {
                    break;
                }
            }
        }));
        loop {
            let replaced = self.channel.lock().unwrap().as_ref().is_none_or(|(g, _)| *g != generation);
            if replaced {
                break;
            }
            match proto::udp::read(&mut reader).await {
                Ok((peer, data)) => {
                    self.stats.traffic.bytes_out.fetch_add(data.len() as u64, Ordering::Relaxed);
                    if let Err(e) = self.socket.send_to(&data, peer).await {
                        debug!(%peer, "udp send failed: {e}");
                    }
                }
                Err(_) => break,
            }
        }
        let mut ch = self.channel.lock().unwrap();
        if ch.as_ref().is_some_and(|(g, _)| *g == generation) {
            *ch = None;
            self.stats.active.store(0, Ordering::Relaxed);
        }
    }
}
