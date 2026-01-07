//! Degree audit scraping module.
//!
//! This module provides functionality for:
//! - Fetching degree audit data from UCSD's DARS system
//! - Parsing the HTML into structured data
//! - Caching results to reduce load
//! - Processing requirements and generating recommendations

// Core modules
pub mod cache;
pub mod client;
pub mod config;
pub mod error;
pub mod job;
pub mod processor;
mod types;

// Re-exports for convenience
pub use cache::AuditCacheState;
pub use client::DegreeAuditClient;
pub use error::DegreeAuditError;
pub use processor::*;
pub use types::*;

use crate::types::WrapperState;
use regex::Regex;
use scraper::{Html, Selector};
use std::sync::Arc;
use tracing::info;

/// Fetches degree audit data from the webregautoin server
///
/// This function calls the `/degree_audit` endpoint on the webregautoin server,
/// which uses Puppeteer to navigate the degree audit system and extract data.
///
/// # Arguments
/// * `state` - The wrapper state containing cookie server configuration
///
/// # Returns
/// * `Ok(DegreeAuditResponse)` - Raw degree audit data including HTML
/// * `Err` - If the request fails or the response is invalid
pub async fn fetch_degree_audit(
    state: &Arc<WrapperState>,
) -> Result<DegreeAuditResponse, Box<dyn std::error::Error>> {
    let address = format!(
        "{}:{}",
        state.cookie_server.address, state.cookie_server.port
    );

    info!("Requesting degree audit data from webregautoin server (http://{address}/degree_audit)");

    let response = state
        .client
        .get(format!("http://{address}/degree_audit"))
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response.text().await.unwrap_or_default();
        return Err(format!("Degree audit request failed with status {}: {}", status, error_text).into());
    }

    let text = response.text().await?;
    let audit_data: DegreeAuditResponse = serde_json::from_str(&text)?;

    info!("Successfully received degree audit data (audit ID: {})", audit_data.audit_id);

    Ok(audit_data)
}

/// Parses the HTML from degree audit response into structured data
///
/// Extracts student information, requirements, subrequirements, and completed courses
/// from the degree audit HTML.
///
/// # Arguments
/// * `raw_audit` - Raw degree audit response with HTML content
///
/// # Returns
/// * `Ok(DegreeAudit)` - Parsed degree audit data
/// * `Err` - If parsing fails
pub fn parse_degree_audit_html(
    raw_audit: &DegreeAuditResponse,
) -> Result<DegreeAudit, Box<dyn std::error::Error>> {
    info!("Parsing degree audit HTML");

    let document = Html::parse_document(&raw_audit.html);

    // Parse student info
    let student_info = parse_student_info(&document)?;

    // Parse requirements
    let requirements = parse_requirements(&document)?;

    info!("Parsed {} requirements from degree audit", requirements.len());

    Ok(DegreeAudit {
        audit_id: raw_audit.audit_id.clone(),
        student_info,
        requirements,
        scraped_at: raw_audit.scraped_at.clone(),
    })
}

/// Extracts student information from the degree audit HTML
fn parse_student_info(document: &Html) -> Result<StudentInfo, Box<dyn std::error::Error>> {
    // Student name from header (e.g., "Alec Asdourian")
    let name_selector = Selector::parse("#headerInfo span.float-right").unwrap();
    let name = document
        .select(&name_selector)
        .next()
        .map(|el| el.text().collect::<String>().trim().to_string());

    // Major from includeTopText (e.g., "Major(s): MA30")
    let major_selector = Selector::parse(".includeTopText").unwrap();
    let major_text = document
        .select(&major_selector)
        .next()
        .map(|el| el.text().collect::<String>());

    let major_regex = Regex::new(r"Major\(s\):\s*([A-Z0-9]+)")?;
    let major = if let Some(text) = major_text {
        major_regex
            .captures(&text)
            .and_then(|caps| caps.get(1))
            .map(|m| m.as_str().to_string())
    } else {
        None
    };

    // Try to extract college (if present in HTML)
    let college = None; // Not clearly visible in the HTML structure

    Ok(StudentInfo {
        student_id: None, // Student ID not readily visible in HTML
        name,
        major,
        college,
    })
}

/// Parses all requirements from the degree audit
fn parse_requirements(document: &Html) -> Result<Vec<Requirement>, Box<dyn std::error::Error>> {
    let mut requirements = Vec::new();

    // Select all requirement divs
    let req_selector = Selector::parse("div.requirement").unwrap();

    for req_element in document.select(&req_selector) {
        if let Ok(requirement) = parse_single_requirement(&req_element) {
            requirements.push(requirement);
        }
    }

    Ok(requirements)
}

