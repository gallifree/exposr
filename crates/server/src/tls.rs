//! Server certificate: either user-provided PEM files, or a self-signed one
//! generated once and persisted in the data directory. Clients pin its
//! SHA-256 fingerprint (shown in the admin UI).

use anyhow::{Context, Result};
use rustls::ServerConfig;
use rustls_pki_types::pem::PemObject;
use rustls_pki_types::{CertificateDer, PrivateKeyDer};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio_rustls::TlsAcceptor;

pub struct ServerTls {
    pub acceptor: TlsAcceptor,
    /// SHA-256 of the leaf certificate (hex).
    pub fingerprint: String,
}

pub fn load(cert: Option<PathBuf>, key: Option<PathBuf>, data_dir: &Path) -> Result<ServerTls> {
    let (cert_path, key_path) = match (cert, key) {
        (Some(c), Some(k)) => (c, k),
        (None, None) => ensure_self_signed(data_dir)?,
        _ => anyhow::bail!("TLS_CERT and TLS_KEY must be set together"),
    };
    let certs: Vec<CertificateDer<'static>> = CertificateDer::pem_file_iter(&cert_path)
        .with_context(|| format!("reading {}", cert_path.display()))?
        .collect::<Result<_, _>>()?;
    let key = PrivateKeyDer::from_pem_file(&key_path).with_context(|| format!("reading {}", key_path.display()))?;
    let fingerprint = proto::sha256_hex(certs.first().context("no certificate in file")?.as_ref());

    let config = ServerConfig::builder_with_provider(proto::tls::provider())
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_single_cert(certs, key)?;
    Ok(ServerTls { acceptor: TlsAcceptor::from(Arc::new(config)), fingerprint })
}

fn ensure_self_signed(dir: &Path) -> Result<(PathBuf, PathBuf)> {
    let cert = dir.join("exposr-cert.pem");
    let key = dir.join("exposr-key.pem");
    if !cert.exists() || !key.exists() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        let ck = rcgen::generate_simple_self_signed(vec!["exposr-server".to_owned()])?;
        std::fs::write(&cert, ck.cert.pem())?;
        std::fs::write(&key, ck.signing_key.serialize_pem())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600))?;
        }
        tracing::info!("generated self-signed certificate in {}", dir.display());
    }
    Ok((cert, key))
}
