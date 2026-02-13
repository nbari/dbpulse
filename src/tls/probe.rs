use super::{TlsConfig, TlsMetadata};
use anyhow::{Context, Result, anyhow};
use chrono::Utc;
use dsn::DSN;
use rustls::{
    ClientConfig, DigitallySignedStruct, SignatureScheme,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime},
};
use rustls_pemfile::{certs, private_key};
use std::{
    io::Cursor,
    net::IpAddr,
    path::Path,
    sync::{Arc, OnceLock},
};
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
use tokio_rustls::{TlsConnector, client::TlsStream};
use x509_parser::prelude::{FromDer, X509Certificate};

// PostgreSQL SSL handshake constants
const POSTGRES_SSL_REQUEST_CODE: i32 = 80_877_103;
const POSTGRES_SSL_REQUEST_LEN: i32 = 8;

// MySQL capability flags
const MYSQL_CLIENT_SSL: u32 = 0x0000_0800;
const MYSQL_CLIENT_PROTOCOL_41: u32 = 0x0000_0200;
const MYSQL_CLIENT_SECURE_CONNECTION: u32 = 0x0000_8000;
const MYSQL_CLIENT_LONG_FLAG: u32 = 0x0000_0004;
const MYSQL_CLIENT_PLUGIN_AUTH: u32 = 0x0008_0000;

static CRYPTO_PROVIDER_INIT: OnceLock<()> = OnceLock::new();

/// Ensure the rustls crypto provider is initialized
///
/// This should be called before any TLS operations. It's safe to call
/// multiple times as initialization only happens once.
///
/// # Panics
///
/// Panics if the crypto provider cannot be installed (should never happen in practice)
pub fn ensure_crypto_provider() {
    CRYPTO_PROVIDER_INIT.get_or_init(|| {
        if let Err(err) = rustls::crypto::ring::default_provider().install_default() {
            eprintln!("failed to install ring crypto provider: {err:?}");
            std::process::exit(1);
        }
    });
}

/// Protocol type for TLS certificate probing
#[derive(Debug, Clone, Copy)]
pub enum TlsProbeProtocol {
    /// `PostgreSQL` requires a STARTTLS-style negotiation (`-starttls postgres`)
    Postgres,
    /// MySQL/MariaDB STARTTLS negotiation (`-starttls mysql`)
    Mysql,
}

/// Perform a lightweight TLS handshake (without certificate verification) to extract
/// certificate metadata including subject, issuer, and expiry.
///
/// # Errors
///
/// Returns an error if the TCP connection, STARTTLS negotiation, TLS handshake,
/// or certificate parsing fails.
pub async fn probe_certificate_expiry(
    dsn: &DSN,
    default_port: u16,
    protocol: TlsProbeProtocol,
    tls: &TlsConfig,
) -> Result<Option<TlsMetadata>> {
    let host = match &dsn.host {
        Some(host) => host.clone(),
        None => return Ok(None),
    };
    let port = dsn.port.unwrap_or(default_port);

    let mut stream = TcpStream::connect((host.as_str(), port))
        .await
        .with_context(|| {
            format!(
                "failed to connect to {host}:{port} for TLS certificate probe (protocol: {protocol:?})"
            )
        })?;

    match protocol {
        TlsProbeProtocol::Postgres => send_postgres_ssl_request(&mut stream).await?,
        TlsProbeProtocol::Mysql => perform_mysql_starttls(&mut stream).await?,
    }

    let connector = build_tls_connector(tls)
        .await
        .context("failed to build TLS connector for certificate probe")?;
    let server_name = server_name_from_host(&host)
        .with_context(|| format!("invalid server name for TLS probe: {host}"))?;
    let tls_stream = connector
        .connect(server_name, stream)
        .await
        .with_context(|| {
            format!("failed to complete TLS handshake for certificate probe ({protocol:?})")
        })?;

    extract_expiry_from_tls_stream(&tls_stream)
        .with_context(|| "failed to extract certificate metadata from TLS stream".to_string())
}

