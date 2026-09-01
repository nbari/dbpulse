use super::TlsMetadata;
use anyhow::{Result, anyhow};
use chrono::Utc;
use rustls::{
    CertificateError, DigitallySignedStruct, Error as TlsError, RootCertStore, SignatureScheme,
    client::{
        WebPkiServerVerifier,
        danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    },
    pki_types::{CertificateDer, ServerName, UnixTime},
};
use std::{
    fmt,
    sync::{Arc, Mutex},
};
use x509_parser::prelude::{FromDer, X509Certificate};

/// Certificate metadata captured during TLS handshake
#[derive(Debug, Clone, Default)]
pub struct CapturedCertMetadata {
    pub subject: Option<String>,
    pub issuer: Option<String>,
    pub expiry_days: Option<i64>,
}

impl From<CapturedCertMetadata> for TlsMetadata {
    fn from(captured: CapturedCertMetadata) -> Self {
        Self {
            cert_subject: captured.subject,
            cert_issuer: captured.issuer,
            cert_expiry_days: captured.expiry_days,
            ..Default::default()
        }
    }
}

/// A custom certificate verifier that captures certificate metadata while
/// delegating actual verification to the standard `WebPKI` verifier.
///
/// This verifier maintains full TLS security by wrapping rustls's built-in
/// `WebPkiServerVerifier` while extracting certificate information for monitoring.
///
/// # Security
///
/// - Does NOT bypass certificate validation
/// - Uses the standard `WebPKI` verifier for all security checks
/// - Only extracts metadata in addition to normal verification
/// - Thread-safe via `Arc<Mutex<>>`
#[derive(Clone)]
pub struct CertCapturingVerifier {
    /// Captured certificate metadata (shared across threads)
    captured: Arc<Mutex<Option<CapturedCertMetadata>>>,
    /// The real verifier that performs actual TLS validation
    inner_verifier: Arc<WebPkiServerVerifier>,
    /// Whether a certificate valid for a different name is still accepted.
    ///
    /// Mirrors sqlx's own `NoHostnameTlsVerifier`, which the driver installs
    /// for every mode except `verify-full`. The probe must accept exactly what
    /// the real connection accepts, or its metrics describe a different
    /// security posture than the one actually in force.
    accept_invalid_hostnames: bool,
}

impl fmt::Debug for CertCapturingVerifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CertCapturingVerifier")
            .field("captured", &self.captured)
            .field("inner_verifier", &"WebPkiServerVerifier")
            .field("accept_invalid_hostnames", &self.accept_invalid_hostnames)
            .finish()
    }
}

impl CertCapturingVerifier {
    /// Create a new certificate-capturing verifier with default `WebPKI` roots
    ///
    /// # Errors
    ///
    /// Returns an error if the `WebPKI` verifier cannot be built
    pub fn new() -> Result<Self> {
        let root_store: RootCertStore = webpki_roots::TLS_SERVER_ROOTS.iter().cloned().collect();
        let inner_verifier = WebPkiServerVerifier::builder(Arc::new(root_store))
            .build()
            .map_err(|e| anyhow!("failed to build WebPKI verifier: {e}"))?;

        Ok(Self {
            captured: Arc::new(Mutex::new(None)),
            inner_verifier,
            accept_invalid_hostnames: false,
        })
    }

    /// Create a verifier with custom root certificates
    ///
    /// `accept_invalid_hostnames` downgrades **only** a name mismatch to
    /// success, exactly as sqlx does for every mode below `verify-full`. Every
    /// other certificate error (expired, unknown issuer, bad signature,
    /// revoked, ...) still fails.
    ///
    /// # Errors
    ///
    /// Returns an error if the `WebPKI` verifier cannot be built or if certificates are invalid
    pub fn with_root_certificates(
        root_store: RootCertStore,
        accept_invalid_hostnames: bool,
    ) -> Result<Self> {
        let inner_verifier = WebPkiServerVerifier::builder(Arc::new(root_store))
            .build()
            .map_err(|e| anyhow!("failed to build WebPKI verifier: {e}"))?;

        Ok(Self {
            captured: Arc::new(Mutex::new(None)),
            inner_verifier,
            accept_invalid_hostnames,
        })
    }

    /// Retrieve captured certificate metadata
    ///
    /// Returns `None` if no certificate has been captured yet (before handshake completes)
    #[must_use]
    pub fn get_captured(&self) -> Option<CapturedCertMetadata> {
        self.captured.lock().ok()?.clone()
    }

