use super::{
    TlsConfig, TlsMetadata,
    probe::{TlsProbeProtocol, probe_certificate_expiry},
};
use anyhow::Result;
use dsn::DSN;
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::RwLock;

/// Certificate metadata cache with TTL.
///
/// Keyed by `host:port`, and dbpulse monitors a single DSN, so this holds one
/// entry in practice. Expiry is enforced on read: a stale entry is reported as
/// a miss and overwritten by the next probe.
pub struct CertCache {
    data: Arc<RwLock<HashMap<String, (TlsMetadata, Instant)>>>,
    ttl: Duration,
}

impl CertCache {
    /// Create a new certificate cache with the given TTL
    #[must_use]
    pub fn new(ttl: Duration) -> Self {
        Self {
            data: Arc::new(RwLock::new(HashMap::new())),
            ttl,
        }
    }

    /// Get cached certificate metadata if still valid
    pub async fn get(&self, key: &str) -> Option<TlsMetadata> {
        let cache = self.data.read().await;
        if let Some((metadata, timestamp)) = cache.get(key)
            && timestamp.elapsed() < self.ttl
        {
            return Some(metadata.clone());
        }
        drop(cache);
        None
    }

    /// Store certificate metadata in cache
    pub async fn set(&self, key: String, metadata: TlsMetadata) {
        let mut cache = self.data.write().await;
        cache.insert(key, (metadata, Instant::now()));
    }
}

/// Identify the probed endpoint for the cache.
///
/// The port comes from the DSN when it sets one, falling back to the driver's
/// default only when it does not. Keying unconditionally on the default port
/// caches a probe of `host:3307` as `host:3306`: harmless while dbpulse
/// monitors a single DSN, but wrong for any caller holding two DSNs on the
/// same host, which would read back the other endpoint's certificate.
fn cache_key(dsn: &DSN, default_port: u16) -> String {
    format!(
        "{}:{}",
        dsn.host.as_deref().unwrap_or(""),
        dsn.port.unwrap_or(default_port)
    )
}

