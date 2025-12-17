/// Database types for course schedule data

#[derive(Debug, Clone)]
pub struct DbCourse {
    pub course_id: i64,
    pub term: String,
    pub subj_code: String,
    pub course_code: String,
    pub subj_course_id: String,
}

#[derive(Debug, Clone)]
pub struct DbSection {
    pub section_id_pk: i64,
    pub course_id: i64,
    pub section_id: String,
    pub section_code: String,
}

#[derive(Debug, Clone)]
pub struct DbMeeting {
    pub meeting_id: i64,
    pub section_id_pk: i64,
    pub meeting_type: Option<String>,
    pub meeting_days_type: String,
    pub meeting_days: Option<String>,  // JSON string
    pub start_hr: Option<i32>,
    pub start_min: Option<i32>,
    pub end_hr: Option<i32>,
    pub end_min: Option<i32>,
    pub building: Option<String>,
    pub room: Option<String>,
    pub instructors: Option<String>,  // JSON string
}
