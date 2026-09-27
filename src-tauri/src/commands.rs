use crate::ai::client::{AiClient, ProgressReporter};
use crate::app::{ApiSettings, AppState, ChatMessage, ChatRole, TopicSession, TopicSort};
use crate::domain::exercise::Attempt;
use chrono::Utc;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

pub struct ManagedState(pub Mutex<AppState>, pub Arc<Mutex<Option<Arc<AtomicBool>>>>);

struct TaskToken {
    flag: Arc<AtomicBool>,
    running: Arc<Mutex<Option<Arc<AtomicBool>>>>,
}

impl Drop for TaskToken {
    fn drop(&mut self) {
        if let Ok(mut running) = self.running.lock() {
            *running = None;
        }
    }
}

fn start_task(state: &State<'_, ManagedState>) -> Result<TaskToken, String> {
    let mut running = state.1.lock().map_err(|e| e.to_string())?;
    if running.is_some() {
        return Err("已有生成任务正在运行".to_string());
    }
    let token = Arc::new(AtomicBool::new(false));
    *running = Some(token.clone());
    Ok(TaskToken {
        flag: token,
        running: state.1.clone(),
    })
}

#[tauri::command]
pub fn cancel_task(state: State<'_, ManagedState>) -> Result<bool, String> {
    let running = state.1.lock().map_err(|e| e.to_string())?;
    if let Some(token) = running.as_ref() {
        token.store(true, Ordering::Relaxed);
        Ok(true)
    } else {
        Ok(false)
    }
}

fn with_state(
    state: &State<'_, ManagedState>,
    update: impl FnOnce(&mut AppState) -> Result<(), String>,
) -> Result<AppState, String> {
    let mut app = state.0.lock().map_err(|e| e.to_string())?;
    update(&mut app)?;
    app.save().map_err(|e| format!("保存本地历史失败：{e}"))?;
    Ok(app.clone())
}

fn progress_reporter(app: AppHandle) -> ProgressReporter {
    Arc::new(move |message| {
        let _ = app.emit("task-progress", message);
    })
}

fn active_topic_mut(app: &mut AppState) -> Result<&mut TopicSession, String> {
    app.active_topic_mut()
        .ok_or_else(|| "当前没有可用话题".to_string())
}

#[tauri::command]
pub fn get_state(state: State<'_, ManagedState>) -> Result<AppState, String> {
    Ok(state.0.lock().map_err(|e| e.to_string())?.clone())
}

#[tauri::command]
pub fn get_storage_path() -> String {
    crate::app::storage_path()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "无法确定配置目录".to_string())
}

#[tauri::command]
pub fn list_skills() -> Vec<crate::skills::SkillInfo> {
    crate::skills::list()
}

#[tauri::command]
pub fn save_settings(
    state: State<'_, ManagedState>,
    mut settings: ApiSettings,
) -> Result<AppState, String> {
    settings.base_url = settings.base_url.trim().trim_end_matches('/').to_string();
    settings.bocha_api_key = settings.bocha_api_key.trim().to_string();
    settings.tavily_api_key = settings.tavily_api_key.trim().to_string();
    settings.tavily_base_url = settings
        .tavily_base_url
        .trim()
        .trim_end_matches('/')
        .to_string();
    if settings.tavily_base_url.is_empty() {
        settings.tavily_base_url = "https://api.tavily.com".to_string();
    }
    settings.jina_api_key = settings.jina_api_key.trim().to_string();
    settings.sandbox_base_url = settings
        .sandbox_base_url
        .trim()
        .trim_end_matches('/')
        .to_string();
    settings.sandbox_api_key = settings.sandbox_api_key.trim().to_string();
    with_state(&state, |app| {
        app.settings = settings;
        Ok(())
    })
}

#[tauri::command]
pub async fn fetch_models(
    state: State<'_, ManagedState>,
    settings: ApiSettings,
) -> Result<AppState, String> {
    let models = AiClient::default()
        .fetch_models(&settings)
        .await
        .map_err(|e| e.to_string())?;
    if models.is_empty() {
        return Err("模型列表为空，请检查 Base URL 或 API Key".to_string());
    }
    with_state(&state, |app| {
        app.settings = settings;
        app.settings.available_models = models;
        if !app
            .settings
            .available_models
            .contains(&app.settings.selected_model)
        {
            app.settings.selected_model = app.settings.available_models[0].clone();
        }
        Ok(())
    })
}

#[tauri::command]
pub fn new_topic(state: State<'_, ManagedState>) -> Result<AppState, String> {
    with_state(&state, |app| {
        let topic = TopicSession::new();
        app.active_topic_id = Some(topic.id);
        app.topics.insert(0, topic);
        Ok(())
    })
}

#[tauri::command]
pub fn switch_topic(state: State<'_, ManagedState>, topic_id: Uuid) -> Result<AppState, String> {
    with_state(&state, |app| {
        if !app.topics.iter().any(|topic| topic.id == topic_id) {
            return Err("找不到该话题".to_string());
        }
        app.active_topic_id = Some(topic_id);
        Ok(())
    })
}

