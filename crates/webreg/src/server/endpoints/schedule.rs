use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use std::sync::Arc;
use tracing::info;

use crate::server::types::ApiErrorType;
use crate::types::WrapperState;

/// GET /live/:term/schedule_data
/// Returns all schedule data (courses, sections, meetings) for a term
pub async fn get_schedule_data(
    Path(term): Path<String>,
    State(s): State<Arc<WrapperState>>,
) -> Response {
    info!("GET /live/{}/schedule_data", term);

    match s.schedule_db.get_all_sections_for_term(&term) {
        Ok(data) => {
            let response: Vec<_> = data
                .into_iter()
                .map(|(section, meetings)| {
                    json!({
                        "section_id": section.section_id,
                        "section_code": section.section_code,
                        "meetings": meetings.into_iter().map(|m| {
                            json!({
                                "type": m.meeting_type,
                                "days_type": m.meeting_days_type,
                                "days": m.meeting_days,
                                "start_hr": m.start_hr,
                                "start_min": m.start_min,
                                "end_hr": m.end_hr,
                                "end_min": m.end_min,
                                "building": m.building,
                                "room": m.room,
                                "instructors": m.instructors,
                            })
                        }).collect::<Vec<_>>()
                    })
                })
                .collect();

            (StatusCode::OK, Json(response)).into_response()
        }
        Err(e) => ApiErrorType::from((
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to fetch schedule data",
            Some(e.to_string()),
        ))
        .into_response(),
    }
}

/// GET /live/:term/schedule_data/:section_id
/// Returns meetings for a specific section
pub async fn get_section_meetings(
    Path((term, section_id)): Path<(String, String)>,
    State(s): State<Arc<WrapperState>>,
) -> Response {
    info!("GET /live/{}/schedule_data/{}", term, section_id);

    match s.schedule_db.get_meetings_for_section(&section_id) {
        Ok(meetings) => {
            let response: Vec<_> = meetings
                .into_iter()
                .map(|m| {
                    json!({
                        "type": m.meeting_type,
                        "days_type": m.meeting_days_type,
                        "days": m.meeting_days,
                        "start_hr": m.start_hr,
                        "start_min": m.start_min,
                        "end_hr": m.end_hr,
                        "end_min": m.end_min,
                        "building": m.building,
                        "room": m.room,
                        "instructors": m.instructors,
                    })
                })
                .collect();

            (StatusCode::OK, Json(response)).into_response()
        }
        Err(e) => ApiErrorType::from((
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to fetch meetings",
            Some(e.to_string()),
        ))
        .into_response(),
    }
}
