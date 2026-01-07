//! API endpoints for degree audit functionality.
//!
//! These endpoints provide access to parsed degree audit data,
//! progress tracking, and course recommendations.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use tracing::{error, info, warn};

use crate::degree_audit::{
    self, DegreeAudit, DegreeAuditError, DegreeProgressProcessor,
};
use crate::server::types::ApiErrorType;
use crate::types::WrapperState;

/// Query parameters for degree audit endpoints.
#[derive(Debug, Deserialize)]
pub struct AuditQueryParams {
    /// If true, bypass cache and fetch fresh data
    #[serde(default)]
    pub refresh: bool,
}

/// Internal helper to get a degree audit.
///
/// Uses the Puppeteer-based `/degree_audit` endpoint on webregautoin server,
/// which handles all browser navigation, authentication, and HTML scraping.
/// This is more reliable than extracting cookies and making HTTP requests.
async fn get_audit_internal(
    state: &Arc<WrapperState>,
    _force_refresh: bool, // Note: refresh not yet implemented for Puppeteer path
) -> Result<DegreeAudit, DegreeAuditError> {
    // Use the Puppeteer-based approach which handles authentication internally
    degree_audit::get_degree_audit(state)
        .await
        .map_err(|e| DegreeAuditError::Network {
            message: e.to_string(),
        })
}

/// Converts DegreeAuditError to API response.
fn audit_error_to_response(error: DegreeAuditError) -> Response {
    let (status, message) = match &error {
        DegreeAuditError::SessionExpired { .. } => (
            StatusCode::UNAUTHORIZED,
            "Session expired - please re-authenticate",
        ),
        DegreeAuditError::NoSession { .. } => (
            StatusCode::UNAUTHORIZED,
            "No active session",
        ),
        DegreeAuditError::CircuitBreakerOpen => (
            StatusCode::SERVICE_UNAVAILABLE,
            "Service temporarily unavailable due to repeated failures",
        ),
        DegreeAuditError::PollTimeout { .. } => (
            StatusCode::GATEWAY_TIMEOUT,
            "Audit generation timed out",
        ),
        DegreeAuditError::CookieFetchError { .. } => (
            StatusCode::BAD_GATEWAY,
            "Failed to fetch authentication cookies",
        ),
        _ => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to fetch degree audit",
        ),
    };

    ApiErrorType::from((status, message, Some(error.to_string()))).into_response()
}

/// GET /degree_audit
///
/// Fetches and returns the full parsed degree audit.
///
/// Query parameters:
/// - `refresh` (optional): Set to `true` to bypass cache
pub async fn get_audit(
    State(s): State<Arc<WrapperState>>,
    Query(params): Query<AuditQueryParams>,
) -> Response {
    info!(
        "GET /degree_audit - Fetching degree audit (refresh={})",
        params.refresh
    );

    match get_audit_internal(&s, params.refresh).await {
        Ok(audit) => (StatusCode::OK, Json(audit)).into_response(),
        Err(e) => {
            error!("Failed to fetch degree audit: {}", e);
            audit_error_to_response(e)
        }
    }
}

/// GET /degree_audit/progress
///
/// Returns computed degree progress with recommendations.
pub async fn get_degree_progress(
    State(s): State<Arc<WrapperState>>,
    Query(params): Query<AuditQueryParams>,
) -> Response {
    info!(
        "GET /degree_audit/progress - Computing degree progress (refresh={})",
        params.refresh
    );

    match get_audit_internal(&s, params.refresh).await {
        Ok(audit) => {
            let processor = DegreeProgressProcessor::new(s.requirements_config.clone());

            match processor.compute_degree_progress(&audit) {
                Ok(progress) => (StatusCode::OK, Json(progress)).into_response(),
                Err(e) => {
                    error!("Failed to compute degree progress: {}", e);
                    ApiErrorType::from((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Failed to compute degree progress",
                        Some(e.to_string()),
                    ))
                    .into_response()
                }
            }
        }
        Err(e) => {
            error!("Failed to fetch degree audit for progress: {}", e);
            audit_error_to_response(e)
        }
    }
}

/// GET /degree_audit/completed_courses
///
/// Returns all completed courses with passing grades (C- or higher).
pub async fn get_completed_courses(
    State(s): State<Arc<WrapperState>>,
    Query(params): Query<AuditQueryParams>,
) -> Response {
    info!(
        "GET /degree_audit/completed_courses (refresh={})",
        params.refresh
    );

    match get_audit_internal(&s, params.refresh).await {
        Ok(audit) => {
            let completed: Vec<_> = audit
                .requirements
                .iter()
                .flat_map(|r| &r.courses)
                .filter(|c| {
                    if let Some(ref grade) = c.grade {
                        crate::degree_audit::GradeValidator::is_passing_grade(grade)
                    } else {
                        false
                    }
                })
                .collect();

            (StatusCode::OK, Json(completed)).into_response()
        }
        Err(e) => {
            error!("Failed to fetch completed courses: {}", e);
            audit_error_to_response(e)
        }
    }
}

