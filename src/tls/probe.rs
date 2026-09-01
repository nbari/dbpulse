use super::{TlsConfig, TlsMetadata, TlsMode, verifier::CertCapturingVerifier};
use anyhow::{Context, Result, anyhow};
use chrono::Utc;
use dsn::DSN;
use rustls::{
    ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime},
};
use rustls_pemfile::{certs, private_key};
use std::{
    io::Cursor,
    net::IpAddr,
    path::Path,
    sync::{Arc, OnceLock},
    time::Duration,
};
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::{Instant, timeout},
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

/// Longest the certificate probe may take before it is abandoned.
///
/// The probe is best-effort metadata collection that runs *after* every
/// required database operation has already succeeded, but still inside the
/// check deadline. Without its own bound, a server that accepts the TCP
/// connection and then stalls (a half-open connection, a wedged TLS
/// terminator) would burn the entire deadline and turn a completed, healthy
/// check into `pulse=0` with `error_type="timeout"`.
///
/// This is only the *ceiling*. The budget actually used is the smaller of this
/// and whatever remains of the check deadline (see [`remaining_probe_budget`]),
/// because the deadline is consumed cumulatively: a check that has already
/// spent 4s of a 5s allowance has 1s left, not 3s.
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// Below this there is no point starting a probe: a TCP connect plus TLS
/// handshake will not finish, and attempting it only risks overrunning the
/// deadline that the required work already satisfied.
const MIN_PROBE_BUDGET: Duration = Duration::from_millis(250);

tokio::task_local! {
    /// When the enclosing health check must be finished.
    ///
    /// Set by the pulse loop around the whole check so best-effort work deep in
    /// the query modules can see the same deadline without threading an extra
    /// argument through every call site.
    pub static CHECK_DEADLINE: Instant;
}

/// How long the probe may run: whatever is left of the check deadline, capped
/// at [`PROBE_TIMEOUT`].
///
/// Returns `None` when too little remains to be worth starting.
fn remaining_probe_budget() -> Option<Duration> {
    let Ok(deadline) = CHECK_DEADLINE.try_with(|deadline| *deadline) else {
        // No deadline in scope (unit tests, direct calls): fall back to the
        // fixed ceiling, which is the old behaviour.
        return Some(PROBE_TIMEOUT);
    };

    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining < MIN_PROBE_BUDGET {
        None
    } else {
        Some(remaining.min(PROBE_TIMEOUT))
    }
}

/// Seconds in a day, used to floor the certificate expiry gauge.
const SECONDS_PER_DAY: i64 = 86_400;

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

/// Perform a lightweight TLS handshake to extract certificate metadata
/// including subject, issuer, and expiry.
///
/// The handshake is verified according to `tls.mode`, matching what the real
/// database connection enforces. The whole probe is bounded by
/// [`PROBE_TIMEOUT`] so a stalled server cannot consume the caller's deadline.
///
/// # Errors
///
/// Returns an error if the probe times out, or if the TCP connection, STARTTLS
/// negotiation, TLS handshake, or certificate parsing fails.
pub async fn probe_certificate_expiry(
    dsn: &DSN,
    default_port: u16,
    protocol: TlsProbeProtocol,
    tls: &TlsConfig,
) -> Result<Option<TlsMetadata>> {
    let Some(budget) = remaining_probe_budget() else {
        // Not enough of the check deadline left to attempt a handshake. The
        // required work already succeeded; spending the remainder here would
        // fail an otherwise healthy check.
        return Ok(None);
    };

    timeout(
        budget,
        probe_certificate_expiry_inner(dsn, default_port, protocol, tls),
    )
    .await
    .map_err(|_| anyhow!("TLS certificate probe timed out after {budget:?} ({protocol:?})"))?
}

async fn probe_certificate_expiry_inner(
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

/// Build the root store used to verify the probe's TLS handshake.
///
/// Uses the operator-supplied CA when the DSN provides one (`sslrootcert` /
/// `sslca`), otherwise the bundled `WebPKI` roots.
async fn probe_root_store(tls: &TlsConfig) -> Result<RootCertStore> {
    let Some(ca_path) = tls.ca.as_deref() else {
        return Ok(webpki_roots::TLS_SERVER_ROOTS.iter().cloned().collect());
    };

    let mut store = RootCertStore::empty();
    for cert in load_cert_chain(ca_path).await? {
        store
            .add(cert)
            .map_err(|e| anyhow!("invalid CA certificate {}: {e}", ca_path.display()))?;
    }
    if store.is_empty() {
        anyhow::bail!("no usable CA certificates in {}", ca_path.display());
    }
    Ok(store)
}

async fn build_tls_connector(tls: &TlsConfig) -> Result<TlsConnector> {
    ensure_crypto_provider();

    // Under verify-ca / verify-full the operator has asked for an authenticated
    // server, and the certificate metadata this probe feeds into
    // dbpulse_tls_cert_expiry_days must be trustworthy: an unverified handshake
    // lets anyone in the path report a comfortable expiry for a certificate
    // that is about to lapse. Under `require` there is no trust anchor to check
    // against, so inspection stays unverified, as documented.
    //
    // Hostname checking has to follow the driver, not our own preference: sqlx
    // enforces it for verify-full only (`accept_invalid_hostnames =
    // !matches!(mode, VerifyFull)`). Verifying more strictly than the real
    // connection would make the probe fail on a certificate the database
    // accepts, silently dropping dbpulse_tls_cert_expiry_days for exactly the
    // operators who asked for verification.
    let builder = match tls.mode {
        TlsMode::VerifyCA | TlsMode::VerifyFull => {
            let verifier = CertCapturingVerifier::with_root_certificates(
                probe_root_store(tls).await?,
                matches!(tls.mode, TlsMode::VerifyCA),
            )?;
            ClientConfig::builder()
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(verifier))
        }
        TlsMode::Disable | TlsMode::Require => ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(NoVerifier)),
    };

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
    Ok(expiry_days_from_remaining(remaining))
}

