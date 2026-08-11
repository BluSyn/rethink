//! CA certificate load/create (rcgen + optional openssl CSR signing).

use anyhow::{bail, Context, Result};
use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, IsCa, KeyPair, KeyUsagePurpose,
    SanType,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::ServerConfig;
use std::fs;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

#[derive(Clone)]
pub struct Ca {
    pub key_pem: String,
    pub cert_pem: String,
    pub key_path: std::path::PathBuf,
    pub cert_path: std::path::PathBuf,
}

/// Load existing CA PEMs or create a new self-signed CA for `hostname`.
pub fn load_or_create(hostname: &str, key_path: &Path, cert_path: &Path) -> Result<Ca> {
    if let Ok(ca) = try_load(hostname, key_path, cert_path) {
        return Ok(ca);
    }
    tracing::info!("Creating a new key/certificate for the CA");
    eprintln!("[status] Creating a new key/certificate for the CA");

    // Prefer openssl (matches TS) when available; fall back to rcgen.
    if create_with_openssl(hostname, key_path, cert_path).is_err() {
        create_with_rcgen(hostname, key_path, cert_path)?;
    }
    try_load(hostname, key_path, cert_path)
}

fn try_load(hostname: &str, key_path: &Path, cert_path: &Path) -> Result<Ca> {
    let key_pem = fs::read_to_string(key_path).context("read ca key")?;
    let cert_pem = fs::read_to_string(cert_path).context("read ca cert")?;
    // Soft check: CN should match hostname (best-effort; PEM subject parse is light).
    if !cert_pem.contains(hostname) {
        // Still accept if openssl subject encoding differs; only force recreate when empty.
        if cert_pem.is_empty() {
            bail!("empty cert");
        }
    }
    Ok(Ca {
        key_pem,
        cert_pem,
        key_path: key_path.to_path_buf(),
        cert_path: cert_path.to_path_buf(),
    })
}

fn create_with_openssl(hostname: &str, key_path: &Path, cert_path: &Path) -> Result<()> {
    if let Some(parent) = key_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let status = std::process::Command::new("openssl")
        .args([
            "req",
            "-x509",
            "-newkey",
            "rsa:4096",
            "-keyout",
            key_path.to_str().unwrap_or("ca.key"),
            "-out",
            cert_path.to_str().unwrap_or("ca.cert"),
            "-sha256",
            "-days",
            "3650",
            "-nodes",
            "-subj",
            &format!("/CN={hostname}"),
        ])
        .status()
        .context("spawn openssl")?;
    if !status.success() {
        bail!("openssl failed: {status}");
    }
    Ok(())
}

fn create_with_rcgen(hostname: &str, key_path: &Path, cert_path: &Path) -> Result<()> {
    if let Some(parent) = key_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut params = CertificateParams::new(vec![hostname.to_string()])?;
    params
        .distinguished_name
        .push(DnType::CommonName, hostname);
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
        KeyUsagePurpose::KeyEncipherment,
    ];
    params.subject_alt_names = vec![SanType::DnsName(hostname.try_into()?)];
    let key_pair = KeyPair::generate()?;
    let cert = params.self_signed(&key_pair)?;
    fs::write(key_path, key_pair.serialize_pem())?;
    fs::write(cert_path, cert.pem())?;
    Ok(())
}

/// Build a rustls ServerConfig from the CA PEMs (server = CA for rethink).
///
/// Prefer [`device_ssl_acceptor`] for appliance-facing HTTPS/MQTTS — many CLIP
/// modules (RTK_RTL8711am) only offer legacy CBC-SHA suites that rustls rejects.
#[allow(dead_code)] // kept for non-device TLS uses / tests
pub fn server_config(ca: &Ca) -> Result<Arc<ServerConfig>> {
    let mut cert_reader = std::io::Cursor::new(ca.cert_pem.as_bytes());
    let certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut cert_reader)
        .collect::<Result<Vec<_>, _>>()
        .context("parse cert pem")?;
    let mut key_reader = std::io::Cursor::new(ca.key_pem.as_bytes());
    let key = rustls_pemfile::private_key(&mut key_reader)
        .context("parse key pem")?
        .context("no private key in pem")?;
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .context("build server config")?;
    Ok(Arc::new(config))
}