/// Parses a single requirement element
fn parse_single_requirement(
    req_element: &scraper::ElementRef,
) -> Result<Requirement, Box<dyn std::error::Error>> {
    // Extract requirement title
    let title_selector = Selector::parse(".reqTitle").unwrap();
    let title = req_element
        .select(&title_selector)
        .next()
        .map(|el| el.text().collect::<String>().trim().to_string())
        .unwrap_or_default();

    // Extract status from class attribute
    let status = if req_element.value().attr("class").unwrap_or("").contains("Status_OK") {
        RequirementStatus::Complete
    } else if req_element.value().attr("class").unwrap_or("").contains("Status_IP") {
        RequirementStatus::InProgress
    } else if req_element.value().attr("class").unwrap_or("").contains("Status_NO") {
        RequirementStatus::NotStarted
    } else {
        RequirementStatus::NotApplicable
    };

    // Extract category (e.g., "category_Major", "category_Overall_GPA")
    let class_attr = req_element.value().attr("class").unwrap_or("");
    let category_regex = Regex::new(r"category_(\w+)")?;
    let category = category_regex
        .captures(class_attr)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().to_string())
        .unwrap_or_else(|| "Unknown".to_string());

    // Extract required hours from attribute
    let credits_required = req_element
        .value()
        .attr("rqdHours")
        .and_then(|s| s.parse::<f32>().ok())
        .filter(|&h| h > 0.0);

    // Parse completed courses from subrequirements
    let courses = parse_courses_from_requirement(req_element)?;

    // Try to get earned units from requirementTotals table first
    // This is more accurate than calculating from courses
    let credits_completed = parse_req_earned_units(req_element).or_else(|| {
        // Fallback: Calculate credits completed from courses
        if !courses.is_empty() {
            Some(courses.iter().filter_map(|c| c.units).sum())
        } else {
            None
        }
    });

    // Parse subrequirements
    let subrequirements = parse_subrequirements(req_element)?;

    Ok(Requirement {
        category,
        name: title,
        status,
        credits_required,
        credits_completed,
        courses,
        subrequirements,
    })
}

/// Parses all completed courses from a requirement's subrequirements
fn parse_courses_from_requirement(
    req_element: &scraper::ElementRef,
) -> Result<Vec<CourseRequirement>, Box<dyn std::error::Error>> {
    let mut courses = Vec::new();

    // Select all completed course tables
    let table_selector = Selector::parse("table.completedCourses").unwrap();
    let row_selector = Selector::parse("tr.takenCourse").unwrap();

    for table in req_element.select(&table_selector) {
        for row in table.select(&row_selector) {
            if let Ok(course) = parse_course_row(&row) {
                courses.push(course);
            }
        }
    }

    Ok(courses)
}

/// Parses a single course row from a completed courses table
fn parse_course_row(
    row: &scraper::ElementRef,
) -> Result<CourseRequirement, Box<dyn std::error::Error>> {
    let term_selector = Selector::parse("td.term").unwrap();
    let course_selector = Selector::parse("td.course").unwrap();
    let credit_selector = Selector::parse("td.credit").unwrap();
    let grade_selector = Selector::parse("td.grade").unwrap();
    let desc_selector = Selector::parse("td.description .descLine").unwrap();

    let term = row
        .select(&term_selector)
        .next()
        .map(|el| el.text().collect::<String>().trim().to_string());

    let course_code = row
        .select(&course_selector)
        .next()
        .map(|el| el.text().collect::<String>().trim().to_string())
        .unwrap_or_default();

    let units = row
        .select(&credit_selector)
        .next()
        .and_then(|el| el.text().collect::<String>().trim().parse::<f32>().ok());

    let grade = row
        .select(&grade_selector)
        .next()
        .map(|el| el.text().collect::<String>().trim().to_string())
        .filter(|s| !s.is_empty());

    let title = row
        .select(&desc_selector)
        .next()
        .map(|el| el.text().collect::<String>().trim().to_string());

    // Determine course status based on grade
    let status = if let Some(ref g) = grade {
        if g == "IP" {
            CourseStatus::InProgress
        } else {
            CourseStatus::Completed
        }
    } else {
        CourseStatus::Completed
    };

    Ok(CourseRequirement {
        course_code,
        title,
        units,
        grade,
        term,
        status,
    })
}

