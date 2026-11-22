//! Client-side TLS: trust either a pinned SHA-256 certificate fingerprint
//! (works with the server's auto-generated self-signed cert), a CA file, or
//! the public Web PKI roots.

use anyhow::{Context, Result, bail};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, ring};
use rustls::{ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme};
use rustls_pki_types::pem::PemObject;
use rustls_pki_types::{CertificateDer, ServerName, UnixTime};
use std::sync::Arc;

pub fn provider() -> Arc<CryptoProvider> {
    Arc::new(ring::default_provider())
}

pub enum Trust {
    Fingerprint(String),
    CaFile(std::path::PathBuf),
    WebPki,
}

pub fn client_config(trust: Trust) -> Result<Arc<ClientConfig>> {
    let builder = ClientConfig::builder_with_provider(provider()).with_safe_default_protocol_versions()?;
    let config = match trust {
        Trust::Fingerprint(fp) => {
            let fp = fp.replace(':', "").to_ascii_lowercase();
            if fp.len() != 64 || !fp.bytes().all(|b| b.is_ascii_hexdigit()) {
                bail!("fingerprint must be a SHA-256 hex digest (64 hex chars)");
            }
            builder
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(PinnedCert { fingerprint: fp, provider: provider() }))
                .with_no_client_auth()
        }
        Trust::CaFile(path) => {
            let mut roots = RootCertStore::empty();
            for cert in CertificateDer::pem_file_iter(&path).with_context(|| format!("reading {}", path.display()))? {
                roots.add(cert?)?;
            }
            builder.with_root_certificates(roots).with_no_client_auth()
        }
        Trust::WebPki => {
            let roots = RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.into() };
            builder.with_root_certificates(roots).with_no_client_auth()
        }
    };
    Ok(Arc::new(config))
}

/// Accepts exactly one end-entity certificate (by SHA-256 of its DER), but still
/// checks that the handshake is signed by that certificate's key.
#[derive(Debug)]
struct PinnedCert {
    fingerprint: String,
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for PinnedCert {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        if crate::sha256_hex(end_entity.as_ref()) == self.fingerprint {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General("server certificate fingerprint mismatch".into()))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}