#[tauri::command]
pub fn delete_topic(state: State<'_, ManagedState>, topic_id: Uuid) -> Result<AppState, String> {
    with_state(&state, |app| {
        app.topics.retain(|topic| topic.id != topic_id);
        if app.active_topic_id == Some(topic_id) {
            app.active_topic_id = app.topics.first().map(|topic| topic.id);
        }
        app.ensure_active_topic();
        Ok(())
    })
}

#[tauri::command]
pub fn rename_topic(
    state: State<'_, ManagedState>,
    topic_id: Uuid,
    title: String,
) -> Result<AppState, String> {
    let title = title.trim().to_string();
    if title.is_empty() {
        return Err("标题不能为空".to_string());
    }
    with_state(&state, |app| {
        let topic = app.topic_mut(topic_id).ok_or("找不到该话题")?;
        topic.title = title;
        topic.updated_at = Utc::now();
        Ok(())
    })
}

#[tauri::command]
pub fn set_topic_sort(state: State<'_, ManagedState>, sort: TopicSort) -> Result<AppState, String> {
    with_state(&state, |app| {
        app.topic_sort = sort;
        Ok(())
    })
}

#[tauri::command]
pub fn select_exercise(
    state: State<'_, ManagedState>,
    exercise_id: Uuid,
) -> Result<AppState, String> {
    with_state(&state, |app| {
        let topic = active_topic_mut(app)?;
        if !topic
            .exercises
            .iter()
            .any(|exercise| exercise.id == exercise_id)
        {
            return Err("找不到该练习".to_string());
        }
        topic.selected_exercise_id = Some(exercise_id);
        topic.updated_at = Utc::now();
        Ok(())
    })
}

#[tauri::command]
pub async fn generate(
    app_handle: AppHandle,
    state: State<'_, ManagedState>,
    topic_text: String,
) -> Result<AppState, String> {
    let topic_text = topic_text.trim().to_string();
    if topic_text.is_empty() {
        return Err("请输入想学习的知识点".to_string());
    }
    let cancel = start_task(&state)?;
    let (topic_id, settings) = {
        let mut app = state.0.lock().map_err(|e| e.to_string())?;
        app.ensure_active_topic();
        let settings = app.settings.clone();
        let topic = active_topic_mut(&mut app)?;
        if topic.title == "新话题" {
            topic.title = topic_text.chars().take(20).collect();
        }
        topic.messages.push(ChatMessage::user(topic_text.clone()));
        topic.updated_at = Utc::now();
        let id = topic.id;
        app.save().map_err(|e| e.to_string())?;
        (id, settings)
    };
    let result = AiClient::default()
        .with_cancel(cancel.flag.clone())
        .explain_and_generate(&settings, &topic_text, Some(progress_reporter(app_handle)))
        .await;
    drop(cancel);
    with_state(&state, |app| {
        let topic = app.topic_mut(topic_id).ok_or("话题已被删除")?;
        match result {
            Ok(response) => {
                if let Some(title) = response
                    .topic_title
                    .filter(|title| !title.trim().is_empty())
                {
                    topic.title = title;
                }
                topic.messages.push(ChatMessage::assistant(
                    response.explanation,
                    Some(response.raw_response),
                ));
                topic.concept = Some(response.concept);
                topic.exercises = response.exercises;
                topic.selected_exercise_id = topic.exercises.first().map(|exercise| exercise.id);
                topic.attempts.clear();
                topic.experiment_prompts.clear();
            }
            Err(error) => {
                topic
                    .messages
                    .push(ChatMessage::assistant(format!("请求失败：{error}"), None));
            }
        }
        topic.updated_at = Utc::now();
        Ok(())
    })
}

#[tauri::command]
pub async fn follow_up(
    state: State<'_, ManagedState>,
    question: String,
) -> Result<AppState, String> {
    let question = question.trim().to_string();
    if question.is_empty() {
        return Err("请输入追问内容".to_string());
    }
    let (topic_id, settings, messages, concept_json, exercises_json, attempts_json) = {
        let mut app = state.0.lock().map_err(|e| e.to_string())?;
        let settings = app.settings.clone();
        let topic = active_topic_mut(&mut app)?;
        topic.messages.push(ChatMessage::user(question));
        topic.updated_at = Utc::now();
        let data = (
            topic.id,
            settings,
            topic.messages.clone(),
            serde_json::to_string(&topic.concept).map_err(|e| e.to_string())?,
            serde_json::to_string(&topic.exercises).map_err(|e| e.to_string())?,
            serde_json::to_string(&topic.attempts).map_err(|e| e.to_string())?,
        );
        app.save().map_err(|e| e.to_string())?;
        data
    };
    let result = AiClient::default()
        .follow_up(
            &settings,
            &messages,
            &concept_json,
            &exercises_json,
            &attempts_json,
        )
        .await;
    with_state(&state, |app| {
        let topic = app.topic_mut(topic_id).ok_or("话题已被删除")?;
        match result {
            Ok(answer) => topic
                .messages
                .push(ChatMessage::assistant(answer.clone(), Some(answer))),
            Err(error) => topic
                .messages
                .push(ChatMessage::assistant(format!("请求失败：{error}"), None)),
        }
        topic.updated_at = Utc::now();
        Ok(())
    })
}

