/// Database module for managing course schedule/meeting time data

mod types;

pub use types::{DbCourse, DbMeeting, DbSection};

use rusqlite::{Connection, Result};
use std::sync::Mutex;
use webweg::types::{CourseSection, MeetingDay};

const SCHEMA_SQL: &str = include_str!("../../../../sql/init_schedules.sql");

pub struct ScheduleDbManager {
    db: Mutex<Connection>,
}

impl ScheduleDbManager {
    /// Creates a new ScheduleDbManager and initializes the database schema
    pub fn new(db_path: &str) -> Self {
        let conn = Connection::open(db_path).expect("Failed to open database");

        // Initialize schema
        conn.execute_batch(SCHEMA_SQL)
            .expect("Failed to initialize database schema");

        Self {
            db: Mutex::new(conn),
        }
    }

    /// Checks if a term already has data in the database
    pub fn term_has_data(&self, term: &str) -> bool {
        let db = self.db.lock().unwrap();
        let mut stmt = db
            .prepare("SELECT COUNT(*) FROM courses WHERE term = ?")
            .unwrap();
        let count: i64 = stmt.query_row([term], |row| row.get(0)).unwrap_or(0);
        count > 0
    }

    /// Inserts course data with all its sections and meetings
    pub fn insert_course_with_sections(
        &self,
        term: &str,
        sections: Vec<CourseSection>,
    ) -> Result<()> {
        if sections.is_empty() {
            return Ok(());
        }

        let db = self.db.lock().unwrap();

        // Extract course info from first section (all sections belong to same course)
        let subj_course_id = &sections[0].subj_course_id;
        let parts: Vec<&str> = subj_course_id.split_whitespace().collect();
        let (subj_code, course_code) = if parts.len() >= 2 {
            (parts[0], parts[1])
        } else {
            (subj_course_id.as_str(), "")
        };

        // Insert or get course
        db.execute(
            "INSERT OR IGNORE INTO courses (term, subj_code, course_code, subj_course_id, created_at)
             VALUES (?1, ?2, ?3, ?4, datetime('now'))",
            (term, subj_code, course_code, subj_course_id),
        )?;

        let course_id: i64 = db.query_row(
            "SELECT course_id FROM courses WHERE term = ? AND subj_course_id = ?",
            (term, subj_course_id),
            |row| row.get(0),
        )?;

        // Insert sections and meetings
        for section in sections {
            // Insert section
            db.execute(
                "INSERT OR IGNORE INTO sections (course_id, section_id, section_code, created_at)
                 VALUES (?1, ?2, ?3, datetime('now'))",
                (course_id, &section.section_id, &section.section_code),
            )?;

            let section_id_pk: i64 = db.query_row(
                "SELECT section_id_pk FROM sections WHERE course_id = ? AND section_id = ?",
                (course_id, &section.section_id),
                |row| row.get(0),
            )?;

            // Insert meetings
            for meeting in &section.meetings {
                let (days_type, days_json) = match &meeting.meeting_days {
                    MeetingDay::Repeated(days) => {
                        ("repeated", Some(serde_json::to_string(days).unwrap()))
                    }
                    MeetingDay::OneTime(date) => {
                        ("onetime", Some(date.clone()))
                    }
                    MeetingDay::None => ("none", None),
                };

                let instructors_json = serde_json::to_string(&meeting.instructors).unwrap();

                let start_hr = Some(meeting.start_hr as i32);
                let start_min = Some(meeting.start_min as i32);
                let end_hr = Some(meeting.end_hr as i32);
                let end_min = Some(meeting.end_min as i32);

                db.execute(
                    "INSERT INTO meetings (
                        section_id_pk, meeting_type, meeting_days_type, meeting_days,
                        start_hr, start_min, end_hr, end_min,
                        building, room, instructors, created_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, datetime('now'))",
                    (
                        section_id_pk,
                        &meeting.meeting_type,
                        days_type,
                        days_json,
                        start_hr,
                        start_min,
                        end_hr,
                        end_min,
                        &meeting.building,
                        &meeting.room,
                        instructors_json,
                    ),
                )?;
            }
        }

        Ok(())
    }

    /// Gets all meetings for a specific section ID
    pub fn get_meetings_for_section(&self, section_id: &str) -> Result<Vec<DbMeeting>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT m.meeting_id, m.section_id_pk, m.meeting_type, m.meeting_days_type,
                    m.meeting_days, m.start_hr, m.start_min, m.end_hr, m.end_min,
                    m.building, m.room, m.instructors
             FROM meetings m
             JOIN sections s ON m.section_id_pk = s.section_id_pk
             WHERE s.section_id = ?",
        )?;

        let meetings = stmt.query_map([section_id], |row| {
            Ok(DbMeeting {
                meeting_id: row.get(0)?,
                section_id_pk: row.get(1)?,
                meeting_type: row.get(2)?,
                meeting_days_type: row.get(3)?,
                meeting_days: row.get(4)?,
                start_hr: row.get(5)?,
                start_min: row.get(6)?,
                end_hr: row.get(7)?,
                end_min: row.get(8)?,
                building: row.get(9)?,
                room: row.get(10)?,
                instructors: row.get(11)?,
            })
        })?;

        meetings.collect()
    }

    /// Gets all sections with their meetings for a specific term
    pub fn get_all_sections_for_term(
        &self,
        term: &str,
    ) -> Result<Vec<(DbSection, Vec<DbMeeting>)>> {
        let db = self.db.lock().unwrap();

        // Get all sections for the term
        let mut stmt = db.prepare(
            "SELECT s.section_id_pk, s.course_id, s.section_id, s.section_code
             FROM sections s
             JOIN courses c ON s.course_id = c.course_id
             WHERE c.term = ?",
        )?;

        let sections: Vec<DbSection> = stmt
            .query_map([term], |row| {
                Ok(DbSection {
                    section_id_pk: row.get(0)?,
                    course_id: row.get(1)?,
                    section_id: row.get(2)?,
                    section_code: row.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>>>()?;

        // For each section, get its meetings
        let mut result = Vec::new();
        for section in sections {
            let mut meeting_stmt = db.prepare(
                "SELECT meeting_id, section_id_pk, meeting_type, meeting_days_type,
                        meeting_days, start_hr, start_min, end_hr, end_min,
                        building, room, instructors
                 FROM meetings
                 WHERE section_id_pk = ?",
            )?;

            let meetings: Vec<DbMeeting> = meeting_stmt
                .query_map([section.section_id_pk], |row| {
                    Ok(DbMeeting {
                        meeting_id: row.get(0)?,
                        section_id_pk: row.get(1)?,
                        meeting_type: row.get(2)?,
                        meeting_days_type: row.get(3)?,
                        meeting_days: row.get(4)?,
                        start_hr: row.get(5)?,
                        start_min: row.get(6)?,
                        end_hr: row.get(7)?,
                        end_min: row.get(8)?,
                        building: row.get(9)?,
                        room: row.get(10)?,
                        instructors: row.get(11)?,
                    })
                })?
                .collect::<Result<Vec<_>>>()?;

            result.push((section, meetings));
        }

        Ok(result)
    }
}
