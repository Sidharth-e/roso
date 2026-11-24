use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use thiserror::Error;

pub const OLLAMA_BASE_URL: &str = "https://ollama.com";
pub const DEFAULT_JUDGE_MODEL: &str = "gemma4:31b-cloud";
pub const DEFAULT_PLANNER_MODEL: &str = "kimi-k3.1-pro:cloud";
pub const DEFAULT_VISION_ACTOR_MODEL: &str = "deepseek-v4.1-flash:cloud";
pub const DEFAULT_CODING_ACTOR_MODEL: &str = "qwen3-vl:235b-cloud";
pub const DEFAULT_WRITING_ACTOR_MODEL: &str = "kimi-k3.1-pro:cloud";
pub const DEFAULT_RECOVERY_MODEL: &str = "qwen3-vl:235b-cloud";
pub const VISION_MODEL: &str = DEFAULT_VISION_ACTOR_MODEL;
pub const MAX_STEPS: u32 = 20;
pub const STEP_DELAY_MS: u64 = 250;
pub const CONFIRM_RISKY_ACTIONS: bool = true;

fn default_judge_model() -> String {
    DEFAULT_JUDGE_MODEL.to_string()
}

fn default_planner_model() -> String {
    DEFAULT_PLANNER_MODEL.to_string()
}

fn default_vision_actor_model() -> String {
    DEFAULT_VISION_ACTOR_MODEL.to_string()
}

fn default_coding_actor_model() -> String {
    DEFAULT_CODING_ACTOR_MODEL.to_string()
}

fn default_writing_actor_model() -> String {
    DEFAULT_WRITING_ACTOR_MODEL.to_string()
}

fn default_recovery_model() -> String {
    DEFAULT_RECOVERY_MODEL.to_string()
}

fn default_true() -> bool {
    true
}

pub fn sanitize_model_name(raw: &str, fallback: &str) -> String {
    let cleaned = raw
        .chars()
        .filter(|c| !c.is_control() && !c.is_whitespace())
        .take(120)
        .collect::<String>()
        .trim()
        .to_string();
    if cleaned.is_empty() {
        fallback.to_string()
    } else {
        cleaned
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelRoutingConfig {
    #[serde(default = "default_judge_model")]
    pub judge_model: String,
    #[serde(default = "default_planner_model")]
    pub planner_model: String,
    #[serde(default = "default_vision_actor_model")]
    pub vision_actor_model: String,
    #[serde(default = "default_coding_actor_model")]
    pub coding_actor_model: String,
    #[serde(default = "default_writing_actor_model")]
    pub writing_actor_model: String,
    #[serde(default = "default_recovery_model")]
    pub recovery_model: String,
    #[serde(default = "default_true")]
    pub enable_judge_verification: bool,
    #[serde(default = "default_true")]
    pub auto_open_panel: bool,
}

impl Default for ModelRoutingConfig {
    fn default() -> Self {
        Self {
            judge_model: default_judge_model(),
            planner_model: default_planner_model(),
            vision_actor_model: default_vision_actor_model(),
            coding_actor_model: default_coding_actor_model(),
            writing_actor_model: default_writing_actor_model(),
            recovery_model: default_recovery_model(),
            enable_judge_verification: true,
            auto_open_panel: true,
        }
    }
}

impl ModelRoutingConfig {
    pub fn sanitized(mut self) -> Self {
        self.judge_model = sanitize_model_name(&self.judge_model, DEFAULT_JUDGE_MODEL);
        self.planner_model = sanitize_model_name(&self.planner_model, DEFAULT_PLANNER_MODEL);
        self.vision_actor_model =
            sanitize_model_name(&self.vision_actor_model, DEFAULT_VISION_ACTOR_MODEL);
        self.coding_actor_model =
            sanitize_model_name(&self.coding_actor_model, DEFAULT_CODING_ACTOR_MODEL);
        self.writing_actor_model =
            sanitize_model_name(&self.writing_actor_model, DEFAULT_WRITING_ACTOR_MODEL);
        self.recovery_model = sanitize_model_name(&self.recovery_model, DEFAULT_RECOVERY_MODEL);
        self
    }
}

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

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    #[serde(default)]
    pub ollama_api_key: String,
    #[serde(default)]
    pub models: ModelRoutingConfig,
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
        self.ollama_api_key = self
            .ollama_api_key
            .chars()
            .filter(|c| !c.is_control())
            .collect::<String>()
            .trim()
            .to_string();
        self.models = self.models.sanitized();
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
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub thought: Option<String>,
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
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub selected_model: Option<String>,
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
        Ok(parsed) => parsed.sanitized(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_goal_and_config_bounds() {
        assert_eq!(validate_goal_input("  Open Notes  ").unwrap(), "Open Notes");
        assert!(validate_goal_input("   ").is_err());
        let too_long = "a".repeat(501);
        assert!(validate_goal_input(&too_long).is_err());

        let cfg = AppConfig {
            ollama_api_key: "  sk-test\n\t123  ".to_string(),
            models: ModelRoutingConfig {
                judge_model: "  \n".to_string(),
                planner_model: "kimi-k3:cloud\t".to_string(),
                ..ModelRoutingConfig::default()
            },
        };
        let sanitized = cfg.sanitized().unwrap();
        assert_eq!(sanitized.ollama_api_key, "sk-test123");
        assert_eq!(sanitized.models.judge_model, DEFAULT_JUDGE_MODEL);
        assert_eq!(sanitized.models.planner_model, "kimi-k3:cloud");
        assert_eq!(
            sanitized.models.vision_actor_model,
            DEFAULT_VISION_ACTOR_MODEL
        );
        assert_eq!(
            sanitized.models.coding_actor_model,
            DEFAULT_CODING_ACTOR_MODEL
        );
        assert_eq!(
            sanitized.models.writing_actor_model,
            DEFAULT_WRITING_ACTOR_MODEL
        );
        assert_eq!(sanitized.models.recovery_model, DEFAULT_RECOVERY_MODEL);
        assert!(sanitized.models.enable_judge_verification);
        assert!(sanitized.models.auto_open_panel);

        let legacy_json = r#"{
            "ollamaBaseUrl": "https://ollama.com",
            "ollamaApiKey": "legacy-key",
            "visionModel": "qwen3-vl:235b-cloud",
            "maxSteps": 30,
            "stepDelayMs": 500,
            "confirmRiskyActions": false
        }"#;
        let parsed_legacy: AppConfig = serde_json::from_str(legacy_json).unwrap();
        assert_eq!(parsed_legacy.ollama_api_key, "legacy-key");
        assert_eq!(parsed_legacy.models, ModelRoutingConfig::default());

        assert_eq!(OLLAMA_BASE_URL, "https://ollama.com");
        assert_eq!(VISION_MODEL, "deepseek-v4.1-flash:cloud");
        assert_eq!(MAX_STEPS, 20);
        assert_eq!(STEP_DELAY_MS, 250);
        assert!(CONFIRM_RISKY_ACTIONS);
    }
}
