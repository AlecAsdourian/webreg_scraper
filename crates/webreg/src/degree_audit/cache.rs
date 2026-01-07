//! TTL-based caching for degree audit results.

use super::types::DegreeAudit;
use dashmap::DashMap;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A session key derived from cookies, used for cache lookups and locking.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct SessionKey(String);

impl SessionKey {
    /// Creates a session key from raw cookie data.
    ///
    /// The cookie value is hashed to avoid storing sensitive session tokens.
    pub fn from_cookie(cookie_value: &str) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(cookie_value.as_bytes());
        let result = hasher.finalize();
        // Use first 16 bytes as hex string
        let hash = hex::encode(&result[..16]);
        Self(hash)
    }

    /// Creates a session key from a JSESSIONID cookie specifically.
    pub fn from_jsessionid(jsessionid: &str) -> Self {
        Self::from_cookie(jsessionid)
    }

    /// Returns the internal hash string (for logging/debugging).
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SessionKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Only show first 8 chars for privacy
        write!(f, "{}...", &self.0[..8.min(self.0.len())])
    }
}

/// A cached audit result with metadata.
#[derive(Clone)]
struct CachedAudit {
    /// The cached audit data
    result: DegreeAudit,
    /// When this entry was cached
    cached_at: Instant,
    /// TTL for this specific entry
    ttl: Duration,
}

/// Thread-safe cache for degree audit results.
///
/// Uses DashMap for concurrent access without external locking.
pub struct AuditCache {
    entries: DashMap<SessionKey, CachedAudit>,
    default_ttl: Duration,
}

impl AuditCache {
    /// Creates a new cache with the specified default TTL.
    pub fn new(default_ttl: Duration) -> Self {
        Self {
            entries: DashMap::new(),
            default_ttl,
        }
    }

    /// Creates a cache with a 5-minute default TTL.
    pub fn with_default_ttl() -> Self {
        Self::new(Duration::from_secs(5 * 60))
    }

    /// Gets a cached audit if it exists and hasn't expired.
    pub fn get(&self, key: &SessionKey) -> Option<DegreeAudit> {
        self.entries.get(key).and_then(|entry| {
            if entry.cached_at.elapsed() < entry.ttl {
                Some(entry.result.clone())
            } else {
                // Entry expired, remove it
                drop(entry);
                self.entries.remove(key);
                None
            }
        })
    }

    /// Inserts an audit result into the cache with the default TTL.
    pub fn insert(&self, key: SessionKey, result: DegreeAudit) {
        self.insert_with_ttl(key, result, self.default_ttl);
    }

    /// Inserts an audit result with a custom TTL.
    pub fn insert_with_ttl(&self, key: SessionKey, result: DegreeAudit, ttl: Duration) {
        self.entries.insert(
            key,
            CachedAudit {
                result,
                cached_at: Instant::now(),
                ttl,
            },
        );
    }

    /// Invalidates (removes) a cached entry.
    pub fn invalidate(&self, key: &SessionKey) {
        self.entries.remove(key);
    }

    /// Clears all entries from the cache.
    pub fn clear(&self) {
        self.entries.clear();
    }

    /// Returns the number of entries in the cache (including expired ones).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true if the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Removes expired entries from the cache.
    ///
    /// Call this periodically if you want proactive cleanup.
    pub fn cleanup_expired(&self) {
        self.entries
            .retain(|_, entry| entry.cached_at.elapsed() < entry.ttl);
    }

    /// Gets cache statistics.
    pub fn stats(&self) -> CacheStats {
        let mut total = 0;
        let mut expired = 0;

        for entry in self.entries.iter() {
            total += 1;
            if entry.cached_at.elapsed() >= entry.ttl {
                expired += 1;
            }
        }

        CacheStats {
            total_entries: total,
            expired_entries: expired,
            active_entries: total - expired,
        }
    }
}

impl Default for AuditCache {
    fn default() -> Self {
        Self::with_default_ttl()
    }
}

/// Cache statistics for monitoring.
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub total_entries: usize,
    pub expired_entries: usize,
    pub active_entries: usize,
}