    /// Extract certificate metadata from DER-encoded certificate
    fn extract_metadata(cert_der: &[u8]) -> Result<CapturedCertMetadata> {
        let (_, cert) = X509Certificate::from_der(cert_der)
            .map_err(|e| anyhow!("failed to parse certificate: {e}"))?;

        let subject = Some(cert.subject().to_string());
        let issuer = Some(cert.issuer().to_string());

        // Calculate expiry days
        let raw = cert.validity().not_after.to_datetime();
        let not_after =
            chrono::DateTime::<Utc>::from_timestamp(raw.unix_timestamp(), raw.nanosecond())
                .ok_or_else(|| anyhow!("invalid certificate expiry timestamp"))?;
        let remaining = not_after - Utc::now();
        // Floored via the shared probe helper: `Duration::num_days` truncates
        // toward zero, so anything in the first 24h after expiry reported `0`,
        // which matches neither documented alert (`< 30 and > 0`, `< 0`).
        let expiry_days = Some(super::probe::expiry_days_from_remaining(remaining));

        Ok(CapturedCertMetadata {
            subject,
            issuer,
            expiry_days,
        })
    }
}

impl ServerCertVerifier for CertCapturingVerifier {
    /// Verify server certificate and capture metadata
    ///
    /// This method:
    /// 1. Extracts certificate metadata (subject, issuer, expiry)
    /// 2. Delegates to the real `WebPKI` verifier for actual validation
    /// 3. Downgrades a pure name mismatch to success when the mode allows it
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, TlsError> {
        // Extract metadata before verification (always attempt this)
        if let Ok(metadata) = Self::extract_metadata(end_entity.as_ref())
            && let Ok(mut captured) = self.captured.lock()
        {
            *captured = Some(metadata);
        }

        // Delegate to real verifier for actual security validation
        let verified = self.inner_verifier.verify_server_cert(
            end_entity,
            intermediates,
            server_name,
            ocsp_response,
            now,
        );

        // `CertificateError` is #[non_exhaustive]: the catch-all keeps every
        // other failure (expired, unknown issuer, bad signature, revoked, ...)
        // fatal, so this stays a name-mismatch exemption and never becomes a
        // blanket accept.
        match verified {
            Err(TlsError::InvalidCertificate(
                CertificateError::NotValidForName | CertificateError::NotValidForNameContext { .. },
            )) if self.accept_invalid_hostnames => Ok(ServerCertVerified::assertion()),
            result => result,
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        self.inner_verifier
            .verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        self.inner_verifier
            .verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.inner_verifier.supported_verify_schemes()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use rustls::crypto::ring::default_provider;
    use std::{io::Cursor, time::Duration};

    fn ensure_crypto_provider() {
        let _ = rustls::crypto::CryptoProvider::install_default(default_provider());
    }

    /// CA that issued [`MISMATCH_PEM`].
    const CA_PEM: &[u8] = include_bytes!("testdata/ca.crt");
    /// An unrelated CA, used as a trust anchor that must *not* validate the leaf.
    const OTHER_CA_PEM: &[u8] = include_bytes!("testdata/other-ca.crt");
    /// Leaf signed by [`CA_PEM`] whose only SAN is `DNS:db.internal` -- so it is
    /// perfectly valid, just not for the host the probe connects to.
    const MISMATCH_PEM: &[u8] = include_bytes!("testdata/mismatch.crt");

    // The fixtures are valid from 2026-09-01 to 2126-08-08. Verification is
    // pinned to fixed instants rather than `UnixTime::now()` so these tests
    // assert the name logic and never start failing because the wall clock,
    // or a CI runner's clock skew, moved.
    const INSIDE_VALIDITY: u64 = 1_798_761_600; // 2027-01-01T00:00:00Z
    const PAST_EXPIRY: u64 = 5_049_129_600; // 2130-01-01T00:00:00Z

    fn load_certs(pem: &[u8]) -> Vec<CertificateDer<'static>> {
        let mut reader = Cursor::new(pem);
        rustls_pemfile::certs(&mut reader)
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap()
    }

    fn root_store(pem: &[u8]) -> RootCertStore {
        let mut store = RootCertStore::empty();
        for cert in load_certs(pem) {
            store.add(cert).unwrap();
        }
        store
    }

    /// Verify [`MISMATCH_PEM`] against `ca_pem` while connecting to "localhost",
    /// which the certificate is deliberately not issued for.
    fn verify_mismatched_leaf(
        ca_pem: &[u8],
        accept_invalid_hostnames: bool,
        now_secs: u64,
    ) -> std::result::Result<ServerCertVerified, TlsError> {
        ensure_crypto_provider();
        let verifier = CertCapturingVerifier::with_root_certificates(
            root_store(ca_pem),
            accept_invalid_hostnames,
        )
        .unwrap();
        let mut chain = load_certs(MISMATCH_PEM);
        let leaf = chain.remove(0);

        verifier.verify_server_cert(
            &leaf,
            &[],
            &ServerName::try_from("localhost").unwrap(),
            &[],
            UnixTime::since_unix_epoch(Duration::from_secs(now_secs)),
        )
    }

    fn is_name_mismatch(error: &TlsError) -> bool {
        matches!(
            error,
            TlsError::InvalidCertificate(
                CertificateError::NotValidForName | CertificateError::NotValidForNameContext { .. }
            )
        )
    }

    /// Regression: the certificate probe must accept exactly what sqlx accepts.
    ///
    /// sqlx enforces hostname matching for `verify-full` only, so under
    /// `verify-ca` the database connection succeeds against a certificate
    /// issued for another name. A probe that rejected it would report no
    /// expiry at all, silently dropping `dbpulse_tls_cert_expiry_days` for
    /// operators who connect by IP or through a service alias.
    #[test]
    fn verify_ca_accepts_a_certificate_issued_for_another_name() {
        let result = verify_mismatched_leaf(CA_PEM, true, INSIDE_VALIDITY);
        assert!(
            result.is_ok(),
            "verify-ca must tolerate a name mismatch like sqlx does, got: {:?}",
            result.err()
        );
    }

    /// The exemption is scoped: `verify-full` still checks the name.
    #[test]
    fn verify_full_rejects_a_certificate_issued_for_another_name() {
        let error = verify_mismatched_leaf(CA_PEM, false, INSIDE_VALIDITY)
            .expect_err("verify-full must reject a name mismatch");
        assert!(
            is_name_mismatch(&error),
            "expected a name-mismatch error, got: {error:?}"
        );
    }

    /// The exemption must not become a blanket accept: an untrusted issuer
    /// still fails even when name mismatches are tolerated.
    #[test]
    fn name_mismatch_exemption_still_rejects_an_untrusted_issuer() {
        let error = verify_mismatched_leaf(OTHER_CA_PEM, true, INSIDE_VALIDITY)
            .expect_err("a certificate from an unknown CA must never verify");
        assert!(
            !is_name_mismatch(&error),
            "expected an issuer error, not a name mismatch: {error:?}"
        );
    }

    /// Same guard for validity: an expired certificate fails under verify-ca.
    #[test]
    fn name_mismatch_exemption_still_rejects_an_expired_certificate() {
        let error = verify_mismatched_leaf(CA_PEM, true, PAST_EXPIRY)
            .expect_err("an expired certificate must never verify");
        assert!(
            !is_name_mismatch(&error),
            "expected an expiry error, not a name mismatch: {error:?}"
        );
    }

    /// Metadata is captured even when verification ultimately fails, so an
    /// operator can still be told *which* certificate was rejected.
    #[test]
    fn metadata_is_captured_during_verification() {
        ensure_crypto_provider();
        let verifier =
            CertCapturingVerifier::with_root_certificates(root_store(CA_PEM), false).unwrap();
        let mut chain = load_certs(MISMATCH_PEM);
        let leaf = chain.remove(0);

        let _ = verifier.verify_server_cert(
            &leaf,
            &[],
            &ServerName::try_from("localhost").unwrap(),
            &[],
            UnixTime::since_unix_epoch(Duration::from_secs(INSIDE_VALIDITY)),
        );

        let captured = verifier
            .get_captured()
            .expect("metadata should be captured");
        assert_eq!(captured.subject.as_deref(), Some("CN=db.internal"));
        assert_eq!(captured.issuer.as_deref(), Some("CN=dbpulse Test CA"));
    }

    #[test]
    fn test_verifier_creation() {
        ensure_crypto_provider();
        let verifier = CertCapturingVerifier::new();
        assert!(verifier.is_ok());
    }

    #[test]
    fn test_captured_initially_none() {
        ensure_crypto_provider();
        let verifier = CertCapturingVerifier::new().unwrap();
        assert!(verifier.get_captured().is_none());
    }

    #[test]
    fn test_metadata_conversion() {
        let captured = CapturedCertMetadata {
            subject: Some("CN=example.com".to_string()),
            issuer: Some("CN=CA".to_string()),
            expiry_days: Some(90),
        };

        let tls_metadata: TlsMetadata = captured.clone().into();
        assert_eq!(tls_metadata.cert_subject, captured.subject);
        assert_eq!(tls_metadata.cert_issuer, captured.issuer);
        assert_eq!(tls_metadata.cert_expiry_days, captured.expiry_days);
    }
}
