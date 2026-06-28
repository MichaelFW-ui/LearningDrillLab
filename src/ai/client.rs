use crate::ai::prompts;
use crate::app::ApiSettings;
use crate::domain::concept::Concept;
use crate::domain::exercise::{Exercise, ExerciseKind, ExperimentPrompt};
use crate::domain::review::ReviewResult;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::json;
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
    #[error("无法解析 AI JSON: {0}")]
    Json(#[from] serde_json::Error),
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
                settings,
                vec![
                    ChatMessageWire::system(prompts::learning_system_prompt()),
                    ChatMessageWire::user(prompts::explain_and_generate_prompt(topic)),
                ],
                true,
            )
            .await?;
        let parsed: LearningResponseWire = serde_json::from_str(&strip_json_fences(&raw))?;
        let concept = parsed.concept.into_concept();
        let concept_id = concept.id;
        let exercises = parsed
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
        let raw = self
            .chat_completion(
                settings,
                vec![
                    ChatMessageWire::system(prompts::learning_system_prompt()),
                    ChatMessageWire::user(prompts::review_prompt(&exercise_json, answer)),
                ],
                true,
            )
            .await?;
        let parsed: ReviewResultWire = serde_json::from_str(&strip_json_fences(&raw))?;
        Ok(parsed.into_review(Some(raw)))
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
        let parsed: ExperimentPromptWire = serde_json::from_str(&strip_json_fences(&raw))?;
        Ok(ExperimentPrompt::new(parsed.title, parsed.prompt))
    }

    async fn chat_completion(
        &self,
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
        let mut payload = json!({
            "model": model,
            "messages": messages,
            "temperature": 0.2
        });

        if json_response {
            payload["response_format"] = json!({ "type": "json_object" });
        }

        let response = self
            .http
            .post(url)
            .bearer_auth(api_key)
            .json(&payload)
            .send()
            .await?;
        let response = ensure_success(response).await?;
        let body: ChatCompletionResponse = response.json().await?;
        body.choices
            .into_iter()
            .next()
            .map(|choice| choice.message.content)
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
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessageContent,
}

#[derive(Debug, Deserialize)]
struct ChatMessageContent {
    content: String,
}

#[derive(Debug, Deserialize)]
struct LearningResponseWire {
    #[serde(default)]
    topic_title: Option<String>,
    explanation: String,
    concept: ConceptWire,
    exercises: Vec<ExerciseWire>,
}

#[derive(Debug, Deserialize)]
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

#[derive(Debug, Deserialize)]
struct ExerciseWire {
    kind: ExerciseKind,
    title: String,
    prompt: String,
    starter_code: String,
    expected_answer: String,
    hints: Vec<String>,
    difficulty: String,
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
