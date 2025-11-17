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
}
