/// Configuration system for college and major requirements
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Top-level requirements configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequirementsConfig {
    pub colleges: HashMap<String, CollegeRequirements>,
    pub majors: HashMap<String, MajorRequirements>,
}

/// College-specific requirements (e.g., Warren, Revelle, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollegeRequirements {
    pub college_code: String,
    pub college_name: String,
    pub requirements: Vec<RequirementCategory>,
}

/// Major-specific requirements (e.g., MA30, CS25, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MajorRequirements {
    pub major_code: String,
    pub major_name: String,
    pub requirements: Vec<RequirementCategory>,
}

/// Category of requirements (e.g., "Lower Division", "Upper Division")
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequirementCategory {
    pub category: String,
    pub subrequirements: Vec<SubrequirementConfig>,
}

/// Configuration for a single subrequirement
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubrequirementConfig {
    pub title: String,
    pub required_units: f32,
    #[serde(default)]
    pub eligible_courses: Vec<String>,
    #[serde(default)]
    pub departments: Vec<String>,
    #[serde(default)]
    pub level_filters: Vec<String>, // "l" (lower), "u" (upper), "g" (graduate)
}

impl RequirementsConfig {
    /// Loads all requirement configs from the requirements_config directory
    ///
    /// # Arguments
    /// * `config_dir` - Path to the requirements_config directory
    ///
    /// # Returns
    /// * `Ok(RequirementsConfig)` - Loaded configuration with all colleges and majors
    /// * `Err` - If directory doesn't exist or files can't be parsed
    pub fn load_from_directory(config_dir: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let colleges_dir = config_dir.join("colleges");
        let majors_dir = config_dir.join("majors");

        let mut colleges = HashMap::new();
        let mut majors = HashMap::new();

        // Load college configs
        if colleges_dir.exists() && colleges_dir.is_dir() {
            for entry in fs::read_dir(colleges_dir)? {
                let entry = entry?;
                let path = entry.path();

                if path.extension().and_then(|s| s.to_str()) == Some("json") {
                    let content = fs::read_to_string(&path)?;
                    let college_req: CollegeRequirements = serde_json::from_str(&content)?;
                    colleges.insert(college_req.college_code.clone(), college_req);
                }
            }
        }

        // Load major configs
        if majors_dir.exists() && majors_dir.is_dir() {
            for entry in fs::read_dir(majors_dir)? {
                let entry = entry?;
                let path = entry.path();

                if path.extension().and_then(|s| s.to_str()) == Some("json") {
                    let content = fs::read_to_string(&path)?;
                    let major_req: MajorRequirements = serde_json::from_str(&content)?;
                    majors.insert(major_req.major_code.clone(), major_req);
                }
            }
        }

        Ok(RequirementsConfig { colleges, majors })
    }

    /// Creates an empty configuration
    pub fn empty() -> Self {
        RequirementsConfig {
            colleges: HashMap::new(),
            majors: HashMap::new(),
        }
    }

    /// Gets college requirements by code
    pub fn get_college(&self, college_code: &str) -> Option<&CollegeRequirements> {
        self.colleges.get(college_code)
    }

    /// Gets major requirements by code
    pub fn get_major(&self, major_code: &str) -> Option<&MajorRequirements> {
        self.majors.get(major_code)
    }
}

impl Default for RequirementsConfig {
    fn default() -> Self {
        Self::empty()
    }
}
