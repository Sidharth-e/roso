use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use thiserror::Error;

pub const MAX_CLASSIFICATION_LATENCY: Duration = Duration::from_secs(20);
const NUDGE_REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Error)]
pub enum ClassifierError {
    #[error("network request failed: {0}")]
    Http(String),
    #[error("request exceeded maximum latency of {0}ms")]
    StaleLatency(u128),
    #[error("API returned status {status}: {body}")]
    UpstreamStatus { status: u16, body: String },
    #[error("invalid API response payload: {0}")]
    MalformedPayload(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityChoice {
    OnTask,
    OffTask,
    Uncertain,
}

impl ActivityChoice {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OnTask => "on_task",
            Self::OffTask => "off_task",
            Self::Uncertain => "uncertain",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemOneState {
    pub declared_task: String,
    pub active_app: String,
    pub elapsed_minutes: u32,
    pub ocr_lines: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SystemOneQuestion {
    #[serde(rename = "type")]
    pub question_type: &'static str,
    pub instructions: &'static str,
    pub criteria: SystemOneCriteria,
}

#[derive(Debug, Clone, Serialize)]
pub struct SystemOneCriteria {
    pub on_task: &'static str,
    pub off_task: &'static str,
    pub uncertain: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct SystemOneRequest {
    pub model: String,
    pub state: SystemOneState,
    pub images: Vec<String>,
    pub questions: HashMap<&'static str, SystemOneQuestion>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SystemOneResponse {
    pub answers: SystemOneAnswers,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SystemOneAnswers {
    pub activity: SystemOneActivityAnswer,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SystemOneActivityAnswer {
    pub choice: ActivityChoice,
    pub probabilities: ActivityProbabilities,
    pub confidence: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ActivityProbabilities {
    #[serde(default)]
    pub on_task: f64,
    pub off_task: f64,
    #[serde(default)]
    pub uncertain: f64,
}

#[derive(Debug, Clone)]
pub struct ClassificationOutcome {
    pub choice: ActivityChoice,
    pub prob_off_task: f64,
    pub confidence: f64,
    pub latency: Duration,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NudgeDirective {
    pub nudge_text: String,
    pub target_ocr_id: Option<u32>,
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
}

#[derive(Debug, Deserialize)]
struct OllamaChatResponse {
    message: OllamaChatMessage,
}

#[derive(Clone)]
pub struct OllamaCloudClient {
    http: Client,
}

impl OllamaCloudClient {
    pub fn new() -> Result<Self, ClassifierError> {
        let http = Client::builder()
            .timeout(MAX_CLASSIFICATION_LATENCY)
            .connect_timeout(Duration::from_secs(8))
            .build()
            .map_err(|e| ClassifierError::Http(e.to_string()))?;
        Ok(Self { http })
    }

    pub fn build_system_one_request(
        model: &str,
        state: SystemOneState,
        base64_jpeg: String,
    ) -> SystemOneRequest {
        let mut questions = HashMap::with_capacity(1);
        questions.insert(
            "activity",
            SystemOneQuestion {
                question_type: "choice",
                instructions: "Is the user actively working on or researching their declared_task, or are they distracted off_task?",
                criteria: SystemOneCriteria {
                    on_task: "Working directly on the declared task, reading relevant documentation, tutorials, or waiting on builds",
                    off_task: "Unrelated entertainment, social media feeds, shopping, or unrelated videos",
                    uncertain: "Insufficient or ambiguous screen context",
                },
            },
        );
        SystemOneRequest {
            model: model.to_string(),
            state,
            images: vec![base64_jpeg],
            questions,
        }
    }

    pub async fn classify_activity(
        &self,
        base_url: &str,
        api_key: &str,
        model: &str,
        state: SystemOneState,
        base64_jpeg: String,
    ) -> Result<ClassificationOutcome, ClassifierError> {
        let endpoint = format!("{}/v1/systemone", base_url.trim_end_matches('/'));
        let effective_model = normalize_model_for_endpoint(base_url, model);
        let payload = Self::build_system_one_request(&effective_model, state, base64_jpeg);
        let headers = build_headers(api_key)?;

        let started = Instant::now();
        let response = self
            .http
            .post(&endpoint)
            .headers(headers)
            .timeout(MAX_CLASSIFICATION_LATENCY)
            .json(&payload)
            .send()
            .await
            .map_err(|err| {
                if err.is_timeout() {
                    ClassifierError::StaleLatency(started.elapsed().as_millis())
                } else {
                    ClassifierError::Http(err.to_string())
                }
            })?;

        let latency = started.elapsed();
        if latency > MAX_CLASSIFICATION_LATENCY {
            return Err(ClassifierError::StaleLatency(latency.as_millis()));
        }

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|err| ClassifierError::Http(err.to_string()))?;

        if !status.is_success() {
            return Err(ClassifierError::UpstreamStatus {
                status: status.as_u16(),
                body: truncate_error_body(&body),
            });
        }

        parse_system_one_response(&body, latency)
    }

    pub async fn generate_nudge(
        &self,
        base_url: &str,
        api_key: &str,
        model: &str,
        declared_task: &str,
        active_app: &str,
        window_title: &str,
        ocr_lines: &[String],
    ) -> Result<NudgeDirective, ClassifierError> {
        let endpoint = format!("{}/api/chat", base_url.trim_end_matches('/'));
        let effective_model = normalize_model_for_endpoint(base_url, model);

        let ocr_block = if ocr_lines.is_empty() {
            "(no visible OCR lines)".to_string()
        } else {
            ocr_lines.join("\n")
        };

        let system_prompt = "You are a witty, warm macOS focus companion cursor. \
Write ONE concise, playful, non-judgmental sentence (max 18 words) nudging the user back to their declared_task. \
Also select target_ocr_id matching the integer ID in brackets [N] of the most distracting visible element on screen, or null if none match. \
Respond strictly as JSON with keys nudge_text and target_ocr_id.";

        let user_prompt = format!(
            "Declared task: {declared_task}\nActive app: {active_app}\nWindow title: {window_title}\nVisible OCR lines:\n{ocr_block}"
        );

        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "nudge_text": { "type": "string" },
                "target_ocr_id": { "type": ["integer", "null"] }
            },
            "required": ["nudge_text", "target_ocr_id"]
        });

        let chat_req = OllamaChatRequest {
            model: effective_model,
            messages: vec![
                OllamaChatMessage {
                    role: "system".to_string(),
                    content: system_prompt.to_string(),
                },
                OllamaChatMessage {
                    role: "user".to_string(),
                    content: user_prompt,
                },
            ],
            stream: false,
            format: schema,
            options: OllamaChatOptions { temperature: 0.4 },
        };

        let mut last_err: Option<ClassifierError> = None;
        for attempt in 0..2 {
            let headers = build_headers(api_key)?;
            match self
                .http
                .post(&endpoint)
                .headers(headers)
                .timeout(NUDGE_REQUEST_TIMEOUT)
                .json(&chat_req)
                .send()
                .await
            {
                Ok(resp) => {
                    let status = resp.status();
                    let text = resp
                        .text()
                        .await
                        .map_err(|e| ClassifierError::Http(e.to_string()))?;
                    if status.is_success() {
                        return parse_nudge_chat_response(&text);
                    }
                    let err = ClassifierError::UpstreamStatus {
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
                    let mapped = ClassifierError::Http(err.to_string());
                    if attempt == 0 {
                        last_err = Some(mapped);
                        tokio::time::sleep(Duration::from_millis(350)).await;
                        continue;
                    }
                    return Err(mapped);
                }
            }
        }

        Err(last_err.unwrap_or_else(|| ClassifierError::Http("unknown nudge error".to_string())))
    }
}

pub fn parse_system_one_response(
    raw_json: &str,
    latency: Duration,
) -> Result<ClassificationOutcome, ClassifierError> {
    if latency > MAX_CLASSIFICATION_LATENCY {
        return Err(ClassifierError::StaleLatency(latency.as_millis()));
    }
    let parsed: SystemOneResponse = serde_json::from_str(raw_json)
        .map_err(|err| ClassifierError::MalformedPayload(err.to_string()))?;
    let prob_off_task = parsed.answers.activity.probabilities.off_task.clamp(0.0, 1.0);
    let confidence = parsed.answers.activity.confidence.clamp(0.0, 1.0);
    Ok(ClassificationOutcome {
        choice: parsed.answers.activity.choice,
        prob_off_task,
        confidence,
        latency,
    })
}

pub fn parse_nudge_chat_response(raw_json: &str) -> Result<NudgeDirective, ClassifierError> {
    let chat: OllamaChatResponse = serde_json::from_str(raw_json)
        .map_err(|err| ClassifierError::MalformedPayload(err.to_string()))?;
    let raw_content = chat.message.content.trim();
    let directive: NudgeDirective = serde_json::from_str(raw_content)
        .map_err(|err| ClassifierError::MalformedPayload(err.to_string()))?;

    let sanitized_text = directive
        .nudge_text
        .chars()
        .filter(|c| !c.is_control())
        .take(200)
        .collect::<String>()
        .trim()
        .to_string();

    if sanitized_text.is_empty() {
        return Err(ClassifierError::MalformedPayload(
            "empty nudge_text returned by model".to_string(),
        ));
    }

    Ok(NudgeDirective {
        nudge_text: sanitized_text,
        target_ocr_id: directive.target_ocr_id.filter(|id| *id >= 1),
    })
}

fn normalize_model_for_endpoint(base_url: &str, model: &str) -> String {
    let is_cloud_direct = base_url.starts_with("https://ollama.com");
    if is_cloud_direct {
        model.trim_end_matches(":cloud").to_string()
    } else {
        model.to_string()
    }
}

fn build_headers(api_key: &str) -> Result<HeaderMap, ClassifierError> {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    let trimmed = api_key.trim();
    if !trimmed.is_empty() {
        let bearer = format!("Bearer {trimmed}");
        let header_val = HeaderValue::from_str(&bearer)
            .map_err(|_| ClassifierError::Http("invalid API key header characters".to_string()))?;
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
    fn serializes_exact_clef_systemone_request_contract() {
        let req = OllamaCloudClient::build_system_one_request(
            "clef",
            SystemOneState {
                declared_task: "Building a Rust focus app".to_string(),
                active_app: "Google Chrome".to_string(),
                elapsed_minutes: 12,
                ocr_lines: vec![
                    "[1] YouTube - Trending".to_string(),
                    "[2] Shorts".to_string(),
                ],
            },
            "rawbase64jpeg".to_string(),
        );
        let val = serde_json::to_value(&req).expect("serialize systemone request");
        assert_eq!(val["model"], "clef");
        assert_eq!(val["state"]["declared_task"], "Building a Rust focus app");
        assert_eq!(val["state"]["active_app"], "Google Chrome");
        assert_eq!(val["state"]["elapsed_minutes"], 12);
        assert_eq!(val["images"][0], "rawbase64jpeg");
        assert_eq!(val["questions"]["activity"]["type"], "choice");
        assert!(val["questions"]["activity"]["criteria"]["on_task"].is_string());
        assert!(val["questions"]["activity"]["criteria"]["off_task"].is_string());
        assert!(val["questions"]["activity"]["criteria"]["uncertain"].is_string());
    }

    #[test]
    fn parses_valid_systemone_response() {
        let sample = r#"{
            "model": "clef",
            "answers": {
                "activity": {
                    "type": "choice",
                    "choice": "off_task",
                    "probabilities": {
                        "on_task": 0.04,
                        "off_task": 0.93,
                        "uncertain": 0.03
                    },
                    "confidence": 0.91
                }
            }
        }"#;
        let outcome =
            parse_system_one_response(sample, Duration::from_millis(420)).expect("valid parse");
        assert_eq!(outcome.choice, ActivityChoice::OffTask);
        assert!((outcome.prob_off_task - 0.93).abs() < 1e-6);
        assert!((outcome.confidence - 0.91).abs() < 1e-6);
    }

    #[test]
    fn rejects_stale_systemone_response_exceeding_20s() {
        let sample = r#"{
            "answers": {
                "activity": {
                    "choice": "off_task",
                    "probabilities": { "off_task": 0.98 },
                    "confidence": 0.95
                }
            }
        }"#;
        let err = parse_system_one_response(sample, Duration::from_millis(20_500))
            .expect_err("latency over 20s must fail");
        assert!(matches!(err, ClassifierError::StaleLatency(_)));
    }

    #[test]
    fn parses_deepseek_nudge_chat_response() {
        let raw = r#"{
            "model": "deepseek-v4.1-flash",
            "message": {
                "role": "assistant",
                "content": "{\"nudge_text\":\"Those Shorts can wait—your Rust compiler misses you!\",\"target_ocr_id\":2}"
            },
            "done": true
        }"#;
        let parsed = parse_nudge_chat_response(raw).expect("valid nudge json");
        assert_eq!(
            parsed.nudge_text,
            "Those Shorts can wait—your Rust compiler misses you!"
        );
        assert_eq!(parsed.target_ocr_id, Some(2));
    }
}
