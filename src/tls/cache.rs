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

/// Certificate metadata cache with TTL
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

    /// Clear expired entries from cache
    pub async fn cleanup(&self) {
        let mut cache = self.data.write().await;
        cache.retain(|_, (_, timestamp)| timestamp.elapsed() < self.ttl);
    }
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
    let cache_key = format!("{}:{}", dsn.host.as_deref().unwrap_or(""), default_port);

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
        let cache = CertCache::new(Duration::from_secs(300));
        assert!(cache.get("test").await.is_none());
    }

    #[tokio::test]
    async fn test_cache_set_get() {
        let cache = CertCache::new(Duration::from_secs(300));
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
        let cache = CertCache::new(Duration::from_secs(300));

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
    async fn test_cache_cleanup() {
        let cache = CertCache::new(Duration::from_millis(100));

        let metadata = TlsMetadata {
            cert_subject: Some("CN=test".to_string()),
            ..Default::default()
        };

        cache.set("test1".to_string(), metadata.clone()).await;
        cache.set("test2".to_string(), metadata.clone()).await;

        // Wait for expiry
        tokio::time::sleep(Duration::from_millis(150)).await;

        // Add a fresh entry
        cache.set("test3".to_string(), metadata).await;

        // Cleanup should remove expired entries
        cache.cleanup().await;

        assert!(cache.get("test1").await.is_none());
        assert!(cache.get("test2").await.is_none());
        assert!(cache.get("test3").await.is_some());
    }

    #[tokio::test]
    async fn test_cache_overwrite() {
        let cache = CertCache::new(Duration::from_secs(300));

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

        let cache = Arc::new(CertCache::new(Duration::from_secs(300)));
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
        let cache = CertCache::new(Duration::from_secs(300));
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
}
