use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use thiserror::Error;

pub const OLLAMA_BASE_URL: &str = "https://ollama.com";
pub const LOCAL_OLLAMA_BASE_URL: &str = "http://localhost:11434";
pub const DEFAULT_JUDGE_MODEL: &str = "gemma4:31b-cloud";
pub const DEFAULT_PLANNER_MODEL: &str = "kimi-k3.1-pro:cloud";
pub const DEFAULT_VISION_ACTOR_MODEL: &str = "deepseek-v4.1-flash:cloud";
pub const DEFAULT_CODING_ACTOR_MODEL: &str = "qwen3-vl:235b-cloud";
pub const DEFAULT_WRITING_ACTOR_MODEL: &str = "kimi-k3.1-pro:cloud";
pub const DEFAULT_RECOVERY_MODEL: &str = "qwen3-vl:235b-cloud";
pub const VISION_MODEL: &str = DEFAULT_VISION_ACTOR_MODEL;
pub const MAX_STEPS: u32 = 40;
pub const STEP_DELAY_MS: u64 = 250;
pub const CONFIRM_RISKY_ACTIONS: bool = true;

fn default_ollama_base_url() -> String {
    OLLAMA_BASE_URL.to_string()
}

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

pub fn is_cloud_endpoint(base_url: &str) -> bool {
    base_url.trim().starts_with("https://ollama.com")
}

pub fn validate_ollama_base_url(raw: &str) -> Result<String, ConfigError> {
    let cleaned = raw
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .trim_end_matches('/')
        .to_string();
    if cleaned.is_empty() {
        return Ok(OLLAMA_BASE_URL.to_string());
    }
    let lower = cleaned.to_ascii_lowercase();
    if lower == "https://ollama.com" {
        return Ok(OLLAMA_BASE_URL.to_string());
    }
    if lower == "http://localhost" {
        return Ok("http://localhost".to_string());
    }
    if lower == "http://127.0.0.1" {
        return Ok("http://127.0.0.1".to_string());
    }
    for prefix in ["http://localhost:", "http://127.0.0.1:"] {
        if let Some(port_str) = lower.strip_prefix(prefix) {
            if !port_str.is_empty()
                && port_str.len() <= 5
                && port_str.chars().all(|c| c.is_ascii_digit())
            {
                if let Ok(port) = port_str.parse::<u16>() {
                    if port >= 1 {
                        return Ok(format!("{prefix}{port}"));
                    }
                }
            }
            return Err(ConfigError::Validation(
                "local Ollama URL port must be between 1 and 65535".to_string(),
            ));
        }
    }
    Err(ConfigError::Validation(
        "Ollama endpoint must be https://ollama.com, http://localhost[:port], or http://127.0.0.1[:port]".to_string(),
    ))
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    #[serde(default = "default_ollama_base_url")]
    pub ollama_base_url: String,
    #[serde(default)]
    pub ollama_api_key: String,
    #[serde(default)]
    pub cloud_consent_accepted: bool,
    #[serde(default)]
    pub models: ModelRoutingConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            ollama_base_url: default_ollama_base_url(),
            ollama_api_key: String::new(),
            cloud_consent_accepted: false,
            models: ModelRoutingConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PublicAppConfig {
    pub ollama_base_url: String,
    pub has_api_key: bool,
    pub api_key_preview: String,
    pub cloud_consent_accepted: bool,
    pub models: ModelRoutingConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAppConfigRequest {
    #[serde(default = "default_ollama_base_url")]
    pub ollama_base_url: String,
    #[serde(default)]
    pub ollama_api_key: String,
    #[serde(default)]
    pub clear_api_key: bool,
    #[serde(default)]
    pub cloud_consent_accepted: bool,
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

    pub fn resolved_base_url(&self) -> String {
        validate_ollama_base_url(&self.ollama_base_url).unwrap_or_else(|_| OLLAMA_BASE_URL.to_string())
    }

    pub fn sanitized(mut self) -> Result<Self, ConfigError> {
        self.ollama_base_url = validate_ollama_base_url(&self.ollama_base_url)?;
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

    pub fn to_public(&self) -> PublicAppConfig {
        let key = self.resolved_api_key();
        let has_api_key = !key.is_empty();
        let api_key_preview = mask_api_key(&key);
        PublicAppConfig {
            ollama_base_url: self.resolved_base_url(),
            has_api_key,
            api_key_preview,
            cloud_consent_accepted: self.cloud_consent_accepted,
            models: self.models.clone(),
        }
    }

    pub fn apply_update(&self, req: UpdateAppConfigRequest) -> Result<Self, ConfigError> {
        let new_key_clean = req
            .ollama_api_key
            .chars()
            .filter(|c| !c.is_control())
            .collect::<String>()
            .trim()
            .to_string();
        let next_key = if req.clear_api_key {
            String::new()
        } else if !new_key_clean.is_empty() {
            new_key_clean
        } else {
            self.ollama_api_key.clone()
        };

        Self {
            ollama_base_url: req.ollama_base_url,
            ollama_api_key: next_key,
            cloud_consent_accepted: req.cloud_consent_accepted,
            models: req.models,
        }
        .sanitized()
    }
}

pub fn mask_api_key(key: &str) -> String {
    let trimmed = key.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let count = trimmed.chars().count();
    if count <= 4 {
        return "••••".to_string();
    }
    let tail: String = trimmed
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("••••{tail}")
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
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o700));
        }
    }
    Ok(dir)
}

