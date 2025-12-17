/// Degree audit scraping module
mod types;

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

    // Calculate credits completed from courses
    let credits_completed = if !courses.is_empty() {
        Some(courses.iter().filter_map(|c| c.units).sum())
    } else {
        None
    };

    Ok(Requirement {
        category,
        name: title,
        status,
        credits_required,
        credits_completed,
        courses,
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