/// GET /degree_audit/subrequirement/:subreq_id/eligible_courses
///
/// Returns all courses eligible for a specific subrequirement.
pub async fn get_eligible_courses_for_subreq(
    Path(subreq_id): Path<String>,
    State(s): State<Arc<WrapperState>>,
    Query(params): Query<AuditQueryParams>,
) -> Response {
    info!(
        "GET /degree_audit/subrequirement/{}/eligible_courses (refresh={})",
        subreq_id, params.refresh
    );

    match get_audit_internal(&s, params.refresh).await {
        Ok(audit) => {
            // Find the subrequirement
            let subreq = audit
                .requirements
                .iter()
                .flat_map(|r| &r.subrequirements)
                .find(|sr| sr.id == subreq_id);

            match subreq {
                Some(sr) => {
                    let response = json!({
                        "subrequirement_id": sr.id,
                        "title": sr.title,
                        "required_units": sr.required_units,
                        "units_completed": sr.units_completed,
                        "units_remaining": sr.units_remaining,
                        "status": sr.status,
                        "eligible_courses": sr.eligible_courses,
                        "category_groups": sr.category_groups,
                    });

                    (StatusCode::OK, Json(response)).into_response()
                }
                None => {
                    warn!("Subrequirement not found: {}", subreq_id);
                    ApiErrorType::from((
                        StatusCode::NOT_FOUND,
                        "Subrequirement not found",
                        Some(format!("No subrequirement with ID: {}", subreq_id)),
                    ))
                    .into_response()
                }
            }
        }
        Err(e) => {
            error!("Failed to fetch eligible courses: {}", e);
            audit_error_to_response(e)
        }
    }
}

/// GET /degree_audit/requirements
///
/// Returns summary of all requirements.
pub async fn get_requirements_summary(
    State(s): State<Arc<WrapperState>>,
    Query(params): Query<AuditQueryParams>,
) -> Response {
    info!(
        "GET /degree_audit/requirements (refresh={})",
        params.refresh
    );

    match get_audit_internal(&s, params.refresh).await {
        Ok(audit) => {
            let summary: Vec<_> = audit
                .requirements
                .iter()
                .map(|r| {
                    json!({
                        "category": r.category,
                        "name": r.name,
                        "status": r.status,
                        "credits_required": r.credits_required,
                        "credits_completed": r.credits_completed,
                        "subrequirements_count": r.subrequirements.len(),
                    })
                })
                .collect();

            (StatusCode::OK, Json(summary)).into_response()
        }
        Err(e) => {
            error!("Failed to fetch requirements summary: {}", e);
            audit_error_to_response(e)
        }
    }
}

/// GET /degree_audit/next_courses
///
/// Returns recommended next courses to take.
pub async fn get_next_courses(
    State(s): State<Arc<WrapperState>>,
    Query(params): Query<AuditQueryParams>,
) -> Response {
    info!(
        "GET /degree_audit/next_courses (refresh={})",
        params.refresh
    );

    match get_audit_internal(&s, params.refresh).await {
        Ok(audit) => {
            let processor = DegreeProgressProcessor::new(s.requirements_config.clone());

            match processor.compute_degree_progress(&audit) {
                Ok(progress) => {
                    (StatusCode::OK, Json(progress.next_courses_to_take)).into_response()
                }
                Err(e) => {
                    error!("Failed to compute next courses: {}", e);
                    ApiErrorType::from((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Failed to compute next courses",
                        Some(e.to_string()),
                    ))
                    .into_response()
                }
            }
        }
        Err(e) => {
            error!("Failed to fetch degree audit for next courses: {}", e);
            audit_error_to_response(e)
        }
    }
}

/// GET /degree_audit/cache_stats
///
/// Returns cache statistics for monitoring.
pub async fn get_cache_stats(State(s): State<Arc<WrapperState>>) -> Response {
    let stats = s.degree_audit_client.cache_stats();
    (
        StatusCode::OK,
        Json(json!({
            "total_entries": stats.total_entries,
            "active_entries": stats.active_entries,
            "expired_entries": stats.expired_entries,
        })),
    )
        .into_response()
}

/// POST /degree_audit/invalidate_cache
///
/// Invalidates the degree audit cache.
pub async fn invalidate_cache(State(s): State<Arc<WrapperState>>) -> Response {
    info!("POST /degree_audit/invalidate_cache");

    // Clear all cache entries
    s.degree_audit_cache_state.cache.clear();

    (StatusCode::OK, Json(json!({ "message": "Cache invalidated" }))).into_response()
}
