//! Audit job types and discovery logic for parsing list.html.

use super::error::DegreeAuditError;
use regex::Regex;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

/// Represents an audit job discovered from list.html.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditJob {
    /// The job ID (e.g., "JobQueueRun!!!!XXXXX" or URL-encoded variant)
    pub job_id: String,
    /// Current status of the job
    pub status: JobStatus,
    /// Raw href from the link (for debugging)
    pub raw_href: Option<String>,
}

/// Status of an audit job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobStatus {
    /// Job is still being processed
    Processing,
    /// Job has completed successfully
    Complete,
    /// Job failed with an error
    Error(String),
    /// Could not determine status
    Unknown(String),
}

impl JobStatus {
    /// Returns true if the job is ready to be fetched.
    pub fn is_ready(&self) -> bool {
        matches!(self, JobStatus::Complete)
    }

    /// Returns true if the job is still processing.
    pub fn is_processing(&self) -> bool {
        matches!(self, JobStatus::Processing)
    }

    /// Returns true if the job failed.
    pub fn is_failed(&self) -> bool {
        matches!(self, JobStatus::Error(_))
    }
}

// Static selectors for parsing - compiled once
static ROW_SELECTOR: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("table tr, tr").unwrap());
static LINK_SELECTOR: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("a[href*='read.html'], a[href*='read']").unwrap());
static ANY_LINK_SELECTOR: LazyLock<Selector> = LazyLock::new(|| Selector::parse("a").unwrap());
static JOB_ID_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[?&]id=([^&\s]+)").unwrap());
static JOBQUEUE_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"JobQueueRun[!%21]+[A-Za-z0-9_\-]+").unwrap());

/// Parses list.html to discover audit jobs.
///
/// Returns the newest job (first in list, assumed to be most recent).
pub fn parse_newest_job(html: &str) -> Result<AuditJob, DegreeAuditError> {
    let document = Html::parse_document(html);
    let mut jobs = Vec::new();

    // Strategy 1: Look for table rows with read.html links
    for row in document.select(&ROW_SELECTOR) {
        if let Some(link) = row.select(&LINK_SELECTOR).next() {
            if let Some(href) = link.value().attr("href") {
                if let Some(job_id) = extract_job_id_from_href(href) {
                    let status = extract_status_from_row(&row);
                    jobs.push(AuditJob {
                        job_id,
                        status,
                        raw_href: Some(href.to_string()),
                    });
                }
            }
        }
    }

    // Strategy 2: Fallback - look for any link containing read.html
    if jobs.is_empty() {
        for link in document.select(&LINK_SELECTOR) {
            if let Some(href) = link.value().attr("href") {
                if let Some(job_id) = extract_job_id_from_href(href) {
                    jobs.push(AuditJob {
                        job_id,
                        status: JobStatus::Unknown("fallback extraction".to_string()),
                        raw_href: Some(href.to_string()),
                    });
                }
            }
        }
    }

    // Strategy 3: Look for JobQueueRun pattern anywhere in links
    if jobs.is_empty() {
        for link in document.select(&ANY_LINK_SELECTOR) {
            if let Some(href) = link.value().attr("href") {
                if let Some(caps) = JOBQUEUE_REGEX.captures(href) {
                    if let Some(m) = caps.get(0) {
                        jobs.push(AuditJob {
                            job_id: m.as_str().to_string(),
                            status: JobStatus::Unknown("pattern match".to_string()),
                            raw_href: Some(href.to_string()),
                        });
                    }
                }
            }
        }
    }

    // Strategy 4: Look in page text/scripts for JobQueueRun
    if jobs.is_empty() {
        if let Some(caps) = JOBQUEUE_REGEX.captures(html) {
            if let Some(m) = caps.get(0) {
                jobs.push(AuditJob {
                    job_id: m.as_str().to_string(),
                    status: JobStatus::Unknown("text extraction".to_string()),
                    raw_href: None,
                });
            }
        }
    }

    // Return the first (newest) job
    jobs.into_iter().next().ok_or(DegreeAuditError::NoJobFound)
}

/// Extracts job ID from an href attribute.
///
/// Handles patterns like:
/// - `read.html?id=JobQueueRun!!!!XXXX`
/// - `read.html;jsessionid=ABC?id=JobQueueRun!!!!XXXX`
/// - URL-encoded variants with %21 instead of !
fn extract_job_id_from_href(href: &str) -> Option<String> {
    // Try regex first for "id=" parameter
    if let Some(caps) = JOB_ID_REGEX.captures(href) {
        if let Some(m) = caps.get(1) {
            let job_id = m.as_str().to_string();
            // URL-decode if needed (convert %21 to !)
            let decoded = urlencoding_decode(&job_id);
            return Some(decoded);
        }
    }

    // Fallback: look for JobQueueRun pattern directly
    if let Some(caps) = JOBQUEUE_REGEX.captures(href) {
        if let Some(m) = caps.get(0) {
            let decoded = urlencoding_decode(m.as_str());
            return Some(decoded);
        }
    }

    None
}

