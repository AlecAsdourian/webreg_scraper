/// Types for degree audit data
use serde::{Deserialize, Serialize};

/// Raw degree audit response from webregautoin
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DegreeAuditResponse {
    #[serde(rename = "auditId")]
    pub audit_id: String,

    #[serde(rename = "scrapedAt")]
    pub scraped_at: String,

    pub url: String,

    /// Full HTML content of the degree audit page
    /// This will be parsed to extract structured data
    pub html: String,
}

/// Parsed degree audit data (to be implemented after HTML inspection)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DegreeAudit {
    pub audit_id: String,
    pub student_info: StudentInfo,
    pub requirements: Vec<Requirement>,
    pub scraped_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StudentInfo {
    pub student_id: Option<String>,
    pub name: Option<String>,
    pub major: Option<String>,
    pub college: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Requirement {
    pub category: String,
    pub name: String,
    pub status: RequirementStatus,
    pub credits_required: Option<f32>,
    pub credits_completed: Option<f32>,
    pub courses: Vec<CourseRequirement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RequirementStatus {
    Complete,
    InProgress,
    NotStarted,
    NotApplicable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CourseRequirement {
    pub course_code: String,
    pub title: Option<String>,
    pub units: Option<f32>,
    pub grade: Option<String>,
    pub term: Option<String>,
    pub status: CourseStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CourseStatus {
    Completed,
    InProgress,
    Planned,
    Required,
}