#[tauri::command]
pub async fn submit_answer(
    app_handle: AppHandle,
    state: State<'_, ManagedState>,
    answer: String,
) -> Result<AppState, String> {
    let answer = answer.trim().to_string();
    if answer.is_empty() {
        return Err("请输入答案后再提交".to_string());
    }
    let cancel = start_task(&state)?;
    let (topic_id, settings, exercise) = {
        let app = state.0.lock().map_err(|e| e.to_string())?;
        let topic = app.active_topic().ok_or("当前没有可用话题")?;
        (
            topic.id,
            app.settings.clone(),
            topic
                .selected_exercise()
                .cloned()
                .ok_or("当前没有选中的练习")?,
        )
    };
    let mut review = AiClient::default()
        .with_cancel(cancel.flag.clone())
        .review_attempt(
            &settings,
            &exercise,
            &answer,
            Some(progress_reporter(app_handle)),
        )
        .await
        .map_err(|e| e.to_string())?;
    let mut attempt = Attempt::new(exercise.id, answer);
    review.attempt_id = Some(attempt.id);
    attempt.review = Some(review);
    with_state(&state, |app| {
        let topic = app.topic_mut(topic_id).ok_or("话题已被删除")?;
        topic.attempts.push(attempt);
        topic.updated_at = Utc::now();
        Ok(())
    })
}

#[tauri::command]
pub async fn regenerate_exercises(
    app_handle: AppHandle,
    state: State<'_, ManagedState>,
) -> Result<AppState, String> {
    let (topic_id, settings, concept, explanation, previous) = {
        let app = state.0.lock().map_err(|e| e.to_string())?;
        let topic = app.active_topic().ok_or("当前没有可用话题")?;
        let concept = topic.concept.clone().ok_or("请先生成讲解")?;
        let explanation = topic
            .messages
            .iter()
            .rev()
            .find(|message| message.role == ChatRole::Assistant && message.raw_response.is_some())
            .map(|message| message.content.clone())
            .ok_or("当前话题缺少讲解上下文")?;
        (
            topic.id,
            app.settings.clone(),
            concept,
            explanation,
            topic.exercises.clone(),
        )
    };
    let cancel = start_task(&state)?;
    let result = AiClient::default()
        .with_cancel(cancel.flag.clone())
        .regenerate_exercises(
            &settings,
            &concept,
            &explanation,
            &previous,
            Some(progress_reporter(app_handle)),
        )
        .await;
    drop(cancel);
    let exercises = result.map_err(|e| e.to_string())?;
    with_state(&state, |app| {
        let topic = app.topic_mut(topic_id).ok_or("话题已被删除")?;
        topic.exercises = exercises;
        topic.selected_exercise_id = topic.exercises.first().map(|exercise| exercise.id);
        topic.attempts.clear();
        topic.experiment_prompts.clear();
        topic.updated_at = Utc::now();
        Ok(())
    })
}

#[tauri::command]
pub async fn request_experiment(state: State<'_, ManagedState>) -> Result<AppState, String> {
    let (topic_id, settings, concept, exercise) = {
        let app = state.0.lock().map_err(|e| e.to_string())?;
        let topic = app.active_topic().ok_or("当前没有可用话题")?;
        (
            topic.id,
            app.settings.clone(),
            topic.concept.clone().ok_or("请先生成知识点讲解")?,
            topic
                .selected_exercise()
                .cloned()
                .ok_or("请先选择一道练习")?,
        )
    };
    let prompt = AiClient::default()
        .generate_experiment_prompt(&settings, &concept, &exercise)
        .await
        .map_err(|e| e.to_string())?;
    with_state(&state, |app| {
        let topic = app.topic_mut(topic_id).ok_or("话题已被删除")?;
        topic.experiment_prompts.push(prompt);
        topic.updated_at = Utc::now();
        Ok(())
    })
}

#[tauri::command]
pub async fn execute_experiment(
    state: State<'_, ManagedState>,
    code: String,
    language: String,
) -> Result<AppState, String> {
    let (topic_id, settings, exercise_id) = {
        let app = state.0.lock().map_err(|e| e.to_string())?;
        let topic = app.active_topic().ok_or("当前没有可用话题")?;
        (
            topic.id,
            app.settings.clone(),
            topic.selected_exercise_id.ok_or("请先选择一道练习")?,
        )
    };
    let run = crate::sandbox::execute(&settings, exercise_id, code, language, None).await?;
    with_state(&state, |app| {
        let topic = app.topic_mut(topic_id).ok_or("话题已被删除")?;
        topic.experiment_runs.push(run);
        topic.updated_at = Utc::now();
        Ok(())
    })
}
