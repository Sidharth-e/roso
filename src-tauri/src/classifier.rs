use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use reqwest::Client;
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlannedActionDecision {
    #[serde(default)]
    pub thought: String,
    pub narration: String,
    #[serde(default)]
    pub plan_step_index: u32,
    pub action_type: String,
    #[serde(default)]
    pub target_ocr_id: Option<u32>,
    #[serde(default)]
    pub x_norm: Option<f64>,
    #[serde(default)]
    pub y_norm: Option<f64>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub modifiers: Vec<String>,
    #[serde(default)]
    pub scroll_delta_y: Option<i32>,
    #[serde(default)]
    pub app_name: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub wait_ms: Option<u64>,
    #[serde(default)]
    pub requires_confirmation: bool,
    #[serde(default)]
    pub summary: Option<String>,
}

#[derive(Debug, Serialize)]
struct OllamaChatRequest {
    model: String,
    messages: Vec<OllamaChatMessage>,
    stream: bool,
    format: serde_json::Value,
    options: OllamaChatOptions,
}

#[derive(Debug, Serialize)]
struct OllamaChatOptions {
    temperature: f32,
}

#[derive(Debug, Serialize, Deserialize)]
struct OllamaChatMessage {
    role: String,
    content: String,
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
            .build()
            .map_err(|e| AgentError::Http(e.to_string()))?;
        Ok(Self { http })
    }

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
        let effective_model = normalize_model_for_endpoint(base_url, model);
        let ocr_block = if ocr_lines.is_empty() {
            "(no visible OCR lines)".to_string()
        } else {
            ocr_lines.join("\n")
        };

        let system_prompt = "You are a macOS task orchestrator companion. \
Break the user's goal into 2 to 6 concise, concrete UI action steps tailored to their current screen. \
Respond strictly as JSON with a \"steps\" array of short imperative strings.";

        let user_prompt = format!(
            "Goal: {goal}\nActive app: {active_app}\nVisible OCR lines:\n{ocr_block}"
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

        let req = OllamaChatRequest {
            model: effective_model,
            messages: vec![
                OllamaChatMessage {
                    role: "system".to_string(),
                    content: system_prompt.to_string(),
                    images: None,
                },
                OllamaChatMessage {
                    role: "user".to_string(),
                    content: user_prompt,
                    images: Some(vec![base64_jpeg]),
                },
            ],
            stream: false,
            format: schema,
            options: OllamaChatOptions { temperature: 0.2 },
        };

        let raw = self.post_chat_with_retry(base_url, api_key, &req).await?;
        parse_task_plan_response(&raw)
    }

    pub async fn decide_next_action(
        &self,
        base_url: &str,
        api_key: &str,
        model: &str,
        ctx: NextActionContext,
    ) -> Result<PlannedActionDecision, AgentError> {
        let effective_model = normalize_model_for_endpoint(base_url, model);

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

        let recent_block = if ctx.recent_steps.is_empty() {
            "(none yet)".to_string()
        } else {
            ctx.recent_steps.join("\n")
        };

        let ocr_block = if ctx.ocr_lines.is_empty() {
            "(no visible OCR lines)".to_string()
        } else {
            ctx.ocr_lines.join("\n")
        };

        let system_prompt = "You are a precise, warm, playful macOS computer-use companion (like Bluey). \
Look at the screenshot, OCR lines, plan, and recent steps, then choose the SINGLE best next action to advance the user's goal.\n\
Available action_type values:\n\
- \"click\", \"double_click\", \"right_click\": specify target_ocr_id matching [N] from OCR lines whenever text matches your target; otherwise provide normalized x_norm and y_norm in [0.0, 1.0] from top-left.\n\
- \"type_text\": set text (types into currently focused input).\n\
- \"press_key\": set key (e.g. \"enter\", \"tab\", \"escape\", \"space\", \"a\"-\"z\", \"up\", \"down\") and modifiers ([\"cmd\"], [\"shift\"], [\"alt\"], [\"ctrl\"]).\n\
- \"scroll\": set scroll_delta_y (negative scrolls down, positive scrolls up) and optional target_ocr_id or x_norm/y_norm.\n\
- \"open_app\": set app_name directly (prefer this over Spotlight when launching an app).\n\
- \"open_url\": set url directly (https:// or http://, prefer this over clicking browser bars when opening a known site).\n\
- \"wait\": set wait_ms (100..5000) if waiting for a page or app to load.\n\
- \"done\": set summary once the goal is visibly completed on screen.\n\
- \"fail\": set summary explaining why the task cannot proceed.\n\
Rules:\n\
1. narration must be ONE friendly, concise line (max 16 words) telling the user what you are doing right now.\n\
2. Set requires_confirmation=true before deleting, sending messages/emails, submitting forms, purchasing, or making irreversible changes.\n\
3. Do not repeat an identical failed or ineffective step; adapt if the UI did not change.\n\
4. Respond strictly as JSON matching the schema.";

        let user_prompt = format!(
            "Goal: {}\nStep: {} / {}\nActive app: {}\nWindow title: {}\nScreen: {}x{}\nPlan:\n{}\nRecent steps:\n{}\nVisible OCR lines:\n{}",
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

        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "thought": { "type": "string" },
                "narration": { "type": "string" },
                "plan_step_index": { "type": "integer" },
                "action_type": {
                    "type": "string",
                    "enum": ["click", "double_click", "right_click", "type_text", "press_key", "scroll", "open_app", "open_url", "wait", "done", "fail"]
                },
                "target_ocr_id": { "type": ["integer", "null"] },
                "x_norm": { "type": ["number", "null"] },
                "y_norm": { "type": ["number", "null"] },
                "text": { "type": ["string", "null"] },
                "key": { "type": ["string", "null"] },
                "modifiers": { "type": "array", "items": { "type": "string" } },
                "scroll_delta_y": { "type": ["integer", "null"] },
                "app_name": { "type": ["string", "null"] },
                "url": { "type": ["string", "null"] },
                "wait_ms": { "type": ["integer", "null"] },
                "requires_confirmation": { "type": "boolean" },
                "summary": { "type": ["string", "null"] }
            },
            "required": ["narration", "plan_step_index", "action_type", "requires_confirmation"]
        });

        let req = OllamaChatRequest {
            model: effective_model,
            messages: vec![
                OllamaChatMessage {
                    role: "system".to_string(),
                    content: system_prompt.to_string(),
                    images: None,
                },
                OllamaChatMessage {
                    role: "user".to_string(),
                    content: user_prompt,
                    images: Some(vec![ctx.base64_jpeg]),
                },
            ],
            stream: false,
            format: schema,
            options: OllamaChatOptions { temperature: 0.15 },
        };

        let raw = self.post_chat_with_retry(base_url, api_key, &req).await?;
        parse_action_decision_response(&raw)
    }

    async fn post_chat_with_retry(
        &self,
        base_url: &str,
        api_key: &str,
        req: &OllamaChatRequest,
    ) -> Result<String, AgentError> {
        let endpoint = format!("{}/api/chat", base_url.trim_end_matches('/'));
        let mut last_err: Option<AgentError> = None;

        for attempt in 0..2 {
            let headers = build_headers(api_key)?;
            match self
                .http
                .post(&endpoint)
                .headers(headers)
                .timeout(AGENT_REQUEST_TIMEOUT)
                .json(req)
                .send()
                .await
            {
                Ok(resp) => {
                    let status = resp.status();
                    let text = resp
                        .text()
                        .await
                        .map_err(|e| AgentError::Http(e.to_string()))?;
                    if status.is_success() {
                        return Ok(text);
                    }
                    let err = AgentError::UpstreamStatus {
                        status: status.as_u16(),
                        body: truncate_error_body(&text),
                    };
                    if status.is_server_error() && attempt == 0 {
                        last_err = Some(err);
                        tokio::time::sleep(Duration::from_millis(350)).await;
                        continue;
                    }
                    return Err(err);
                }
                Err(err) => {
                    let mapped = AgentError::Http(err.to_string());
                    if attempt == 0 {
                        last_err = Some(mapped);
                        tokio::time::sleep(Duration::from_millis(350)).await;
                        continue;
                    }
                    return Err(mapped);
                }
            }
        }

        Err(last_err.unwrap_or_else(|| AgentError::Http("unknown agent error".to_string())))
    }
}

