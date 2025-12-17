# Modifications to webreg_scraper

This is a modified version of [webreg_scraper](https://github.com/ewang2002/webreg_scraper) by Edward Wang, licensed under the MIT License.

## Original Project

- **Author**: Edward Wang
- **Repository**: https://github.com/ewang2002/webreg_scraper
- **License**: MIT (see LICENSE-ORIGINAL)

## Modifications Made

### Added Features

1. **Course Meeting Time/Schedule Data Collection**
   - SQLite database (`schedules.db`) for storing meeting times, days, locations
   - Initial scrape function that runs once at startup
   - Database schema with normalized tables: courses, sections, meetings

2. **Enhanced CSV Output**
   - Added columns: `meeting_type`, `meeting_days`, `start_time`, `end_time`, `building`, `room`
   - One row per meeting (sections with multiple meetings = multiple rows)
   - Maintains backward compatibility with original enrollment tracking

3. **New API Endpoints**
   - `GET /live/:term/schedule_data` - Returns all schedule data for a term
   - `GET /live/:term/schedule_data/:section_id` - Returns meetings for specific section
   - JSON responses with comprehensive meeting details

### Files Created

- `sql/init_schedules.sql` - Database schema
- `crates/webreg/src/db/mod.rs` - Database manager
- `crates/webreg/src/db/types.rs` - Database types
- `crates/webreg/src/server/endpoints/schedule.rs` - Schedule API endpoints

### Files Modified

- `crates/webreg/Cargo.toml` - Added rusqlite dependency
- `crates/webreg/src/main.rs` - Added db module
- `crates/webreg/src/types.rs` - Added schedule_db field to WrapperState
- `crates/webreg/src/scraper/tracker.rs` - Initial scrape + CSV enhancements
- `crates/webreg/src/server/mod.rs` - Registered new endpoints
- `crates/webreg/src/server/endpoints/mod.rs` - Added schedule module
- `scripts/webregautoin/tsconfig.json` - Added skipLibCheck for TypeScript compilation

## Purpose

These modifications enable the UCSD Class Planning Tool to access comprehensive course schedule data including meeting times, days of week, locations, and instructors - essential for building a visual class scheduler with calendar integration.

## Modified By

- Alec Asdourian
- Date: December 2024
- For: UCSD Class Planning Tool project
