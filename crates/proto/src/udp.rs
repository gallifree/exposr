//! UDP datagrams tunnelled over a stream: frame = `[peer][payload]`, where
//! peer = `4|6` tag byte, IP octets, `u16` BE port.

use anyhow::{Result, bail};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use tokio::io::{AsyncRead, AsyncWrite};

use crate::frame::{read_frame, write_frame};

pub fn encode(peer: SocketAddr, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(19 + data.len());
    match peer.ip() {
        IpAddr::V4(ip) => {
            out.push(4);
            out.extend_from_slice(&ip.octets());
        }
        IpAddr::V6(ip) => {
            out.push(6);
            out.extend_from_slice(&ip.octets());
        }
    }
    out.extend_from_slice(&peer.port().to_be_bytes());
    out.extend_from_slice(data);
    out
}

pub fn decode(buf: &[u8]) -> Result<(SocketAddr, &[u8])> {
    let (ip, rest): (IpAddr, &[u8]) = match buf.first() {
        Some(4) if buf.len() >= 7 => {
            let o: [u8; 4] = buf[1..5].try_into()?;
            (Ipv4Addr::from(o).into(), &buf[5..])
        }
        Some(6) if buf.len() >= 19 => {
            let o: [u8; 16] = buf[1..17].try_into()?;
            (Ipv6Addr::from(o).into(), &buf[17..])
        }
        _ => bail!("malformed udp frame"),
    };
    let port = u16::from_be_bytes([rest[0], rest[1]]);
    Ok((SocketAddr::new(ip, port), &rest[2..]))
}

pub async fn read<R: AsyncRead + Unpin>(r: &mut R) -> Result<(SocketAddr, Vec<u8>)> {
    let buf = read_frame(r).await?;
    let (peer, data) = decode(&buf)?;
    Ok((peer, data.to_vec()))
}

pub async fn write<W: AsyncWrite + Unpin>(w: &mut W, peer: SocketAddr, data: &[u8]) -> Result<()> {
    write_frame(w, &encode(peer, data)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        for peer in ["1.2.3.4:5678", "[::1]:53"] {
            let peer: SocketAddr = peer.parse().unwrap();
            let buf = encode(peer, b"hello");
            let (p, d) = decode(&buf).unwrap();
            assert_eq!((p, d), (peer, &b"hello"[..]));
        }
    }
}
