use crate::ai::prompts;
use crate::app::{ApiSettings, ChatMessage, ChatRole};
use crate::domain::concept::Concept;
use crate::domain::exercise::{Exercise, ExerciseKind, ExperimentPrompt};
use crate::domain::review::ReviewResult;
use directories::ProjectDirs;
use reqwest::StatusCode;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs::OpenOptions;
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AiError {
    #[error("请先在设置页填写 API Key")]
    MissingApiKey,
    #[error("请先在设置页选择模型")]
    MissingModel,
    #[error("API 请求失败: {0}")]
    Request(#[from] reqwest::Error),
    #[error("API 返回 HTTP {status}: {body}")]
    Http { status: StatusCode, body: String },
    #[error("API 响应里没有可用内容")]
    EmptyResponse,
    #[error("API 响应未正常完成: finish_reason={finish_reason}")]
    IncompleteResponse { finish_reason: String },
    #[error("无法解析 AI JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("AI 质量门未通过: {0}")]
    QualityGateFailed(String),
}

#[derive(Clone)]
pub struct AiClient {
    http: reqwest::Client,
}

impl Default for AiClient {
    fn default() -> Self {
        Self {
            http: reqwest::Client::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExplainAndGenerateResponse {
    pub topic_title: Option<String>,
    pub explanation: String,
    pub concept: Concept,
    pub exercises: Vec<Exercise>,
    pub raw_response: String,
}

impl AiClient {
    pub async fn fetch_models(&self, settings: &ApiSettings) -> Result<Vec<String>, AiError> {
        let api_key = require_api_key(settings)?;
        let url = format!("{}/models", normalize_base_url(&settings.base_url));
        let response = self.http.get(url).bearer_auth(api_key).send().await?;
        let response = ensure_success(response).await?;
        let body: ModelListResponse = response.json().await?;
        let mut models: Vec<String> = body.data.into_iter().map(|model| model.id).collect();
        models.sort();
        models.dedup();
        Ok(models)
    }

    pub async fn explain_and_generate(
        &self,
        settings: &ApiSettings,
        topic: &str,
    ) -> Result<ExplainAndGenerateResponse, AiError> {
        let raw = self
            .chat_completion(
                "concept.explain",
                settings,
                vec![
                    ChatMessageWire::system(prompts::learning_system_prompt()),
                    ChatMessageWire::user(prompts::explain_topic_prompt(topic)),
                ],
                true,
            )
            .await?;
        let parsed: TopicExplanationWire = parse_ai_json(&raw)?;
        let concept_json = serde_json::to_string_pretty(&parsed.concept)?;
        let exercises = self
            .generate_validated_exercises(settings, &concept_json, &parsed.explanation, None)
            .await?;
        let concept = parsed.concept.into_concept();
        let concept_id = concept.id;
        let exercises = exercises
            .exercises
            .into_iter()
            .map(|exercise| exercise.into_exercise(Some(concept_id)))
            .collect();
        Ok(ExplainAndGenerateResponse {
            topic_title: parsed.topic_title,
            explanation: parsed.explanation,
            concept,
            exercises,
            raw_response: raw,
        })
    }

    pub async fn review_attempt(
        &self,
        settings: &ApiSettings,
        exercise: &Exercise,
        answer: &str,
    ) -> Result<ReviewResult, AiError> {
        let exercise_json = serde_json::to_string_pretty(exercise)?;
        let validation_raw = self
            .chat_completion(
                "review.exercise_gate",
                settings,
                vec![
                    ChatMessageWire::system(prompts::learning_system_prompt()),
                    ChatMessageWire::user(prompts::review_exercise_gate_prompt(&exercise_json)),
                ],
                true,
            )
            .await?;
        let validation: ExerciseValidationWire = parse_ai_json(&validation_raw)?;
        if !validation.is_accepted() {
            return Ok(ReviewResult::new(
                None,
                true,
                100,
                format!(
                    "这道题未通过题目前提审查，因此不应按原题扣分。审查结论：{}",
                    validation.summary()
                ),
                validation.blocking_issues,
                "建议重新生成练习，或把这道题改成可观察输出/显式断言的题目。".to_string(),
                vec!["重新生成练习后再提交答案。".to_string()],
                Some(validation_raw),
            ));
        }

        let raw = self
            .chat_completion(
                "review",
                settings,
                vec![
                    ChatMessageWire::system(prompts::learning_system_prompt()),
                    ChatMessageWire::user(prompts::review_prompt(&exercise_json, answer)),
                ],
                true,
            )
            .await?;
        let parsed: ReviewResultWire = parse_ai_json(&raw)?;
        Ok(parsed.into_review(Some(raw)))
    }

    pub async fn regenerate_exercises(
        &self,
        settings: &ApiSettings,
        concept: &Concept,
        explanation_context: &str,
        previous_exercises: &[Exercise],
    ) -> Result<Vec<Exercise>, AiError> {
        let concept_json = serde_json::to_string_pretty(concept)?;
        let previous_exercises_json = serde_json::to_string_pretty(previous_exercises)?;
        let exercises = self
            .generate_validated_exercises(
                settings,
                &concept_json,
                explanation_context,
                Some(&previous_exercises_json),
            )
            .await?;
        Ok(exercises
            .exercises
            .into_iter()
            .map(|exercise| exercise.into_exercise(Some(concept.id)))
            .collect())
    }

    pub async fn follow_up(
        &self,
        settings: &ApiSettings,
        messages: &[ChatMessage],
        concept_json: &str,
        exercises_json: &str,
        attempts_json: &str,
    ) -> Result<String, AiError> {
        let mut wire_messages = vec![
            ChatMessageWire::system(prompts::follow_up_system_prompt()),
            ChatMessageWire::user(prompts::follow_up_context_prompt(
                concept_json,
                exercises_json,
                attempts_json,
            )),
        ];

        wire_messages.extend(messages.iter().map(ChatMessageWire::from_chat_message));

        self.chat_completion("follow_up", settings, wire_messages, false)
            .await
    }

    pub async fn generate_experiment_prompt(
        &self,
        settings: &ApiSettings,
        concept: &Concept,
        exercise: &Exercise,
    ) -> Result<ExperimentPrompt, AiError> {
        let concept_json = serde_json::to_string_pretty(concept)?;
        let exercise_json = serde_json::to_string_pretty(exercise)?;
        let raw = self
            .chat_completion(
                "experiment_prompt",
                settings,
                vec![
                    ChatMessageWire::system(prompts::learning_system_prompt()),
                    ChatMessageWire::user(prompts::experiment_prompt(
                        &concept_json,
                        &exercise_json,
                    )),
                ],
                true,
            )
            .await?;
        let parsed: ExperimentPromptWire = parse_ai_json(&raw)?;
        Ok(ExperimentPrompt::new(parsed.title, parsed.prompt))
    }

    async fn generate_validated_exercises(
        &self,
        settings: &ApiSettings,
        concept_json: &str,
        explanation_context: &str,
        previous_exercises_json: Option<&str>,
    ) -> Result<ExerciseSetWire, AiError> {
        const MAX_ATTEMPTS_PER_SLOT: usize = 3;
        const DIFFICULTY_PLAN: [&str; 4] = ["easy", "medium", "hard", "hard"];

        let mut accepted = Vec::new();
        let mut rejected: Vec<RejectedExerciseWire> = Vec::new();

        while accepted.len() < DIFFICULTY_PLAN.len() {
            let slot = accepted.len() + 1;
            let target_difficulty = DIFFICULTY_PLAN[slot - 1];
            let mut accepted_this_slot = false;

            for attempt in 1..=MAX_ATTEMPTS_PER_SLOT {
                let accepted_json = serde_json::to_string_pretty(&accepted)?;
                let rejected_json = serde_json::to_string_pretty(&rejected)?;
                let raw = self
                    .chat_completion(
                        "exercise.generate_one",
                        settings,
                        vec![
                            ChatMessageWire::system(prompts::learning_system_prompt()),
                            ChatMessageWire::user(prompts::generate_one_exercise_prompt(
                                concept_json,
                                explanation_context,
                                previous_exercises_json,
                                &accepted_json,
                                &rejected_json,
                                slot,
                                target_difficulty,
                                attempt,
                            )),
                        ],
                        true,
                    )
                    .await?;
                let exercise: ExerciseWire = parse_ai_json(&raw)?;
                let exercise_json = serde_json::to_string_pretty(&exercise)?;
                let validation_raw = self
                    .chat_completion(
                        "exercise.validate_one",
                        settings,
                        vec![
                            ChatMessageWire::system(prompts::learning_system_prompt()),
                            ChatMessageWire::user(prompts::validate_one_exercise_prompt(
                                concept_json,
                                explanation_context,
                                &exercise_json,
                                target_difficulty,
                            )),
                        ],
                        true,
                    )
                    .await?;
                let validation: ExerciseValidationWire = parse_ai_json(&validation_raw)?;

                if validation.is_accepted() {
                    accepted.push(exercise);
                    accepted_this_slot = true;
                    break;
                }

                rejected.push(RejectedExerciseWire {
                    exercise,
                    validation,
                });
            }

            if !accepted_this_slot {
                return Err(AiError::QualityGateFailed(format!(
                    "第 {slot} 道练习连续 {MAX_ATTEMPTS_PER_SLOT} 次未通过独立题目前提审查，已停止展示可疑题目"
                )));
            }
        }

        Ok(ExerciseSetWire {
            exercises: accepted,
        })
    }

    async fn chat_completion(
        &self,
        label: &str,
        settings: &ApiSettings,
        messages: Vec<ChatMessageWire>,
        json_response: bool,
    ) -> Result<String, AiError> {
        let api_key = require_api_key(settings)?;
        let model = require_model(settings)?;
        let url = format!(
            "{}/chat/completions",
            normalize_base_url(&settings.base_url)
        );
        let deepseek_thinking = is_deepseek_base_url(&settings.base_url);
        let mut payload = json!({
            "model": model,
            "messages": messages
        });

        if deepseek_thinking {
            payload["thinking"] = json!({ "type": "enabled" });
            payload["reasoning_effort"] = json!("high");
        } else {
            payload["temperature"] = json!(0.2);
        }

        if json_response {
            payload["response_format"] = json!({ "type": "json_object" });
            payload["max_tokens"] = json!(8192);
        }

        log_ai_request(label, &payload);

        let response = self
            .http
            .post(url)
            .bearer_auth(api_key)
            .json(&payload)
            .send()
            .await?;
        let response = ensure_success(response).await?;
        let body: ChatCompletionResponse = response.json().await?;
        let choice = body
            .choices
            .into_iter()
            .next()
            .ok_or(AiError::EmptyResponse)?;
        log_ai_response(
            label,
            choice.message.reasoning_content.as_deref(),
            choice.message.content.as_deref().unwrap_or_default(),
            choice.finish_reason.as_deref(),
        );

        if let Some(finish_reason) = choice.finish_reason.as_deref() {
            if !matches!(finish_reason, "stop" | "tool_calls") {
                return Err(AiError::IncompleteResponse {
                    finish_reason: finish_reason.to_string(),
                });
            }
        }

        choice
            .message
            .content
            .filter(|content| !content.trim().is_empty())
            .ok_or(AiError::EmptyResponse)
    }
}

fn require_api_key(settings: &ApiSettings) -> Result<String, AiError> {
    let key = settings.api_key.trim();
    if key.is_empty() {
        return Err(AiError::MissingApiKey);
    }
    Ok(key.to_string())
}

fn require_model(settings: &ApiSettings) -> Result<String, AiError> {
    let model = settings.selected_model.trim();
    if model.is_empty() {
        return Err(AiError::MissingModel);
    }
    Ok(model.to_string())
}

fn normalize_base_url(base_url: &str) -> String {
    base_url.trim().trim_end_matches('/').to_string()
}

fn is_deepseek_base_url(base_url: &str) -> bool {
    base_url.contains("api.deepseek.com")
}

fn log_ai_request(label: &str, payload: &Value) {
    log_ai_event(label, "request", &redact_payload(payload));
}

fn log_ai_response(
    label: &str,
    reasoning_content: Option<&str>,
    content: &str,
    finish_reason: Option<&str>,
) {
    let mut body = String::new();
    if let Some(finish_reason) = finish_reason {
        body.push_str("finish_reason: ");
        body.push_str(finish_reason);
        body.push_str("\n\n");
    }
    if let Some(reasoning_content) = reasoning_content {
        if !reasoning_content.trim().is_empty() {
            body.push_str("reasoning_content:\n");
            body.push_str(reasoning_content);
            body.push_str("\n\n");
        }
    }
    body.push_str("content:\n");
    body.push_str(content);
    log_ai_event(label, "response", &body);
}

fn log_ai_parse_repair(raw: &str, repaired: &str) {
    log_ai_event("json.parse", "repair_raw", raw);
    log_ai_event("json.parse", "repair_repaired", repaired);
}

fn log_ai_event(label: &str, kind: &str, body: &str) {
    let timestamp = unix_timestamp_secs();
    let entry = format!(
        "\n===== AI {kind} [{label}] {timestamp} =====\n{body}\n===== end AI {kind} [{label}] =====\n"
    );

    eprintln!("{entry}");

    if let Some(path) = ai_debug_log_path() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
            let _ = file.write_all(entry.as_bytes());
        }
    }
}

fn redact_payload(payload: &Value) -> String {
    serde_json::to_string_pretty(payload).unwrap_or_else(|_| "<failed to serialize payload>".into())
}

fn unix_timestamp_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn ai_debug_log_path() -> Option<std::path::PathBuf> {
    ProjectDirs::from("dev", "LearningDrillLab", "LearningDrillLab")
        .map(|dirs| dirs.config_dir().join("ai-debug.log"))
}

async fn ensure_success(response: reqwest::Response) -> Result<reqwest::Response, AiError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response
        .text()
        .await
        .unwrap_or_else(|_| "<failed to read body>".to_string());
    Err(AiError::Http { status, body })
}

fn strip_json_fences(raw: &str) -> String {
    let trimmed = raw.trim();
    if !trimmed.starts_with("```") {
        return trimmed.to_string();
    }

    let without_open = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .unwrap_or(trimmed)
        .trim_start();
    without_open
        .strip_suffix("```")
        .unwrap_or(without_open)
        .trim()
        .to_string()
}

fn parse_ai_json<T: DeserializeOwned>(raw: &str) -> Result<T, AiError> {
    let stripped = strip_json_fences(raw);
    match serde_json::from_str(&stripped) {
        Ok(value) => Ok(value),
        Err(original_error) => {
            let repaired = escape_invalid_json_backslashes(&stripped);
            if repaired != stripped {
                log_ai_parse_repair(&stripped, &repaired);
            }
            serde_json::from_str(&repaired).map_err(|_| original_error.into())
        }
    }
}

fn escape_invalid_json_backslashes(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;

    while let Some(ch) = chars.next() {
        if !in_string {
            if ch == '"' {
                in_string = true;
            }
            output.push(ch);
            continue;
        }

        if escaped {
            if is_valid_json_escape(ch) {
                output.push('\\');
                output.push(ch);
            } else if ch == 'u' {
                let mut unicode_digits = String::new();
                for _ in 0..4 {
                    if let Some(next) = chars.peek().copied() {
                        if next.is_ascii_hexdigit() {
                            unicode_digits.push(next);
                            chars.next();
                        } else {
                            break;
                        }
                    }
                }

                if unicode_digits.len() == 4 {
                    output.push('\\');
                    output.push('u');
                    output.push_str(&unicode_digits);
                } else {
                    output.push_str("\\\\u");
                    output.push_str(&unicode_digits);
                }
            } else {
                output.push_str("\\\\");
                output.push(ch);
            }
            escaped = false;
            continue;
        }

        match ch {
            '\\' => escaped = true,
            '"' => {
                in_string = false;
                output.push(ch);
            }
            _ => output.push(ch),
        }
    }

    if escaped {
        output.push_str("\\\\");
    }

    output
}

fn is_valid_json_escape(ch: char) -> bool {
    matches!(ch, '"' | '\\' | '/' | 'b' | 'f' | 'n' | 'r' | 't')
}

#[derive(Debug, Deserialize)]
struct ModelListResponse {
    data: Vec<ModelInfo>,
}

#[derive(Debug, Deserialize)]
struct ModelInfo {
    id: String,
}

#[derive(Debug, Serialize)]
struct ChatMessageWire {
    role: String,
    content: String,
}

impl ChatMessageWire {
    fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: content.into(),
        }
    }

    fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
        }
    }

    fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into(),
        }
    }

    fn from_chat_message(message: &ChatMessage) -> Self {
        match message.role {
            ChatRole::User => Self::user(message.content.clone()),
            ChatRole::Assistant => Self::assistant(message.content.clone()),
        }
    }
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessageContent,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatMessageContent {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    reasoning_content: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct TopicExplanationWire {
    #[serde(default)]
    topic_title: Option<String>,
    explanation: String,
    concept: ConceptWire,
}

#[derive(Debug, Serialize, Deserialize)]
struct ConceptWire {
    title: String,
    language: String,
    summary: String,
    key_points: Vec<String>,
}

impl ConceptWire {
    fn into_concept(self) -> Concept {
        Concept::new(self.title, self.language, self.summary, self.key_points)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ExerciseWire {
    kind: ExerciseKind,
    title: String,
    prompt: String,
    starter_code: String,
    expected_answer: String,
    hints: Vec<String>,
    difficulty: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ExerciseSetWire {
    exercises: Vec<ExerciseWire>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ExerciseValidationWire {
    verdict: String,
    checked_claims: Vec<String>,
    blocking_issues: Vec<String>,
    risk_notes: Vec<String>,
}

impl ExerciseValidationWire {
    fn is_accepted(&self) -> bool {
        self.verdict.trim().eq_ignore_ascii_case("accepted")
    }

    fn summary(&self) -> String {
        if self.blocking_issues.is_empty() {
            return self.verdict.clone();
        }
        self.blocking_issues.join("；")
    }
}

#[derive(Debug, Clone, Serialize)]
struct RejectedExerciseWire {
    exercise: ExerciseWire,
    validation: ExerciseValidationWire,
}

impl ExerciseWire {
    fn into_exercise(self, concept_id: Option<uuid::Uuid>) -> Exercise {
        Exercise::new(
            concept_id,
            self.kind,
            self.title,
            self.prompt,
            self.starter_code,
            self.expected_answer,
            self.hints,
            self.difficulty,
        )
    }
}

#[derive(Debug, Deserialize)]
struct ReviewResultWire {
    is_correct: bool,
    score: u8,
    summary: String,
    mistakes: Vec<String>,
    corrected_answer: String,
    next_steps: Vec<String>,
}

impl ReviewResultWire {
    fn into_review(self, raw_response: Option<String>) -> ReviewResult {
        ReviewResult::new(
            None,
            self.is_correct,
            self.score.min(100),
            self.summary,
            self.mistakes,
            self.corrected_answer,
            self.next_steps,
            raw_response,
        )
    }
}

#[derive(Debug, Deserialize)]
struct ExperimentPromptWire {
    title: String,
    prompt: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn parse_ai_json_repairs_invalid_string_backslash_escape() {
        let raw = r#"{"pattern":"use \d+ to match digits"}"#;

        let parsed: Value = parse_ai_json(raw).expect("invalid escape should be repaired");

        assert_eq!(parsed["pattern"], "use \\d+ to match digits");
    }

    #[test]
    fn parse_ai_json_keeps_valid_json_escapes() {
        let raw = r#"{"line":"a\nb","quote":"\"ok\""}"#;

        let parsed: Value = parse_ai_json(raw).expect("valid json should parse normally");

        assert_eq!(parsed["line"], "a\nb");
        assert_eq!(parsed["quote"], "\"ok\"");
    }

    #[test]
    fn parse_ai_json_handles_fenced_json_with_invalid_escape() {
        let raw = "```json\n{\"path\":\"C:\\model\\weights\"}\n```";

        let parsed: Value =
            parse_ai_json(raw).expect("fenced json should be stripped and repaired");

        assert_eq!(parsed["path"], "C:\\model\\weights");
    }
}
