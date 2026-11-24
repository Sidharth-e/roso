use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use reqwest::Client;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;

const AGENT_REQUEST_TIMEOUT: Duration = Duration::from_secs(35);

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("network request failed: {0}")]
    Http(String),
    #[error("API returned status {status}: {body}")]
    UpstreamStatus { status: u16, body: String },
    #[error("invalid API response payload: {0}")]
    MalformedPayload(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskPlan {
    pub steps: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct NextActionContext {
    pub goal: String,
    pub plan_steps: Vec<String>,
    pub step_number: u32,
    pub max_steps: u32,
    pub active_app: String,
    pub window_title: String,
    pub screen_width: u32,
    pub screen_height: u32,
    pub ocr_lines: Vec<String>,
    pub recent_steps: Vec<String>,
    pub base64_jpeg: String,
}

fn deserialize_null_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    T: Default + Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    Ok(Option::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FollowUpAction {
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub narration: String,
    pub action_type: String,
    #[serde(default)]
    pub target_ocr_id: Option<u32>,
    #[serde(default)]
    pub x_norm: Option<f64>,
    #[serde(default)]
    pub y_norm: Option<f64>,
    #[serde(default)]
    pub from_x_norm: Option<f64>,
    #[serde(default)]
    pub from_y_norm: Option<f64>,
    #[serde(default)]
    pub to_x_norm: Option<f64>,
    #[serde(default)]
    pub to_y_norm: Option<f64>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub replace: bool,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub submit: bool,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub modifiers: Vec<String>,
    #[serde(default)]
    pub scroll_delta_y: Option<i32>,
    #[serde(default)]
    pub scroll_delta_x: Option<i32>,
    #[serde(default)]
    pub app_name: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub wait_ms: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub requires_confirmation: bool,
}

impl FollowUpAction {
    pub fn into_planned_decision(self, plan_step_index: u32) -> PlannedActionDecision {
        PlannedActionDecision {
            thought: String::new(),
            narration: self.narration,
            plan_step_index,
            action_type: self.action_type,
            target_ocr_id: self.target_ocr_id,
            x_norm: self.x_norm,
            y_norm: self.y_norm,
            from_x_norm: self.from_x_norm,
            from_y_norm: self.from_y_norm,
            to_x_norm: self.to_x_norm,
            to_y_norm: self.to_y_norm,
            text: self.text,
            replace: self.replace,
            submit: self.submit,
            key: self.key,
            modifiers: self.modifiers,
            scroll_delta_y: self.scroll_delta_y,
            scroll_delta_x: self.scroll_delta_x,
            app_name: self.app_name,
            url: self.url,
            path: self.path,
            wait_ms: self.wait_ms,
            requires_confirmation: self.requires_confirmation,
            summary: None,
            plan_steps: Vec::new(),
            follow_up_actions: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlannedActionDecision {
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub thought: String,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub narration: String,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub plan_step_index: u32,
    pub action_type: String,
    #[serde(default)]
    pub target_ocr_id: Option<u32>,
    #[serde(default)]
    pub x_norm: Option<f64>,
    #[serde(default)]
    pub y_norm: Option<f64>,
    #[serde(default)]
    pub from_x_norm: Option<f64>,
    #[serde(default)]
    pub from_y_norm: Option<f64>,
    #[serde(default)]
    pub to_x_norm: Option<f64>,
    #[serde(default)]
    pub to_y_norm: Option<f64>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub replace: bool,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub submit: bool,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub modifiers: Vec<String>,
    #[serde(default)]
    pub scroll_delta_y: Option<i32>,
    #[serde(default)]
    pub scroll_delta_x: Option<i32>,
    #[serde(default)]
    pub app_name: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub wait_ms: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub requires_confirmation: bool,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub plan_steps: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub follow_up_actions: Vec<FollowUpAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StepVerification {
    pub status: String,
    pub observation: String,
    #[serde(default)]
    pub goal_achieved: bool,
    #[serde(default)]
    pub recovery_hint: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct VerifyStepContext {
    pub goal: String,
    pub step_number: u32,
    pub action_description: String,
    pub expected_narration: String,
    pub before_app: String,
    pub after_app: String,
    pub before_title: String,
    pub after_title: String,
    pub visual_diff_pct: f64,
    pub action_output: Option<String>,
    pub visible_targets: Vec<String>,
    pub after_base64_jpeg: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecoveryPlan {
    pub diagnosis: String,
    pub recovery_steps: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct RecoveryContext {
    pub goal: String,
    pub failed_action: String,
    pub observation: String,
    pub recovery_hint: Option<String>,
    pub active_app: String,
    pub window_title: String,
    pub recent_steps: Vec<String>,
    pub visible_targets: Vec<String>,
    pub base64_jpeg: String,
}

#[derive(Debug, Serialize)]
struct OllamaChatRequest {
    model: String,
    messages: Vec<OllamaChatMessage>,
    stream: bool,
    think: bool,
    keep_alive: String,
    format: serde_json::Value,
    options: OllamaChatOptions,
}

#[derive(Debug, Serialize)]
struct OllamaChatOptions {
    temperature: f32,
    num_predict: u32,
}

#[derive(Debug, Serialize, Deserialize)]
struct OllamaChatMessage {
    role: String,
    #[serde(default)]
    content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    thinking: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    images: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct OllamaChatResponse {
    message: OllamaChatMessage,
}

#[derive(Clone)]
pub struct OllamaAgentClient {
    http: Client,
}

impl OllamaAgentClient {
    pub fn new() -> Result<Self, AgentError> {
        let http = Client::builder()
            .timeout(AGENT_REQUEST_TIMEOUT)
            .connect_timeout(Duration::from_secs(8))
            .tcp_nodelay(true)
            .pool_idle_timeout(Duration::from_secs(90))
            .build()
            .map_err(|e| AgentError::Http(e.to_string()))?;
        Ok(Self { http })
    }

    #[allow(dead_code)]
    pub async fn create_task_plan(
        &self,
        base_url: &str,
        api_key: &str,
        model: &str,
        goal: &str,
        active_app: &str,
        ocr_lines: &[String],
        base64_jpeg: String,
    ) -> Result<TaskPlan, AgentError> {
        let ocr_block = format_lines_block(ocr_lines, "(no visible targets)");
        let system_prompt = "You are a macOS task orchestrator companion. \
Break the user's goal into 2 to 6 concise, concrete UI action steps tailored to their current screen. \
Respond strictly as a single valid JSON object with a \"steps\" array of short imperative strings (no markdown fences).";

        let user_prompt = format!(
            "Goal: {goal}\nActive app: {active_app}\nVisible targets:\n{ocr_block}"
        );

        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "steps": {
                    "type": "array",
                    "items": { "type": "string" }
                }
            },
            "required": ["steps"]
        });

        self.request_structured_json(
            base_url,
            api_key,
            model,
            system_prompt,
            user_prompt,
            Some(vec![base64_jpeg]),
            schema,
            0.2,
            512,
            parse_task_plan_response,
        )
        .await
    }

    pub async fn decide_next_action(
        &self,
        base_url: &str,
        api_key: &str,
        model: &str,
        ctx: NextActionContext,
    ) -> Result<PlannedActionDecision, AgentError> {
        let plan_block = if ctx.plan_steps.is_empty() {
            "(no plan)".to_string()
        } else {
            ctx.plan_steps
                .iter()
                .enumerate()
                .map(|(i, s)| format!("{}. {s}", i + 1))
                .collect::<Vec<_>>()
                .join("\n")
        };

        let recent_block = format_lines_block(&ctx.recent_steps, "(none yet)");
        let ocr_block = format_lines_block(&ctx.ocr_lines, "(no visible targets)");

        let system_prompt = "You are a fast, precise macOS computer-use companion. \
Screenshot overlays: Set-of-Mark boxes [N] (blue=Accessibility UI element, amber=OCR text) and 0.1-0.9 normalized grid. \
Visible targets lists [N] (Role) \"Label\" @(x=..., y=...) in normalized [0.0, 1.0] top-left coordinates.\n\
Available action_type values:\n\
- \"click\", \"double_click\", \"right_click\": set target_ocr_id matching an exact [N] ID from Visible targets, or x_norm and y_norm if no box covers it.\n\
- \"drag\": set from_x_norm, from_y_norm, to_x_norm, to_y_norm in [0.0, 1.0].\n\
- \"type_text\" / \"paste_text\": set text, optional replace=true (Cmd+A+Backspace first), optional submit=true (press Return). Prefer paste_text for multi-line or long text.\n\
- \"read_clipboard\": reads current macOS clipboard text.\n\
- \"press_key\": set key and modifiers ([\"cmd\"], [\"shift\"], [\"alt\"], [\"ctrl\"]).\n\
- \"scroll\": set scroll_delta_y (neg=down, pos=up), optional scroll_delta_x, optional target_ocr_id or x_norm/y_norm.\n\
- \"open_app\" / \"focus_app\": set app_name.\n\
- \"open_url\": set url (https/http) and optional app_name (e.g. \"Safari\" or \"Google Chrome\").\n\
- \"open_path\": set path.\n\
- \"wait\": set wait_ms (100..5000).\n\
- \"done\" / \"fail\": set summary.\n\
Rules:\n\
1. Always include required keys: narration, plan_step_index, action_type, requires_confirmation, plus only the fields needed for that action_type (omit unused fields).\n\
2. When Plan is (no plan), include plan_steps (2-5 short imperative steps).\n\
3. Optionally include follow_up_actions (0-3 steps) ONLY for deterministic sequences that do not need a fresh screenshot (e.g. click input -> paste_text with submit=true; open_app -> press_key cmd+n -> paste_text; open_url -> scroll). Never put done or fail in follow_up_actions.\n\
4. Use ONE open_url with app_name to open a site; never re-type the same URL afterward.\n\
5. narration: max 14 friendly words. Set requires_confirmation=true before deleting, sending, submitting forms, or purchasing.\n\
6. Respond strictly as ONE raw JSON object (no markdown fences).\n\
Example: {\"narration\":\"Opening Calculator\",\"plan_step_index\":0,\"action_type\":\"open_app\",\"app_name\":\"Calculator\",\"requires_confirmation\":false,\"plan_steps\":[\"Open Calculator\",\"Type 42*18\",\"Check result\"],\"follow_up_actions\":[{\"narration\":\"Compute 42*18\",\"action_type\":\"type_text\",\"text\":\"42*18\",\"submit\":true,\"requires_confirmation\":false}]}";

        let user_prompt = format!(
            "Goal: {}\nStep: {} / {}\nActive app: {}\nWindow title: {}\nScreen: {}x{}\nPlan:\n{}\nRecent steps:\n{}\nVisible targets:\n{}",
            ctx.goal,
            ctx.step_number,
            ctx.max_steps,
            ctx.active_app,
            ctx.window_title,
            ctx.screen_width,
            ctx.screen_height,
            plan_block,
            recent_block,
            ocr_block
        );

        let follow_up_action_enum = serde_json::json!([
            "click",
            "double_click",
            "right_click",
            "drag",
            "type_text",
            "paste_text",
            "read_clipboard",
            "press_key",
            "scroll",
            "open_app",
            "focus_app",
            "open_url",
            "open_path",
            "wait"
        ]);

        let follow_up_item_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "narration": { "type": "string" },
                "action_type": { "type": "string", "enum": follow_up_action_enum },
                "target_ocr_id": { "type": "integer" },
                "x_norm": { "type": "number" },
                "y_norm": { "type": "number" },
                "text": { "type": "string" },
                "replace": { "type": "boolean" },
                "submit": { "type": "boolean" },
                "key": { "type": "string" },
                "modifiers": { "type": "array", "items": { "type": "string" } },
                "scroll_delta_y": { "type": "integer" },
                "app_name": { "type": "string" },
                "url": { "type": "string" },
                "path": { "type": "string" },
                "wait_ms": { "type": "integer" },
                "requires_confirmation": { "type": "boolean" }
            },
            "required": ["narration", "action_type", "requires_confirmation"]
        });

        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "narration": { "type": "string" },
                "plan_step_index": { "type": "integer" },
                "action_type": {
                    "type": "string",
                    "enum": [
                        "click",
                        "double_click",
                        "right_click",
                        "drag",
                        "type_text",
                        "paste_text",
                        "read_clipboard",
                        "press_key",
                        "scroll",
                        "open_app",
                        "focus_app",
                        "open_url",
                        "open_path",
                        "wait",
                        "done",
                        "fail"
                    ]
                },
                "target_ocr_id": { "type": "integer" },
                "x_norm": { "type": "number" },
                "y_norm": { "type": "number" },
                "from_x_norm": { "type": "number" },
                "from_y_norm": { "type": "number" },
                "to_x_norm": { "type": "number" },
                "to_y_norm": { "type": "number" },
                "text": { "type": "string" },
                "replace": { "type": "boolean" },
                "submit": { "type": "boolean" },
                "key": { "type": "string" },
                "modifiers": { "type": "array", "items": { "type": "string" } },
                "scroll_delta_y": { "type": "integer" },
                "scroll_delta_x": { "type": "integer" },
                "app_name": { "type": "string" },
                "url": { "type": "string" },
                "path": { "type": "string" },
                "wait_ms": { "type": "integer" },
                "requires_confirmation": { "type": "boolean" },
                "summary": { "type": "string" },
                "plan_steps": {
                    "type": "array",
                    "items": { "type": "string" }
                },
                "follow_up_actions": {
                    "type": "array",
                    "items": follow_up_item_schema
                }
            },
            "required": ["narration", "plan_step_index", "action_type", "requires_confirmation"]
        });

        self.request_structured_json(
            base_url,
            api_key,
            model,
            system_prompt,
            user_prompt,
            Some(vec![ctx.base64_jpeg]),
            schema,
            0.15,
            1024,
            parse_action_decision_response,
        )
        .await
    }

    #[allow(dead_code)]
    pub async fn verify_step_outcome(
        &self,
        base_url: &str,
        api_key: &str,
        model: &str,
        ctx: VerifyStepContext,
    ) -> Result<StepVerification, AgentError> {
        let targets_block = format_lines_block(&ctx.visible_targets, "(no visible targets)");
        let output_line = ctx.action_output.unwrap_or_else(|| "None".to_string());

        let system_prompt = "You are a visual step-verification judge for a macOS desktop agent. \
Look at the AFTER screenshot, window change, visual diff percentage, and visible targets to verify whether the last action succeeded.\n\
Return status:\n\
- \"succeeded\": the intended effect clearly happened on screen (or clipboard/app/URL updated as intended).\n\
- \"partial\": the UI changed partly (e.g. page still loading or text partially entered) and needs another step.\n\
- \"failed\": nothing useful happened or the wrong thing happened (e.g. 404 page, garbled URL, modal error, focus missed).\n\
- \"needs_recovery\": stuck in a bad state requiring a different approach.\n\
Set goal_achieved=true ONLY if the user's overall goal is already completely satisfied on this screen.\n\
Respond strictly as ONE raw JSON object with keys status, observation, goal_achieved, and recovery_hint.";

        let user_prompt = format!(
            "Goal: {}\nStep #{} Action: {}\nExpected effect: {}\nApp: {} -> {}\nWindow title: \"{}\" -> \"{}\"\nVisual screen change: {:.1}%\nTool output: {}\nVisible targets AFTER action:\n{}",
            ctx.goal,
            ctx.step_number,
            ctx.action_description,
            ctx.expected_narration,
            ctx.before_app,
            ctx.after_app,
            ctx.before_title,
            ctx.after_title,
            ctx.visual_diff_pct,
            output_line,
            targets_block
        );

        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "status": {
                    "type": "string",
                    "enum": ["succeeded", "partial", "failed", "needs_recovery"]
                },
                "observation": { "type": "string" },
                "goal_achieved": { "type": "boolean" },
                "recovery_hint": { "type": ["string", "null"] }
            },
            "required": ["status", "observation", "goal_achieved"]
        });

        self.request_structured_json(
            base_url,
            api_key,
            model,
            system_prompt,
            user_prompt,
            Some(vec![ctx.after_base64_jpeg]),
            schema,
            0.1,
            512,
            parse_step_verification_response,
        )
        .await
    }

    pub async fn create_recovery_plan(
        &self,
        base_url: &str,
        api_key: &str,
        model: &str,
        ctx: RecoveryContext,
    ) -> Result<RecoveryPlan, AgentError> {
        let recent_block = format_lines_block(&ctx.recent_steps, "(none)");
        let targets_block = format_lines_block(&ctx.visible_targets, "(no visible targets)");
        let hint_str = ctx.recovery_hint.unwrap_or_else(|| "None".to_string());

        let system_prompt = "You are a self-healing macOS task recovery planner. \
A previous step failed or stalled. Diagnose why it failed from the current screenshot and propose 2 to 5 concrete alternative recovery_steps using better tools \
(for example: use open_url with app_name instead of typing in an address bar, use paste_text with replace=true to fix garbled text, use press_key cmd+l, or use focus_app).\n\
Respond strictly as ONE raw JSON object with keys diagnosis and recovery_steps.";

        let user_prompt = format!(
            "Goal: {}\nFailed action: {}\nVerification observation: {}\nRecovery hint: {}\nActive app: {}\nWindow title: \"{}\"\nRecent steps:\n{}\nVisible targets:\n{}",
            ctx.goal,
            ctx.failed_action,
            ctx.observation,
            hint_str,
            ctx.active_app,
            ctx.window_title,
            recent_block,
            targets_block
        );

        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "diagnosis": { "type": "string" },
                "recovery_steps": {
                    "type": "array",
                    "items": { "type": "string" }
                }
            },
            "required": ["diagnosis", "recovery_steps"]
        });

        self.request_structured_json(
            base_url,
            api_key,
            model,
            system_prompt,
            user_prompt,
            Some(vec![ctx.base64_jpeg]),
            schema,
            0.2,
            512,
            parse_recovery_plan_response,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn request_structured_json<T, F>(
        &self,
        base_url: &str,
        api_key: &str,
        model: &str,
        system_prompt: &str,
        user_prompt: String,
        images: Option<Vec<String>>,
        schema: serde_json::Value,
        base_temp: f32,
        num_predict: u32,
        parser: F,
    ) -> Result<T, AgentError>
    where
        F: Fn(&str) -> Result<T, AgentError>,
    {
        let effective_model = normalize_model_for_endpoint(base_url, model);
        let mut last_err: Option<AgentError> = None;

        for attempt in 0..3 {
            let format = if attempt == 0 {
                schema.clone()
            } else {
                serde_json::json!("json")
            };
            let temperature = if attempt == 0 {
                base_temp
            } else {
                (base_temp + 0.1).min(0.35)
            };
            let req = OllamaChatRequest {
                model: effective_model.clone(),
                messages: vec![
                    OllamaChatMessage {
                        role: "system".to_string(),
                        content: system_prompt.to_string(),
                        thinking: None,
                        images: None,
                    },
                    OllamaChatMessage {
                        role: "user".to_string(),
                        content: user_prompt.clone(),
                        thinking: None,
                        images: images.clone(),
                    },
                ],
                stream: false,
                think: false,
                keep_alive: "15m".to_string(),
                format,
                options: OllamaChatOptions {
                    temperature,
                    num_predict,
                },
            };

            match self.post_chat_once(base_url, api_key, &req).await {
                Ok(raw) => match parser(&raw) {
                    Ok(parsed) => return Ok(parsed),
                    Err(err) => {
                        last_err = Some(err);
                        if attempt < 2 {
                            tokio::time::sleep(Duration::from_millis(150)).await;
                            continue;
                        }
                    }
                },
                Err(err) => {
                    let retryable = is_retryable_error(&err);
                    last_err = Some(err);
                    if retryable && attempt < 2 {
                        tokio::time::sleep(Duration::from_millis(150)).await;
                        continue;
                    }
                    break;
                }
            }
        }

        Err(last_err.unwrap_or_else(|| AgentError::Http("unknown agent error".to_string())))
    }

    async fn post_chat_once(
        &self,
        base_url: &str,
        api_key: &str,
        req: &OllamaChatRequest,
    ) -> Result<String, AgentError> {
        let endpoint = format!("{}/api/chat", base_url.trim_end_matches('/'));
        let headers = build_headers(api_key)?;
        let resp = self
            .http
            .post(&endpoint)
            .headers(headers)
            .timeout(AGENT_REQUEST_TIMEOUT)
            .json(req)
            .send()
            .await
            .map_err(|e| AgentError::Http(e.to_string()))?;

        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| AgentError::Http(e.to_string()))?;
        if status.is_success() {
            return Ok(text);
        }
        Err(AgentError::UpstreamStatus {
            status: status.as_u16(),
            body: truncate_error_body(&text),
        })
    }
}

