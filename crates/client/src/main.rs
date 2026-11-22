use anyhow::Result;
use clap::Parser;
use proto::tls::Trust;
use std::path::PathBuf;

/// Tunnel client: exposes local services through a tunnel server.
#[derive(Parser)]
#[command(version)]
struct Args {
    /// Server control address, e.g. `tunnel.example.com:2333`.
    #[arg(long, env = "EXPOSR_SERVER")]
    server: String,
    /// Client token (shown once in the admin UI).
    #[arg(long, env = "EXPOSR_TOKEN", hide_env_values = true)]
    token: String,
    /// Pin the server certificate by SHA-256 fingerprint (from the admin UI).
    #[arg(long, env = "EXPOSR_FINGERPRINT", conflicts_with = "ca")]
    fingerprint: Option<String>,
    /// Trust this CA (PEM) instead of the public roots.
    #[arg(long, env = "EXPOSR_CA")]
    ca: Option<PathBuf>,
    /// TLS server name, if different from the host in `--server`.
    #[arg(long, env = "EXPOSR_SERVER_NAME")]
    server_name: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
    let args = Args::parse();
    let trust = match (args.fingerprint, args.ca) {
        (Some(fp), _) => Trust::Fingerprint(fp),
        (None, Some(ca)) => Trust::CaFile(ca),
        (None, None) => Trust::WebPki,
    };
    let server_name = args.server_name.or_else(|| {
        // A pinned self-signed cert is issued for "exposr-server"; the name is irrelevant then.
        matches!(trust, Trust::Fingerprint(_)).then(|| "exposr-server".to_owned())
    });
    let cfg = exposr_client::Config {
        server: args.server,
        server_name,
        token: args.token,
        tls: proto::tls::client_config(trust)?,
    };
    exposr_client::run(cfg).await
}
