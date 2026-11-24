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
    pub replace: bool,
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
    think: bool,
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
            "(no visible targets)".to_string()
        } else {
            ocr_lines.join("\n")
        };

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

        let mut last_err: Option<AgentError> = None;
        for attempt in 0..3 {
            let format = if attempt == 0 {
                schema.clone()
            } else {
                serde_json::json!("json")
            };
            let temperature = if attempt == 0 { 0.2 } else { 0.25 };
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
                        images: Some(vec![base64_jpeg.clone()]),
                    },
                ],
                stream: false,
                think: false,
                format,
                options: OllamaChatOptions { temperature },
            };

            match self.post_chat_once(base_url, api_key, &req).await {
                Ok(raw) => match parse_task_plan_response(&raw) {
                    Ok(plan) => return Ok(plan),
                    Err(err) => {
                        last_err = Some(err);
                        if attempt < 2 {
                            tokio::time::sleep(Duration::from_millis(350)).await;
                            continue;
                        }
                    }
                },
                Err(err) => {
                    let retryable = is_retryable_error(&err);
                    last_err = Some(err);
                    if retryable && attempt < 2 {
                        tokio::time::sleep(Duration::from_millis(350)).await;
                        continue;
                    }
                    break;
                }
            }
        }

        Err(last_err.unwrap_or_else(|| AgentError::Http("unknown plan error".to_string())))
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
            "(no visible targets)".to_string()
        } else {
            ctx.ocr_lines.join("\n")
        };

        let system_prompt = "You are a precise, warm, playful macOS computer-use companion (like Bluey). \
The screenshot has Set-of-Mark numbered boxes ([1], [2], ...) and a subtle 0.1-0.9 normalized coordinate grid overlaid on it. \
Blue boxes are clickable macOS Accessibility UI elements (buttons, inputs, links, tabs); amber boxes are OCR text regions. \
Each line in Visible targets lists [N] (Role) \"Label\" @(x=..., y=...) in normalized [0.0, 1.0] top-left coordinates. \
Inspect the screenshot, Visible targets, plan, and recent steps, then choose the SINGLE best next action.\n\
Available action_type values:\n\
- \"click\", \"double_click\", \"right_click\": STRONGLY prefer target_ocr_id matching an exact [N] ID from Visible targets or the image overlay. Only use x_norm and y_norm (reading directly from the 0.1-0.9 grid) if no numbered box covers the exact element. Never invent a target_ocr_id that is not in Visible targets.\n\
- \"type_text\": set text (types into currently focused input), and set replace=true whenever replacing existing or garbled text in an address bar, search box, or input field (automatically runs Cmd+A + Backspace before typing).\n\
- \"press_key\": set key (e.g. \"enter\", \"tab\", \"escape\", \"space\", \"backspace\", \"forward_delete\", \"a\"-\"z\", \"up\", \"down\") and modifiers ([\"cmd\"], [\"shift\"], [\"alt\"], [\"ctrl\"]).\n\
- \"scroll\": set scroll_delta_y (negative scrolls down, positive scrolls up) and optional target_ocr_id or x_norm/y_norm.\n\
- \"open_app\": set app_name directly (prefer this over Spotlight when launching an app).\n\
- \"open_url\": set url directly (https:// or http://, prefer this over clicking browser bars when navigating to a URL).\n\
- \"wait\": set wait_ms (100..5000) if waiting for a page or app to load.\n\
- \"done\": set summary once the goal is visibly completed on screen.\n\
- \"fail\": set summary explaining why the task cannot proceed.\n\
Rules:\n\
1. narration must be ONE friendly, concise line (max 16 words) telling the user what you are doing right now.\n\
2. Set requires_confirmation=true before deleting, sending messages/emails, submitting forms, purchasing, or making irreversible changes.\n\
3. Never click an already-focused address bar expecting it to clear text—use open_url directly, or press_key cmd+l, or type_text with replace=true.\n\
4. Do not repeat an identical failed or ineffective step; adapt if the UI did not change.\n\
5. Respond strictly as ONE raw JSON object with keys narration, plan_step_index, action_type, requires_confirmation, and relevant action fields (no markdown fences).";

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
                "replace": { "type": "boolean" },
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

        let mut last_err: Option<AgentError> = None;
        for attempt in 0..3 {
            let format = if attempt == 0 {
                schema.clone()
            } else {
                serde_json::json!("json")
            };
            let temperature = if attempt == 0 { 0.15 } else { 0.25 };
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
                        images: Some(vec![ctx.base64_jpeg.clone()]),
                    },
                ],
                stream: false,
                think: false,
                format,
                options: OllamaChatOptions { temperature },
            };

            match self.post_chat_once(base_url, api_key, &req).await {
                Ok(raw) => match parse_action_decision_response(&raw) {
                    Ok(decision) => return Ok(decision),
                    Err(err) => {
                        last_err = Some(err);
                        if attempt < 2 {
                            tokio::time::sleep(Duration::from_millis(350)).await;
                            continue;
                        }
                    }
                },
                Err(err) => {
                    let retryable = is_retryable_error(&err);
                    last_err = Some(err);
                    if retryable && attempt < 2 {
                        tokio::time::sleep(Duration::from_millis(350)).await;
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

pub fn parse_action_decision_response(raw_json: &str) -> Result<PlannedActionDecision, AgentError> {
    let chat: OllamaChatResponse = serde_json::from_str(raw_json)
        .map_err(|err| AgentError::MalformedPayload(err.to_string()))?;
    let mut decision: PlannedActionDecision = parse_json_from_message(&chat)?;

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
        format!("unparseable assistant output: {}", sanitize_line(&preview_source, 120))
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
                "content": "{\"narration\":\"Replacing URL\",\"plan_step_index\":1,\"action_type\":\"type_text\",\"text\":\"https://github.com/trending\",\"replace\":true,\"requires_confirmation\":false}"
            }
        }"#;
        let replace_dec = parse_action_decision_response(replace_raw).expect("replace decision");
        assert!(replace_dec.replace);
        assert_eq!(replace_dec.text.as_deref(), Some("https://github.com/trending"));
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
}