fn format_lines_block(lines: &[String], empty_fallback: &str) -> String {
    if lines.is_empty() {
        empty_fallback.to_string()
    } else {
        lines.join("\n")
    }
}

fn is_retryable_error(err: &AgentError) -> bool {
    match err {
        AgentError::Http(_) | AgentError::MalformedPayload(_) => true,
        AgentError::UpstreamStatus { status, .. } => *status >= 500 || *status == 429,
    }
}

pub fn parse_task_plan_response(raw_json: &str) -> Result<TaskPlan, AgentError> {
    let chat: OllamaChatResponse = serde_json::from_str(raw_json)
        .map_err(|err| AgentError::MalformedPayload(err.to_string()))?;
    let parsed: TaskPlan = parse_json_from_message(&chat)?;

    let steps: Vec<String> = parsed
        .steps
        .into_iter()
        .map(|s| sanitize_line(&s, 140))
        .filter(|s| !s.is_empty())
        .take(6)
        .collect();

    if steps.is_empty() {
        return Err(AgentError::MalformedPayload(
            "task plan returned zero steps".to_string(),
        ));
    }

    Ok(TaskPlan { steps })
}

fn is_supported_follow_up_action(action_type: &str) -> bool {
    matches!(
        action_type,
        "click"
            | "double_click"
            | "right_click"
            | "drag"
            | "type_text"
            | "paste_text"
            | "read_clipboard"
            | "press_key"
            | "scroll"
            | "open_app"
            | "focus_app"
            | "open_url"
            | "open_path"
            | "wait"
    )
}

