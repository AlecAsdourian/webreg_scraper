-- Database schema for course schedule/meeting time data
-- This stores meeting times, days, locations for courses scraped from WebReg

-- Courses table (basic course information)
CREATE TABLE IF NOT EXISTS courses (
    course_id INTEGER PRIMARY KEY AUTOINCREMENT,
    term VARCHAR(10) NOT NULL,
    subj_code VARCHAR(10) NOT NULL,
    course_code VARCHAR(10) NOT NULL,
    subj_course_id VARCHAR(50) NOT NULL,
    created_at DATETIME NOT NULL,
    UNIQUE(term, subj_course_id)
);

CREATE INDEX idx_courses_term ON courses(term);
CREATE INDEX idx_courses_lookup ON courses(term, subj_code, course_code);

-- Sections table (section-level data)
CREATE TABLE IF NOT EXISTS sections (
    section_id_pk INTEGER PRIMARY KEY AUTOINCREMENT,
    course_id INTEGER NOT NULL,
    section_id VARCHAR(20) NOT NULL,
    section_code VARCHAR(10) NOT NULL,
    created_at DATETIME NOT NULL,
    FOREIGN KEY (course_id) REFERENCES courses(course_id) ON DELETE CASCADE,
    UNIQUE(course_id, section_id)
);

CREATE INDEX idx_sections_course ON sections(course_id);
CREATE INDEX idx_sections_lookup ON sections(section_id);

-- Meetings table (individual meeting times for each section)
CREATE TABLE IF NOT EXISTS meetings (
    meeting_id INTEGER PRIMARY KEY AUTOINCREMENT,
    section_id_pk INTEGER NOT NULL,
    meeting_type VARCHAR(10),
    meeting_days_type VARCHAR(10) NOT NULL,  -- 'repeated', 'onetime', or 'none'
    meeting_days TEXT,  -- JSON array for repeated, string for onetime, null for none
    start_hr INTEGER,
    start_min INTEGER,
    end_hr INTEGER,
    end_min INTEGER,
    building VARCHAR(50),
    room VARCHAR(50),
    instructors TEXT,  -- JSON array of instructor names
    created_at DATETIME NOT NULL,
    FOREIGN KEY (section_id_pk) REFERENCES sections(section_id_pk) ON DELETE CASCADE
);

CREATE INDEX idx_meetings_section ON meetings(section_id_pk);
