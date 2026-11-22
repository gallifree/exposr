//! Bidirectional byte copy that updates live traffic counters as it goes.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

#[derive(Debug, Default)]
pub struct Counters {
    /// Visitor → service.
    pub bytes_in: AtomicU64,
    /// Service → visitor.
    pub bytes_out: AtomicU64,
}

async fn copy<R, W>(mut r: R, mut w: W, counter: Option<(&Counters, bool)>) -> std::io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut buf = vec![0u8; 32 * 1024];
    loop {
        let n = r.read(&mut buf).await?;
        if n == 0 {
            let _ = w.shutdown().await;
            return Ok(());
        }
        w.write_all(&buf[..n]).await?;
        if let Some((c, inbound)) = counter {
            let target = if inbound { &c.bytes_in } else { &c.bytes_out };
            target.fetch_add(n as u64, Ordering::Relaxed);
        }
    }
}

/// Copies `visitor <-> service` until both directions are done or one fails.
pub async fn pump<A, B>(visitor: A, service: B, counters: Option<Arc<Counters>>) -> std::io::Result<()>
where
    A: AsyncRead + AsyncWrite + Unpin,
    B: AsyncRead + AsyncWrite + Unpin,
{
    let (vr, vw) = tokio::io::split(visitor);
    let (sr, sw) = tokio::io::split(service);
    let c = counters.as_deref();
    tokio::try_join!(copy(vr, sw, c.map(|c| (c, true))), copy(sr, vw, c.map(|c| (c, false))))?;
    Ok(())
}
