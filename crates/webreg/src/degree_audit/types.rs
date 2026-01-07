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
    pub subrequirements: Vec<Subrequirement>,
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

/// Represents an eligible course extracted from selectcourses table
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct EligibleCourse {
    pub department: String,        // e.g., "MATH", "CSE"
    pub course_number: String,     // e.g., "170A", "107"
    pub full_code: String,         // e.g., "MATH 170A", "CSE 107"
}

/// Course category grouping (e.g., "APPLIED MATH", "GEN MATH-CS")
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CourseCategory {
    pub name: String,                        // e.g., "APPLIED MATH"
    pub courses: Vec<EligibleCourse>,
}

/// Represents a subrequirement (from div.subrequirement)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subrequirement {
    pub id: String,                           // From subrequirement id attribute
    pub title: String,                        // e.g., "General Math-CS Electives"
    pub required_units: f32,                  // From rqdhours attribute
    pub units_completed: f32,                 // Calculated from completed courses
    pub units_remaining: f32,                 // required_units - units_completed
    pub status: RequirementStatus,            // Parsed from status class
    pub eligible_courses: Vec<EligibleCourse>, // Courses that can fulfill this
    pub completed_courses: Vec<CourseRequirement>, // Already completed
    pub category_groups: Vec<CourseCategory>,  // Groups like "APPLIED MATH", "COMPUTATIONAL"
}

/// Summary per requirement for progress tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequirementSummary {
    pub category: String,
    pub name: String,
    pub status: RequirementStatus,
    pub units_required: f32,
    pub units_completed: f32,
    pub units_remaining: f32,
    pub subrequirements_count: usize,
    pub completed_subrequirements: usize,
}

/// Recommended next course to take
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NextCourseRecommendation {
    pub subrequirement_title: String,
    pub priority: u32,  // 1 = highest priority
    pub eligible_courses: Vec<EligibleCourse>,
    pub units_needed: f32,
}

/// Aggregated degree progress data for UI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DegreeProgress {
    pub audit_id: String,
    pub student_info: StudentInfo,
    pub total_units_required: f32,
    pub total_units_completed: f32,
    pub total_units_remaining: f32,
    pub requirements_summary: Vec<RequirementSummary>,
    pub next_courses_to_take: Vec<NextCourseRecommendation>,
}

/// Grade validation helper
#[derive(Debug, Clone)]
pub struct GradeValidator;

impl GradeValidator {
    /// Checks if a grade is C- or higher (passing for major requirements)
    pub fn is_passing_grade(grade: &str) -> bool {
        matches!(
            grade,
            "A+" | "A" | "A-" | "B+" | "B" | "B-" | "C+" | "C" | "C-" | "TP" | "P"
        )
    }

    /// Calculates units earned based on grade
    pub fn units_earned(grade: &str, course_units: f32) -> f32 {
        if Self::is_passing_grade(grade) {
            course_units
        } else {
            0.0
        }
    }
}