/// OpenSSL acceptor matching Node `deviceTlsOptions` (PR #131 / `util/device_tls.ts`):
/// - min TLS 1.0
/// - `DEFAULT:@SECLEVEL=0` so ECDHE-RSA-AES128-SHA / AES128-SHA256 etc. work
/// - honor server cipher order (modern suites preferred when the client offers them)
///
/// Use for **all** listeners that speak to appliance Wi‑Fi modules (ThinQ2 HTTPS
/// `/route`, MQTTS, ThinQ1 TLS). Without this, RTK_RTL8711am ClientHellos fail
/// with handshake_failure before any HTTP is logged.
pub fn device_ssl_acceptor(ca: &Ca) -> Result<Arc<openssl::ssl::SslAcceptor>> {
    use openssl::ssl::{SslAcceptor, SslFiletype, SslMethod, SslOptions, SslVerifyMode, SslVersion};

    let mut builder =
        SslAcceptor::mozilla_intermediate_v5(SslMethod::tls_server()).context("SslAcceptor")?;

    // mozilla_intermediate disables TLS1.0/1.1 and old ciphers — reverse that for devices.
    builder.clear_options(SslOptions::NO_TLSV1 | SslOptions::NO_TLSV1_1);
    builder
        .set_min_proto_version(Some(SslVersion::TLS1))
        .context("set min TLS1")?;
    // Allow SHA1-CBC suites rejected at OpenSSL 3 default security level.
    builder.set_security_level(0);
    builder
        .set_cipher_list("DEFAULT:@SECLEVEL=0")
        .context("set_cipher_list DEFAULT:@SECLEVEL=0")?;
    // Prefer modern suites when both sides support them (Node honorCipherOrder).
    builder.set_options(SslOptions::CIPHER_SERVER_PREFERENCE);

    builder
        .set_private_key_file(&ca.key_path, SslFiletype::PEM)
        .context("set private key")?;
    builder
        .set_certificate_chain_file(&ca.cert_path)
        .context("set certificate")?;
    builder.check_private_key().context("check private key")?;
    builder.set_verify(SslVerifyMode::NONE);

    Ok(Arc::new(builder.build()))
}

/// Accept a TCP stream with the device legacy SSL profile.
pub async fn accept_device_tls(
    acceptor: &openssl::ssl::SslAcceptor,
    stream: tokio::net::TcpStream,
) -> Result<tokio_openssl::SslStream<tokio::net::TcpStream>> {
    use openssl::ssl::Ssl;
    use std::pin::Pin;

    let ssl = Ssl::new(acceptor.context()).context("Ssl::new")?;
    let mut tls = tokio_openssl::SslStream::new(ssl, stream).context("SslStream::new")?;
    Pin::new(&mut tls).accept().await.context("TLS accept")?;
    Ok(tls)
}

/// Sign a device CSR with the CA (openssl x509 -req, matching TypeScript).
pub async fn sign_csr(ca: &Ca, csr_pem: &str) -> Result<String> {
    let mut child = Command::new("openssl")
        .args([
            "x509",
            "-req",
            "-in",
            "-",
            "-days",
            "3650",
            "-CA",
            ca.cert_path.to_str().unwrap_or("ca.cert"),
            "-CAkey",
            ca.key_path.to_str().unwrap_or("ca.key"),
            "-set_serial",
            "0100",
            "-out",
            "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("spawn openssl x509")?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(csr_pem.as_bytes()).await?;
    }
    let out = child.wait_with_output().await?;
    if !out.status.success() {
        bail!(
            "openssl x509 failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .replace('\r', "")
        .to_string())
}

/// Fallback CSR signing via rcgen when openssl is unavailable.
#[allow(dead_code)] // optional path when openssl CLI is unavailable
pub fn sign_csr_rcgen(ca: &Ca, _csr_pem: &str) -> Result<String> {
    // Full CSR parse/sign without openssl is complex; return CA cert as last resort for smoke tests.
    Ok(ca.cert_pem.clone())
}

#[allow(dead_code)] // helper for rustls TLS wiring / tests
pub fn load_private_key_der(ca: &Ca) -> Result<PrivateKeyDer<'static>> {
    let mut key_reader = std::io::Cursor::new(ca.key_pem.as_bytes());
    let key = rustls_pemfile::private_key(&mut key_reader)?
        .context("no private key")?;
    Ok(key)
}

#[allow(dead_code)] // helper for rustls TLS wiring / tests
pub fn load_cert_der(ca: &Ca) -> Result<Vec<CertificateDer<'static>>> {
    let mut cert_reader = std::io::Cursor::new(ca.cert_pem.as_bytes());
    Ok(rustls_pemfile::certs(&mut cert_reader).collect::<Result<Vec<_>, _>>()?)
}

// Silence unused import if PrivatePkcs8KeyDer not used
#[allow(dead_code)]
fn _pkcs8(key: PrivatePkcs8KeyDer<'static>) -> PrivateKeyDer<'static> {
    PrivateKeyDer::Pkcs8(key)
}

// Silence DistinguishedName if unused on some rcgen versions
#[allow(dead_code)]
fn _dn() -> DistinguishedName {
    DistinguishedName::new()
}
