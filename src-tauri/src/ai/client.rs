use crate::ai::prompts;
use crate::app::{ApiSettings, ChatMessage, ChatRole};
use crate::domain::concept::Concept;
use crate::domain::exercise::{Exercise, ExerciseKind, ExperimentPrompt, VerificationEvidence};
use crate::domain::review::ReviewResult;
use directories::ProjectDirs;
use reqwest::StatusCode;
use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;

pub type ProgressReporter = Arc<dyn Fn(String) + Send + Sync + 'static>;

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
    #[error("{label} API 返回 HTTP {status}: {body}")]
    LabeledHttp {
        label: String,
        status: StatusCode,
        body: String,
    },
    #[error("API 响应里没有可用内容")]
    EmptyResponse,
    #[error("API 响应未正常完成: finish_reason={finish_reason}")]
    IncompleteResponse { finish_reason: String },
    #[error("无法解析 AI JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("AI 质量门未通过: {0}")]
    QualityGateFailed(String),
    #[error("任务已取消")]
    Cancelled,
}

#[derive(Clone)]
pub struct AiClient {
    http: reqwest::Client,
    cancel: Option<Arc<AtomicBool>>,
}

impl Default for AiClient {
    fn default() -> Self {
        Self {
            http: reqwest::Client::new(),
            cancel: None,
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

#[derive(Debug, Clone)]
struct SearchToolRuntime {
    bocha_key: Option<String>,
    tavily_key: Option<String>,
    tavily_base_url: String,
    jina_key: Option<String>,
}

impl AiClient {
    pub fn with_cancel(mut self, cancel: Arc<AtomicBool>) -> Self {
        self.cancel = Some(cancel);
        self
    }

    fn check_cancelled(&self) -> Result<(), AiError> {
        if self
            .cancel
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Relaxed))
        {
            Err(AiError::Cancelled)
        } else {
            Ok(())
        }
    }

    async fn send_with_cancel(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, AiError> {
        self.check_cancelled()?;
        let Some(cancel) = self.cancel.clone() else {
            return Ok(request.send().await?);
        };
        tokio::select! {
            response = request.send() => Ok(response?),
            _ = wait_for_cancel(cancel) => Err(AiError::Cancelled),
        }
    }

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
        progress: Option<ProgressReporter>,
    ) -> Result<ExplainAndGenerateResponse, AiError> {
        report_progress(progress.as_ref(), "正在整理学习目标并生成讲解...");
        let raw = self
            .chat_completion_with_search(
                "concept.explain",
                settings,
                vec![
                    ChatMessageWire::system(prompts::learning_system_prompt()),
                    ChatMessageWire::user(prompts::explain_topic_prompt(topic)),
                ],
                true,
                progress.as_ref(),
            )
            .await?;
        report_progress(progress.as_ref(), "讲解已生成，正在解析结构化内容...");
        let parsed: TopicExplanationWire = self
            .parse_ai_json_or_repair("concept.explain", settings, &raw)
            .await?;
        report_progress(progress.as_ref(), "讲解解析完成，正在准备生成练习...");
        let concept_json = serde_json::to_string_pretty(&parsed.concept)?;
        let exercises = self
            .generate_validated_exercises(
                settings,
                &concept_json,
                &parsed.explanation,
                None,
                progress.as_ref(),
            )
            .await?;
        report_progress(progress.as_ref(), "练习全部通过审查，正在写入学习会话...");
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
            .chat_completion_with_search(
                "review.exercise_gate",
                settings,
                vec![
                    ChatMessageWire::system(prompts::learning_system_prompt()),
                    ChatMessageWire::user(prompts::review_exercise_gate_prompt(&exercise_json)),
                ],
                true,
                None,
            )
            .await?;
        let validation: ExerciseValidationWire = self
            .parse_ai_json_or_repair("review.exercise_gate", settings, &validation_raw)
            .await?;
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
            .chat_completion_with_search(
                "review",
                settings,
                vec![
                    ChatMessageWire::system(prompts::learning_system_prompt()),
                    ChatMessageWire::user(prompts::review_prompt(&exercise_json, answer)),
                ],
                true,
                None,
            )
            .await?;
        let parsed: ReviewResultWire = self
            .parse_ai_json_or_repair("review", settings, &raw)
            .await?;
        Ok(parsed.into_review(Some(raw)))
    }

    pub async fn regenerate_exercises(
        &self,
        settings: &ApiSettings,
        concept: &Concept,
        explanation_context: &str,
        previous_exercises: &[Exercise],
        progress: Option<ProgressReporter>,
    ) -> Result<Vec<Exercise>, AiError> {
        report_progress(
            progress.as_ref(),
            "正在读取当前讲解上下文，准备重新生成练习...",
        );
        let concept_json = serde_json::to_string_pretty(concept)?;
        let previous_exercises_json = serde_json::to_string_pretty(previous_exercises)?;
        let exercises = self
            .generate_validated_exercises(
                settings,
                &concept_json,
                explanation_context,
                Some(&previous_exercises_json),
                progress.as_ref(),
            )
            .await?;
        report_progress(progress.as_ref(), "新练习全部通过审查，正在替换旧练习...");
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

        self.chat_completion_with_search("follow_up", settings, wire_messages, false, None)
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
        let parsed: ExperimentPromptWire = self
            .parse_ai_json_or_repair("experiment_prompt", settings, &raw)
            .await?;
        Ok(ExperimentPrompt::new(parsed.title, parsed.prompt))
    }

    async fn generate_validated_exercises(
        &self,
        settings: &ApiSettings,
        concept_json: &str,
        explanation_context: &str,
        previous_exercises_json: Option<&str>,
        progress: Option<&ProgressReporter>,
    ) -> Result<ExerciseSetWire, AiError> {
        const MAX_ATTEMPTS_PER_SLOT: usize = 3;
        const MAX_AGENT_ACTIONS: usize = 8;
        const MIN_EXERCISES: usize = 4;
        const MAX_EXERCISES: usize = 6;

        let mut accepted = Vec::new();
        let mut rejected: Vec<RejectedExerciseWire> = Vec::new();

        for action in 0..MAX_AGENT_ACTIONS {
            self.check_cancelled()?;
            report_progress(progress, "技能 curriculum 正在选择下一步练习动作...");
            let accepted_json = serde_json::to_string_pretty(&accepted)?;
            let rejected_json = serde_json::to_string_pretty(&rejected)?;
            let decision_raw = self.chat_completion(
                "agent.curriculum",
                settings,
                vec![
                    ChatMessageWire::system(crate::skills::load("curriculum")),
                    ChatMessageWire::user(format!(
                        "Concept: {concept_json}\nExplanation: {explanation_context}\nPrevious exercises: {}\nAccepted: {accepted_json}\nRejected: {rejected_json}\nAction {}/{}. Return only JSON {{\"action\":\"draft_exercise|finish\",\"difficulty\":\"easy|medium|hard\",\"reason\":\"short reason\"}}. Finish requires at least {MIN_EXERCISES} accepted exercises. Maximum {MAX_EXERCISES}.",
                        previous_exercises_json.unwrap_or("[]"), action + 1, MAX_AGENT_ACTIONS
                    )),
                ],
                true,
            ).await?;
            let decision: AgentDecisionWire = self
                .parse_ai_json_or_repair("agent.curriculum", settings, &decision_raw)
                .await?;
            if !matches!(decision.action.as_str(), "finish" | "draft_exercise") {
                return Err(AiError::QualityGateFailed(
                    "curriculum 技能返回无效动作".to_string(),
                ));
            }
            if decision.action == "finish"
                && accepted.len() >= MIN_EXERCISES
                && has_difficulty_coverage(&accepted)
            {
                report_progress(
                    progress,
                    format!("技能 curriculum 已完成 {} 道练习。", accepted.len()),
                );
                break;
            }
            if accepted.len() >= MAX_EXERCISES {
                break;
            }
            let slot = accepted.len() + 1;
            let target_difficulty = match decision.difficulty.as_str() {
                "easy" => "easy",
                "medium" => "medium",
                "hard" => "hard",
                _ => {
                    return Err(AiError::QualityGateFailed(
                        "curriculum 技能返回无效难度".to_string(),
                    ))
                }
            };
            let mut accepted_this_slot = false;
            report_progress(
                progress,
                format!(
                    "技能 curriculum 选择第 {slot} 道练习（难度：{target_difficulty}）：{}",
                    decision.reason
                ),
            );

            for attempt in 1..=MAX_ATTEMPTS_PER_SLOT {
                self.check_cancelled()?;
                let accepted_json = serde_json::to_string_pretty(&accepted)?;
                let rejected_json = serde_json::to_string_pretty(&rejected)?;
                report_progress(
                    progress,
                    format!(
                        "正在生成第 {slot} 道练习，第 {attempt}/{MAX_ATTEMPTS_PER_SLOT} 次尝试..."
                    ),
                );
                let raw = self
                    .chat_completion_with_search(
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
                        progress,
                    )
                    .await?;
                report_progress(
                    progress,
                    format!("第 {slot} 道练习已生成，正在解析题目 JSON..."),
                );
                let exercise: ExerciseWire = self
                    .parse_ai_json_or_repair("exercise.generate_one", settings, &raw)
                    .await?;
                let exercise_json = serde_json::to_string_pretty(&exercise)?;
                report_progress(
                    progress,
                    format!("正在审查第 {slot} 道练习是否符合讲解上下文..."),
                );
                let validation_raw = self
                    .chat_completion_with_search(
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
                        progress,
                    )
                    .await?;
                let validation: ExerciseValidationWire = self
                    .parse_ai_json_or_repair("exercise.validate_one", settings, &validation_raw)
                    .await?;

                if validation.is_accepted() {
                    report_progress(
                        progress,
                        format!("第 {slot} 道练习通过上下文审查，正在做展示前可评分性审查..."),
                    );
                    let gate_raw = self
                        .chat_completion_with_search(
                            "exercise.review_gate",
                            settings,
                            vec![
                                ChatMessageWire::system(prompts::learning_system_prompt()),
                                ChatMessageWire::user(prompts::review_exercise_gate_prompt(
                                    &exercise_json,
                                )),
                            ],
                            true,
                            progress,
                        )
                        .await?;
                    let gate_validation: ExerciseValidationWire = self
                        .parse_ai_json_or_repair("exercise.review_gate", settings, &gate_raw)
                        .await?;

                    if !gate_validation.is_accepted() {
                        report_progress(
                            progress,
                            format!("第 {slot} 道练习展示前审查未通过，正在重试生成..."),
                        );
                        rejected.push(RejectedExerciseWire {
                            exercise,
                            validation: gate_validation,
                        });
                        continue;
                    }

                    let mut exercise = exercise;
                    let evidence = self
                        .verify_exercise_in_sandbox(settings, &exercise, progress)
                        .await?;
                    if evidence.status == "contradicted" {
                        rejected.push(RejectedExerciseWire {
                            exercise,
                            validation: ExerciseValidationWire {
                                verdict: "rejected".to_string(),
                                checked_claims: vec!["沙箱实际输出与题目预期对照".to_string()],
                                blocking_issues: vec![evidence.note],
                                risk_notes: Vec::new(),
                            },
                        });
                        continue;
                    }
                    exercise.verification = Some(evidence);
                    accepted.push(exercise);
                    accepted_this_slot = true;
                    report_progress(progress, format!("第 {slot} 道练习已通过全部审查。"));
                    break;
                }

                report_progress(
                    progress,
                    format!("第 {slot} 道练习上下文审查未通过，正在重试生成..."),
                );
                rejected.push(RejectedExerciseWire {
                    exercise,
                    validation,
                });
            }

            if !accepted_this_slot {
                report_progress(
                    progress,
                    format!("第 {slot} 道练习未通过审查，技能将读取原因并重新决策。"),
                );
            }
        }

        if accepted.len() < MIN_EXERCISES || !has_difficulty_coverage(&accepted) {
            return Err(AiError::QualityGateFailed(format!(
                "技能预算耗尽，{} 道练习通过审查，未满足至少 {MIN_EXERCISES} 道且覆盖 easy、medium、hard 的要求",
                accepted.len()
            )));
        }

        Ok(ExerciseSetWire {
            exercises: accepted,
        })
    }

    async fn verify_exercise_in_sandbox(
        &self,
        settings: &ApiSettings,
        exercise: &ExerciseWire,
        progress: Option<&ProgressReporter>,
    ) -> Result<VerificationEvidence, AiError> {
        if settings.sandbox_base_url.trim().is_empty() {
            return Ok(VerificationEvidence {
                status: "unverified".to_string(),
                note: "尚未配置远端沙箱，题目已通过文本审查，待实验验证。".to_string(),
                ..Default::default()
            });
        }
        report_progress(
            progress,
            "技能 experiment-verification 正在设计可复现实验...",
        );
        let exercise_json = serde_json::to_string_pretty(exercise)?;
        let raw = self.chat_completion(
            "agent.experiment_probe",
            settings,
            vec![
                ChatMessageWire::system(crate::skills::load("experiment-verification")),
                ChatMessageWire::user(format!(
                    "Exercise: {exercise_json}\nReturn only JSON: {{\"runnable\":boolean,\"code\":\"complete source code\",\"language\":\"py|js|ts|go|java|c|cpp|php|rs|r|f90|d|sh\",\"expected_stdout\":\"exact expected stdout, trim surrounding whitespace for comparison\",\"reason\":\"short reason\"}}. If expected output cannot be stated, runnable must be false."
                )),
            ],
            true,
        ).await?;
        let probe: SandboxProbeWire = self
            .parse_ai_json_or_repair("agent.experiment_probe", settings, &raw)
            .await?;
        if !probe.runnable {
            return Ok(VerificationEvidence {
                status: "not_applicable".to_string(),
                note: probe.reason,
                ..Default::default()
            });
        }
        report_progress(progress, format!("沙箱正在执行 {} 实验...", probe.language));
        let execution = crate::sandbox::execute(
            settings,
            uuid::Uuid::new_v4(),
            probe.code.clone(),
            probe.language,
            None,
        );
        let result = if let Some(cancel) = self.cancel.clone() {
            tokio::select! {
                result = execution => result,
                _ = wait_for_cancel(cancel) => return Err(AiError::Cancelled),
            }
        } else {
            execution.await
        };
        match result {
            Ok(run) => {
                if matches!(
                    run.status.as_str(),
                    "authentication"
                        | "authorization"
                        | "rate_limited"
                        | "resource_exhausted"
                        | "timeout"
                        | "internal_server"
                        | "service_unavailable"
                        | "external_service"
                ) {
                    return Ok(VerificationEvidence {
                        status: "unavailable".to_string(),
                        note: format!("沙箱未能完成实验：{}。{}", run.status, run.stderr),
                        code: probe.code,
                        stdout: run.stdout,
                        stderr: run.stderr,
                    });
                }
                let consistent =
                    run.status == "completed" && run.stdout.trim() == probe.expected_stdout.trim();
                let assessment_raw = self.chat_completion(
                    "agent.experiment_assess",
                    settings,
                    vec![
                        ChatMessageWire::system(crate::skills::load("experiment-verification")),
                        ChatMessageWire::user(format!(
                            "Exercise: {exercise_json}\nProbe code: {}\nExpected stdout: {:?}\nActual status: {}\nActual stdout: {:?}\nActual stderr: {:?}\nDoes this probe truly validate the exercise's central premise? Return only JSON {{\"verdict\":\"supported|contradicted|inconclusive\",\"reason\":\"specific reason\"}}.",
                            probe.code, probe.expected_stdout, run.status, run.stdout, run.stderr
                        )),
                    ],
                    true,
                ).await?;
                let assessment: ObservationAssessmentWire = self
                    .parse_ai_json_or_repair("agent.experiment_assess", settings, &assessment_raw)
                    .await?;
                let status = if !consistent || assessment.verdict == "contradicted" {
                    "contradicted"
                } else if assessment.verdict == "supported" {
                    "verified"
                } else {
                    "inconclusive"
                };
                let note = format!(
                    "实际状态 {}，实际输出 {:?}，预期输出 {:?}。{}",
                    run.status, run.stdout, probe.expected_stdout, assessment.reason
                );
                Ok(VerificationEvidence {
                    status: status.to_string(),
                    note,
                    code: probe.code,
                    stdout: run.stdout,
                    stderr: run.stderr,
                })
            }
            Err(error) => Ok(VerificationEvidence {
                status: "unavailable".to_string(),
                note: format!("沙箱实验暂不可用：{error}"),
                code: probe.code,
                ..Default::default()
            }),
        }
    }

    async fn parse_ai_json_or_repair<T>(
        &self,
        label: &str,
        settings: &ApiSettings,
        raw: &str,
    ) -> Result<T, AiError>
    where
        T: DeserializeOwned,
    {
        match parse_ai_json(raw) {
            Ok(parsed) => Ok(parsed),
            Err(error) => {
                log_ai_event(label, "json_parse_failed", &error.to_string());
                let repaired = self.repair_ai_json(label, settings, raw, &error).await?;
                match parse_ai_json(&repaired) {
                    Ok(parsed) => Ok(parsed),
                    Err(repair_error) => {
                        log_ai_event(label, "json_repair_failed", &repair_error.to_string());
                        let rebuilt = self
                            .rebuild_ai_json(label, settings, raw, &repair_error)
                            .await?;
                        parse_ai_json(&rebuilt)
                    }
                }
            }
        }
    }

    async fn repair_ai_json(
        &self,
        label: &str,
        settings: &ApiSettings,
        raw: &str,
        error: &AiError,
    ) -> Result<String, AiError> {
        let raw_json_string = serde_json::to_string(raw)?;
        let schema = json_repair_schema(label);
        let repair_prompt = format!(
            r#"The following assistant output was intended to be a single JSON object, but it is malformed.

Parse error:
{error}

Expected schema:
{schema}

Repair it into valid JSON without changing the data model, field names, language, markdown content, code blocks, URLs, or meaning.
Most failures are caused by unescaped double quotes inside JSON strings, unfinished strings, or truncated closing braces. Escape inner quotes correctly.
Return only the repaired JSON object. Do not wrap it in markdown fences.

Malformed JSON is provided below as a JSON string. Decode it mentally first; do not output this wrapper string:
{raw_json_string}"#
        );

        self.chat_completion(
            &format!("{label}.json_repair"),
            settings,
            vec![
                ChatMessageWire::system(
                    "You repair malformed JSON. Return only valid JSON. Do not explain.",
                ),
                ChatMessageWire::user(repair_prompt),
            ],
            true,
        )
        .await
    }

    async fn rebuild_ai_json(
        &self,
        label: &str,
        settings: &ApiSettings,
        raw: &str,
        error: &AiError,
    ) -> Result<String, AiError> {
        let raw_json_string = serde_json::to_string(raw)?;
        let schema = json_repair_schema(label);
        let prompt = format!(
            r#"The prior JSON repair failed.

Parse error after repair:
{error}

Reconstruct a valid JSON object from the malformed assistant output.

Expected schema:
{schema}

Rules:
- Return exactly one valid JSON object.
- Preserve all useful Chinese explanation content, markdown, code blocks, source URLs, and technical details from the malformed output.
- Use the expected schema and field names exactly.
- Do not summarize, shorten, or replace content with placeholders.
- Escape all quotes inside JSON strings.
- Do not wrap the JSON in markdown fences.

Malformed assistant output is encoded as this JSON string:
{raw_json_string}"#
        );

        self.chat_completion(
            &format!("{label}.json_rebuild"),
            settings,
            vec![
                ChatMessageWire::system(
                    "You reconstruct malformed assistant output into a valid JSON object that matches the requested schema. Return only valid JSON.",
                ),
                ChatMessageWire::user(prompt),
            ],
            true,
        )
        .await
    }

    async fn run_search_query(
        &self,
        search_runtime: &SearchToolRuntime,
        query: &str,
    ) -> Result<Vec<SearchResultWire>, AiError> {
        let mut failures = Vec::new();
        for provider in search_runtime.search_providers() {
            let provider_name = provider.name();
            let result = match provider {
                SearchProvider::Tavily { api_key, base_url } => {
                    self.search_tavily(api_key, base_url, query).await
                }
                SearchProvider::Bocha { api_key } => self.search_bocha(api_key, query).await,
            };

            match result {
                Ok(mut results) => {
                    if !failures.is_empty() {
                        results.insert(0, fallback_search_note(query, provider_name, &failures));
                    }
                    return Ok(results);
                }
                Err(error) => {
                    log_ai_event(
                        "search.fallback",
                        "provider_failed",
                        &format!("{provider_name}: {error}"),
                    );
                    failures.push(format!("{provider_name}: {error}"));
                }
            }
        }

        if failures.is_empty() {
            Ok(Vec::new())
        } else {
            Err(AiError::QualityGateFailed(format!(
                "所有搜索服务都失败：{}",
                failures.join("；")
            )))
        }
    }

    async fn run_fetch_urls(
        &self,
        jina_key: Option<&str>,
        urls: &[String],
    ) -> Result<Vec<FetchedPageWire>, AiError> {
        let mut pages = Vec::new();
        for (index, url) in urls.iter().enumerate() {
            if index > 0 {
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
            pages.push(self.fetch_jina_reader(jina_key, url).await?);
        }
        Ok(pages)
    }

    async fn search_bocha(
        &self,
        api_key: &str,
        query: &str,
    ) -> Result<Vec<SearchResultWire>, AiError> {
        let payload = json!({
            "query": query,
            "freshness": "noLimit",
            "summary": true,
            "count": 10
        });
        log_ai_request("search.bocha", &payload);
        let response = self
            .http
            .post("https://api.bochaai.com/v1/web-search")
            .bearer_auth(api_key)
            .json(&payload)
            .send()
            .await?;
        let response = ensure_success_labeled("search.bocha", response).await?;
        let body: Value = response.json().await?;
        log_ai_event(
            "search.bocha",
            "response",
            &serde_json::to_string_pretty(&body).unwrap_or_else(|_| body.to_string()),
        );
        Ok(parse_bocha_results(query, &body))
    }

    async fn search_tavily(
        &self,
        api_key: &str,
        base_url: &str,
        query: &str,
    ) -> Result<Vec<SearchResultWire>, AiError> {
        let payload = json!({
            "query": query,
            "search_depth": "advanced",
            "max_results": 10,
            "include_answer": true,
            "include_raw_content": false
        });
        log_ai_request("search.tavily", &payload);
        let response = self
            .http
            .post(tavily_search_url(base_url))
            .bearer_auth(api_key)
            .json(&payload)
            .send()
            .await?;
        let response = ensure_success_labeled("search.tavily", response).await?;
        let body: Value = response.json().await?;
        log_ai_event(
            "search.tavily",
            "response",
            &serde_json::to_string_pretty(&body).unwrap_or_else(|_| body.to_string()),
        );
        Ok(parse_tavily_results(query, &body))
    }

    async fn fetch_jina_reader(
        &self,
        api_key: Option<&str>,
        url: &str,
    ) -> Result<FetchedPageWire, AiError> {
        match self.fetch_jina_reader_once(None, url).await {
            Ok(page) => return Ok(page),
            Err(error) if should_retry_jina_with_key(error.status) => {
                log_ai_event(
                    "fetch.jina.public",
                    "fallback_to_key",
                    &format!("HTTP {}: {}", error.status, error.body),
                );
                if let Some(api_key) = api_key {
                    return self
                        .fetch_jina_reader_once(Some(api_key), url)
                        .await
                        .map_err(|error| AiError::LabeledHttp {
                            label: "fetch.jina.key".to_string(),
                            status: error.status,
                            body: error.body,
                        });
                }
                return Err(AiError::LabeledHttp {
                    label: "fetch.jina.public".to_string(),
                    status: error.status,
                    body: error.body,
                });
            }
            Err(error) => {
                return Err(AiError::LabeledHttp {
                    label: "fetch.jina.public".to_string(),
                    status: error.status,
                    body: error.body,
                })
            }
        }
    }

    async fn fetch_jina_reader_once(
        &self,
        api_key: Option<&str>,
        url: &str,
    ) -> Result<FetchedPageWire, HttpStatusError> {
        let reader_url = jina_reader_url(url);
        let label = if api_key.is_some() {
            "fetch.jina.key"
        } else {
            "fetch.jina.public"
        };
        log_ai_event(label, "request", &reader_url);
        let mut request = self
            .http
            .get(&reader_url)
            .header(reqwest::header::ACCEPT, "text/plain");
        if let Some(api_key) = api_key {
            request = request.bearer_auth(api_key);
        }
        let response = request.send().await?;
        let response = ensure_success_status(response).await?;
        let body = response.text().await?;
        log_ai_event(label, "response", &body);
        let (content, truncated) = truncate_text(&body, 18_000);
        Ok(FetchedPageWire {
            provider: if api_key.is_some() {
                "Jina Reader (API key)".to_string()
            } else {
                "Jina Reader (public)".to_string()
            },
            url: url.to_string(),
            content,
            truncated,
        })
    }

    async fn chat_completion(
        &self,
        label: &str,
        settings: &ApiSettings,
        messages: Vec<ChatMessageWire>,
        json_response: bool,
    ) -> Result<String, AiError> {
        self.chat_completion_inner(label, settings, messages, json_response, None, None)
            .await
    }

    async fn chat_completion_with_search(
        &self,
        label: &str,
        settings: &ApiSettings,
        messages: Vec<ChatMessageWire>,
        json_response: bool,
        progress: Option<&ProgressReporter>,
    ) -> Result<String, AiError> {
        let search_runtime = search_tool_runtime(settings);
        self.chat_completion_inner(
            label,
            settings,
            messages,
            json_response,
            search_runtime.as_ref(),
            progress,
        )
        .await
    }

    async fn chat_completion_inner(
        &self,
        label: &str,
        settings: &ApiSettings,
        mut messages: Vec<ChatMessageWire>,
        json_response: bool,
        search_runtime: Option<&SearchToolRuntime>,
        progress: Option<&ProgressReporter>,
    ) -> Result<String, AiError> {
        const MAX_TOOL_ROUNDS: usize = 4;

        let api_key = require_api_key(settings)?;
        let model = require_model(settings)?;
        let url = format!(
            "{}/chat/completions",
            normalize_base_url(&settings.base_url)
        );
        let deepseek_thinking = is_deepseek_base_url(&settings.base_url);

        for round in 0..=MAX_TOOL_ROUNDS {
            self.check_cancelled()?;
            let payload = chat_completion_payload(
                &model,
                &messages,
                json_response,
                deepseek_thinking,
                search_runtime,
            );

            let round_label = if round == 0 {
                label.to_string()
            } else {
                format!("{label}.tool_round_{round}")
            };
            log_ai_request(&round_label, &payload);

            let response = self
                .send_with_cancel(self.http.post(&url).bearer_auth(&api_key).json(&payload))
                .await?;
            let response = ensure_success(response).await?;
            let body: ChatCompletionResponse = response.json().await?;
            let choice = body
                .choices
                .into_iter()
                .next()
                .ok_or(AiError::EmptyResponse)?;
            log_ai_response(
                &round_label,
                choice.message.reasoning_content.as_deref(),
                choice.message.content.as_deref().unwrap_or_default(),
                choice.finish_reason.as_deref(),
            );

            if choice.finish_reason.as_deref() == Some("length") {
                return self
                    .continue_after_length(
                        &round_label,
                        &api_key,
                        &url,
                        &model,
                        messages,
                        choice.message.content.unwrap_or_default(),
                        deepseek_thinking,
                    )
                    .await;
            }

            if let Some(finish_reason) = choice.finish_reason.as_deref() {
                if !matches!(finish_reason, "stop" | "tool_calls") {
                    return Err(AiError::IncompleteResponse {
                        finish_reason: finish_reason.to_string(),
                    });
                }
            }

            if let Some(tool_calls) = choice
                .message
                .tool_calls
                .clone()
                .filter(|calls| !calls.is_empty())
            {
                let Some(search_runtime) = search_runtime else {
                    return Err(AiError::EmptyResponse);
                };
                messages.push(ChatMessageWire::assistant_with_tool_calls(
                    choice.message.content,
                    tool_calls.clone(),
                ));

                for tool_call in tool_calls {
                    let tool_result = self
                        .execute_tool_call(search_runtime, &tool_call, progress)
                        .await?;
                    messages.push(ChatMessageWire::tool(tool_call.id, tool_result));
                }
                continue;
            }

            return choice
                .message
                .content
                .filter(|content| !content.trim().is_empty())
                .ok_or(AiError::EmptyResponse);
        }

        self.finalize_after_tool_limit(
            label,
            &api_key,
            &url,
            &model,
            messages,
            json_response,
            deepseek_thinking,
            MAX_TOOL_ROUNDS,
        )
        .await
    }

    async fn finalize_after_tool_limit(
        &self,
        label: &str,
        api_key: &str,
        url: &str,
        model: &str,
        mut messages: Vec<ChatMessageWire>,
        json_response: bool,
        deepseek_thinking: bool,
        max_tool_rounds: usize,
    ) -> Result<String, AiError> {
        messages.push(ChatMessageWire::user(format!(
            "You have already used the available search/fetch tool budget ({max_tool_rounds} rounds). Do not call any more tools. Use the evidence already present in the conversation and produce the final answer now. If evidence is incomplete, state the uncertainty instead of searching again."
        )));
        let payload =
            chat_completion_payload(model, &messages, json_response, deepseek_thinking, None);
        let final_label = format!("{label}.tool_limit_final");
        log_ai_request(&final_label, &payload);

        let response = self
            .send_with_cancel(self.http.post(url).bearer_auth(api_key).json(&payload))
            .await?;
        let response = ensure_success(response).await?;
        let body: ChatCompletionResponse = response.json().await?;
        let choice = body
            .choices
            .into_iter()
            .next()
            .ok_or(AiError::EmptyResponse)?;
        log_ai_response(
            &final_label,
            choice.message.reasoning_content.as_deref(),
            choice.message.content.as_deref().unwrap_or_default(),
            choice.finish_reason.as_deref(),
        );

        if choice.finish_reason.as_deref() == Some("length") {
            return self
                .continue_after_length(
                    &final_label,
                    api_key,
                    url,
                    model,
                    messages,
                    choice.message.content.unwrap_or_default(),
                    deepseek_thinking,
                )
                .await;
        }

        if let Some(finish_reason) = choice.finish_reason.as_deref() {
            if !matches!(finish_reason, "stop") {
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

    async fn continue_after_length(
        &self,
        label: &str,
        api_key: &str,
        url: &str,
        model: &str,
        mut messages: Vec<ChatMessageWire>,
        first_content: String,
        deepseek_thinking: bool,
    ) -> Result<String, AiError> {
        const MAX_CONTINUATIONS: usize = 3;

        let mut combined = first_content;
        if combined.trim().is_empty() {
            return Err(AiError::EmptyResponse);
        }

        messages.push(ChatMessageWire::assistant(combined.clone()));
        for index in 1..=MAX_CONTINUATIONS {
            self.check_cancelled()?;
            messages.push(ChatMessageWire::user(
                "The previous response was cut off by the output token limit. Continue exactly from the next character. Do not repeat previous text. Do not summarize or shorten. Do not add commentary. If this is JSON, continue the same JSON text until it is complete.",
            ));
            let payload = chat_completion_payload(model, &messages, false, deepseek_thinking, None);
            let continuation_label = format!("{label}.length_continue_{index}");
            log_ai_request(&continuation_label, &payload);

            let response = self
                .send_with_cancel(self.http.post(url).bearer_auth(api_key).json(&payload))
                .await?;
            let response = ensure_success(response).await?;
            let body: ChatCompletionResponse = response.json().await?;
            let choice = body
                .choices
                .into_iter()
                .next()
                .ok_or(AiError::EmptyResponse)?;
            let content = choice.message.content.unwrap_or_default();
            log_ai_response(
                &continuation_label,
                choice.message.reasoning_content.as_deref(),
                &content,
                choice.finish_reason.as_deref(),
            );
            if content.trim().is_empty() {
                return Err(AiError::EmptyResponse);
            }

            combined.push_str(&content);
            messages.push(ChatMessageWire::assistant(content));

            match choice.finish_reason.as_deref() {
                Some("stop") | None => return Ok(combined),
                Some("length") => continue,
                Some(finish_reason) => {
                    return Err(AiError::IncompleteResponse {
                        finish_reason: finish_reason.to_string(),
                    })
                }
            }
        }

        Ok(combined)
    }

    async fn execute_tool_call(
        &self,
        search_runtime: &SearchToolRuntime,
        tool_call: &ToolCallWire,
        progress: Option<&ProgressReporter>,
    ) -> Result<String, AiError> {
        match tool_call.function.name.as_str() {
            "web_search" => {
                let args: WebSearchToolArgs = serde_json::from_str(&tool_call.function.arguments)?;
                let query = sanitize_search_query(args.query, args.queries, 300);
                if query.is_empty() {
                    return Ok(json!({
                        "error": "query must contain at least one concrete search query"
                    })
                    .to_string());
                }

                report_progress(progress, format!("正在搜索资料：{query}"));
                let mut results = self.run_search_query(search_runtime, &query).await?;
                dedupe_sources(&mut results);
                report_progress(
                    progress,
                    format!("搜索完成，找到 {} 条候选资料。", results.len()),
                );
                Ok(serde_json::to_string(&json!({
                    "purpose": args.purpose,
                    "query": query,
                    "results": results
                }))?)
            }
            "web_fetch" => {
                let args: WebFetchToolArgs = serde_json::from_str(&tool_call.function.arguments)?;
                let urls = sanitize_urls(args.urls, 5);
                if urls.is_empty() {
                    return Ok(json!({
                        "error": "urls must contain at least one http(s) URL"
                    })
                    .to_string());
                }

                report_progress(progress, format!("正在抓取资料正文：{} 个 URL", urls.len()));
                let pages = self
                    .run_fetch_urls(search_runtime.jina_key.as_deref(), &urls)
                    .await?;
                report_progress(
                    progress,
                    format!("资料正文抓取完成：{} 个页面。", pages.len()),
                );
                Ok(serde_json::to_string(&json!({
                    "purpose": args.purpose,
                    "urls": urls,
                    "pages": pages
                }))?)
            }
            _ => Ok(json!({
                "error": format!("unsupported tool: {}", tool_call.function.name)
            })
            .to_string()),
        }
    }
}

async fn wait_for_cancel(cancel: Arc<AtomicBool>) {
    while !cancel.load(Ordering::Relaxed) {
        tokio::time::sleep(Duration::from_millis(100)).await;
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

fn search_tool_runtime(settings: &ApiSettings) -> Option<SearchToolRuntime> {
    let bocha_key = non_empty_string(&settings.bocha_api_key);
    let tavily_key = non_empty_string(&settings.tavily_api_key);
    let tavily_base_url = settings.tavily_base_url.trim();
    let jina_key = non_empty_string(&settings.jina_api_key);
    if bocha_key.is_none() && tavily_key.is_none() && jina_key.is_none() {
        return None;
    }

    Some(SearchToolRuntime {
        bocha_key,
        tavily_key,
        tavily_base_url: if tavily_base_url.is_empty() {
            default_tavily_base_url()
        } else {
            tavily_base_url.to_string()
        },
        jina_key,
    })
}

fn non_empty_string(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn report_progress(progress: Option<&ProgressReporter>, message: impl Into<String>) {
    if let Some(progress) = progress {
        progress(message.into());
    }
}

fn chat_completion_payload(
    model: &str,
    messages: &[ChatMessageWire],
    json_response: bool,
    deepseek_thinking: bool,
    search_runtime: Option<&SearchToolRuntime>,
) -> Value {
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
    }

    if let Some(search_runtime) = search_runtime {
        let mut tools = Vec::new();
        if search_runtime.has_search_provider() {
            tools.push(web_search_tool_definition(search_runtime));
        }
        tools.push(web_fetch_tool_definition());
        if !tools.is_empty() {
            payload["tools"] = json!(tools);
            payload["tool_choice"] = json!("auto");
        }
    }

    payload
}

fn json_repair_schema(label: &str) -> &'static str {
    if label.starts_with("concept.explain") {
        r#"{
  "topic_title": string | null,
  "explanation": string,
  "concept": {
    "title": string,
    "language": string,
    "summary": string,
    "key_points": string[]
  }
}"#
    } else if label.starts_with("exercise.generate_one") {
        r#"{
  "kind": "short_answer" | "code_prediction" | "fill_in_blank" | "debugging" | "implementation" | "concept_map",
  "title": string,
  "prompt": string,
  "starter_code": string,
  "expected_answer": string,
  "hints": string[],
  "difficulty": "easy" | "medium" | "hard"
}"#
    } else if label.starts_with("exercise.validate_one")
        || label.starts_with("exercise.review_gate")
        || label.starts_with("review.exercise_gate")
    {
        r#"{
  "verdict": string,
  "checked_claims": string[],
  "blocking_issues": string[],
  "risk_notes": string[]
}"#
    } else if label.starts_with("review") {
        r#"{
  "is_correct": boolean,
  "score": number,
  "summary": string,
  "mistakes": string[],
  "corrected_answer": string,
  "next_steps": string[]
}"#
    } else if label.starts_with("agent.curriculum") {
        r#"{
  "action": "draft_exercise | finish",
  "difficulty": "easy | medium | hard",
  "reason": string
}"#
    } else if label.starts_with("agent.experiment_probe") {
        r#"{
  "runnable": boolean,
  "code": string,
  "language": string,
  "expected_stdout": string,
  "reason": string
}"#
    } else if label.starts_with("agent.experiment_assess") {
        r#"{
  "verdict": "supported | contradicted | inconclusive",
  "reason": string
}"#
    } else if label.starts_with("experiment_prompt") {
        r#"{
  "title": string,
  "prompt": string
}"#
    } else {
        "A single valid JSON object matching the original requested schema."
    }
}