pub fn parse_action_decision_response(raw_json: &str) -> Result<PlannedActionDecision, AgentError> {
    let chat: OllamaChatResponse = serde_json::from_str(raw_json)
        .map_err(|err| AgentError::MalformedPayload(err.to_string()))?;
    let mut decision: PlannedActionDecision = parse_json_from_message(&chat)?;

    decision.action_type = decision.action_type.trim().to_ascii_lowercase();
    let valid_action = is_supported_follow_up_action(decision.action_type.as_str())
        || matches!(decision.action_type.as_str(), "done" | "fail");
    if !valid_action {
        return Err(AgentError::MalformedPayload(format!(
            "unsupported action_type \"{}\"",
            decision.action_type
        )));
    }

    decision.narration = sanitize_line(&decision.narration, 180);
    if decision.narration.is_empty() {
        decision.narration = format!("Performing {}", decision.action_type);
    }
    decision.thought = sanitize_line(&decision.thought, 240);
    decision.target_ocr_id = decision.target_ocr_id.filter(|id| *id >= 1);
    decision.x_norm = decision.x_norm.map(|v| v.clamp(0.0, 1.0));
    decision.y_norm = decision.y_norm.map(|v| v.clamp(0.0, 1.0));
    decision.from_x_norm = decision.from_x_norm.map(|v| v.clamp(0.0, 1.0));
    decision.from_y_norm = decision.from_y_norm.map(|v| v.clamp(0.0, 1.0));
    decision.to_x_norm = decision.to_x_norm.map(|v| v.clamp(0.0, 1.0));
    decision.to_y_norm = decision.to_y_norm.map(|v| v.clamp(0.0, 1.0));
    decision.summary = decision
        .summary
        .map(|s| sanitize_line(&s, 280))
        .filter(|s| !s.is_empty());

    decision.plan_steps = decision
        .plan_steps
        .into_iter()
        .map(|s| sanitize_line(&s, 140))
        .filter(|s| !s.is_empty())
        .take(6)
        .collect();

    decision.follow_up_actions = decision
        .follow_up_actions
        .into_iter()
        .filter_map(|mut act| {
            act.action_type = act.action_type.trim().to_ascii_lowercase();
            if !is_supported_follow_up_action(act.action_type.as_str()) {
                return None;
            }
            act.narration = sanitize_line(&act.narration, 180);
            if act.narration.is_empty() {
                act.narration = format!("Performing {}", act.action_type);
            }
            act.target_ocr_id = act.target_ocr_id.filter(|id| *id >= 1);
            act.x_norm = act.x_norm.map(|v| v.clamp(0.0, 1.0));
            act.y_norm = act.y_norm.map(|v| v.clamp(0.0, 1.0));
            act.from_x_norm = act.from_x_norm.map(|v| v.clamp(0.0, 1.0));
            act.from_y_norm = act.from_y_norm.map(|v| v.clamp(0.0, 1.0));
            act.to_x_norm = act.to_x_norm.map(|v| v.clamp(0.0, 1.0));
            act.to_y_norm = act.to_y_norm.map(|v| v.clamp(0.0, 1.0));
            Some(act)
        })
        .take(3)
        .collect();

    Ok(decision)
}