/// Parses all subrequirements from a requirement element
fn parse_subrequirements(
    req_element: &scraper::ElementRef,
) -> Result<Vec<Subrequirement>, Box<dyn std::error::Error>> {
    let mut subrequirements = Vec::new();

    let subreq_selector = Selector::parse("div.subrequirement").unwrap();

    for subreq_elem in req_element.select(&subreq_selector) {
        if let Ok(subreq) = parse_single_subrequirement(&subreq_elem) {
            subrequirements.push(subreq);
        }
    }

    Ok(subrequirements)
}

/// Parses a single subrequirement div
fn parse_single_subrequirement(
    subreq_elem: &scraper::ElementRef,
) -> Result<Subrequirement, Box<dyn std::error::Error>> {
    // Extract id attribute
    let id = subreq_elem
        .value()
        .attr("id")
        .unwrap_or("unknown")
        .to_string();

    // Extract title
    let title_selector = Selector::parse(".subreqTitle").unwrap();
    let title = subreq_elem
        .select(&title_selector)
        .next()
        .map(|el| el.text().collect::<String>().trim().to_string())
        .unwrap_or_default();

    // Extract required units from rqdhours attribute
    let required_units = subreq_elem
        .value()
        .attr("rqdhours")
        .and_then(|s| s.parse::<f32>().ok())
        .unwrap_or(0.0);

    // Parse status from class attribute
    let class_attr = subreq_elem.value().attr("class").unwrap_or("");
    let status = if class_attr.contains("Status_OK") {
        RequirementStatus::Complete
    } else if class_attr.contains("Status_IP") {
        RequirementStatus::InProgress
    } else if class_attr.contains("Status_NO") {
        RequirementStatus::NotStarted
    } else {
        RequirementStatus::NotApplicable
    };

    // Parse eligible courses from selectcourses table
    let eligible_courses = parse_eligible_courses(&subreq_elem)?;

    // Parse category groups
    let category_groups = parse_course_categories(&subreq_elem)?;

    // Parse completed courses from completedCourses table
    let completed_courses = parse_completed_courses_in_subreq(&subreq_elem)?;

    // Try to get earned units from subrequirementTotals table first
    // This is more accurate than calculating from courses since some subrequirements
    // show totals without listing individual courses
    let units_completed = parse_subreq_earned_units(&subreq_elem).unwrap_or_else(|| {
        // Fallback: Calculate completed units from courses (only count passing grades)
        completed_courses
            .iter()
            .filter_map(|c| {
                if let (Some(grade), Some(units)) = (&c.grade, c.units) {
                    if GradeValidator::is_passing_grade(grade) {
                        Some(units)
                    } else {
                        None
                    }
                } else {
                    c.units
                }
            })
            .sum()
    });

    let units_remaining = (required_units - units_completed).max(0.0);

    Ok(Subrequirement {
        id,
        title,
        required_units,
        units_completed,
        units_remaining,
        status,
        eligible_courses,
        completed_courses,
        category_groups,
    })
}

/// Parses eligible courses from selectcourses table
fn parse_eligible_courses(
    subreq_elem: &scraper::ElementRef,
) -> Result<Vec<EligibleCourse>, Box<dyn std::error::Error>> {
    let mut courses = Vec::new();

    let table_selector = Selector::parse("table.selectcourses").unwrap();
    let course_selector = Selector::parse("span.course").unwrap();

    for table in subreq_elem.select(&table_selector) {
        for course_span in table.select(&course_selector) {
            // Extract department from attribute
            let department = course_span
                .value()
                .attr("department")
                .unwrap_or("")
                .trim()
                .to_string();

            // Extract course number from attribute
            let course_number = course_span
                .value()
                .attr("number")
                .unwrap_or("")
                .trim()
                .to_string();

            // Extract full code from span.number text
            let number_selector = Selector::parse("span.number").unwrap();
            let full_code = course_span
                .select(&number_selector)
                .next()
                .map(|el| el.text().collect::<String>().trim().to_string())
                .unwrap_or_else(|| format!("{} {}", department, course_number));

            if !department.is_empty() && !course_number.is_empty() {
                courses.push(EligibleCourse {
                    department,
                    course_number,
                    full_code,
                });
            }
        }
    }

    Ok(courses)
}