fn web_search_tool_definition(search_runtime: &SearchToolRuntime) -> Value {
    let providers = search_runtime.provider_names().join(", ");
    json!({
        "type": "function",
        "function": {
            "name": "web_search",
            "description": format!("Search the web with one configured provider ({providers}). Use this before making version-sensitive, library-specific, API-specific, or non-obvious factual claims. Send one concise search query containing multiple keywords; do not send multiple separate queries. Inspect returned source URLs, and call web_fetch for primary or disputed sources when available."),
            "parameters": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "One keyword-rich search query. Put multiple keywords in this single string; do not create an array of separate searches."
                    },
                    "purpose": {
                        "type": "string",
                        "description": "Short reason for this search round."
                    }
                },
                "required": ["query"]
            }
        }
    })
}

fn web_fetch_tool_definition() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "web_fetch",
            "description": "Fetch readable markdown content from specific source URLs with Jina Reader. Public no-key Reader is used first and requests are rate-limited; a configured Jina API key is only used as fallback after public auth/rate-limit failures. Use this after web_search when a snippet is not enough to verify a claim, especially for official docs, API references, release notes, and disputed facts.",
            "parameters": {
                "type": "object",
                "properties": {
                    "urls": {
                        "type": "array",
                        "description": "1 to 5 http(s) URLs to read. Prefer official documentation or primary sources from search results.",
                        "items": { "type": "string" }
                    },
                    "purpose": {
                        "type": "string",
                        "description": "Short reason for fetching these sources."
                    }
                },
                "required": ["urls"]
            }
        }
    })
}