pub fn config_file_path() -> Result<PathBuf, ConfigError> {
    Ok(app_support_dir()?.join("config.json"))
}

pub fn history_file_path() -> Result<PathBuf, ConfigError> {
    Ok(app_support_dir()?.join("task-history.json"))
}

fn write_private_file(path: &Path, content: &str) -> Result<(), ConfigError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
        Ok(())
    }
    #[cfg(not(unix))]
    {
        fs::write(path, content)?;
        Ok(())
    }
}

// Debug builds are ad-hoc signed with a new hash every build, so Keychain "Always Allow"
// never sticks. Skip the Keychain there; the key stays in the 0600 config file instead.
#[cfg(all(target_os = "macos", not(test), not(debug_assertions)))]
mod keychain {
    use std::ffi::c_void;

    const SERVICE: &str = "com.focuscompanion.desktop";
    const ACCOUNT: &str = "ollama_api_key";
    const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;

    #[link(name = "Security", kind = "framework")]
    extern "C" {
        static kSecClass: *const c_void;
        static kSecClassGenericPassword: *const c_void;
        static kSecAttrService: *const c_void;
        static kSecAttrAccount: *const c_void;
        static kSecValueData: *const c_void;
        static kSecReturnData: *const c_void;
        static kSecMatchLimit: *const c_void;
        static kSecMatchLimitOne: *const c_void;
        fn SecItemCopyMatching(query: *const c_void, result: *mut *const c_void) -> i32;
        fn SecItemAdd(attributes: *const c_void, result: *mut *const c_void) -> i32;
        fn SecItemUpdate(query: *const c_void, attributes_to_update: *const c_void) -> i32;
        fn SecItemDelete(query: *const c_void) -> i32;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        static kCFBooleanTrue: *const c_void;
        static kCFTypeDictionaryKeyCallBacks: u8;
        static kCFTypeDictionaryValueCallBacks: u8;
        fn CFDictionaryCreate(
            allocator: *const c_void,
            keys: *const *const c_void,
            values: *const *const c_void,
            num_values: isize,
            key_callbacks: *const u8,
            value_callbacks: *const u8,
        ) -> *const c_void;
        fn CFStringCreateWithBytes(
            alloc: *const c_void,
            bytes: *const u8,
            num_bytes: isize,
            encoding: u32,
            is_external_representation: u8,
        ) -> *const c_void;
        fn CFDataCreate(alloc: *const c_void, bytes: *const u8, length: isize) -> *const c_void;
        fn CFDataGetBytePtr(the_data: *const c_void) -> *const u8;
        fn CFDataGetLength(the_data: *const c_void) -> isize;
        fn CFDataGetTypeID() -> usize;
        fn CFGetTypeID(cf: *const c_void) -> usize;
        fn CFRelease(cf: *const c_void);
    }

    struct CfPtr(*const c_void);

    impl CfPtr {
        fn new(ptr: *const c_void) -> Option<Self> {
            if ptr.is_null() {
                None
            } else {
                Some(Self(ptr))
            }
        }
    }