async fn send_postgres_ssl_request(stream: &mut TcpStream) -> Result<()> {
    let mut packet = [0u8; POSTGRES_SSL_REQUEST_LEN as usize];
    packet[..4].copy_from_slice(&POSTGRES_SSL_REQUEST_LEN.to_be_bytes());
    packet[4..].copy_from_slice(&POSTGRES_SSL_REQUEST_CODE.to_be_bytes());

    stream
        .write_all(&packet)
        .await
        .context("failed to send PostgreSQL SSLRequest packet")?;

    let mut response = [0u8; 1];
    stream
        .read_exact(&mut response)
        .await
        .context("failed to read PostgreSQL SSLRequest response")?;

    if response[0] != b'S' {
        anyhow::bail!("PostgreSQL server does not accept TLS connections");
    }

    Ok(())
}

async fn perform_mysql_starttls(stream: &mut TcpStream) -> Result<()> {
    let mut header = [0u8; 4];
    stream
        .read_exact(&mut header)
        .await
        .context("failed to read MySQL handshake header")?;
    let payload_len = u32::from_le_bytes([header[0], header[1], header[2], 0]);
    let mut payload = vec![0u8; payload_len as usize];
    stream
        .read_exact(&mut payload)
        .await
        .context("failed to read MySQL handshake payload")?;

    let (capabilities, charset) = parse_mysql_handshake(&payload)?;
    if capabilities & MYSQL_CLIENT_SSL == 0 {
        anyhow::bail!("MySQL server does not support TLS connections");
    }

    let mut client_flags = MYSQL_CLIENT_PROTOCOL_41
        | MYSQL_CLIENT_SSL
        | MYSQL_CLIENT_SECURE_CONNECTION
        | MYSQL_CLIENT_LONG_FLAG
        | MYSQL_CLIENT_PLUGIN_AUTH;
    client_flags &= capabilities | MYSQL_CLIENT_SSL;

    let max_packet = 16_777_216_u32;
    let collation = if charset == 0 { 0x21 } else { charset };

    let payload_len = 4 + 4 + 1 + 23;
    let mut packet = Vec::with_capacity(payload_len + 4);
    packet.extend_from_slice(&payload_len.to_le_bytes()[..3]);
    packet.push(1);
    packet.extend_from_slice(&client_flags.to_le_bytes());
    packet.extend_from_slice(&max_packet.to_le_bytes());
    packet.push(collation);
    packet.extend_from_slice(&[0u8; 23]);

    stream
        .write_all(&packet)
        .await
        .context("failed to send MySQL SSLRequest")?;

    Ok(())
}

fn parse_mysql_handshake(payload: &[u8]) -> Result<(u32, u8)> {
    if payload.is_empty() {
        anyhow::bail!("empty MySQL handshake payload");
    }

    let mut cursor = 0;
    cursor += 1; // protocol version

    let rest = payload
        .get(cursor..)
        .context("invalid MySQL handshake: missing protocol version")?;
    let version_end = rest
        .iter()
        .position(|&b| b == 0)
        .context("invalid MySQL handshake: missing version terminator")?;
    cursor += version_end + 1; // server version string + null

    if payload.len() < cursor + 4 + 8 + 1 + 2 {
        anyhow::bail!("unexpectedly short MySQL handshake");
    }
    cursor += 4; // connection id
    cursor += 8; // auth plugin data part 1
    cursor += 1; // filler

    let lower_capabilities = payload
        .get(cursor..cursor + 2)
        .context("invalid MySQL handshake: missing lower capabilities")?;
    let mut capabilities = u32::from(u16::from_le_bytes(
        lower_capabilities
            .try_into()
            .context("invalid MySQL handshake capability encoding")?,
    ));
    cursor += 2;

    let mut charset = 0u8;
    if let Some(&value) = payload.get(cursor) {
        charset = value;
        cursor += 1;
    }

    if payload.len() >= cursor + 2 {
        cursor += 2; // status flags
    }
    if payload.len() >= cursor + 2 {
        let upper_capabilities = payload
            .get(cursor..cursor + 2)
            .context("invalid MySQL handshake: missing upper capabilities")?;
        let upper = u32::from(u16::from_le_bytes(
            upper_capabilities
                .try_into()
                .context("invalid MySQL handshake upper capability encoding")?,
        ));
        capabilities |= upper << 16;
    }

    Ok((capabilities, charset))
}

