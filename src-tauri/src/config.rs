use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("unable to resolve macOS Application Support directory")]
    SupportDirUnavailable,
    #[error("validation error: {0}")]
    Validation(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    pub ollama_base_url: String,
    pub ollama_api_key: String,
    pub vision_model: String,
    pub max_steps: u32,
    pub step_delay_ms: u64,
    pub confirm_risky_actions: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            ollama_base_url: "https://ollama.com".to_string(),
            ollama_api_key: String::new(),
            vision_model: "deepseek-v4.1-flash:cloud".to_string(),
            max_steps: 20,
            step_delay_ms: 250,
            confirm_risky_actions: true,
        }
    }
}

impl AppConfig {
    pub fn resolved_api_key(&self) -> String {
        let trimmed = self.ollama_api_key.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
        std::env::var("OLLAMA_API_KEY")
            .unwrap_or_default()
            .trim()
            .to_string()
    }

    pub fn sanitized(mut self) -> Result<Self, ConfigError> {
        self.ollama_base_url = validate_base_url(&self.ollama_base_url)?;
        self.ollama_api_key = self
            .ollama_api_key
            .chars()
            .filter(|c| !c.is_control())
            .collect::<String>()
            .trim()
            .to_string();
        self.vision_model = validate_model_name(&self.vision_model, "vision_model")?;
        if !(1..=50).contains(&self.max_steps) {
            return Err(ConfigError::Validation(
                "max_steps must be between 1 and 50".to_string(),
            ));
        }
        if !(50..=5000).contains(&self.step_delay_ms) {
            return Err(ConfigError::Validation(
                "step_delay_ms must be between 50 and 5000".to_string(),
            ));
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskStepRecord {
    pub step_number: u32,
    pub timestamp: DateTime<Utc>,
    pub action_type: String,
    pub description: String,
    pub narration: String,
    pub status: String,
    pub target_ocr_id: Option<u32>,
    pub target_x: Option<f64>,
    pub target_y: Option<f64>,
    #[serde(default)]
    pub verification_note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskHistoryRecord {
    pub task_id: String,
    pub goal: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
    pub outcome: String,
    pub summary: String,
    pub step_count: u32,
    pub steps: Vec<TaskStepRecord>,
}

pub fn app_support_dir() -> Result<PathBuf, ConfigError> {
    let base = dirs::data_dir().ok_or(ConfigError::SupportDirUnavailable)?;
    let dir = base.join("TaskCompanion");
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
    Ok(dir)
}

pub fn config_file_path() -> Result<PathBuf, ConfigError> {
    Ok(app_support_dir()?.join("config.json"))
}

pub fn history_file_path() -> Result<PathBuf, ConfigError> {
    Ok(app_support_dir()?.join("task-history.json"))
}

pub fn load_config() -> Result<AppConfig, ConfigError> {
    let path = config_file_path()?;
    if !path.exists() {
        let default_cfg = AppConfig::default();
        save_config(&default_cfg)?;
        return Ok(default_cfg);
    }
    let raw = fs::read_to_string(&path)?;
    match serde_json::from_str::<AppConfig>(&raw) {
        Ok(mut parsed) => {
            if parsed.vision_model == "qwen3-vl:235b-cloud" {
                parsed.vision_model = "deepseek-v4.1-flash:cloud".to_string();
                return save_config(&parsed);
            }
            parsed.sanitized()
        }
        Err(_) => {
            let default_cfg = AppConfig::default();
            save_config(&default_cfg)?;
            Ok(default_cfg)
        }
    }
}

pub fn save_config(config: &AppConfig) -> Result<AppConfig, ConfigError> {
    let validated = config.clone().sanitized()?;
    let path = config_file_path()?;
    let serialized = serde_json::to_string_pretty(&validated)?;
    fs::write(&path, serialized)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = fs::Permissions::from_mode(0o600);
        let _ = fs::set_permissions(&path, perms);
    }
    Ok(validated)
}

pub fn load_history() -> Result<Vec<TaskHistoryRecord>, ConfigError> {
    let path = history_file_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = fs::read_to_string(path)?;
    let mut records: Vec<TaskHistoryRecord> = serde_json::from_str(&raw)?;
    records.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    Ok(records)
}

pub fn append_history_record(record: TaskHistoryRecord) -> Result<(), ConfigError> {
    let mut existing = load_history().unwrap_or_default();
    existing.insert(0, record);
    if existing.len() > 100 {
        existing.truncate(100);
    }
    let path = history_file_path()?;
    let serialized = serde_json::to_string_pretty(&existing)?;
    fs::write(&path, serialized)?;
    Ok(())
}

pub fn validate_goal_input(goal: &str) -> Result<String, ConfigError> {
    let cleaned = goal
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .to_string();
    if cleaned.is_empty() {
        return Err(ConfigError::Validation(
            "task goal must not be empty".to_string(),
        ));
    }
    if cleaned.chars().count() > 500 {
        return Err(ConfigError::Validation(
            "task goal must not exceed 500 characters".to_string(),
        ));
    }
    Ok(cleaned)
}

fn validate_base_url(url: &str) -> Result<String, ConfigError> {
    let trimmed = url.trim().trim_end_matches('/').to_string();
    let allowed = trimmed.starts_with("https://ollama.com")
        || trimmed.starts_with("http://localhost:")
        || trimmed == "http://localhost"
        || trimmed.starts_with("http://127.0.0.1:")
        || trimmed == "http://127.0.0.1";
    if !allowed {
        return Err(ConfigError::Validation(
            "ollama_base_url must target https://ollama.com or localhost".to_string(),
        ));
    }
    Ok(trimmed)
}

fn validate_model_name(name: &str, field: &str) -> Result<String, ConfigError> {
    let trimmed = name.trim().to_string();
    if trimmed.is_empty() || trimmed.len() > 120 {
        return Err(ConfigError::Validation(format!(
            "{field} must be between 1 and 120 characters"
        )));
    }
    let valid_chars = trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':' | '/'));
    if !valid_chars {
        return Err(ConfigError::Validation(format!(
            "{field} contains invalid characters"
        )));
    }
    Ok(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_goal_and_config_bounds() {
        assert_eq!(validate_goal_input("  Open Notes  ").unwrap(), "Open Notes");
        assert!(validate_goal_input("   ").is_err());
        let too_long = "a".repeat(501);
        assert!(validate_goal_input(&too_long).is_err());

        let mut cfg = AppConfig::default();
        assert!(cfg.clone().sanitized().is_ok());

        cfg.max_steps = 0;
        assert!(cfg.clone().sanitized().is_err());

        cfg.max_steps = 20;
        cfg.step_delay_ms = 20;
        assert!(cfg.sanitized().is_err());
    }
}