pub fn parse_task_plan_response(raw_json: &str) -> Result<TaskPlan, AgentError> {
    let chat: OllamaChatResponse = serde_json::from_str(raw_json)
        .map_err(|err| AgentError::MalformedPayload(err.to_string()))?;
    let content = extract_json_object(chat.message.content.trim());
    let parsed: TaskPlan = serde_json::from_str(content)
        .map_err(|err| AgentError::MalformedPayload(err.to_string()))?;

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

pub fn parse_action_decision_response(raw_json: &str) -> Result<PlannedActionDecision, AgentError> {
    let chat: OllamaChatResponse = serde_json::from_str(raw_json)
        .map_err(|err| AgentError::MalformedPayload(err.to_string()))?;
    let content = extract_json_object(chat.message.content.trim());
    let mut decision: PlannedActionDecision = serde_json::from_str(content)
        .map_err(|err| AgentError::MalformedPayload(err.to_string()))?;

    decision.action_type = decision.action_type.trim().to_ascii_lowercase();
    let valid_action = matches!(
        decision.action_type.as_str(),
        "click"
            | "double_click"
            | "right_click"
            | "type_text"
            | "press_key"
            | "scroll"
            | "open_app"
            | "open_url"
            | "wait"
            | "done"
            | "fail"
    );
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
    decision.summary = decision
        .summary
        .map(|s| sanitize_line(&s, 280))
        .filter(|s| !s.is_empty());

    Ok(decision)
}

fn extract_json_object(raw: &str) -> &str {
    let trimmed = raw.trim();
    if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}')) {
        if start <= end {
            return &trimmed[start..=end];
        }
    }
    trimmed
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
            "model": "qwen3-vl:235b",
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
            "model": "qwen3-vl:235b",
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

        let bad_json = r#"{"message":{"role":"assistant","content":"not json"}}"#;
        assert!(matches!(
            parse_task_plan_response(bad_json),
            Err(AgentError::MalformedPayload(_))
        ));
    }
}
