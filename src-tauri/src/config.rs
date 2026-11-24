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
    pub classifier_model: String,
    pub nudge_model: String,
    pub developer_mode: bool,
    pub default_duration_minutes: u32,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            ollama_base_url: "https://ollama.com".to_string(),
            ollama_api_key: String::new(),
            classifier_model: "clef".to_string(),
            nudge_model: "deepseek-v4.1-flash:cloud".to_string(),
            developer_mode: false,
            default_duration_minutes: 25,
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
        self.classifier_model = validate_model_name(&self.classifier_model, "classifier_model")?;
        self.nudge_model = validate_model_name(&self.nudge_model, "nudge_model")?;
        if !(1..=240).contains(&self.default_duration_minutes) {
            return Err(ConfigError::Validation(
                "default_duration_minutes must be between 1 and 240".to_string(),
            ));
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbabilityLogEntry {
    pub timestamp: DateTime<Utc>,
    pub choice: String,
    pub prob_off_task: f64,
    pub confidence: f64,
    pub evidence_focus_seconds: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionHistoryRecord {
    pub session_id: String,
    pub declared_task: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
    pub planned_duration_minutes: u32,
    pub developer_mode: bool,
    pub check_count: u32,
    pub probability_logs: Vec<ProbabilityLogEntry>,
    pub nudge_timestamps: Vec<DateTime<Utc>>,
}

pub fn app_support_dir() -> Result<PathBuf, ConfigError> {
    let base = dirs::data_dir().ok_or(ConfigError::SupportDirUnavailable)?;
    let dir = base.join("FocusCompanion");
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
    Ok(dir)
}

pub fn config_file_path() -> Result<PathBuf, ConfigError> {
    Ok(app_support_dir()?.join("config.json"))
}

pub fn history_file_path() -> Result<PathBuf, ConfigError> {
    Ok(app_support_dir()?.join("focus-history.json"))
}

pub fn load_config() -> Result<AppConfig, ConfigError> {
    let path = config_file_path()?;
    if !path.exists() {
        let default_cfg = AppConfig::default();
        save_config(&default_cfg)?;
        return Ok(default_cfg);
    }
    let raw = fs::read_to_string(path)?;
    let parsed: AppConfig = serde_json::from_str(&raw)?;
    parsed.sanitized()
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

pub fn load_history() -> Result<Vec<SessionHistoryRecord>, ConfigError> {
    let path = history_file_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = fs::read_to_string(path)?;
    let mut records: Vec<SessionHistoryRecord> = serde_json::from_str(&raw)?;
    records.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    Ok(records)
}

pub fn append_history_record(record: SessionHistoryRecord) -> Result<(), ConfigError> {
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

pub fn validate_task_input(task: &str) -> Result<String, ConfigError> {
    let cleaned = task
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .to_string();
    if cleaned.is_empty() {
        return Err(ConfigError::Validation(
            "declared task must not be empty".to_string(),
        ));
    }
    if cleaned.chars().count() > 240 {
        return Err(ConfigError::Validation(
            "declared task must not exceed 240 characters".to_string(),
        ));
    }
    Ok(cleaned)
}

pub fn validate_duration_minutes(minutes: u32) -> Result<u32, ConfigError> {
    if !(1..=240).contains(&minutes) {
        return Err(ConfigError::Validation(
            "duration must be between 1 and 240 minutes".to_string(),
        ));
    }
    Ok(minutes)
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
