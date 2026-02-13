use super::TlsMetadata;
use anyhow::{Result, anyhow};
use chrono::Utc;
use rustls::{
    DigitallySignedStruct, Error as TlsError, RootCertStore, SignatureScheme,
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
}

impl fmt::Debug for CertCapturingVerifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CertCapturingVerifier")
            .field("captured", &self.captured)
            .field("inner_verifier", &"WebPkiServerVerifier")
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
        })
    }

    /// Create a verifier with custom root certificates
    ///
    /// # Errors
    ///
    /// Returns an error if the `WebPKI` verifier cannot be built or if certificates are invalid
    pub fn with_root_certificates(root_store: RootCertStore) -> Result<Self> {
        let inner_verifier = WebPkiServerVerifier::builder(Arc::new(root_store))
            .build()
            .map_err(|e| anyhow!("failed to build WebPKI verifier: {e}"))?;

        Ok(Self {
            captured: Arc::new(Mutex::new(None)),
            inner_verifier,
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
        let expiry_days = Some(remaining.num_days());

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
    /// 3. Returns the verification result unchanged
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
        self.inner_verifier.verify_server_cert(
            end_entity,
            intermediates,
            server_name,
            ocsp_response,
            now,
        )
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

    fn ensure_crypto_provider() {
        let _ = rustls::crypto::CryptoProvider::install_default(default_provider());
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
