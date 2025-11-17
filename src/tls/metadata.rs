/// TLS connection metadata extracted after handshake
#[derive(Debug, Clone, Default)]
pub struct TlsMetadata {
    /// TLS protocol version (e.g., "TLSv1.3")
    pub version: Option<String>,
    /// Cipher suite used (e.g., `TLS_AES_256_GCM_SHA384`)
    pub cipher: Option<String>,
    /// Certificate subject DN
    pub cert_subject: Option<String>,
    /// Certificate issuer DN
    pub cert_issuer: Option<String>,
    /// Days until certificate expiration (negative if expired)
    pub cert_expiry_days: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tls_metadata_default() {
        let metadata = TlsMetadata::default();
        assert!(metadata.version.is_none());
        assert!(metadata.cipher.is_none());
        assert!(metadata.cert_subject.is_none());
        assert!(metadata.cert_issuer.is_none());
        assert!(metadata.cert_expiry_days.is_none());
    }

    #[test]
    fn test_tls_metadata_clone() {
        let metadata = TlsMetadata {
            version: Some("TLSv1.3".to_string()),
            cipher: Some("AES256-GCM".to_string()),
            cert_subject: Some("CN=example.com".to_string()),
            cert_issuer: Some("CN=CA".to_string()),
            cert_expiry_days: Some(90),
        };

        let cloned = metadata.clone();
        assert_eq!(cloned.version, metadata.version);
        assert_eq!(cloned.cert_expiry_days, metadata.cert_expiry_days);
    }
}
