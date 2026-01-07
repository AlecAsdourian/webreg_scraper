//! HTTP client for degree audit operations.
//!
//! Handles the job-queue pattern:
//! 1. POST/GET to create.html triggers audit generation
//! 2. Follow 302 redirect to list.html?autoPoll=true
//! 3. Parse list.html to discover job ID
//! 4. Poll until job completes
//! 5. Fetch read.html?id=... to get the audit HTML

use super::cache::{AuditCacheState, SessionKey};
use super::error::DegreeAuditError;
use super::job::{parse_newest_job, page_indicates_processing, AuditJob};
use super::types::DegreeAudit;
use super::{parse_degree_audit_html, DegreeAuditResponse};
use rand::Rng;
use reqwest::header::{COOKIE, LOCATION};
use reqwest::redirect::Policy;
use reqwest::{Client, StatusCode};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, error, info, warn};
use url::Url;

/// Base URL for the degree audit system.
const DARS_BASE_URL: &str = "https://act.ucsd.edu/studentDarsSelfservice";

/// Paths for degree audit endpoints.
const CREATE_PATH: &str = "/audit/create.html";
const LIST_PATH: &str = "/audit/list.html";
const READ_PATH: &str = "/audit/read.html";

/// Configuration for the degree audit client.
#[derive(Debug, Clone)]
pub struct DegreeAuditConfig {
    /// Base URL for DARS (degree audit system)
    pub base_url: String,
    /// Maximum number of poll attempts
    pub max_poll_attempts: u32,
    /// Base delay between polls (will use exponential backoff)
    pub poll_interval_base: Duration,
    /// Maximum total time to wait for job completion
    pub max_poll_timeout: Duration,
    /// User agent string
    pub user_agent: String,
}

impl Default for DegreeAuditConfig {
    fn default() -> Self {
        Self {
            base_url: DARS_BASE_URL.to_string(),
            max_poll_attempts: 30,
            poll_interval_base: Duration::from_millis(500),
            max_poll_timeout: Duration::from_secs(120),
            user_agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36".to_string(),
        }
    }
}

/// Client for fetching degree audits from UCSD's DARS system.
pub struct DegreeAuditClient {
    /// HTTP client configured for manual redirects on create
    client_no_redirect: Client,
    /// HTTP client that follows redirects (for list/read)
    client_with_redirect: Client,
    /// Configuration
    config: DegreeAuditConfig,
    /// Cache and circuit breaker state
    cache_state: Arc<AuditCacheState>,
}

impl DegreeAuditClient {
    /// Creates a new degree audit client with default configuration.
    pub fn new(cache_state: Arc<AuditCacheState>) -> Result<Self, DegreeAuditError> {
        Self::with_config(DegreeAuditConfig::default(), cache_state)
    }

    /// Creates a new client with custom configuration.
    pub fn with_config(
        config: DegreeAuditConfig,
        cache_state: Arc<AuditCacheState>,
    ) -> Result<Self, DegreeAuditError> {
        // Client with NO redirects - for create.html so we can inspect Location header
        let client_no_redirect = Client::builder()
            .redirect(Policy::none())
            .user_agent(&config.user_agent)
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| DegreeAuditError::Network {
                message: format!("Failed to build HTTP client: {}", e),
            })?;