/// Get certificate metadata with caching
///
/// This function demonstrates the recommended approach:
/// - Check cache first
/// - Only probe on cache miss
/// - Dramatically reduces connection overhead
///
/// # Errors
///
/// Returns an error if certificate probing fails
pub async fn get_cert_metadata_cached(
    dsn: &DSN,
    default_port: u16,
    protocol: TlsProbeProtocol,
    tls: &TlsConfig,
    cache: &CertCache,
) -> Result<Option<TlsMetadata>> {
    let cache_key = cache_key(dsn, default_port);

    // Try cache first
    if let Some(cached) = cache.get(&cache_key).await {
        return Ok(Some(cached));
    }

    // Cache miss - probe and cache result
    if let Some(metadata) = probe_certificate_expiry(dsn, default_port, protocol, tls).await? {
        cache.set(cache_key, metadata.clone()).await;
        Ok(Some(metadata))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[tokio::test]
    async fn test_cache_creation() {
        let cache = CertCache::new(Duration::from_mins(5));
        assert!(cache.get("test").await.is_none());
    }

    #[tokio::test]
    async fn test_cache_set_get() {
        let cache = CertCache::new(Duration::from_mins(5));
        let metadata = TlsMetadata {
            cert_subject: Some("CN=test".to_string()),
            cert_issuer: Some("CN=CA".to_string()),
            cert_expiry_days: Some(90),
            ..Default::default()
        };

        cache.set("test".to_string(), metadata.clone()).await;
        let retrieved = cache.get("test").await;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().cert_subject, metadata.cert_subject);
    }

    #[tokio::test]
    async fn test_cache_expiry() {
        let cache = CertCache::new(Duration::from_millis(100));
        let metadata = TlsMetadata {
            cert_subject: Some("CN=test".to_string()),
            ..Default::default()
        };

        cache.set("test".to_string(), metadata).await;
        assert!(cache.get("test").await.is_some());

        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(cache.get("test").await.is_none());
    }

    #[tokio::test]
    async fn test_cache_multiple_entries() {
        let cache = CertCache::new(Duration::from_mins(5));

        let metadata1 = TlsMetadata {
            cert_subject: Some("CN=server1".to_string()),
            cert_expiry_days: Some(30),
            ..Default::default()
        };

        let metadata2 = TlsMetadata {
            cert_subject: Some("CN=server2".to_string()),
            cert_expiry_days: Some(60),
            ..Default::default()
        };

        cache
            .set("server1:5432".to_string(), metadata1.clone())
            .await;
        cache
            .set("server2:3306".to_string(), metadata2.clone())
            .await;

        let retrieved1 = cache.get("server1:5432").await;
        let retrieved2 = cache.get("server2:3306").await;

        assert!(retrieved1.is_some());
        assert!(retrieved2.is_some());
        assert_eq!(retrieved1.unwrap().cert_subject, metadata1.cert_subject);
        assert_eq!(retrieved2.unwrap().cert_subject, metadata2.cert_subject);
    }

    #[tokio::test]
    async fn test_cache_overwrite() {
        let cache = CertCache::new(Duration::from_mins(5));

        let metadata1 = TlsMetadata {
            cert_subject: Some("CN=old".to_string()),
            cert_expiry_days: Some(10),
            ..Default::default()
        };

        let metadata2 = TlsMetadata {
            cert_subject: Some("CN=new".to_string()),
            cert_expiry_days: Some(90),
            ..Default::default()
        };

        cache.set("test".to_string(), metadata1).await;
        cache.set("test".to_string(), metadata2.clone()).await;

        let retrieved = cache.get("test").await;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().cert_subject, metadata2.cert_subject);
    }

    #[tokio::test]
    async fn test_cache_concurrent_access() {
        use std::sync::Arc;

        let cache = Arc::new(CertCache::new(Duration::from_mins(5)));
        let metadata = TlsMetadata {
            cert_subject: Some("CN=concurrent".to_string()),
            ..Default::default()
        };

        // Spawn multiple concurrent writes
        let mut handles = vec![];
        for i in 0..10 {
            let cache_clone = cache.clone();
            let metadata_clone = metadata.clone();
            let handle = tokio::spawn(async move {
                cache_clone.set(format!("key{i}"), metadata_clone).await;
            });
            handles.push(handle);
        }

        // Wait for all writes to complete
        for handle in handles {
            handle.await.unwrap();
        }

        // Verify all entries exist
        for i in 0..10 {
            assert!(cache.get(&format!("key{i}")).await.is_some());
        }
    }

    #[tokio::test]
    async fn test_cache_zero_ttl() {
        let cache = CertCache::new(Duration::from_secs(0));
        let metadata = TlsMetadata {
            cert_subject: Some("CN=test".to_string()),
            ..Default::default()
        };

        cache.set("test".to_string(), metadata).await;

        // With zero TTL, entry should expire immediately
        assert!(cache.get("test").await.is_none());
    }

    #[tokio::test]
    async fn test_cache_full_metadata() {
        let cache = CertCache::new(Duration::from_mins(5));
        let metadata = TlsMetadata {
            version: Some("TLSv1.3".to_string()),
            cipher: Some("TLS_AES_256_GCM_SHA384".to_string()),
            cert_subject: Some("CN=test.example.com,O=Test Org".to_string()),
            cert_issuer: Some("CN=Test CA,O=Test Org".to_string()),
            cert_expiry_days: Some(90),
        };

        cache.set("test".to_string(), metadata.clone()).await;
        let retrieved = cache.get("test").await;

        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.version, metadata.version);
        assert_eq!(retrieved.cipher, metadata.cipher);
        assert_eq!(retrieved.cert_subject, metadata.cert_subject);
        assert_eq!(retrieved.cert_issuer, metadata.cert_issuer);
        assert_eq!(retrieved.cert_expiry_days, metadata.cert_expiry_days);
    }

    /// Regression: the key must name the endpoint actually probed. The probe
    /// connects to `dsn.port` when set, but the key used the driver's default
    /// port unconditionally, so `host:3307` was cached as `host:3306`.
    #[test]
    fn cache_key_uses_the_dsn_port_when_set() {
        let dsn = dsn::parse("mysql://u:p@tcp(db.example.com:3307)/db").unwrap();
        assert_eq!(cache_key(&dsn, 3306), "db.example.com:3307");
    }

    #[test]
    fn cache_key_falls_back_to_the_default_port() {
        let dsn = dsn::parse("mysql://u:p@tcp(db.example.com)/db").unwrap();
        assert_eq!(cache_key(&dsn, 3306), "db.example.com:3306");
    }

    #[test]
    fn cache_key_distinguishes_ports_on_the_same_host() {
        let a = dsn::parse("postgres://u:p@tcp(db.example.com:5432)/db").unwrap();
        let b = dsn::parse("postgres://u:p@tcp(db.example.com:5433)/db").unwrap();
        assert_ne!(cache_key(&a, 5432), cache_key(&b, 5432));
    }
}