    impl Drop for CfPtr {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { CFRelease(self.0) };
            }
        }
    }

    fn cf_str(s: &str) -> Option<CfPtr> {
        CfPtr::new(unsafe {
            CFStringCreateWithBytes(
                std::ptr::null(),
                s.as_ptr(),
                s.len() as isize,
                K_CF_STRING_ENCODING_UTF8,
                0,
            )
        })
    }

    fn cf_dict(keys: &[*const c_void], values: &[*const c_void]) -> Option<CfPtr> {
        CfPtr::new(unsafe {
            CFDictionaryCreate(
                std::ptr::null(),
                keys.as_ptr(),
                values.as_ptr(),
                keys.len() as isize,
                &kCFTypeDictionaryKeyCallBacks,
                &kCFTypeDictionaryValueCallBacks,
            )
        })
    }

    pub fn load_api_key() -> Option<String> {
        let svc = cf_str(SERVICE)?;
        let acct = cf_str(ACCOUNT)?;
        unsafe {
            let keys = [
                kSecClass,
                kSecAttrService,
                kSecAttrAccount,
                kSecReturnData,
                kSecMatchLimit,
            ];
            let values = [
                kSecClassGenericPassword,
                svc.0,
                acct.0,
                kCFBooleanTrue,
                kSecMatchLimitOne,
            ];
            let query = cf_dict(&keys, &values)?;
            let mut result: *const c_void = std::ptr::null();
            let status = SecItemCopyMatching(query.0, &mut result);
            let data = CfPtr::new(result)?;
            if status != 0 || CFGetTypeID(data.0) != CFDataGetTypeID() {
                return None;
            }
            let len = CFDataGetLength(data.0);
            if len <= 0 {
                return None;
            }
            let ptr = CFDataGetBytePtr(data.0);
            let bytes = std::slice::from_raw_parts(ptr, len as usize);
            let s = String::from_utf8_lossy(bytes).trim().to_string();
            if s.is_empty() {
                None
            } else {
                Some(s)
            }
        }
    }

    pub fn store_api_key(key: &str) -> bool {
        let Some(svc) = cf_str(SERVICE) else {
            return false;
        };
        let Some(acct) = cf_str(ACCOUNT) else {
            return false;
        };
        let trimmed = key.trim();
        unsafe {
            let query_keys = [kSecClass, kSecAttrService, kSecAttrAccount];
            let query_vals = [kSecClassGenericPassword, svc.0, acct.0];
            let Some(query) = cf_dict(&query_keys, &query_vals) else {
                return false;
            };

            if trimmed.is_empty() {
                let status = SecItemDelete(query.0);
                return status == 0 || status == -25300;
            }

            let Some(data) = CfPtr::new(CFDataCreate(
                std::ptr::null(),
                trimmed.as_ptr(),
                trimmed.len() as isize,
            )) else {
                return false;
            };

            let update_keys = [kSecValueData];
            let update_vals = [data.0];
            let Some(update_attrs) = cf_dict(&update_keys, &update_vals) else {
                return false;
            };

            let update_status = SecItemUpdate(query.0, update_attrs.0);
            if update_status == 0 {
                return true;
            }

            let add_keys = [kSecClass, kSecAttrService, kSecAttrAccount, kSecValueData];
            let add_vals = [kSecClassGenericPassword, svc.0, acct.0, data.0];
            let Some(add_attrs) = cf_dict(&add_keys, &add_vals) else {
                return false;
            };
            SecItemAdd(add_attrs.0, std::ptr::null_mut()) == 0
        }
    }
}

#[cfg(not(all(target_os = "macos", not(test), not(debug_assertions))))]
mod keychain {
    pub fn load_api_key() -> Option<String> {
        None
    }

    pub fn store_api_key(_key: &str) -> bool {
        false
    }
}