        // Client that follows redirects - for list/read
        let client_with_redirect = Client::builder()
            .redirect(Policy::limited(10))
            .user_agent(&config.user_agent)
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| DegreeAuditError::Network {
                message: format!("Failed to build HTTP client: {}", e),
            })?;

        Ok(Self {
            client_no_redirect,
            client_with_redirect,
            config,
            cache_state,
        })
    }

    /// Fetches the degree audit, using cache if available.
    ///
    /// This is the main entry point for getting a degree audit.
    ///
    /// # Arguments
    /// * `cookies` - The authentication cookies (from Puppeteer auth server)
    /// * `force_refresh` - If true, bypass cache and fetch fresh data
    ///
    /// # Returns
    /// * `Ok(DegreeAudit)` - The parsed degree audit
    /// * `Err(DegreeAuditError)` - If the operation fails
    pub async fn get_or_create_audit(
        &self,
        cookies: &str,
        force_refresh: bool,
    ) -> Result<DegreeAudit, DegreeAuditError> {
        let correlation_id = generate_correlation_id();
        let session_key = SessionKey::from_cookie(cookies);

        info!(
            correlation_id = %correlation_id,
            session = %session_key,
            "Starting degree audit retrieval"
        );

        // Check circuit breaker
        if self.cache_state.circuit_breaker.is_open() {
            warn!(
                correlation_id = %correlation_id,
                "Circuit breaker is open, rejecting request"
            );
            return Err(DegreeAuditError::CircuitBreakerOpen);
        }

        // Check cache first (unless force_refresh)
        if !force_refresh {
            if let Some(cached) = self.cache_state.cache.get(&session_key) {
                info!(
                    correlation_id = %correlation_id,
                    "Returning cached degree audit"
                );
                return Ok(cached);
            }
        }

        // Acquire per-session lock to prevent concurrent operations
        let lock = self.cache_state.get_session_lock(&session_key);
        let _guard = lock.lock().await;

        // Double-check cache after acquiring lock
        if !force_refresh {
            if let Some(cached) = self.cache_state.cache.get(&session_key) {
                info!(
                    correlation_id = %correlation_id,
                    "Returning cached degree audit (post-lock)"
                );
                return Ok(cached);
            }
        }

        // Execute the full audit flow
        let start = Instant::now();
        let result = self.execute_audit_flow(cookies, &correlation_id).await;

        match &result {
            Ok(audit) => {
                self.cache_state.circuit_breaker.record_success();
                self.cache_state.cache.insert(session_key, audit.clone());
                info!(
                    correlation_id = %correlation_id,
                    duration_ms = start.elapsed().as_millis() as u64,
                    "Degree audit completed successfully"
                );
            }
            Err(e) => {
                if e.is_retryable() {
                    self.cache_state.circuit_breaker.record_failure();
                }
                error!(
                    correlation_id = %correlation_id,
                    error = %e,
                    duration_ms = start.elapsed().as_millis() as u64,
                    "Degree audit failed"
                );
            }
        }

        result
    }

    /// Executes the full audit flow: create -> discover -> poll -> fetch -> parse.
    async fn execute_audit_flow(
        &self,
        cookies: &str,
        correlation_id: &str,
    ) -> Result<DegreeAudit, DegreeAuditError> {
        // Step 1: Trigger audit creation
        let list_url = self.trigger_create(cookies, correlation_id).await?;

        // Step 2: Discover job from list page
        let job = self
            .fetch_list_and_discover(&list_url, cookies, correlation_id)
            .await?;

        // Step 3: Poll until ready (if not already complete)
        let ready_job_id = if job.status.is_ready() {
            info!(
                correlation_id = %correlation_id,
                job_id = %job.job_id,
                "Job already complete, skipping poll"
            );
            job.job_id
        } else {
            self.poll_until_ready(job, cookies, correlation_id).await?
        };

        // Step 4: Fetch the audit HTML
        let html = self
            .fetch_audit_html(&ready_job_id, cookies, correlation_id)
            .await?;

        // Step 5: Parse the HTML
        let raw_response = DegreeAuditResponse {
            audit_id: ready_job_id.clone(),
            scraped_at: chrono::Utc::now().to_rfc3339(),
            url: format!("{}{}?id={}", self.config.base_url, READ_PATH, ready_job_id),
            html,
        };

        parse_degree_audit_html(&raw_response).map_err(|e| DegreeAuditError::ParseError {
            message: e.to_string(),
        })
    }

    /// Step 1: Triggers audit creation by calling create.html.
    ///
    /// Returns the redirect URL (should be list.html?autoPoll=true).
    async fn trigger_create(
        &self,
        cookies: &str,
        correlation_id: &str,
    ) -> Result<String, DegreeAuditError> {
        let url = format!("{}{}", self.config.base_url, CREATE_PATH);
        info!(
            correlation_id = %correlation_id,
            url = %url,
            "Triggering audit creation"
        );

        let response = self
            .client_no_redirect
            .get(&url)
            .header(COOKIE, cookies)
            .send()
            .await?;

        // Check for session expiry first
        self.check_session_valid(&response, correlation_id)?;

        match response.status() {
            StatusCode::FOUND | StatusCode::SEE_OTHER | StatusCode::MOVED_PERMANENTLY => {
                let location = response
                    .headers()
                    .get(LOCATION)
                    .and_then(|h| h.to_str().ok())
                    .ok_or_else(|| DegreeAuditError::UnexpectedResponse {
                        message: "302 response missing Location header".to_string(),
                    })?;

                // Validate it's redirecting to list.html
                if !location.contains("list.html") && !location.contains("list") {
                    warn!(
                        correlation_id = %correlation_id,
                        location = %location,
                        "Unexpected redirect location (expected list.html)"
                    );
                }

                info!(
                    correlation_id = %correlation_id,
                    location = %location,
                    "Create redirected successfully"
                );

                // Build absolute URL if relative
                let absolute_url = if location.starts_with("http") {
                    location.to_string()
                } else if location.starts_with('/') {
                    // Absolute path
                    let base = Url::parse(&self.config.base_url)?;
                    format!("{}://{}{}", base.scheme(), base.host_str().unwrap_or(""), location)
                } else {
                    // Relative path
                    format!("{}/{}", self.config.base_url, location)
                };

                Ok(absolute_url)
            }
            StatusCode::OK => {
                // Some systems return 200 with the list page directly
                warn!(
                    correlation_id = %correlation_id,
                    "Create returned 200 instead of redirect, using list.html directly"
                );
                Ok(format!("{}{}?autoPoll=true", self.config.base_url, LIST_PATH))
            }
            status => Err(DegreeAuditError::UnexpectedResponse {
                message: format!(
                    "Expected 302 redirect from create.html, got {}",
                    status
                ),
            }),
        }
    }

    /// Step 2: Fetches list.html and discovers the newest job.
    async fn fetch_list_and_discover(
        &self,
        list_url: &str,
        cookies: &str,
        correlation_id: &str,
    ) -> Result<AuditJob, DegreeAuditError> {
        info!(
            correlation_id = %correlation_id,
            url = %list_url,
            "Fetching job list"
        );

        let response = self
            .client_with_redirect
            .get(list_url)
            .header(COOKIE, cookies)
            .send()
            .await?;

        self.check_session_valid(&response, correlation_id)?;

        if !response.status().is_success() {
            return Err(DegreeAuditError::UnexpectedResponse {
                message: format!("list.html returned status {}", response.status()),
            });
        }

        let html = response.text().await?;

        // Check if page indicates processing
        if page_indicates_processing(&html) {
            debug!(
                correlation_id = %correlation_id,
                "List page indicates job is processing"
            );
        }

        let job = parse_newest_job(&html)?;
        info!(
            correlation_id = %correlation_id,
            job_id = %job.job_id,
            status = ?job.status,
            "Discovered job from list"
        );

        Ok(job)
    }

    /// Step 3: Polls until the job is ready.
    async fn poll_until_ready(
        &self,
        initial_job: AuditJob,
        cookies: &str,
        correlation_id: &str,
    ) -> Result<String, DegreeAuditError> {
        let start = Instant::now();
        let mut attempts = 0u32;
        let mut current_job = initial_job;

        info!(
            correlation_id = %correlation_id,
            job_id = %current_job.job_id,
            "Starting poll for job completion"
        );

        loop {
            // Check if job is ready
            if current_job.status.is_ready() {
                info!(
                    correlation_id = %correlation_id,
                    job_id = %current_job.job_id,
                    attempts = attempts,
                    elapsed_ms = start.elapsed().as_millis() as u64,
                    "Job is ready"
                );
                return Ok(current_job.job_id);
            }

            // Check if job failed
            if current_job.status.is_failed() {
                return Err(DegreeAuditError::JobFailed {
                    reason: format!("{:?}", current_job.status),
                });
            }

            // Check limits
            attempts += 1;
            if attempts > self.config.max_poll_attempts {
                return Err(DegreeAuditError::PollTimeout {
                    attempts,
                    elapsed_secs: start.elapsed().as_secs_f64(),
                });
            }

            if start.elapsed() > self.config.max_poll_timeout {
                return Err(DegreeAuditError::PollTimeout {
                    attempts,
                    elapsed_secs: start.elapsed().as_secs_f64(),
                });
            }

            // Calculate delay with exponential backoff and jitter
            let delay = self.calculate_poll_delay(attempts);
            debug!(
                correlation_id = %correlation_id,
                attempt = attempts,
                delay_ms = delay.as_millis() as u64,
                "Waiting before next poll"
            );
            tokio::time::sleep(delay).await;

            // Re-fetch list page
            let list_url = format!("{}{}?autoPoll=true", self.config.base_url, LIST_PATH);
            current_job = self
                .fetch_list_and_discover(&list_url, cookies, correlation_id)
                .await?;
        }
    }

    /// Calculates poll delay with exponential backoff and jitter.
    fn calculate_poll_delay(&self, attempt: u32) -> Duration {
        let base = self.config.poll_interval_base.as_millis() as u64;
        // Exponential backoff: base * 2^min(attempt-1, 5)
        let exponential = base * 2u64.pow(attempt.saturating_sub(1).min(5));
        // Cap at 10 seconds
        let capped = exponential.min(10_000);
        // Add jitter: 0-20% of the delay
        let jitter = rand::thread_rng().gen_range(0..=(capped / 5));
        Duration::from_millis(capped + jitter)
    }

    /// Step 4: Fetches the completed audit HTML from read.html.
    async fn fetch_audit_html(
        &self,
        job_id: &str,
        cookies: &str,
        correlation_id: &str,
    ) -> Result<String, DegreeAuditError> {
        // URL-encode the job ID for the query parameter
        let encoded_job_id = urlencoding::encode(job_id);
        let url = format!(
            "{}{}?id={}",
            self.config.base_url, READ_PATH, encoded_job_id
        );

        info!(
            correlation_id = %correlation_id,
            url = %url,
            "Fetching audit report"
        );

        let response = self
            .client_with_redirect
            .get(&url)
            .header(COOKIE, cookies)
            .send()
            .await?;

        self.check_session_valid(&response, correlation_id)?;

        if !response.status().is_success() {
            return Err(DegreeAuditError::UnexpectedResponse {
                message: format!("read.html returned status {}", response.status()),
            });
        }

        let html = response.text().await?;

        // Basic validation that we got an audit page
        if html.len() < 1000 {
            warn!(
                correlation_id = %correlation_id,
                html_len = html.len(),
                "Audit HTML seems too short"
            );
        }

        Ok(html)
    }

    /// Checks if the response indicates a valid session.
    ///
    /// Returns an error if redirected to SSO/login page.
    fn check_session_valid(
        &self,
        response: &reqwest::Response,
        correlation_id: &str,
    ) -> Result<(), DegreeAuditError> {
        let url = response.url().as_str();

        // Check for SSO/login redirects
        let sso_indicators = [
            "login.ucsd.edu",
            "sso.ucsd.edu",
            "shib",
            "shibboleth",
            "idp",
            "saml",
            "login?",
            "/login",
            "auth.ucsd.edu",
        ];

        for indicator in &sso_indicators {
            if url.to_lowercase().contains(indicator) {
                warn!(
                    correlation_id = %correlation_id,
                    url = %url,
                    "Session expired - redirected to SSO"
                );
                return Err(DegreeAuditError::SessionExpired {
                    redirect_url: url.to_string(),
                });
            }
        }

        Ok(())
    }

    /// Invalidates the cache for a specific session.
    pub fn invalidate_cache(&self, cookies: &str) {
        let session_key = SessionKey::from_cookie(cookies);
        self.cache_state.cache.invalidate(&session_key);
    }

    /// Returns cache statistics.
    pub fn cache_stats(&self) -> super::cache::CacheStats {
        self.cache_state.cache.stats()
    }
}