impl SearchToolRuntime {
    fn provider_names(&self) -> Vec<&'static str> {
        let mut providers = Vec::new();
        if self.bocha_key.is_some() {
            providers.push("Bocha");
        }
        if self.tavily_key.is_some() {
            providers.push("Tavily");
        }
        providers
    }

    fn has_search_provider(&self) -> bool {
        self.tavily_key.is_some() || self.bocha_key.is_some()
    }

    fn search_providers(&self) -> Vec<SearchProvider<'_>> {
        let mut providers = Vec::new();
        if let Some(api_key) = self.tavily_key.as_deref() {
            providers.push(SearchProvider::Tavily {
                api_key,
                base_url: &self.tavily_base_url,
            });
        }
        if let Some(api_key) = self.bocha_key.as_deref() {
            providers.push(SearchProvider::Bocha { api_key });
        }
        providers
    }
}

enum SearchProvider<'a> {
    Tavily { api_key: &'a str, base_url: &'a str },
    Bocha { api_key: &'a str },
}

impl SearchProvider<'_> {
    fn name(&self) -> &'static str {
        match self {
            Self::Tavily { .. } => "Tavily",
            Self::Bocha { .. } => "Bocha",
        }
    }
}

fn fallback_search_note(query: &str, provider_name: &str, failures: &[String]) -> SearchResultWire {
    SearchResultWire {
        provider: "Search fallback".to_string(),
        query: query.to_string(),
        title: format!("Using {provider_name} after another search provider failed"),
        url: "local://search-fallback".to_string(),
        snippet: format!(
            "Search continued with {provider_name}. Earlier provider failures: {}",
            failures.join("; ")
        ),
    }
}