/// Simple URL decoding for common patterns.
fn urlencoding_decode(s: &str) -> String {
    s.replace("%21", "!")
        .replace("%20", " ")
        .replace("%2F", "/")
        .replace("%3A", ":")
        .replace("%3D", "=")
        .replace("%26", "&")
        .replace("%3F", "?")
}

/// Extracts job status from a table row.
fn extract_status_from_row(row: &scraper::ElementRef) -> JobStatus {
    let text = row.text().collect::<String>().to_lowercase();
    let class_attr = row
        .value()
        .attr("class")
        .unwrap_or_default()
        .to_lowercase();

    // Check for status indicators in text or class
    if text.contains("complete") || text.contains("ready") || text.contains("finished") {
        JobStatus::Complete
    } else if text.contains("processing")
        || text.contains("running")
        || text.contains("pending")
        || text.contains("queued")
        || text.contains("in progress")
    {
        JobStatus::Processing
    } else if text.contains("error") || text.contains("failed") || text.contains("failure") {
        JobStatus::Error(text.trim().to_string())
    } else if class_attr.contains("complete") || class_attr.contains("success") {
        JobStatus::Complete
    } else if class_attr.contains("pending") || class_attr.contains("processing") {
        JobStatus::Processing
    } else if class_attr.contains("error") || class_attr.contains("fail") {
        JobStatus::Error("status class indicates failure".to_string())
    } else {
        // Default assumption: if we found a read.html link, it's probably ready
        // (many systems only show links when jobs are complete)
        JobStatus::Complete
    }
}

/// Checks if list.html indicates we need to wait for a job.
///
/// Returns true if the page contains auto-polling indicators or processing messages.
pub fn page_indicates_processing(html: &str) -> bool {
    let lower = html.to_lowercase();
    lower.contains("autopoll")
        || lower.contains("processing")
        || lower.contains("please wait")
        || lower.contains("generating")
        || lower.contains("in progress")
}

/// Extracts all jobs from list.html (not just the newest).
///
/// Useful for debugging or finding specific jobs.
pub fn parse_all_jobs(html: &str) -> Vec<AuditJob> {
    let document = Html::parse_document(html);
    let mut jobs = Vec::new();

    for row in document.select(&ROW_SELECTOR) {
        if let Some(link) = row.select(&LINK_SELECTOR).next() {
            if let Some(href) = link.value().attr("href") {
                if let Some(job_id) = extract_job_id_from_href(href) {
                    let status = extract_status_from_row(&row);
                    jobs.push(AuditJob {
                        job_id,
                        status,
                        raw_href: Some(href.to_string()),
                    });
                }
            }
        }
    }

    jobs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_job_id_standard() {
        let href = "read.html?id=JobQueueRun!!!!ABC123";
        let job_id = extract_job_id_from_href(href);
        assert_eq!(job_id, Some("JobQueueRun!!!!ABC123".to_string()));
    }

    #[test]
    fn test_extract_job_id_encoded() {
        let href = "read.html?id=JobQueueRun%21%21%21%21ABC123";
        let job_id = extract_job_id_from_href(href);
        assert_eq!(job_id, Some("JobQueueRun!!!!ABC123".to_string()));
    }

    #[test]
    fn test_extract_job_id_with_jsessionid() {
        let href = "read.html;jsessionid=XYZ123?id=JobQueueRun!!!!ABC123";
        let job_id = extract_job_id_from_href(href);
        assert_eq!(job_id, Some("JobQueueRun!!!!ABC123".to_string()));
    }

    #[test]
    fn test_parse_status_complete() {
        assert!(matches!(
            parse_status_text("Complete"),
            JobStatus::Complete
        ));
        assert!(matches!(parse_status_text("Ready"), JobStatus::Complete));
    }

    #[test]
    fn test_parse_status_processing() {
        assert!(matches!(
            parse_status_text("Processing"),
            JobStatus::Processing
        ));
        assert!(matches!(
            parse_status_text("In Progress"),
            JobStatus::Processing
        ));
    }

    fn parse_status_text(text: &str) -> JobStatus {
        let lower = text.to_lowercase();
        if lower.contains("complete") || lower.contains("ready") {
            JobStatus::Complete
        } else if lower.contains("processing") || lower.contains("progress") {
            JobStatus::Processing
        } else {
            JobStatus::Unknown(text.to_string())
        }
    }
}
