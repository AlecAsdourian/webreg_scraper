/// Degree progress processing and analysis
use super::config::RequirementsConfig;
use super::types::*;
use std::collections::HashSet;

/// Processes degree audit data to compute progress and recommendations
pub struct DegreeProgressProcessor {
    requirements_config: RequirementsConfig,
}

impl DegreeProgressProcessor {
    /// Creates a new processor with the given requirements configuration
    pub fn new(requirements_config: RequirementsConfig) -> Self {
        Self {
            requirements_config,
        }
    }

    /// Computes comprehensive degree progress from parsed audit
    ///
    /// # Arguments
    /// * `audit` - Parsed degree audit data
    ///
    /// # Returns
    /// * `Ok(DegreeProgress)` - Computed progress with recommendations
    /// * `Err` - If computation fails
    pub fn compute_degree_progress(
        &self,
        audit: &DegreeAudit,
    ) -> Result<DegreeProgress, Box<dyn std::error::Error>> {
        // Calculate total units completed (only count passing grades)
        let total_units_completed: f32 = audit
            .requirements
            .iter()
            .flat_map(|r| &r.courses)
            .filter_map(|c| {
                if let Some(ref grade) = c.grade {
                    if GradeValidator::is_passing_grade(grade) {
                        c.units
                    } else {
                        None
                    }
                } else {
                    c.units
                }
            })
            .sum();

        // Standard UCSD requirement (can be customized based on major)
        let total_units_required = 180.0;
        let total_units_remaining = (total_units_required - total_units_completed).max(0.0);

        // Build requirement summaries
        let requirements_summary = self.build_requirement_summaries(&audit.requirements);

        // Compute next courses to take
        let next_courses_to_take = self.compute_next_course_recommendations(
            &audit.requirements,
            &audit.student_info,
        )?;

        Ok(DegreeProgress {
            audit_id: audit.audit_id.clone(),
            student_info: audit.student_info.clone(),
            total_units_required,
            total_units_completed,
            total_units_remaining,
            requirements_summary,
            next_courses_to_take,
        })
    }

    /// Builds summary information for each requirement
    fn build_requirement_summaries(&self, requirements: &[Requirement]) -> Vec<RequirementSummary> {
        requirements
            .iter()
            .map(|req| {
                let units_required = req.credits_required.unwrap_or(0.0);
                let units_completed = req.credits_completed.unwrap_or(0.0);
                let units_remaining = (units_required - units_completed).max(0.0);

                let completed_subrequirements = req
                    .subrequirements
                    .iter()
                    .filter(|s| matches!(s.status, RequirementStatus::Complete))
                    .count();

                RequirementSummary {
                    category: req.category.clone(),
                    name: req.name.clone(),
                    status: req.status.clone(),
                    units_required,
                    units_completed,
                    units_remaining,
                    subrequirements_count: req.subrequirements.len(),
                    completed_subrequirements,
                }
            })
            .collect()
    }

    /// Computes recommendations for next courses to take
    ///
    /// Prioritizes incomplete subrequirements and filters out already completed courses.
    fn compute_next_course_recommendations(
        &self,
        requirements: &[Requirement],
        _student_info: &StudentInfo,
    ) -> Result<Vec<NextCourseRecommendation>, Box<dyn std::error::Error>> {
        let mut recommendations = Vec::new();

        // Build set of completed course codes for filtering
        let completed_courses: HashSet<String> = requirements
            .iter()
            .flat_map(|r| &r.courses)
            .filter(|c| {
                if let Some(ref grade) = c.grade {
                    GradeValidator::is_passing_grade(grade)
                } else {
                    false
                }
            })
            .map(|c| c.course_code.clone())
            .collect();

        // Collect recommendations from incomplete subrequirements
        let mut priority = 1;

        for req in requirements {
            // Skip completed requirements
            if matches!(req.status, RequirementStatus::Complete) {
                continue;
            }

            for subreq in &req.subrequirements {
                // Skip completed subrequirements
                if matches!(subreq.status, RequirementStatus::Complete) {
                    continue;
                }

                // Filter out already completed courses
                let available_courses: Vec<EligibleCourse> = subreq
                    .eligible_courses
                    .iter()
                    .filter(|course| !completed_courses.contains(&course.full_code))
                    .cloned()
                    .collect();

                // Only add if there are available courses and units remaining
                if !available_courses.is_empty() && subreq.units_remaining > 0.0 {
                    recommendations.push(NextCourseRecommendation {
                        subrequirement_title: subreq.title.clone(),
                        priority,
                        eligible_courses: available_courses,
                        units_needed: subreq.units_remaining,
                    });
                    priority += 1;
                }
            }
        }

        // Sort by priority (already set sequentially, but ensure ordering)
        recommendations.sort_by_key(|r| r.priority);

        Ok(recommendations)
    }

    /// Matches completed courses against a subrequirement config
    ///
    /// Useful for validating which courses fulfill a particular requirement.
    ///
    /// # Arguments
    /// * `completed_courses` - List of courses the student has completed
    /// * `subreq_config` - Configuration for the subrequirement
    ///
    /// # Returns
    /// * Vector of courses that match the subrequirement criteria
    pub fn match_courses_to_subrequirement(
        &self,
        completed_courses: &[CourseRequirement],
        subreq_config: &super::config::SubrequirementConfig,
    ) -> Vec<CourseRequirement> {
        completed_courses
            .iter()
            .filter(|course| {
                // Check if course is in eligible_courses list
                if !subreq_config.eligible_courses.is_empty() {
                    return subreq_config
                        .eligible_courses
                        .iter()
                        .any(|eligible| course.course_code.contains(eligible));
                }

                // Check if course is in specified departments
                if !subreq_config.departments.is_empty() {
                    let course_dept = course.course_code.split_whitespace().next().unwrap_or("");
                    return subreq_config
                        .departments
                        .iter()
                        .any(|dept| course_dept == dept);
                }

                false
            })
            .cloned()
            .collect()
    }

    /// Gets the requirements configuration
    pub fn config(&self) -> &RequirementsConfig {
        &self.requirements_config
    }
}