async fn build_tls_connector(tls: &TlsConfig) -> Result<TlsConnector> {
    ensure_crypto_provider();

    let builder = ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(NoVerifier));

    let config = if let (Some(cert_path), Some(key_path)) = (&tls.cert, &tls.key) {
        let certs = load_cert_chain(cert_path.as_path()).await?;
        let key = load_private_key(key_path.as_path()).await?;
        builder.with_client_auth_cert(certs, key)?
    } else {
        builder.with_no_client_auth()
    };

    Ok(TlsConnector::from(Arc::new(config)))
}

async fn load_cert_chain(path: &Path) -> Result<Vec<CertificateDer<'static>>> {
    let data = fs::read(path)
        .await
        .with_context(|| format!("failed to read certificate {}", path.display()))?;
    let mut reader = Cursor::new(data);
    let parsed = certs(&mut reader)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| anyhow!("invalid certificate PEM: {e}"))?;

    if parsed.is_empty() {
        anyhow::bail!("no certificates found in {}", path.display());
    }

    Ok(parsed)
}

async fn load_private_key(path: &Path) -> Result<PrivateKeyDer<'static>> {
    let data = fs::read(path)
        .await
        .with_context(|| format!("failed to read private key {}", path.display()))?;

    let mut reader = Cursor::new(data);
    private_key(&mut reader)
        .map_err(|e| anyhow!("invalid private key PEM: {e}"))?
        .ok_or_else(|| anyhow!("no private key found in {}", path.display()))
}

fn server_name_from_host(host: &str) -> Result<ServerName<'static>> {
    host.parse::<IpAddr>().map_or_else(
        |_| {
            ServerName::try_from(host.to_string())
                .map_err(|_| anyhow!("invalid server name: {host}"))
        },
        |ip| Ok(ServerName::from(ip).to_owned()),
    )
}

/// Extract certificate metadata (subject, issuer, expiry) from DER-encoded certificate
fn extract_cert_metadata(cert_der: &[u8]) -> Result<TlsMetadata> {
    let (_, cert) = X509Certificate::from_der(cert_der)
        .map_err(|e| anyhow!("failed to parse certificate: {e}"))?;

    // Extract subject
    let cert_subject = Some(cert.subject().to_string());

    // Extract issuer
    let cert_issuer = Some(cert.issuer().to_string());

    // Calculate expiry days
    let cert_expiry_days = Some(calculate_expiry_days(cert_der)?);

    Ok(TlsMetadata {
        cert_subject,
        cert_issuer,
        cert_expiry_days,
        ..Default::default()
    })
}

fn extract_expiry_from_tls_stream(stream: &TlsStream<TcpStream>) -> Result<Option<TlsMetadata>> {
    let (_, connection) = stream.get_ref();
    let certs = connection.peer_certificates();
    let Some(certs) = certs else {
        return Ok(None);
    };
    let Some(cert) = certs.first() else {
        return Ok(None);
    };

    extract_cert_metadata(cert.as_ref()).map(Some)
}

fn calculate_expiry_days(cert_der: &[u8]) -> Result<i64> {
    let (_, cert) = X509Certificate::from_der(cert_der)
        .map_err(|e| anyhow!("failed to parse certificate: {e}"))?;
    let raw = cert.validity().not_after.to_datetime();
    let not_after = chrono::DateTime::<Utc>::from_timestamp(raw.unix_timestamp(), raw.nanosecond())
        .ok_or_else(|| anyhow!("invalid certificate expiry timestamp"))?;
    let remaining = not_after - Utc::now();
    Ok(remaining.num_days())
}

