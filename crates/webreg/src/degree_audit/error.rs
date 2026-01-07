//! Error types for the degree audit subsystem.

use thiserror::Error;

/// Errors that can occur during degree audit operations.
#[derive(Debug, Error, Clone)]
pub enum DegreeAuditError {
    /// Network/HTTP request failed
    #[error("Network error: {message}")]
    Network { message: String },

    /// Session has expired - client was redirected to SSO login
    #[error("Session expired, redirected to: {redirect_url}")]
    SessionExpired { redirect_url: String },

    /// No active session (missing required cookies)
    #[error("No active session: {message}")]
    NoSession { message: String },

    /// Server returned an unexpected response
    #[error("Unexpected response: {message}")]
    UnexpectedResponse { message: String },

    /// Could not find any audit job in list.html
    #[error("No audit job found in list page")]
    NoJobFound,

    /// The audit job failed on the server side
    #[error("Audit job failed: {reason}")]
    JobFailed { reason: String },

    /// Polling timed out waiting for job completion
    #[error("Poll timeout after {attempts} attempts ({elapsed_secs:.1}s elapsed)")]
    PollTimeout { attempts: u32, elapsed_secs: f64 },

    /// Failed to parse HTML content
    #[error("Parse error: {message}")]
    ParseError { message: String },

    /// URL parsing/construction failed
    #[error("URL error: {message}")]
    UrlError { message: String },

    /// Circuit breaker is open due to repeated failures
    #[error("Circuit breaker open - too many recent failures")]
    CircuitBreakerOpen,

    /// An operation is already in progress for this session
    #[error("Audit operation already in progress for this session")]
    OperationInProgress,

    /// Cookie fetch from auth server failed
    #[error("Failed to fetch cookies from auth server: {message}")]
    CookieFetchError { message: String },
}

impl DegreeAuditError {
    /// Returns true if this error indicates the session needs to be refreshed.
    pub fn needs_reauth(&self) -> bool {
        matches!(
            self,
            DegreeAuditError::SessionExpired { .. } | DegreeAuditError::NoSession { .. }
        )
    }

    /// Returns true if this error is potentially transient and retryable.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            DegreeAuditError::Network { .. }
                | DegreeAuditError::PollTimeout { .. }
                | DegreeAuditError::UnexpectedResponse { .. }
        )
    }
}

impl From<reqwest::Error> for DegreeAuditError {
    fn from(err: reqwest::Error) -> Self {
        DegreeAuditError::Network {
            message: err.to_string(),
        }
    }
}

impl From<url::ParseError> for DegreeAuditError {
    fn from(err: url::ParseError) -> Self {
        DegreeAuditError::UrlError {
            message: err.to_string(),
        }
    }
}

impl From<std::io::Error> for DegreeAuditError {
    fn from(err: std::io::Error) -> Self {
        DegreeAuditError::Network {
            message: err.to_string(),
        }
    }
}
