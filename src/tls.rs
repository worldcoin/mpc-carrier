//! Transport Layer Security.

use rustls::{ClientConfig, RootCertStore, ServerConfig};
use rustls_pemfile::{certs, private_key};
use std::fs::File;
use std::io::{self, BufReader};
use std::path::Path;
use std::sync::Arc;
use thiserror::Error;

/// Error returned by [`init`].
#[allow(missing_docs)]
#[derive(Error, Debug)]
pub enum Error {
    #[error("certificate chain file: {0}")]
    CertChainIo(io::Error),
    #[error("certificate priv key file: {0}")]
    CertPrivKeyIo(io::Error),
    #[error("certificate priv key unrecognized")]
    CertPrivKeyMissing,
    #[error("TLS server configuration: {0}")]
    ServerConfig(rustls::Error),
    #[error("TLS client configuration: {0}")]
    ClientConfig(rustls::Error),
}

/// Initializes [`TlsAcceptor`].
pub fn init(
    cert_chain: &Path,
    cert_priv_key: &Path,
) -> Result<(Arc<ServerConfig>, Arc<ClientConfig>), Error> {
    let cert_chain = File::open(cert_chain).map_err(Error::CertChainIo)?;
    let cert_priv_key = File::open(cert_priv_key).map_err(Error::CertPrivKeyIo)?;
    let cert_chain = certs(&mut BufReader::new(cert_chain))
        .collect::<Result<Vec<_>, _>>()
        .map_err(Error::CertChainIo)?;
    let cert_priv_key = private_key(&mut BufReader::new(cert_priv_key))
        .map_err(Error::CertPrivKeyIo)?
        .ok_or(Error::CertPrivKeyMissing)?;

    let server_config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert_chain.clone(), cert_priv_key.clone_key())
        .map_err(Error::ServerConfig)?;

    let mut root_cert_store = RootCertStore::empty();
    root_cert_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let client_config = ClientConfig::builder()
        .with_root_certificates(root_cert_store)
        .with_client_auth_cert(cert_chain, cert_priv_key)
        .map_err(Error::ClientConfig)?;

    Ok((Arc::new(server_config), Arc::new(client_config)))
}