/// Convert "time left before `not_after`" into the whole days reported by
/// `dbpulse_tls_cert_expiry_days`.
///
/// Floors rather than truncating. `Duration::num_days` rounds toward zero, so a
/// certificate that expired an hour ago reported `0` -- identical to one with
/// 23 hours still to run. The documented alerts are `< 30 and > 0` for
/// "expiring" and `< 0` for "expired", so for the whole first day after expiry
/// a dead certificate matched neither and nobody was paged. Flooring guarantees
/// an expired certificate is always `<= -1` while still reporting `0` for "less
/// than a day left".
///
/// Shared by every path that turns a certificate `not_after` into days -- the
/// probe, the capturing verifier, and the MySQL `Ssl_server_not_after`
/// fallback -- so the boundary behaviour cannot drift between them again.
pub(crate) fn expiry_days_from_remaining(remaining: chrono::Duration) -> i64 {
    remaining.num_seconds().div_euclid(SECONDS_PER_DAY)
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

    /// Regression: an expired certificate must never report `0` days.
    ///
    /// `Duration::num_days` truncates toward zero, so everything in the 24
    /// hours *after* expiry rounded up to `0`. The documented alerts are
    /// `< 30 and > 0` (expiring) and `< 0` (expired); `0` matches neither, so
    /// a certificate that had just lapsed was invisible to both for a full day
    /// -- precisely the window in which someone needs to be told.
    #[test]
    fn a_just_expired_certificate_reports_a_negative_day_count() {
        for hours_ago in [1, 6, 23] {
            let days = expiry_days_from_remaining(chrono::Duration::hours(-hours_ago));
            assert!(
                days < 0,
                "expired {hours_ago}h ago should be negative, got {days}"
            );
        }
    }

    /// The other half of the same boundary: still valid, however briefly, must
    /// not be reported as expired.
    #[test]
    fn a_certificate_with_less_than_a_day_left_reports_zero() {
        for hours_left in [1, 6, 23] {
            let days = expiry_days_from_remaining(chrono::Duration::hours(hours_left));
            assert_eq!(days, 0, "{hours_left}h left should floor to 0, got {days}");
        }
    }

    /// Regression: the probe budget must shrink with the check deadline.
    ///
    /// `PROBE_TIMEOUT` was applied as a flat 3s starting whenever the probe
    /// happened to run. Because the deadline is consumed cumulatively, a check
    /// that had already spent 4s of its 5s allowance would grant the probe 3s
    /// more and blow the deadline -- failing a check whose required work had
    /// already succeeded, purely to collect optional metadata.
    #[tokio::test]
    async fn probe_budget_is_capped_by_the_remaining_check_deadline() {
        let deadline = Instant::now() + Duration::from_millis(800);
        let budget = CHECK_DEADLINE
            .scope(deadline, async { remaining_probe_budget() })
            .await
            .expect("800ms is worth probing");

        assert!(
            budget <= Duration::from_millis(800),
            "budget {budget:?} must not exceed the time left in the check"
        );
        assert!(
            budget < PROBE_TIMEOUT,
            "budget must shrink below the ceiling"
        );
    }

    /// With plenty of deadline left the ceiling still applies.
    #[tokio::test]
    async fn probe_budget_is_capped_by_the_ceiling() {
        let deadline = Instant::now() + Duration::from_secs(600);
        let budget = CHECK_DEADLINE
            .scope(deadline, async { remaining_probe_budget() })
            .await
            .expect("plenty of time remains");

        assert_eq!(budget, PROBE_TIMEOUT);
    }

    /// With effectively no deadline left the probe is skipped rather than
    /// started, so it cannot be the reason a healthy check is failed.
    #[tokio::test]
    async fn probe_is_skipped_when_the_deadline_is_nearly_spent() {
        let deadline = Instant::now() + Duration::from_millis(10);
        let budget = CHECK_DEADLINE
            .scope(deadline, async { remaining_probe_budget() })
            .await;

        assert!(budget.is_none(), "should skip, got {budget:?}");
    }

    /// Outside a check (unit tests, direct calls) the ceiling is used.
    #[test]
    fn probe_budget_falls_back_to_the_ceiling_without_a_deadline() {
        assert_eq!(remaining_probe_budget(), Some(PROBE_TIMEOUT));
    }

    #[test]
    fn whole_day_counts_are_unchanged() {
        assert_eq!(expiry_days_from_remaining(chrono::Duration::days(30)), 30);
        assert_eq!(expiry_days_from_remaining(chrono::Duration::days(1)), 1);
        assert_eq!(expiry_days_from_remaining(chrono::Duration::zero()), 0);
        assert_eq!(expiry_days_from_remaining(chrono::Duration::days(-1)), -1);
    }
}