pub fn parse_step_verification_response(raw_json: &str) -> Result<StepVerification, AgentError> {
    let chat: OllamaChatResponse = serde_json::from_str(raw_json)
        .map_err(|err| AgentError::MalformedPayload(err.to_string()))?;
    let mut ver: StepVerification = parse_json_from_message(&chat)?;
    ver.status = ver.status.trim().to_ascii_lowercase();
    if !matches!(
        ver.status.as_str(),
        "succeeded" | "partial" | "failed" | "needs_recovery"
    ) {
        return Err(AgentError::MalformedPayload(format!(
            "unsupported verification status \"{}\"",
            ver.status
        )));
    }
    ver.observation = sanitize_line(&ver.observation, 240);
    if ver.observation.is_empty() {
        ver.observation = format!("Verification status: {}", ver.status);
    }
    ver.recovery_hint = ver
        .recovery_hint
        .map(|s| sanitize_line(&s, 220))
        .filter(|s| !s.is_empty());
    Ok(ver)
}

pub fn parse_recovery_plan_response(raw_json: &str) -> Result<RecoveryPlan, AgentError> {
    let chat: OllamaChatResponse = serde_json::from_str(raw_json)
        .map_err(|err| AgentError::MalformedPayload(err.to_string()))?;
    let mut plan: RecoveryPlan = parse_json_from_message(&chat)?;
    plan.diagnosis = sanitize_line(&plan.diagnosis, 220);
    if plan.diagnosis.is_empty() {
        plan.diagnosis = "Previous approach stalled; trying alternative steps.".to_string();
    }
    plan.recovery_steps = plan
        .recovery_steps
        .into_iter()
        .map(|s| sanitize_line(&s, 140))
        .filter(|s| !s.is_empty())
        .take(5)
        .collect();
    if plan.recovery_steps.is_empty() {
        return Err(AgentError::MalformedPayload(
            "recovery plan returned zero steps".to_string(),
        ));
    }
    Ok(plan)
}