fn normalize_base_url(base_url: &str) -> String {
    base_url.trim().trim_end_matches('/').to_string()
}

fn default_tavily_base_url() -> String {
    "https://api.tavily.com".to_string()
}

fn tavily_search_url(base_url: &str) -> String {
    let base_url = normalize_base_url(base_url);
    if base_url.ends_with("/search") {
        base_url
    } else {
        format!("{base_url}/search")
    }
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
    if !ai_debug_enabled() {
        return;
    }

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

fn ai_debug_enabled() -> bool {
    matches!(
        std::env::var("LEARNING_DRILL_LAB_AI_DEBUG")
            .unwrap_or_default()
            .as_str(),
        "1" | "true" | "TRUE" | "yes" | "YES"
    )
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

async fn ensure_success_labeled(
    label: &str,
    response: reqwest::Response,
) -> Result<reqwest::Response, AiError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response
        .text()
        .await
        .unwrap_or_else(|_| "<failed to read body>".to_string());
    Err(AiError::LabeledHttp {
        label: label.to_string(),
        status,
        body,
    })
}

#[derive(Debug)]
struct HttpStatusError {
    status: StatusCode,
    body: String,
}

impl From<reqwest::Error> for HttpStatusError {
    fn from(error: reqwest::Error) -> Self {
        Self {
            status: error.status().unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            body: error.to_string(),
        }
    }
}

async fn ensure_success_status(
    response: reqwest::Response,
) -> Result<reqwest::Response, HttpStatusError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response
        .text()
        .await
        .unwrap_or_else(|_| "<failed to read body>".to_string());
    Err(HttpStatusError { status, body })
}