pub fn load_config() -> Result<AppConfig, ConfigError> {
    let path = config_file_path()?;
    if !path.exists() {
        let mut default_cfg = AppConfig::default();
        if let Some(kc_key) = keychain::load_api_key() {
            default_cfg.ollama_api_key = kc_key;
        }
        save_config(&default_cfg)?;
        return Ok(default_cfg);
    }
    let raw = fs::read_to_string(&path)?;
    match serde_json::from_str::<AppConfig>(&raw) {
        Ok(mut parsed) => {
            // A bad base URL must not discard the rest of the config (including the Keychain
            // key lookup below): fall back to the default endpoint and keep going.
            if validate_ollama_base_url(&parsed.ollama_base_url).is_err() {
                parsed.ollama_base_url = default_ollama_base_url();
            }
            let mut sanitized = parsed.sanitized()?;
            if sanitized.ollama_api_key.is_empty() {
                if let Some(kc_key) = keychain::load_api_key() {
                    sanitized.ollama_api_key = kc_key;
                }
            } else if keychain::store_api_key(&sanitized.ollama_api_key) {
                let mut disk_copy = sanitized.clone();
                disk_copy.ollama_api_key = String::new();
                if let Ok(serialized) = serde_json::to_string_pretty(&disk_copy) {
                    let _ = write_private_file(&path, &serialized);
                }
            }
            Ok(sanitized)
        }
        Err(_) => {
            let mut default_cfg = AppConfig::default();
            if let Some(kc_key) = keychain::load_api_key() {
                default_cfg.ollama_api_key = kc_key;
            }
            save_config(&default_cfg)?;
            Ok(default_cfg)
        }
    }
}

pub fn save_config(config: &AppConfig) -> Result<AppConfig, ConfigError> {
    let validated = config.clone().sanitized()?;
    let path = config_file_path()?;
    let mut disk_cfg = validated.clone();
    if keychain::store_api_key(&validated.ollama_api_key) {
        disk_cfg.ollama_api_key = String::new();
    }
    let serialized = serde_json::to_string_pretty(&disk_cfg)?;
    write_private_file(&path, &serialized)?;
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
    write_private_file(&path, &serialized)?;
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
            ollama_base_url: "https://ollama.com/".to_string(),
            ollama_api_key: "  sk-test\n\t123456  ".to_string(),
            cloud_consent_accepted: true,
            models: ModelRoutingConfig {
                judge_model: "  \n".to_string(),
                planner_model: "kimi-k3:cloud\t".to_string(),
                ..ModelRoutingConfig::default()
            },
        };
        let sanitized = cfg.sanitized().unwrap();
        assert_eq!(sanitized.ollama_base_url, "https://ollama.com");
        assert_eq!(sanitized.ollama_api_key, "sk-test123456");
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

        let public_cfg = sanitized.to_public();
        assert!(public_cfg.has_api_key);
        assert_eq!(public_cfg.api_key_preview, "••••3456");

        let keep_key_update = sanitized
            .apply_update(UpdateAppConfigRequest {
                ollama_base_url: "http://localhost:11434".to_string(),
                ollama_api_key: String::new(),
                clear_api_key: false,
                cloud_consent_accepted: true,
                models: ModelRoutingConfig::default(),
            })
            .unwrap();
        assert_eq!(keep_key_update.ollama_base_url, "http://localhost:11434");
        assert_eq!(keep_key_update.ollama_api_key, "sk-test123456");

        let cleared = keep_key_update
            .apply_update(UpdateAppConfigRequest {
                ollama_base_url: "https://ollama.com".to_string(),
                ollama_api_key: String::new(),
                clear_api_key: true,
                cloud_consent_accepted: false,
                models: ModelRoutingConfig::default(),
            })
            .unwrap();
        assert!(cleared.ollama_api_key.is_empty());
        assert!(!cleared.to_public().has_api_key);

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
        assert_eq!(MAX_STEPS, 40);
        assert_eq!(STEP_DELAY_MS, 250);
        assert!(CONFIRM_RISKY_ACTIONS);
    }

    #[test]
    fn validates_ollama_endpoint_allowlist() {
        assert_eq!(
            validate_ollama_base_url("https://ollama.com/").unwrap(),
            "https://ollama.com"
        );
        assert_eq!(
            validate_ollama_base_url("http://localhost:11434/").unwrap(),
            "http://localhost:11434"
        );
        assert_eq!(
            validate_ollama_base_url("http://127.0.0.1:11434").unwrap(),
            "http://127.0.0.1:11434"
        );
        assert!(validate_ollama_base_url("https://evil.example.com").is_err());
        assert!(validate_ollama_base_url("http://localhost:11434/evil").is_err());
        assert!(validate_ollama_base_url("http://localhost:0").is_err());
    }
}