fn parse_json_from_message<T: DeserializeOwned>(
    chat: &OllamaChatResponse,
) -> Result<T, AgentError> {
    let cleaned_content = strip_think_tags(chat.message.content.trim());
    if !cleaned_content.is_empty() {
        if let Ok(val) = serde_json::from_str::<T>(&cleaned_content) {
            return Ok(val);
        }
        for candidate in extract_balanced_json_candidates(&cleaned_content)
            .into_iter()
            .rev()
        {
            if let Ok(val) = serde_json::from_str::<T>(candidate) {
                return Ok(val);
            }
        }
    }

    if let Some(thinking) = chat.message.thinking.as_deref() {
        let trimmed_thinking = thinking.trim();
        if !trimmed_thinking.is_empty() {
            for candidate in extract_balanced_json_candidates(trimmed_thinking)
                .into_iter()
                .rev()
            {
                if let Ok(val) = serde_json::from_str::<T>(candidate) {
                    return Ok(val);
                }
            }
        }
    }

    let preview_source = if !cleaned_content.is_empty() {
        cleaned_content
    } else {
        chat.message
            .thinking
            .as_deref()
            .unwrap_or("")
            .trim()
            .to_string()
    };

    let detail = if preview_source.is_empty() {
        "empty assistant content".to_string()
    } else {
        format!("unparseable assistant output: {}", sanitize_line(&preview_source, 260))
    };

    Err(AgentError::MalformedPayload(detail))
}

