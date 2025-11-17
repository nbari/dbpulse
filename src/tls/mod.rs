//! TLS certificate observability and configuration module
//!
//! This module provides TLS certificate monitoring capabilities for database
//! connections, with support for both `PostgreSQL` and `MySQL`.
//!
//! # Module Organization
//!
//! - `config` - TLS configuration and modes
//! - `metadata` - Certificate metadata structures
//! - `probe` - Certificate probing and extraction
//! - `verifier` - Custom certificate verifiers
//! - `cache` - Certificate metadata caching
//!
//! # Example
//!
//! ```rust,ignore
//! use dbpulse::tls::{TlsConfig, TlsMode, TlsProbeProtocol, probe_certificate_expiry};
//!
//! let tls_config = TlsConfig {
//!     mode: TlsMode::VerifyFull,
//!     ca: Some("/etc/ssl/certs/ca.crt".into()),
//!     ..Default::default()
//! };
//!
//! let metadata = probe_certificate_expiry(
//!     &dsn,
//!     5432,
//!     TlsProbeProtocol::Postgres,
//!     &tls_config
//! ).await?;
//! ```

pub mod cache;
pub mod config;
pub mod metadata;
pub mod probe;
pub mod verifier;

// Re-export commonly used types
pub use cache::{CertCache, get_cert_metadata_cached};
pub use config::{TlsConfig, TlsMode};
pub use metadata::TlsMetadata;
pub use probe::{TlsProbeProtocol, ensure_crypto_provider, probe_certificate_expiry};
pub use verifier::{CapturedCertMetadata, CertCapturingVerifier};