/// URL encoding helper.
mod urlencoding {
    pub fn encode(s: &str) -> String {
        let mut result = String::with_capacity(s.len() * 3);
        for c in s.chars() {
            match c {
                'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => {
                    result.push(c);
                }
                _ => {
                    for byte in c.to_string().as_bytes() {
                        result.push_str(&format!("%{:02X}", byte));
                    }
                }
            }
        }
        result
    }
}

/// Generates a unique correlation ID for request tracing.
fn generate_correlation_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros();
    let random: u32 = rand::thread_rng().gen();
    format!("{:x}-{:08x}", timestamp & 0xFFFFFFFF, random)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_url_encoding() {
        assert_eq!(
            urlencoding::encode("JobQueueRun!!!!ABC"),
            "JobQueueRun%21%21%21%21ABC"
        );
    }

    #[test]
    fn test_poll_delay_backoff() {
        let cache_state = Arc::new(AuditCacheState::new());
        let client = DegreeAuditClient::new(cache_state).unwrap();

        let d1 = client.calculate_poll_delay(1);
        let d2 = client.calculate_poll_delay(2);
        let d3 = client.calculate_poll_delay(3);

        // Each should be roughly double (with jitter)
        assert!(d2 > d1);
        assert!(d3 > d2);
    }
}