fn strip_think_tags(raw: &str) -> String {
    let mut result = raw.to_string();
    loop {
        let Some(start) = result.find("<think>") else {
            break;
        };
        let Some(rel_end) = result[start..].find("</think>") else {
            break;
        };
        let end = start + rel_end + "</think>".len();
        result.replace_range(start..end, "");
    }
    result.trim().to_string()
}

fn extract_balanced_json_candidates(text: &str) -> Vec<&str> {
    let mut candidates = Vec::new();
    let mut depth: usize = 0;
    let mut in_string = false;
    let mut escaped = false;
    let mut start_idx: Option<usize> = None;

    for (idx, ch) in text.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => {
                if depth > 0 {
                    in_string = true;
                }
            }
            '{' => {
                if depth == 0 {
                    start_idx = Some(idx);
                }
                depth += 1;
            }
            '}' => {
                if depth > 0 {
                    depth -= 1;
                    if depth == 0 {
                        if let Some(s) = start_idx.take() {
                            candidates.push(&text[s..=idx]);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    candidates
}

fn sanitize_line(raw: &str, max_chars: usize) -> String {
    raw.chars()
        .filter(|c| !c.is_control())
        .take(max_chars)
        .collect::<String>()
        .trim()
        .to_string()
}

fn normalize_model_for_endpoint(base_url: &str, model: &str) -> String {
    let is_cloud_direct = base_url.starts_with("https://ollama.com");
    if is_cloud_direct {
        model.trim_end_matches(":cloud").to_string()
    } else {
        model.to_string()
    }
}

fn build_headers(api_key: &str) -> Result<HeaderMap, AgentError> {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    let trimmed = api_key.trim();
    if !trimmed.is_empty() {
        let bearer = format!("Bearer {trimmed}");
        let header_val = HeaderValue::from_str(&bearer)
            .map_err(|_| AgentError::Http("invalid API key header characters".to_string()))?;
        headers.insert(AUTHORIZATION, header_val);
    }
    Ok(headers)
}

fn truncate_error_body(body: &str) -> String {
    body.chars()
        .filter(|c| !c.is_control())
        .take(240)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_task_plan_and_sanitizes_steps() {
        let raw = r#"{
            "model": "deepseek-v4.1-flash",
            "message": {
                "role": "assistant",
                "content": "{\"steps\":[\"Open Notes app\",\"Create a new note\",\"Type shopping list\",\"\",\"Step 4\",\"Step 5\",\"Step 6\",\"Step 7\"]}"
            },
            "done": true
        }"#;
        let plan = parse_task_plan_response(raw).expect("valid plan");
        assert_eq!(plan.steps.len(), 6);
        assert_eq!(plan.steps[0], "Open Notes app");
    }

    #[test]
    fn parses_valid_action_decision_and_clamps_coordinates() {
        let raw = r#"{
            "model": "deepseek-v4.1-flash",
            "message": {
                "role": "assistant",
                "content": "{\"thought\":\"Click compose button\",\"narration\":\"Opening a fresh note for you!\",\"plan_step_index\":1,\"action_type\":\"CLICK\",\"target_ocr_id\":4,\"x_norm\":1.4,\"y_norm\":-0.2,\"requires_confirmation\":false}"
            },
            "done": true
        }"#;
        let decision = parse_action_decision_response(raw).expect("valid decision");
        assert_eq!(decision.action_type, "click");
        assert_eq!(decision.target_ocr_id, Some(4));
        assert_eq!(decision.x_norm, Some(1.0));
        assert_eq!(decision.y_norm, Some(0.0));
        assert_eq!(decision.narration, "Opening a fresh note for you!");
        assert!(!decision.replace);

        let replace_raw = r#"{
            "message": {
                "role": "assistant",
                "content": "{\"narration\":\"Replacing URL\",\"plan_step_index\":1,\"action_type\":\"paste_text\",\"text\":\"https://github.com/trending\",\"replace\":true,\"submit\":true,\"requires_confirmation\":false}"
            }
        }"#;
        let replace_dec = parse_action_decision_response(replace_raw).expect("replace decision");
        assert_eq!(replace_dec.action_type, "paste_text");
        assert!(replace_dec.replace);
        assert!(replace_dec.submit);
        assert_eq!(replace_dec.text.as_deref(), Some("https://github.com/trending"));
    }

    #[test]
    fn parses_step_verification_and_recovery_plan() {
        let ver_raw = r#"{
            "message": {
                "role": "assistant",
                "content": "{\"status\":\"needs_recovery\",\"observation\":\"Safari shows 404 due to duplicated URL text.\",\"goal_achieved\":false,\"recovery_hint\":\"Use open_url with Safari directly\"}"
            }
        }"#;
        let ver = parse_step_verification_response(ver_raw).expect("valid verification");
        assert_eq!(ver.status, "needs_recovery");
        assert!(!ver.goal_achieved);
        assert_eq!(ver.recovery_hint.as_deref(), Some("Use open_url with Safari directly"));

        let rec_raw = r#"{
            "message": {
                "role": "assistant",
                "content": "{\"diagnosis\":\"Address bar had leftover text\",\"recovery_steps\":[\"Open https://github.com/trending directly in Safari\",\"Scroll down to view trending repos\"]}"
            }
        }"#;
        let rec = parse_recovery_plan_response(rec_raw).expect("valid recovery plan");
        assert_eq!(rec.diagnosis, "Address bar had leftover text");
        assert_eq!(rec.recovery_steps.len(), 2);
    }

    #[test]
    fn recovers_json_from_thinking_field_and_strips_think_tags() {
        let from_thinking = r#"{
            "message": {
                "role": "assistant",
                "content": "",
                "thinking": "Let me inspect {note} first. Then here is the JSON: {\"narration\":\"Clicking search bar\",\"plan_step_index\":0,\"action_type\":\"click\",\"target_ocr_id\":2,\"requires_confirmation\":false}"
            }
        }"#;
        let recovered = parse_action_decision_response(from_thinking).expect("recover from thinking");
        assert_eq!(recovered.action_type, "click");
        assert_eq!(recovered.target_ocr_id, Some(2));

        let with_think_block = r#"{
            "message": {
                "role": "assistant",
                "content": "<think>Considering {braces} inside thinking</think>\n```json\n{\"steps\":[\"Open Safari\",\"Navigate to trending\"]}\n```"
            }
        }"#;
        let plan = parse_task_plan_response(with_think_block).expect("strip think block");
        assert_eq!(plan.steps, vec!["Open Safari", "Navigate to trending"]);
    }

    #[test]
    fn rejects_malformed_or_unsupported_action_responses() {
        let bad_action = r#"{
            "message": {
                "role": "assistant",
                "content": "{\"narration\":\"hi\",\"plan_step_index\":0,\"action_type\":\"run_shell\",\"requires_confirmation\":false}"
            }
        }"#;
        assert!(matches!(
            parse_action_decision_response(bad_action),
            Err(AgentError::MalformedPayload(_))
        ));

        let empty_content = r#"{"message":{"role":"assistant","content":""}}"#;
        assert!(matches!(
            parse_action_decision_response(empty_content),
            Err(AgentError::MalformedPayload(_))
        ));
    }

    #[test]
    fn parses_plan_steps_and_follow_up_actions_with_caps_and_filtering() {
        let raw = r#"{
            "message": {
                "role": "assistant",
                "content": "{\"narration\":\"Opening Notes\",\"plan_step_index\":0,\"action_type\":\"open_app\",\"app_name\":\"Notes\",\"requires_confirmation\":false,\"plan_steps\":[\"Open Notes\",\"Create new note\",\"\",\"Paste checklist\"],\"follow_up_actions\":[{\"narration\":\"New note\",\"action_type\":\"press_key\",\"key\":\"n\",\"modifiers\":[\"cmd\"],\"requires_confirmation\":false},{\"narration\":\"Bad terminal\",\"action_type\":\"done\",\"requires_confirmation\":false},{\"narration\":\"Click area\",\"action_type\":\"click\",\"x_norm\":1.5,\"y_norm\":-0.2,\"target_ocr_id\":0,\"requires_confirmation\":false},{\"narration\":\"Paste\",\"action_type\":\"paste_text\",\"text\":\"- Task\",\"submit\":true,\"requires_confirmation\":false},{\"narration\":\"Extra\",\"action_type\":\"scroll\",\"scroll_delta_y\":-3,\"requires_confirmation\":false}]}"
            }
        }"#;

        let dec = parse_action_decision_response(raw).expect("valid batched decision");
        assert_eq!(dec.plan_steps, vec!["Open Notes", "Create new note", "Paste checklist"]);
        assert_eq!(dec.follow_up_actions.len(), 3);
        assert_eq!(dec.follow_up_actions[0].action_type, "press_key");
        assert_eq!(dec.follow_up_actions[1].action_type, "click");
        assert_eq!(dec.follow_up_actions[1].x_norm, Some(1.0));
        assert_eq!(dec.follow_up_actions[1].y_norm, Some(0.0));
        assert_eq!(dec.follow_up_actions[1].target_ocr_id, None);
        assert_eq!(dec.follow_up_actions[2].action_type, "paste_text");

        let converted = dec.follow_up_actions[2].clone().into_planned_decision(1);
        assert_eq!(converted.plan_step_index, 1);
        assert_eq!(converted.action_type, "paste_text");
        assert!(converted.submit);
    }

    #[test]
    fn handles_explicit_null_fields_gracefully() {
        let raw = r#"{
            "message": {
                "role": "assistant",
                "content": "{\"narration\":null,\"plan_step_index\":null,\"action_type\":\"open_app\",\"app_name\":\"Calculator\",\"modifiers\":null,\"replace\":null,\"submit\":null,\"requires_confirmation\":null,\"plan_steps\":null,\"follow_up_actions\":null}"
            }
        }"#;

        let dec = parse_action_decision_response(raw).expect("null fields should default cleanly");
        assert_eq!(dec.action_type, "open_app");
        assert_eq!(dec.app_name.as_deref(), Some("Calculator"));
        assert_eq!(dec.narration, "Performing open_app");
        assert_eq!(dec.plan_step_index, 0);
        assert!(dec.modifiers.is_empty());
        assert!(!dec.replace);
        assert!(!dec.submit);
        assert!(!dec.requires_confirmation);
        assert!(dec.plan_steps.is_empty());
        assert!(dec.follow_up_actions.is_empty());
    }
}