/// Parses course category groups (e.g., "APPLIED MATH", "COMPUTATIONAL")
fn parse_course_categories(
    subreq_elem: &scraper::ElementRef,
) -> Result<Vec<CourseCategory>, Box<dyn std::error::Error>> {
    let mut categories = Vec::new();

    let table_selector = Selector::parse("table.selectcourses").unwrap();
    let fromcourselist_selector = Selector::parse("td.fromcourselist table tr").unwrap();

    for table in subreq_elem.select(&table_selector) {
        for row in table.select(&fromcourselist_selector) {
            let text = row.text().collect::<String>();

            // Extract category name (usually all caps at start of line, before first course)
            // Pattern: "APPLIED MATH  MATH 170A,170B,..."
            let category_regex = Regex::new(r"^([A-Z][A-Z\s\-]+?)\s{2,}")?;
            let category_name = if let Some(caps) = category_regex.captures(&text) {
                caps.get(1).map(|m| m.as_str().trim().to_string())
            } else {
                None
            };

            // Parse courses in this row
            let course_selector = Selector::parse("span.course").unwrap();
            let row_courses: Vec<EligibleCourse> = row
                .select(&course_selector)
                .filter_map(|span| {
                    let department = span.value().attr("department")?.trim().to_string();
                    let course_number = span.value().attr("number")?.trim().to_string();
                    let number_selector = Selector::parse("span.number").unwrap();
                    let full_code = span
                        .select(&number_selector)
                        .next()
                        .map(|el| el.text().collect::<String>().trim().to_string())
                        .unwrap_or_else(|| format!("{} {}", department, course_number));

                    if !department.is_empty() && !course_number.is_empty() {
                        Some(EligibleCourse {
                            department,
                            course_number,
                            full_code,
                        })
                    } else {
                        None
                    }
                })
                .collect();

            if let Some(name) = category_name {
                if !row_courses.is_empty() {
                    categories.push(CourseCategory {
                        name,
                        courses: row_courses,
                    });
                }
            }
        }
    }

    Ok(categories)
}

/// Parses completed courses within a subrequirement
fn parse_completed_courses_in_subreq(
    subreq_elem: &scraper::ElementRef,
) -> Result<Vec<CourseRequirement>, Box<dyn std::error::Error>> {
    let mut courses = Vec::new();

    let table_selector = Selector::parse("table.completedCourses").unwrap();
    let row_selector = Selector::parse("tr.takenCourse").unwrap();

    for table in subreq_elem.select(&table_selector) {
        for row in table.select(&row_selector) {
            if let Ok(course) = parse_course_row(&row) {
                courses.push(course);
            }
        }
    }

    Ok(courses)
}

/// Parses earned units from the subrequirementTotals table
///
/// The HTML structure is:
/// ```html
/// <table class="subrequirementTotals">
///   <tr class="subreqEarned">
///     <td class="bigcolumn">
///       <span class="hours number">108.00</span>
///     </td>
///   </tr>
/// </table>
/// ```
fn parse_subreq_earned_units(subreq_elem: &scraper::ElementRef) -> Option<f32> {
    let table_selector = Selector::parse("table.subrequirementTotals").ok()?;
    let earned_selector = Selector::parse("tr.subreqEarned span.hours.number").ok()?;

    for table in subreq_elem.select(&table_selector) {
        if let Some(earned_span) = table.select(&earned_selector).next() {
            let text = earned_span.text().collect::<String>();
            if let Ok(units) = text.trim().parse::<f32>() {
                return Some(units);
            }
        }
    }

    None
}

/// Parses earned units from the requirementTotals table
///
/// The HTML structure is:
/// ```html
/// <table class="requirementTotals">
///   <tr class="reqEarned">
///     <td class="hourscount bigcolumn">
///       <span class="hours number">164.00</span>
///     </td>
///   </tr>
/// </table>
/// ```
fn parse_req_earned_units(req_elem: &scraper::ElementRef) -> Option<f32> {
    let table_selector = Selector::parse("table.requirementTotals").ok()?;
    let earned_selector = Selector::parse("tr.reqEarned span.hours.number").ok()?;

    for table in req_elem.select(&table_selector) {
        if let Some(earned_span) = table.select(&earned_selector).next() {
            let text = earned_span.text().collect::<String>();
            if let Ok(units) = text.trim().parse::<f32>() {
                return Some(units);
            }
        }
    }

    None
}

/// Fetches and parses degree audit data in one step
///
/// Convenience function that combines fetch and parse operations.
///
/// # Arguments
/// * `state` - The wrapper state
///
/// # Returns
/// * `Ok(DegreeAudit)` - Fully parsed degree audit data
/// * `Err` - If fetch or parse fails
pub async fn get_degree_audit(
    state: &Arc<WrapperState>,
) -> Result<DegreeAudit, Box<dyn std::error::Error>> {
    let raw_audit = fetch_degree_audit(state).await?;
    let parsed_audit = parse_degree_audit_html(&raw_audit)?;
    Ok(parsed_audit)
}
