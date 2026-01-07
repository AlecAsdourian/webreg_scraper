use std::sync::Arc;

use axum::routing::{get, post};
use axum::{middleware as mw, Router};

use crate::server::endpoints::{degree_audit, schedule, status, ww_cookies, ww_general};
use crate::server::middleware::*;
use crate::types::WrapperState;

mod endpoints;
mod middleware;
mod types;
mod util;

/// Creates a router that can be used by `axum`.
///
/// # Parameters
/// - `app_state`: The app server state.
///
/// # Returns
/// The router.
pub fn create_router(app_state: Arc<WrapperState>) -> Router {
    // Router whose endpoints require cookie header
    let cookie_router = Router::new()
        .route("/add_section", post(ww_cookies::post_add_section))
        .route(
            "/validate_add_section",
            post(ww_cookies::post_validate_add_section),
        )
        .route("/drop_section", post(ww_cookies::post_drop_section))
        .route("/add_plan", post(ww_cookies::post_add_plan))
        .route(
            "/validate_add_plan",
            post(ww_cookies::post_validate_add_plan),
        )
        .route("/remove_plan", post(ww_cookies::post_remove_plan))
        .route("/schedule", get(ww_cookies::get_schedule))
        .route("/schedule_list", get(ww_cookies::get_schedule_list))
        .route("/register_term", post(ww_cookies::post_register_term))
        .route("/events", get(ww_cookies::get_events))
        .route("/rename_schedule", post(ww_cookies::post_rename_schedule))
        .layer(mw::from_fn(cookie_validator::check_cookies));

    // General router
    let parsed_router = Router::new()
        .route("/course_info", get(ww_general::get_course_info))
        .route("/prerequisites", get(ww_general::get_prerequisites))
        .route("/search", get(ww_general::get_search_courses))
        .route("/department_codes", get(ww_general::get_department_codes))
        .route("/subject_codes", get(ww_general::get_subject_codes))
        .route("/course_text", get(ww_general::get_course_text))
        .route("/section_text", get(ww_general::get_section_text))
        .route("/schedule_data", get(schedule::get_schedule_data))
        .route("/schedule_data/:section_id", get(schedule::get_section_meetings))
        .merge(cookie_router)
        .layer(mw::from_fn_with_state(
            app_state.clone(),
            term_validator::validate_term,
        ));

    // Live WebReg router.
    let webreg_router = Router::new()
        .merge(parsed_router)
        .layer(mw::from_fn_with_state(
            app_state.clone(),
            running_validator::validate_wrapper_running,
        ));

    // Degree audit router (not nested under /live/:term/ since it's student-specific)
    let degree_audit_router = Router::new()
        .route("/degree_audit", get(degree_audit::get_audit))
        .route("/degree_audit/progress", get(degree_audit::get_degree_progress))
        .route(
            "/degree_audit/completed_courses",
            get(degree_audit::get_completed_courses),
        )
        .route(
            "/degree_audit/requirements",
            get(degree_audit::get_requirements_summary),
        )
        .route(
            "/degree_audit/next_courses",
            get(degree_audit::get_next_courses),
        )
        .route(
            "/degree_audit/subrequirement/:subreq_id/eligible_courses",
            get(degree_audit::get_eligible_courses_for_subreq),
        )
        // Cache management endpoints
        .route(
            "/degree_audit/cache_stats",
            get(degree_audit::get_cache_stats),
        )
        .route(
            "/degree_audit/invalidate_cache",
            post(degree_audit::invalidate_cache),
        );

    let router = Router::new()
        .route("/health", get(status::get_health))
        .nest("/live/:term", webreg_router)
        .route("/terms", get(ww_general::get_all_terms))
        .route("/timing/:term", get(status::get_timing_stats))
        .route("/login_stat/:stat", get(status::get_login_script_stats))
        .merge(degree_audit_router)
        .with_state(app_state.clone());

    #[cfg(feature = "auth")]
    {
        router.layer(mw::from_fn_with_state(
            app_state.clone(),
            auth_validator::auth,
        ))
    }
    #[cfg(not(feature = "auth"))]
    {
        router
    }
}
