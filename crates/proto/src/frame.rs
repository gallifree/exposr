//! `u32` big-endian length prefix + payload. Read/written by hand (no buffering
//! codec) so that raw bytes following a hello frame are never swallowed.

use anyhow::{Context, Result, bail};
use serde::{Serialize, de::DeserializeOwned};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const MAX_FRAME: usize = 1 << 20;

pub async fn read_frame<R: AsyncRead + Unpin>(r: &mut R) -> Result<Vec<u8>> {
    let len = r.read_u32().await? as usize;
    if len > MAX_FRAME {
        bail!("frame too large: {len} bytes");
    }
    let mut buf = vec![0; len];
    r.read_exact(&mut buf).await?;
    Ok(buf)
}

pub async fn write_frame<W: AsyncWrite + Unpin>(w: &mut W, payload: &[u8]) -> Result<()> {
    if payload.len() > MAX_FRAME {
        bail!("frame too large: {} bytes", payload.len());
    }
    w.write_u32(payload.len() as u32).await?;
    w.write_all(payload).await?;
    w.flush().await?;
    Ok(())
}

pub async fn read_json<T: DeserializeOwned, R: AsyncRead + Unpin>(r: &mut R) -> Result<T> {
    let buf = read_frame(r).await?;
    serde_json::from_slice(&buf).context("invalid message")
}

pub async fn write_json<T: Serialize, W: AsyncWrite + Unpin>(w: &mut W, msg: &T) -> Result<()> {
    write_frame(w, &serde_json::to_vec(msg)?).await
}