/// Circuit breaker for protecting against repeated failures.
pub struct CircuitBreaker {
    failure_count: std::sync::atomic::AtomicU32,
    last_failure: std::sync::Mutex<Option<Instant>>,
    threshold: u32,
    recovery_time: Duration,
}

impl CircuitBreaker {
    /// Creates a new circuit breaker.
    ///
    /// - `threshold`: Number of failures before the breaker opens
    /// - `recovery_time`: How long to wait before allowing requests again
    pub fn new(threshold: u32, recovery_time: Duration) -> Self {
        Self {
            failure_count: std::sync::atomic::AtomicU32::new(0),
            last_failure: std::sync::Mutex::new(None),
            threshold,
            recovery_time,
        }
    }

    /// Creates a circuit breaker with default settings (5 failures, 30s recovery).
    pub fn with_defaults() -> Self {
        Self::new(5, Duration::from_secs(30))
    }

    /// Returns true if the circuit breaker is open (blocking requests).
    pub fn is_open(&self) -> bool {
        let count = self
            .failure_count
            .load(std::sync::atomic::Ordering::Relaxed);
        if count < self.threshold {
            return false;
        }

        // Check if recovery time has passed
        if let Ok(guard) = self.last_failure.lock() {
            if let Some(last) = *guard {
                if last.elapsed() > self.recovery_time {
                    // Reset and allow requests
                    drop(guard);
                    self.reset();
                    return false;
                }
            }
        }

        true
    }

    /// Records a successful operation, resetting the failure count.
    pub fn record_success(&self) {
        self.failure_count
            .store(0, std::sync::atomic::Ordering::Relaxed);
    }

    /// Records a failed operation.
    pub fn record_failure(&self) {
        self.failure_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if let Ok(mut guard) = self.last_failure.lock() {
            *guard = Some(Instant::now());
        }
    }

    /// Resets the circuit breaker state.
    pub fn reset(&self) {
        self.failure_count
            .store(0, std::sync::atomic::Ordering::Relaxed);
        if let Ok(mut guard) = self.last_failure.lock() {
            *guard = None;
        }
    }

    /// Returns the current failure count.
    pub fn failure_count(&self) -> u32 {
        self.failure_count
            .load(std::sync::atomic::Ordering::Relaxed)
    }
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self::with_defaults()
    }
}

/// Helper module for hex encoding (avoiding extra dependency).
mod hex {
    pub fn encode(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{:02x}", b)).collect()
    }
}

/// Shared state wrapper combining cache and circuit breaker.
pub struct AuditCacheState {
    pub cache: AuditCache,
    pub circuit_breaker: CircuitBreaker,
    /// Per-session locks to prevent concurrent operations
    pub session_locks: DashMap<SessionKey, Arc<tokio::sync::Mutex<()>>>,
}

impl AuditCacheState {
    /// Creates a new cache state with default settings.
    pub fn new() -> Self {
        Self {
            cache: AuditCache::with_default_ttl(),
            circuit_breaker: CircuitBreaker::with_defaults(),
            session_locks: DashMap::new(),
        }
    }

    /// Creates a new cache state with custom TTL.
    pub fn with_ttl(ttl: Duration) -> Self {
        Self {
            cache: AuditCache::new(ttl),
            circuit_breaker: CircuitBreaker::with_defaults(),
            session_locks: DashMap::new(),
        }
    }

    /// Gets or creates a lock for the given session.
    pub fn get_session_lock(&self, key: &SessionKey) -> Arc<tokio::sync::Mutex<()>> {
        self.session_locks
            .entry(key.clone())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }
}

impl Default for AuditCacheState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_key_hashing() {
        let key1 = SessionKey::from_cookie("session123");
        let key2 = SessionKey::from_cookie("session123");
        let key3 = SessionKey::from_cookie("session456");

        assert_eq!(key1, key2);
        assert_ne!(key1, key3);
    }

    #[test]
    fn test_circuit_breaker_threshold() {
        let cb = CircuitBreaker::new(3, Duration::from_secs(1));

        assert!(!cb.is_open());
        cb.record_failure();
        assert!(!cb.is_open());
        cb.record_failure();
        assert!(!cb.is_open());
        cb.record_failure();
        assert!(cb.is_open());

        cb.record_success();
        assert!(!cb.is_open());
    }
}