/// Custom certificate verifier that accepts any certificate without validation.
///
/// # Security Note
///
/// This verifier is **ONLY** used for certificate inspection during the probe phase
/// to extract certificate metadata (subject, issuer, expiry dates). The actual database
/// connection uses proper certificate verification according to the configured `TlsMode`:
///
/// - `Disable`: No TLS
/// - `Require`: TLS required, no verification (accepts any cert)
/// - `VerifyCA`: Verify against CA
/// - `VerifyFull`: Full verification (chain + hostname)
#[derive(Debug)]
struct NoVerifier;

impl ServerCertVerifier for NoVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::RSA_PKCS1_SHA256,
            SignatureScheme::RSA_PKCS1_SHA384,
            SignatureScheme::RSA_PKCS1_SHA512,
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::ECDSA_NISTP384_SHA384,
            SignatureScheme::ECDSA_NISTP521_SHA512,
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::RSA_PSS_SHA384,
            SignatureScheme::RSA_PSS_SHA512,
            SignatureScheme::ED25519,
        ]
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn test_crypto_provider_init() {
        // Should not panic
        ensure_crypto_provider();
        ensure_crypto_provider(); // Second call should be idempotent
    }

    #[test]
    fn test_server_name_from_hostname() {
        let result = server_name_from_host("example.com");
        assert!(result.is_ok());

        let result = server_name_from_host("db.example.com");
        assert!(result.is_ok());
    }

    #[test]
    fn test_server_name_from_ipv4() {
        let result = server_name_from_host("127.0.0.1");
        assert!(result.is_ok());

        let result = server_name_from_host("192.168.1.100");
        assert!(result.is_ok());
    }

    #[test]
    fn test_server_name_from_ipv6() {
        let result = server_name_from_host("::1");
        assert!(result.is_ok());

        let result = server_name_from_host("2001:db8::1");
        assert!(result.is_ok());
    }

    #[test]
    fn test_server_name_invalid() {
        // Empty string should fail
        let result = server_name_from_host("");
        assert!(result.is_err());

        // Invalid characters should fail
        let result = server_name_from_host("invalid host name with spaces");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_mysql_handshake_empty() {
        let result = parse_mysql_handshake(&[]);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty"));
    }

    #[test]
    fn test_parse_mysql_handshake_too_short() {
        // Protocol version only
        let payload = vec![10u8];
        let result = parse_mysql_handshake(&payload);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_mysql_handshake_valid() {
        // Minimal valid MySQL handshake
        let mut payload = vec![10u8]; // protocol version
        payload.extend_from_slice(b"5.7.0\0"); // version string with null terminator
        payload.extend_from_slice(&[0u8; 4]); // connection id
        payload.extend_from_slice(&[0u8; 8]); // auth plugin data part 1
        payload.push(0); // filler

        // Add capabilities (2 bytes for lower part)
        payload.extend_from_slice(&0x0800u16.to_le_bytes()); // CLIENT_SSL capability

        let result = parse_mysql_handshake(&payload);
        assert!(result.is_ok());
        let (capabilities, _charset) = result.unwrap();
        assert!(capabilities & MYSQL_CLIENT_SSL != 0);
    }

    #[test]
    fn test_no_verifier_debug() {
        let verifier = NoVerifier;
        let debug_str = format!("{verifier:?}");
        assert!(debug_str.contains("NoVerifier"));
    }

    #[test]
    fn test_no_verifier_supported_schemes() {
        let verifier = NoVerifier;
        let schemes = verifier.supported_verify_schemes();
        assert!(!schemes.is_empty());
        assert!(schemes.contains(&SignatureScheme::RSA_PKCS1_SHA256));
        assert!(schemes.contains(&SignatureScheme::ED25519));
    }

    #[test]
    fn test_tls_probe_protocol_debug() {
        let proto = TlsProbeProtocol::Postgres;
        let debug_str = format!("{proto:?}");
        assert!(debug_str.contains("Postgres"));

        let proto = TlsProbeProtocol::Mysql;
        let debug_str = format!("{proto:?}");
        assert!(debug_str.contains("Mysql"));
    }
}