fn should_retry_jina_with_key(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::TOO_MANY_REQUESTS
    )
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

#[derive(Debug, Clone, Serialize)]
struct ChatMessageWire {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<ToolCallWire>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

impl ChatMessageWire {
    fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    fn assistant_with_tool_calls(content: Option<String>, tool_calls: Vec<ToolCallWire>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: Some(content.unwrap_or_default()),
            tool_calls: Some(tool_calls),
            tool_call_id: None,
        }
    }

    fn tool(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: "tool".to_string(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
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
    #[serde(default)]
    tool_calls: Option<Vec<ToolCallWire>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ToolCallWire {
    id: String,
    r#type: String,
    function: ToolFunctionCallWire,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ToolFunctionCallWire {
    name: String,
    arguments: String,
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
    #[serde(skip)]
    verification: Option<VerificationEvidence>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ExerciseSetWire {
    exercises: Vec<ExerciseWire>,
}

fn has_difficulty_coverage(exercises: &[ExerciseWire]) -> bool {
    ["easy", "medium", "hard"].iter().all(|difficulty| {
        exercises
            .iter()
            .any(|exercise| exercise.difficulty == *difficulty)
    })
}

#[derive(Debug, Deserialize)]
struct AgentDecisionWire {
    action: String,
    difficulty: String,
    reason: String,
}

#[derive(Debug, Deserialize)]
struct SandboxProbeWire {
    runnable: bool,
    code: String,
    language: String,
    expected_stdout: String,
    reason: String,
}

#[derive(Debug, Deserialize)]
struct ObservationAssessmentWire {
    verdict: String,
    reason: String,
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
        let mut exercise = Exercise::new(
            concept_id,
            self.kind,
            self.title,
            self.prompt,
            self.starter_code,
            self.expected_answer,
            self.hints,
            self.difficulty,
        );
        exercise.verification = self.verification;
        exercise
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SearchResultWire {
    provider: String,
    query: String,
    title: String,
    url: String,
    snippet: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FetchedPageWire {
    provider: String,
    url: String,
    content: String,
    truncated: bool,
}

#[derive(Debug, Deserialize)]
struct WebSearchToolArgs {
    #[serde(default)]
    query: String,
    #[serde(default, deserialize_with = "deserialize_query_terms")]
    queries: Vec<String>,
    #[serde(default)]
    purpose: String,
}

#[derive(Debug, Deserialize)]
struct WebFetchToolArgs {
    #[serde(default, deserialize_with = "deserialize_string_list")]
    urls: Vec<String>,
    #[serde(default)]
    purpose: String,
}

fn deserialize_string_list<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    Ok(string_list_from_value(&value))
}

fn deserialize_query_terms<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    Ok(query_terms_from_value(&value))
}

fn string_list_from_value(value: &Value) -> Vec<String> {
    match value {
        Value::Array(items) => items
            .iter()
            .filter_map(|item| match item {
                Value::String(value) => Some(value.clone()),
                Value::Null => None,
                other => Some(other.to_string()),
            })
            .collect(),
        Value::String(value) => split_delimited_string_list(value),
        Value::Null => Vec::new(),
        other => vec![other.to_string()],
    }
}

fn query_terms_from_value(value: &Value) -> Vec<String> {
    match value {
        Value::Array(items) => items
            .iter()
            .filter_map(|item| match item {
                Value::String(value) => Some(value.clone()),
                Value::Null => None,
                other => Some(other.to_string()),
            })
            .collect(),
        Value::String(value) => vec![value.clone()],
        Value::Null => Vec::new(),
        other => vec![other.to_string()],
    }
}

fn split_delimited_string_list(value: &str) -> Vec<String> {
    value
        .split(|ch| ch == ',' || ch == '\n')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn sanitize_search_query(query: String, query_terms: Vec<String>, max_chars: usize) -> String {
    let raw_query = if query.trim().is_empty() {
        query_terms.join(" ")
    } else {
        query
    };
    let normalized = raw_query
        .replace([',', '\n', '\r', '\t'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    truncate_text(&normalized, max_chars).0
}

fn sanitize_urls(urls: Vec<String>, limit: usize) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    for url in urls {
        let url = url.trim().to_string();
        let lower = url.to_lowercase();
        if !(lower.starts_with("https://") || lower.starts_with("http://")) {
            continue;
        }
        if !seen.insert(lower) {
            continue;
        }
        output.push(url);
        if output.len() >= limit {
            break;
        }
    }
    output
}

fn parse_bocha_results(query: &str, body: &Value) -> Vec<SearchResultWire> {
    let mut results = Vec::new();
    collect_bocha_array(query, &body["webPages"]["value"], &mut results);
    collect_bocha_array(query, &body["data"]["webPages"]["value"], &mut results);
    collect_bocha_array(query, &body["data"]["results"], &mut results);
    collect_bocha_array(query, &body["results"], &mut results);
    dedupe_sources(&mut results);
    results
}

fn collect_bocha_array(query: &str, value: &Value, results: &mut Vec<SearchResultWire>) {
    let Some(items) = value.as_array() else {
        return;
    };
    for item in items {
        let title = first_string(item, &["name", "title"]);
        let url = first_string(item, &["url", "link"]);
        if title.is_empty() || url.is_empty() {
            continue;
        }
        let snippet = first_string(item, &["summary", "snippet", "content", "description"]);
        results.push(SearchResultWire {
            provider: "Bocha".to_string(),
            query: query.to_string(),
            title,
            url,
            snippet: truncate_text(&snippet, 2_000).0,
        });
    }
}

fn parse_tavily_results(query: &str, body: &Value) -> Vec<SearchResultWire> {
    let mut results = Vec::new();
    if let Some(answer) = body["answer"]
        .as_str()
        .filter(|answer| !answer.trim().is_empty())
    {
        results.push(SearchResultWire {
            provider: "Tavily".to_string(),
            query: query.to_string(),
            title: "Tavily answer summary".to_string(),
            url: "tavily://answer".to_string(),
            snippet: truncate_text(answer.trim(), 2_000).0,
        });
    }

    let Some(items) = body["results"].as_array() else {
        return results;
    };
    for item in items {
        let title = first_string(item, &["title", "name"]);
        let url = first_string(item, &["url", "link"]);
        if title.is_empty() || url.is_empty() {
            continue;
        }
        let snippet = first_string(item, &["content", "snippet", "summary", "description"]);
        results.push(SearchResultWire {
            provider: "Tavily".to_string(),
            query: query.to_string(),
            title,
            url,
            snippet: truncate_text(&snippet, 2_000).0,
        });
    }
    dedupe_sources(&mut results);
    results
}

fn first_string(item: &Value, keys: &[&str]) -> String {
    keys.iter()
        .find_map(|key| item.get(*key).and_then(Value::as_str))
        .map(|value| value.trim().to_string())
        .unwrap_or_default()
}

fn dedupe_sources(results: &mut Vec<SearchResultWire>) {
    let mut seen = HashSet::new();
    results.retain(|result| seen.insert(result.url.to_lowercase()));
}

fn jina_reader_url(url: &str) -> String {
    format!("https://r.jina.ai/{}", url.trim())
}

fn truncate_text(value: &str, max_chars: usize) -> (String, bool) {
    let mut output = String::new();
    let mut truncated = false;
    for (index, ch) in value.chars().enumerate() {
        if index >= max_chars {
            truncated = true;
            break;
        }
        output.push(ch);
    }
    (output, truncated)
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

    #[test]
    fn parse_bocha_results_reads_top_level_web_pages() {
        let body = json!({
            "webPages": {
                "value": [
                    {
                        "name": "Rust async",
                        "url": "https://doc.rust-lang.org/book/",
                        "summary": "Futures are polled by an executor."
                    }
                ]
            }
        });

        let results = parse_bocha_results("rust async", &body);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].provider, "Bocha");
        assert_eq!(results[0].title, "Rust async");
        assert_eq!(results[0].snippet, "Futures are polled by an executor.");
    }

    #[test]
    fn parse_tavily_results_reads_content_snippet() {
        let body = json!({
            "answer": "short summary",
            "results": [
                {
                    "title": "Tavily Search API",
                    "url": "https://docs.tavily.com/documentation/api-reference/endpoint/search",
                    "content": "The search endpoint returns ranked results."
                }
            ]
        });

        let results = parse_tavily_results("tavily search api", &body);

        assert_eq!(results.len(), 2);
        assert_eq!(results[1].provider, "Tavily");
        assert_eq!(results[1].title, "Tavily Search API");
        assert_eq!(
            results[1].snippet,
            "The search endpoint returns ranked results."
        );
    }

    #[test]
    fn web_search_args_accept_legacy_queries_but_build_one_search_query() {
        let args: WebSearchToolArgs = serde_json::from_str(
            r#"{"queries":"UE5 Gameplay Ability System GAS overview,Unreal Engine 5 GAS documentation,GameplayAbilitySystem UE5 official docs","purpose":"verify GAS docs"}"#,
        )
        .expect("comma-separated query string should be accepted");

        assert_eq!(
            args.queries,
            vec!["UE5 Gameplay Ability System GAS overview,Unreal Engine 5 GAS documentation,GameplayAbilitySystem UE5 official docs".to_string()]
        );
        assert_eq!(
            sanitize_search_query(args.query, args.queries, 300),
            "UE5 Gameplay Ability System GAS overview Unreal Engine 5 GAS documentation GameplayAbilitySystem UE5 official docs"
        );
        assert_eq!(args.purpose, "verify GAS docs");
    }

    #[test]
    fn web_search_args_prefer_single_query_field() {
        let args: WebSearchToolArgs = serde_json::from_str(
            r#"{"query":"UE5 GAS GameplayAbility official docs","queries":["should","not","matter"]}"#,
        )
        .expect("single query field should be accepted");

        assert_eq!(
            sanitize_search_query(args.query, args.queries, 300),
            "UE5 GAS GameplayAbility official docs"
        );
    }

    #[test]
    fn web_fetch_args_accept_newline_separated_url_string() {
        let args: WebFetchToolArgs = serde_json::from_str(
            "{\"urls\":\"https://example.com/docs\\nhttps://example.com/api\"}",
        )
        .expect("newline-separated url string should be accepted");

        assert_eq!(
            args.urls,
            vec![
                "https://example.com/docs".to_string(),
                "https://example.com/api".to_string()
            ]
        );
    }

    #[test]
    fn sanitize_urls_keeps_only_unique_http_urls() {
        let urls = sanitize_urls(
            vec![
                " https://example.com/docs ".to_string(),
                "HTTPS://example.com/docs".to_string(),
                "file:///tmp/a".to_string(),
                "http://example.com/api".to_string(),
            ],
            5,
        );

        assert_eq!(
            urls,
            vec![
                "https://example.com/docs".to_string(),
                "http://example.com/api".to_string()
            ]
        );
    }

    #[test]
    fn tavily_search_url_supports_official_and_proxy_base_urls() {
        assert_eq!(
            tavily_search_url("https://api.tavily.com"),
            "https://api.tavily.com/search"
        );
        assert_eq!(
            tavily_search_url("https://tavily-proxy.example.com/api/tavily"),
            "https://tavily-proxy.example.com/api/tavily/search"
        );
        assert_eq!(
            tavily_search_url("https://tavily-proxy.example.com/api/tavily/search"),
            "https://tavily-proxy.example.com/api/tavily/search"
        );
    }

    #[test]
    fn jina_key_does_not_enable_search_provider() {
        let runtime = SearchToolRuntime {
            bocha_key: None,
            tavily_key: None,
            tavily_base_url: default_tavily_base_url(),
            jina_key: Some("jina_test".to_string()),
        };

        assert!(!runtime.has_search_provider());
        assert!(runtime.search_providers().is_empty());
        assert!(runtime.provider_names().is_empty());
    }

    #[test]
    fn search_providers_try_tavily_then_bocha() {
        let runtime = SearchToolRuntime {
            bocha_key: Some("bocha_test".to_string()),
            tavily_key: Some("tavily_test".to_string()),
            tavily_base_url: default_tavily_base_url(),
            jina_key: None,
        };
        let names = runtime
            .search_providers()
            .iter()
            .map(SearchProvider::name)
            .collect::<Vec<_>>();

        assert_eq!(names, vec!["Tavily", "Bocha"]);
    }

    #[test]
    fn curriculum_agent_drives_generation_with_mock_model() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let server = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            let mut request_count = 0;
            let mut curriculum_count = 0;
            while std::time::Instant::now() < deadline && request_count < 18 {
                let (mut socket, _) = match listener.accept() {
                    Ok(pair) => pair,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(error) => panic!("mock accept failed: {error}"),
                };
                socket
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut received = Vec::new();
                let mut buffer = [0_u8; 8192];
                loop {
                    let count = socket.read(&mut buffer).unwrap();
                    received.extend_from_slice(&buffer[..count]);
                    let Some(header_end) = received.windows(4).position(|part| part == b"\r\n\r\n")
                    else {
                        continue;
                    };
                    let header =
                        String::from_utf8_lossy(&received[..header_end]).to_ascii_lowercase();
                    let length = header
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length: "))
                        .unwrap()
                        .parse::<usize>()
                        .unwrap();
                    if received.len() >= header_end + 4 + length {
                        break;
                    }
                }
                let header_end = received
                    .windows(4)
                    .position(|part| part == b"\r\n\r\n")
                    .unwrap();
                let payload: Value = serde_json::from_slice(&received[header_end + 4..]).unwrap();
                let system = payload["messages"][0]["content"].as_str().unwrap_or("");
                let user = payload["messages"][1]["content"].as_str().unwrap_or("");
                let content = if system.contains("name: curriculum") {
                    curriculum_count += 1;
                    if curriculum_count == 5 {
                        json!({"action":"finish","difficulty":"hard","reason":"coverage complete"})
                    } else {
                        let difficulty = ["easy", "medium", "hard", "hard"][curriculum_count - 1];
                        json!({"action":"draft_exercise","difficulty":difficulty,"reason":"next gap"})
                    }
                } else if user.contains("Concept Explainer agent") {
                    json!({"topic_title":"Python 变量","explanation":"Python 变量与打印。","concept":{"title":"变量","language":"Python","summary":"变量保存值。","key_points":["赋值", "打印"]}})
                } else if user.contains("Exercise Writer agent") {
                    let slot = (1..=4)
                        .find(|slot| user.contains(&format!("slot {slot}.")))
                        .unwrap();
                    let difficulty = ["easy", "medium", "hard", "hard"][slot - 1];
                    json!({"kind":"FillBlank","title":format!("题目 {slot}"),"prompt":"打印变量","starter_code":"x = 1\nprint(x)","expected_answer":"1","hints":[],"difficulty":difficulty})
                } else {
                    json!({"verdict":"accepted","checked_claims":[],"blocking_issues":[],"risk_notes":[]})
                };
                let body = json!({"choices":[{"message":{"content":content.to_string()},"finish_reason":"stop"}]}).to_string();
                write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
                request_count += 1;
            }
            (request_count, curriculum_count)
        });
        let mut settings = ApiSettings::default();
        settings.base_url = format!("http://{address}/v1");
        settings.api_key = "test-key".to_string();
        settings.selected_model = "mock-model".to_string();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let result = runtime
            .block_on(AiClient::default().explain_and_generate(&settings, "Python 变量", None))
            .unwrap();
        let (requests, decisions) = server.join().unwrap();
        assert_eq!(requests, 18);
        assert_eq!(decisions, 5);
        assert_eq!(result.exercises.len(), 4);
        assert!(result.exercises.iter().all(|exercise| exercise
            .verification
            .as_ref()
            .is_some_and(|v| v.status == "unverified")));
    }

    #[test]
    fn cancelling_an_inflight_model_request_returns_promptly() {
        use std::io::Read;
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut bytes = [0_u8; 1024];
            let _ = socket.read(&mut bytes);
            std::thread::sleep(Duration::from_millis(600));
        });
        let mut settings = ApiSettings::default();
        settings.base_url = format!("http://{address}/v1");
        settings.api_key = "test-key".to_string();
        settings.selected_model = "mock-model".to_string();
        let flag = Arc::new(AtomicBool::new(false));
        let trigger = flag.clone();
        let canceller = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            trigger.store(true, Ordering::Relaxed);
        });
        let started = std::time::Instant::now();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let result = runtime.block_on(AiClient::default().with_cancel(flag).chat_completion(
            "cancel-test",
            &settings,
            vec![ChatMessageWire::user("hello")],
            false,
        ));
        canceller.join().unwrap();
        server.join().unwrap();
        assert!(matches!(result, Err(AiError::Cancelled)));
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn sandbox_observation_blocks_a_conflicting_exercise() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let model_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let model_address = model_listener.local_addr().unwrap();
        let model_server = std::thread::spawn(move || {
            for index in 0..2 {
                let (mut socket, _) = model_listener.accept().unwrap();
                let mut bytes = [0_u8; 8192];
                let _ = socket.read(&mut bytes).unwrap();
                let content = if index == 0 {
                    json!({"runnable":true,"code":"print(43)","language":"py","expected_stdout":"42","reason":"check printed value"})
                } else {
                    json!({"verdict":"supported","reason":"the probe prints the value"})
                };
                let body = json!({"choices":[{"message":{"content":content.to_string()},"finish_reason":"stop"}]}).to_string();
                write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
            }
        });
        let sandbox_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let sandbox_address = sandbox_listener.local_addr().unwrap();
        let sandbox_server = std::thread::spawn(move || {
            let (mut socket, _) = sandbox_listener.accept().unwrap();
            let mut bytes = [0_u8; 4096];
            let _ = socket.read(&mut bytes).unwrap();
            let body = "{\"session_id\":\"s1\",\"stdout\":\"43\\n\",\"stderr\":\"\",\"files\":[]}";
            write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
        });
        let mut settings = ApiSettings::default();
        settings.base_url = format!("http://{model_address}/v1");
        settings.api_key = "test-key".to_string();
        settings.selected_model = "mock-model".to_string();
        settings.sandbox_base_url = format!("http://{sandbox_address}");
        let exercise = ExerciseWire {
            kind: ExerciseKind::PredictCompileResult,
            title: "输出".to_string(),
            prompt: "预测输出".to_string(),
            starter_code: "print(43)".to_string(),
            expected_answer: "42".to_string(),
            hints: Vec::new(),
            difficulty: "easy".to_string(),
            verification: None,
        };
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let evidence = runtime
            .block_on(AiClient::default().verify_exercise_in_sandbox(&settings, &exercise, None))
            .unwrap();
        model_server.join().unwrap();
        sandbox_server.join().unwrap();
        assert_eq!(evidence.status, "contradicted");
        assert_eq!(evidence.stdout, "43\n");
    }
}
