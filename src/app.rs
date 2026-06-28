use crate::ai::client::AiClient;
use crate::domain::concept::Concept;
use crate::domain::exercise::{Attempt, Exercise, ExperimentPrompt};
use crate::ui::chat_panel::ChatPanel;
use crate::ui::drill_panel::DrillPanel;
use chrono::{DateTime, Utc};
use dioxus::dioxus_core::spawn_forever;
use dioxus::prelude::*;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

pub(crate) static APP_STATE: GlobalSignal<AppState> = Signal::global(AppState::load);

const APP_CSS: &str = r#"
:root {
  color: #202225;
  background: #f3f4f1;
  font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
}

* { box-sizing: border-box; }

body { margin: 0; }
button, input, textarea, select {
  font: inherit;
}

button {
  border: 1px solid #cfd4d0;
  background: #f8faf8;
  color: #1f2924;
  border-radius: 7px;
  padding: 8px 12px;
  cursor: pointer;
}

button:hover { background: #eef3ef; }
button.primary { background: #23685a; color: white; border-color: #23685a; }
button.primary:hover { background: #1d574c; }
button.ghost { background: transparent; }
button:disabled { cursor: not-allowed; opacity: 0.56; }

textarea, input, select {
  width: 100%;
  border: 1px solid #ccd3ce;
  border-radius: 7px;
  background: white;
  color: #202225;
  padding: 10px;
  outline: none;
}

textarea:focus, input:focus, select:focus {
  border-color: #23685a;
  box-shadow: 0 0 0 3px rgba(35, 104, 90, 0.13);
}

.app-shell {
  height: 100vh;
  display: flex;
  flex-direction: column;
  background: #f3f4f1;
}

.topbar {
  height: 54px;
  border-bottom: 1px solid #d9ded9;
  background: #fbfcfa;
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 16px;
}

.brand {
  display: flex;
  gap: 10px;
  align-items: baseline;
}

.brand h1 {
  margin: 0;
  font-size: 18px;
  font-weight: 720;
}

.brand span { color: #69726c; font-size: 13px; }
.top-actions { display: flex; gap: 8px; align-items: center; }

.workspace {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: minmax(300px, 42%) minmax(340px, 58%);
}

.chat-panel, .drill-panel {
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.chat-panel {
  border-right: 1px solid #d9ded9;
  background: #fbfcfa;
}

.panel-header {
  padding: 14px;
  border-bottom: 1px solid #e2e5e1;
  display: grid;
  gap: 8px;
}

.topic-list {
  max-height: 170px;
  overflow: auto;
  border-bottom: 1px solid #e2e5e1;
  padding: 8px;
}

.topic-item {
  width: 100%;
  margin-bottom: 6px;
  display: grid;
  grid-template-columns: 1fr auto auto;
  gap: 6px;
  align-items: stretch;
}

.topic-open {
  text-align: left;
  display: flex;
  flex-direction: column;
  gap: 3px;
}

.topic-open.selected {
  background: #dfece7;
  border-color: #9dbeb4;
}

.topic-title { font-weight: 650; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.topic-meta { color: #68736e; font-size: 12px; }
.topic-tools { display: flex; gap: 6px; }
.topic-tool { min-width: 36px; padding: 8px; }
.topic-rename {
  display: grid;
  grid-template-columns: 1fr auto auto;
  gap: 6px;
}

.messages {
  flex: 1;
  min-height: 0;
  overflow: auto;
  padding: 14px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.message {
  max-width: 92%;
  padding: 11px 12px;
  border-radius: 8px;
  line-height: 1.45;
  white-space: pre-wrap;
}

.message.user {
  align-self: flex-end;
  background: #23685a;
  color: white;
}

.message.assistant {
  align-self: flex-start;
  background: #eef1ee;
  color: #202225;
}

.message.assistant.rich-message {
  max-width: 96%;
  white-space: normal;
}

.rich-message h3 {
  margin: 8px 0 6px;
  font-size: 15px;
}

.rich-message h3:first-child { margin-top: 0; }
.rich-message p { margin: 6px 0; }
.rich-message strong { font-weight: 720; }
.rich-message em { font-style: italic; }
.rich-message del { color: #65716a; }
.rich-message a { color: #1f6f61; font-weight: 650; text-decoration: underline; }
.rich-message ul, .rich-message ol { margin: 6px 0; padding-left: 20px; }
.rich-message li { margin: 3px 0; }
.rich-message li > p { margin: 2px 0; }
.rich-message input[type="checkbox"] { margin-right: 6px; }
.rich-message blockquote {
  margin: 8px 0;
  padding: 4px 0 4px 10px;
  border-left: 3px solid #9dbeb4;
  color: #4c5752;
}
.rich-message hr {
  border: 0;
  border-top: 1px solid #cfd7d2;
  margin: 10px 0;
}

.rich-message code {
  font-family: "SFMono-Regular", Consolas, "Liberation Mono", monospace;
  font-size: 0.92em;
  background: #dde5df;
  border-radius: 4px;
  padding: 1px 4px;
}

.rich-message .code-block {
  margin: 8px 0;
}

.rich-message .code-language {
  display: inline-block;
  margin-bottom: 4px;
  color: #647069;
  font-size: 12px;
  font-weight: 650;
}

.rich-message pre {
  margin: 0;
  background: #202723;
  color: #eef8f2;
  border-radius: 7px;
  padding: 10px;
  overflow: auto;
  white-space: pre;
}

.rich-message pre code {
  display: block;
  min-width: max-content;
  background: transparent;
  border-radius: 0;
  padding: 0;
  color: inherit;
}

.rich-message .table-scroll {
  margin: 8px 0;
  overflow-x: auto;
}

.rich-message table {
  width: max-content;
  min-width: 100%;
  border-collapse: collapse;
  font-size: 13px;
}

.rich-message th, .rich-message td {
  border: 1px solid #cbd4ce;
  padding: 6px 8px;
  text-align: left;
  vertical-align: top;
}

.rich-message thead td {
  background: #dfe8e2;
  font-weight: 720;
}

.composer {
  border-top: 1px solid #e2e5e1;
  padding: 12px;
  display: grid;
  gap: 8px;
}

.composer textarea { min-height: 86px; resize: vertical; }
.composer-row { display: flex; gap: 8px; justify-content: flex-end; align-items: center; }

.drill-panel {
  padding: 16px;
  overflow: auto;
  gap: 14px;
}

.status-line {
  border-bottom: 1px solid #e2e5e1;
  color: #5e6a65;
  font-size: 13px;
  padding: 9px 16px;
  background: #fbfcfa;
}

.status-line.error { color: #8c1d18; background: #fff4f2; }

.section {
  border-bottom: 1px solid #e2e5e1;
  padding-bottom: 14px;
}

.section h2, .section h3 {
  margin: 0 0 8px;
}

.section h2 { font-size: 18px; }
.section h3 { font-size: 15px; }
.muted { color: #69726c; }

.key-points {
  margin: 8px 0 0;
  padding-left: 20px;
}

.exercise-tabs {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.exercise-tab.selected {
  background: #243d35;
  color: white;
  border-color: #243d35;
}

.code-block {
  background: #202723;
  color: #eef8f2;
  border-radius: 8px;
  padding: 12px;
  overflow: auto;
  white-space: pre-wrap;
}

.answer-box { min-height: 150px; resize: vertical; }
.code-input {
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", monospace;
  font-size: 13px;
  line-height: 1.5;
  tab-size: 2;
  white-space: pre;
  overflow: auto;
  font-variant-ligatures: none;
}
.review-box, .experiment-box {
  background: #fbfcfa;
  border: 1px solid #d9ded9;
  border-radius: 8px;
  padding: 12px;
}

.settings-page {
  max-width: 880px;
  width: 100%;
  margin: 0 auto;
  padding: 24px;
  display: grid;
  gap: 16px;
}

.settings-card {
  background: #fbfcfa;
  border: 1px solid #d9ded9;
  border-radius: 8px;
  padding: 16px;
  display: grid;
  gap: 12px;
}

.warning {
  border: 1px solid #e2b95c;
  background: #fff8df;
  color: #5d4700;
  border-radius: 8px;
  padding: 12px;
}

.form-grid {
  display: grid;
  grid-template-columns: 170px 1fr;
  gap: 12px;
  align-items: center;
}

.form-actions { display: flex; gap: 8px; justify-content: flex-end; }

@media (max-width: 860px) {
  .form-grid { grid-template-columns: 1fr; }
}
"#;

const APP_JS: &str = r#"
document.addEventListener("keydown", function(event) {
  var target = event.target;
  if (!target || !target.classList || !target.classList.contains("code-input")) {
    return;
  }

  if (event.key !== "Tab") {
    return;
  }

  event.preventDefault();
  var start = target.selectionStart || 0;
  var end = target.selectionEnd || 0;
  var value = target.value || "";
  target.value = value.slice(0, start) + "  " + value.slice(end);
  target.selectionStart = target.selectionEnd = start + 2;
  target.dispatchEvent(new Event("input", { bubbles: true }));
});
"#;

#[allow(non_snake_case)]
pub fn App() -> Element {
    let snapshot = APP_STATE.read().clone();

    rsx! {
        style { "{APP_CSS}" }
        script { "{APP_JS}" }
        div { class: "app-shell",
            div { class: "topbar",
                div { class: "brand",
                    h1 { "Learning Drill Lab" }
                    span { "编程语言结构记忆训练" }
                }
                div { class: "top-actions",
                    button {
                        class: "ghost",
                        onclick: move |_| set_view(AppView::Workspace),
                        "工作台"
                    }
                    button {
                        class: "ghost",
                        onclick: move |_| set_view(AppView::Settings),
                        "设置"
                    }
                }
            }

            if let Some(error) = &snapshot.error {
                div { class: "status-line error", "{error}" }
            } else if let Some(loading) = &snapshot.loading {
                div { class: "status-line", "{loading}" }
            } else if let Some(status) = &snapshot.status {
                div { class: "status-line", "{status}" }
            }

            if snapshot.view == AppView::Settings {
                SettingsPage {}
            } else {
                div { class: "workspace",
                    ChatPanel {}
                    DrillPanel {}
                }
            }
        }
    }
}

#[component]
#[allow(non_snake_case)]
fn SettingsPage() -> Element {
    let snapshot = APP_STATE.read().clone();
    let selected_model = snapshot.settings.selected_model.clone();

    rsx! {
        div { class: "settings-page",
            div { class: "settings-card",
                h2 { "API 设置" }
                div { class: "warning",
                    "当前为开发版本：API Key 会以明文存储在本地配置文件中，不会上传云端。请只在可信设备上使用。"
                }
                div { class: "form-grid",
                    label { "Base URL" }
                    input {
                        value: "{snapshot.settings.base_url}",
                        placeholder: "https://api.openai.com/v1",
                        oninput: move |event| update_base_url(event.value())
                    }
                    label { "API Key" }
                    input {
                        r#type: "password",
                        value: "{snapshot.settings.api_key}",
                        placeholder: "sk-...",
                        oninput: move |event| update_api_key(event.value())
                    }
                    label { "模型" }
                    select {
                        value: "{selected_model}",
                        onchange: move |event| select_model(event.value()),
                        if snapshot.settings.available_models.is_empty() {
                            option { value: "", "请先拉取模型列表" }
                        }
                        for model in snapshot.settings.available_models.iter() {
                            option {
                                value: "{model}",
                                selected: *model == selected_model,
                                "{model}"
                            }
                        }
                    }
                }
                div { class: "form-actions",
                    button {
                        onclick: move |_| fetch_models(),
                        disabled: snapshot.is_busy(),
                        "拉取模型列表"
                    }
                    button {
                        class: "primary",
                        onclick: move |_| save_settings(),
                        "保存设置"
                    }
                }
                p { class: "muted",
                    "Base URL 请填写包含版本路径的地址，例如 https://api.openai.com/v1。模型列表通过 GET /models 获取。"
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiSettings {
    pub base_url: String,
    pub api_key: String,
    pub selected_model: String,
    pub available_models: Vec<String>,
}

impl Default for ApiSettings {
    fn default() -> Self {
        Self {
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: String::new(),
            selected_model: String::new(),
            available_models: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppView {
    #[default]
    Workspace,
    Settings,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum TopicSort {
    #[default]
    UpdatedDesc,
    CreatedDesc,
    TitleAsc,
}

impl TopicSort {
    pub fn label(&self) -> &'static str {
        match self {
            Self::UpdatedDesc => "最近更新",
            Self::CreatedDesc => "最近创建",
            Self::TitleAsc => "标题 A-Z",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ChatRole {
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatMessage {
    pub id: Uuid,
    pub role: ChatRole,
    pub content: String,
    pub raw_response: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl ChatMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            role: ChatRole::User,
            content: content.into(),
            raw_response: None,
            created_at: Utc::now(),
        }
    }

    pub fn assistant(content: impl Into<String>, raw_response: Option<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            role: ChatRole::Assistant,
            content: content.into(),
            raw_response,
            created_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TopicSession {
    pub id: Uuid,
    pub title: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub messages: Vec<ChatMessage>,
    pub concept: Option<Concept>,
    pub exercises: Vec<Exercise>,
    pub selected_exercise_id: Option<Uuid>,
    pub attempts: Vec<Attempt>,
    pub experiment_prompts: Vec<ExperimentPrompt>,
}

impl TopicSession {
    pub fn new() -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            title: "新话题".to_string(),
            created_at: now,
            updated_at: now,
            messages: Vec::new(),
            concept: None,
            exercises: Vec::new(),
            selected_exercise_id: None,
            attempts: Vec::new(),
            experiment_prompts: Vec::new(),
        }
    }

    pub fn selected_exercise(&self) -> Option<&Exercise> {
        let selected = self.selected_exercise_id?;
        self.exercises
            .iter()
            .find(|exercise| exercise.id == selected)
    }

    pub fn latest_review_for_selected(&self) -> Option<&crate::domain::review::ReviewResult> {
        let exercise_id = self.selected_exercise_id?;
        self.attempts
            .iter()
            .rev()
            .find(|attempt| attempt.exercise_id == exercise_id)
            .and_then(|attempt| attempt.review.as_ref())
    }

    pub fn latest_experiment_prompt(&self) -> Option<&ExperimentPrompt> {
        self.experiment_prompts.last()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppState {
    pub settings: ApiSettings,
    pub topics: Vec<TopicSession>,
    pub active_topic_id: Option<Uuid>,
    #[serde(default)]
    pub topic_sort: TopicSort,
    #[serde(skip)]
    pub view: AppView,
    #[serde(skip)]
    pub renaming_topic_id: Option<Uuid>,
    #[serde(skip)]
    pub rename_input: String,
    #[serde(skip)]
    pub chat_input: String,
    #[serde(skip)]
    pub answer_input: String,
    #[serde(skip)]
    pub loading: Option<String>,
    #[serde(skip)]
    pub error: Option<String>,
    #[serde(skip)]
    pub status: Option<String>,
}

impl Default for AppState {
    fn default() -> Self {
        let mut state = Self {
            settings: ApiSettings::default(),
            topics: Vec::new(),
            active_topic_id: None,
            topic_sort: TopicSort::UpdatedDesc,
            view: AppView::Workspace,
            renaming_topic_id: None,
            rename_input: String::new(),
            chat_input: String::new(),
            answer_input: String::new(),
            loading: None,
            error: None,
            status: None,
        };
        state.ensure_active_topic();
        state
    }
}

impl AppState {
    pub fn load() -> Self {
        let Some(path) = storage_path() else {
            return Self::default();
        };

        let Ok(raw) = fs::read_to_string(path) else {
            return Self::default();
        };

        let mut state = serde_json::from_str::<Self>(&raw).unwrap_or_default();
        state.view = AppView::Workspace;
        state.renaming_topic_id = None;
        state.rename_input = String::new();
        state.chat_input = String::new();
        state.answer_input = String::new();
        state.loading = None;
        state.error = None;
        state.status = Some("已加载本地历史记录".to_string());
        state.ensure_active_topic();
        state
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let Some(path) = storage_path() else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn is_busy(&self) -> bool {
        self.loading.is_some()
    }

    pub fn active_topic(&self) -> Option<&TopicSession> {
        let id = self.active_topic_id?;
        self.topics.iter().find(|topic| topic.id == id)
    }

    pub fn active_topic_mut(&mut self) -> Option<&mut TopicSession> {
        let id = self.active_topic_id?;
        self.topics.iter_mut().find(|topic| topic.id == id)
    }

    pub fn topic_mut(&mut self, topic_id: Uuid) -> Option<&mut TopicSession> {
        self.topics.iter_mut().find(|topic| topic.id == topic_id)
    }

    pub fn sorted_topics(&self) -> Vec<TopicSession> {
        let mut topics = self.topics.clone();
        match self.topic_sort {
            TopicSort::UpdatedDesc => topics.sort_by(|a, b| b.updated_at.cmp(&a.updated_at)),
            TopicSort::CreatedDesc => topics.sort_by(|a, b| b.created_at.cmp(&a.created_at)),
            TopicSort::TitleAsc => {
                topics.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
            }
        }
        topics
    }

    pub fn ensure_active_topic(&mut self) {
        if self.topics.is_empty() {
            let topic = TopicSession::new();
            self.active_topic_id = Some(topic.id);
            self.topics.push(topic);
        } else if self.active_topic_id.is_none() {
            self.active_topic_id = self.topics.first().map(|topic| topic.id);
        }
    }
}

pub fn set_view(view: AppView) {
    APP_STATE.write().view = view;
}

pub fn update_chat_input(value: String) {
    APP_STATE.write().chat_input = value;
}

pub fn update_answer_input(value: String) {
    APP_STATE.write().answer_input = value;
}

pub fn update_base_url(value: String) {
    APP_STATE.write().settings.base_url = value;
}

pub fn update_api_key(value: String) {
    APP_STATE.write().settings.api_key = value;
}

pub fn select_model(value: String) {
    {
        let mut app = APP_STATE.write();
        app.settings.selected_model = value;
        app.status = Some("已选择模型".to_string());
        app.error = None;
    }
    persist_signal();
}

pub fn set_topic_sort(sort: TopicSort) {
    {
        let mut app = APP_STATE.write();
        app.topic_sort = sort;
        app.status = Some(format!("历史话题已按{}排序", sort.label()));
        app.error = None;
    }
    persist_signal();
}

pub fn save_settings() {
    {
        let mut app = APP_STATE.write();
        app.settings.base_url = app
            .settings
            .base_url
            .trim()
            .trim_end_matches('/')
            .to_string();
        app.status = Some("设置已保存到本地".to_string());
        app.error = None;
    }
    persist_signal();
}

pub fn fetch_models() {
    let settings = {
        let mut app = APP_STATE.write();
        app.settings.base_url = app
            .settings
            .base_url
            .trim()
            .trim_end_matches('/')
            .to_string();
        app.loading = Some("正在拉取模型列表...".to_string());
        app.error = None;
        app.status = None;
        app.settings.clone()
    };

    spawn_forever(async move {
        let result = AiClient::default().fetch_models(&settings).await;
        match result {
            Ok(models) => {
                let mut app = APP_STATE.write();
                app.loading = None;
                if models.is_empty() {
                    app.error = Some("模型列表为空，请检查 Base URL 或 API Key".to_string());
                } else {
                    app.settings.available_models = models;
                    if app.settings.selected_model.is_empty()
                        || !app
                            .settings
                            .available_models
                            .contains(&app.settings.selected_model)
                    {
                        app.settings.selected_model = app.settings.available_models[0].clone();
                    }
                    app.status = Some(format!(
                        "已拉取 {} 个模型",
                        app.settings.available_models.len()
                    ));
                }
            }
            Err(error) => {
                let mut app = APP_STATE.write();
                app.loading = None;
                app.error = Some(error.to_string());
            }
        }
        persist_signal();
    });
}

pub fn new_topic() {
    {
        let mut app = APP_STATE.write();
        let topic = TopicSession::new();
        app.active_topic_id = Some(topic.id);
        app.topics.insert(0, topic);
        app.chat_input.clear();
        app.answer_input.clear();
        app.renaming_topic_id = None;
        app.rename_input.clear();
        app.error = None;
        app.status = Some("已创建新话题".to_string());
    }
    persist_signal();
}

pub fn switch_topic(topic_id: Uuid) {
    {
        let mut app = APP_STATE.write();
        app.active_topic_id = Some(topic_id);
        app.chat_input.clear();
        app.answer_input.clear();
        app.renaming_topic_id = None;
        app.rename_input.clear();
        app.error = None;
        app.status = Some("已切换历史话题".to_string());
    }
    persist_signal();
}

pub fn delete_topic(topic_id: Uuid) {
    {
        let mut app = APP_STATE.write();
        app.topics.retain(|topic| topic.id != topic_id);
        if app.active_topic_id == Some(topic_id) {
            app.active_topic_id = app.topics.first().map(|topic| topic.id);
        }
        app.renaming_topic_id = None;
        app.rename_input.clear();
        app.ensure_active_topic();
        app.chat_input.clear();
        app.answer_input.clear();
        app.error = None;
        app.status = Some("已删除历史话题".to_string());
    }
    persist_signal();
}

pub fn begin_rename_topic(topic_id: Uuid) {
    let title = {
        let app = APP_STATE.read();
        app.topics
            .iter()
            .find(|topic| topic.id == topic_id)
            .map(|topic| topic.title.clone())
    };

    if let Some(title) = title {
        let mut app = APP_STATE.write();
        app.renaming_topic_id = Some(topic_id);
        app.rename_input = title;
        app.error = None;
    }
}

pub fn update_rename_input(value: String) {
    APP_STATE.write().rename_input = value;
}

pub fn cancel_rename_topic() {
    let mut app = APP_STATE.write();
    app.renaming_topic_id = None;
    app.rename_input.clear();
}

pub fn commit_rename_topic() {
    {
        let mut app = APP_STATE.write();
        let Some(topic_id) = app.renaming_topic_id else {
            return;
        };
        let title = app.rename_input.trim().to_string();
        if title.is_empty() {
            app.error = Some("标题不能为空".to_string());
            return;
        }
        if let Some(topic) = app.topic_mut(topic_id) {
            topic.title = title;
            topic.updated_at = Utc::now();
        }
        app.renaming_topic_id = None;
        app.rename_input.clear();
        app.error = None;
        app.status = Some("话题已重命名".to_string());
    }
    persist_signal();
}

pub fn select_exercise(exercise_id: Uuid) {
    {
        let mut app = APP_STATE.write();
        if let Some(topic) = app.active_topic_mut() {
            topic.selected_exercise_id = Some(exercise_id);
            topic.updated_at = Utc::now();
        }
        app.answer_input.clear();
        app.error = None;
    }
    persist_signal();
}

pub fn generate_from_chat_input() {
    start_learning_generation();
}

pub fn follow_up_from_chat_input() {
    send_follow_up_request();
}

pub fn regenerate_from_topic() {
    let (topic_id, settings, concept, explanation_context, previous_exercises) = {
        let mut app = APP_STATE.write();
        let settings = app.settings.clone();
        let Some(topic) = app.active_topic() else {
            app.error = Some("当前没有可用话题".to_string());
            return;
        };
        let Some(concept) = topic.concept.clone() else {
            app.error = Some("请先生成讲解，再重新生成练习".to_string());
            return;
        };
        let explanation_context = explanation_context_for_topic(topic);
        if explanation_context.trim().is_empty() {
            app.error = Some("当前话题缺少可用于出题的讲解上下文".to_string());
            return;
        }
        let topic_id = topic.id;
        let previous_exercises = topic.exercises.clone();

        app.answer_input.clear();
        app.loading = Some("AI 正在基于当前讲解分步生成并审查练习...".to_string());
        app.error = None;
        app.status = None;

        (
            topic_id,
            settings,
            concept,
            explanation_context,
            previous_exercises,
        )
    };
    persist_signal();

    spawn_forever(async move {
        let result = AiClient::default()
            .regenerate_exercises(
                &settings,
                &concept,
                &explanation_context,
                &previous_exercises,
            )
            .await;
        match result {
            Ok(exercises) => {
                let mut app = APP_STATE.write();
                if let Some(topic) = app.topic_mut(topic_id) {
                    topic.exercises = exercises;
                    topic.selected_exercise_id =
                        topic.exercises.first().map(|exercise| exercise.id);
                    topic.attempts.clear();
                    topic.experiment_prompts.clear();
                    topic.updated_at = Utc::now();
                }
                app.loading = None;
                app.status = Some("已基于当前讲解重新生成练习".to_string());
                app.error = None;
            }
            Err(error) => {
                let mut app = APP_STATE.write();
                app.loading = None;
                app.error = Some(error.to_string());
            }
        }
        persist_signal();
    });
}

fn start_learning_generation() {
    let (topic_id, topic_text, settings) = {
        let mut app = APP_STATE.write();
        app.ensure_active_topic();

        let topic_text = app.chat_input.trim().to_string();

        if topic_text.trim().is_empty() {
            app.error = Some("请输入想学习的知识点".to_string());
            return;
        }

        let active_topic_id = app.active_topic_id.expect("active topic exists");
        let settings = app.settings.clone();
        let topic = app.active_topic_mut().expect("active topic exists");
        if topic.title == "新话题" {
            topic.title = "总结标题中...".to_string();
        }
        topic.messages.push(ChatMessage::user(topic_text.clone()));
        topic.updated_at = Utc::now();

        app.chat_input.clear();
        app.answer_input.clear();
        app.loading = Some("AI 正在分步讲解、生成并审查练习...".to_string());
        app.error = None;
        app.status = None;
        (active_topic_id, topic_text, settings)
    };
    persist_signal();

    spawn_forever(async move {
        let result = AiClient::default()
            .explain_and_generate(&settings, &topic_text)
            .await;
        match result {
            Ok(response) => {
                let mut app = APP_STATE.write();
                if let Some(topic) = app.topic_mut(topic_id) {
                    if let Some(title) =
                        normalized_title(response.topic_title.as_deref(), &response.concept.title)
                    {
                        topic.title = title;
                    }
                    topic.messages.push(ChatMessage::assistant(
                        response.explanation.clone(),
                        Some(response.raw_response),
                    ));
                    topic.concept = Some(response.concept);
                    topic.exercises = response.exercises;
                    topic.selected_exercise_id =
                        topic.exercises.first().map(|exercise| exercise.id);
                    topic.attempts.clear();
                    topic.experiment_prompts.clear();
                    topic.updated_at = Utc::now();
                }
                app.loading = None;
                app.status = Some("已生成讲解和练习".to_string());
                app.error = None;
            }
            Err(error) => {
                let mut app = APP_STATE.write();
                app.loading = None;
                app.error = Some(error.to_string());
                if let Some(topic) = app.topic_mut(topic_id) {
                    if topic.title == "总结标题中..." {
                        topic.title = title_from_input(&topic_text);
                    }
                    topic
                        .messages
                        .push(ChatMessage::assistant(format!("请求失败：{error}"), None));
                    topic.updated_at = Utc::now();
                }
            }
        }
        persist_signal();
    });
}

fn explanation_context_for_topic(topic: &TopicSession) -> String {
    topic
        .messages
        .iter()
        .filter(|message| {
            message.role == ChatRole::Assistant && !message.content.starts_with("请求失败：")
        })
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>()
        .join("\n\n---\n\n")
}

fn send_follow_up_request() {
    let (topic_id, settings, messages, concept_json, exercises_json, attempts_json) = {
        let mut app = APP_STATE.write();
        let question = app.chat_input.trim().to_string();
        if question.is_empty() {
            app.error = Some("请输入追问内容".to_string());
            return;
        }

        let settings = app.settings.clone();
        let Some(topic) = app.active_topic_mut() else {
            app.error = Some("当前没有可用话题".to_string());
            return;
        };
        let topic_id = topic.id;
        topic.messages.push(ChatMessage::user(question));
        topic.updated_at = Utc::now();
        let messages = topic.messages.clone();
        let concept_json =
            serde_json::to_string_pretty(&topic.concept).unwrap_or_else(|_| "null".to_string());
        let exercises_json =
            serde_json::to_string_pretty(&topic.exercises).unwrap_or_else(|_| "[]".to_string());
        let attempts_json =
            serde_json::to_string_pretty(&topic.attempts).unwrap_or_else(|_| "[]".to_string());

        app.chat_input.clear();
        app.loading = Some("AI 正在回复追问...".to_string());
        app.error = None;
        app.status = None;

        (
            topic_id,
            settings,
            messages,
            concept_json,
            exercises_json,
            attempts_json,
        )
    };
    persist_signal();

    spawn_forever(async move {
        let result = AiClient::default()
            .follow_up(
                &settings,
                &messages,
                &concept_json,
                &exercises_json,
                &attempts_json,
            )
            .await;
        match result {
            Ok(answer) => {
                let mut app = APP_STATE.write();
                if let Some(topic) = app.topic_mut(topic_id) {
                    topic
                        .messages
                        .push(ChatMessage::assistant(answer.clone(), Some(answer)));
                    topic.updated_at = Utc::now();
                }
                app.loading = None;
                app.status = Some("已回复追问".to_string());
                app.error = None;
            }
            Err(error) => {
                let mut app = APP_STATE.write();
                app.loading = None;
                app.error = Some(error.to_string());
                if let Some(topic) = app.topic_mut(topic_id) {
                    topic
                        .messages
                        .push(ChatMessage::assistant(format!("请求失败：{error}"), None));
                    topic.updated_at = Utc::now();
                }
            }
        }
        persist_signal();
    });
}

pub fn submit_answer() {
    let (topic_id, settings, exercise, answer) = {
        let mut app = APP_STATE.write();
        let answer = app.answer_input.trim().to_string();
        if answer.is_empty() {
            app.error = Some("请输入答案后再提交".to_string());
            return;
        }
        let Some(topic) = app.active_topic() else {
            app.error = Some("当前没有可用话题".to_string());
            return;
        };
        let topic_id = topic.id;
        let Some(exercise) = topic.selected_exercise().cloned() else {
            app.error = Some("当前没有选中的练习".to_string());
            return;
        };
        app.loading = Some("AI 正在 review 答案...".to_string());
        app.error = None;
        app.status = None;
        (topic_id, app.settings.clone(), exercise, answer)
    };

    spawn_forever(async move {
        let result = AiClient::default()
            .review_attempt(&settings, &exercise, &answer)
            .await;
        match result {
            Ok(mut review) => {
                let mut attempt = Attempt::new(exercise.id, answer);
                review.attempt_id = Some(attempt.id);
                attempt.review = Some(review);

                let mut app = APP_STATE.write();
                if let Some(topic) = app.topic_mut(topic_id) {
                    topic.attempts.push(attempt);
                    topic.updated_at = Utc::now();
                }
                app.answer_input.clear();
                app.loading = None;
                app.status = Some("Review 已完成".to_string());
                app.error = None;
            }
            Err(error) => {
                let mut app = APP_STATE.write();
                app.loading = None;
                app.error = Some(error.to_string());
            }
        }
        persist_signal();
    });
}

pub fn request_experiment() {
    let (topic_id, settings, concept, exercise) = {
        let mut app = APP_STATE.write();
        let Some(topic) = app.active_topic() else {
            app.error = Some("当前没有可用话题".to_string());
            return;
        };
        let topic_id = topic.id;
        let Some(concept) = topic.concept.clone() else {
            app.error = Some("请先生成知识点讲解".to_string());
            return;
        };
        let Some(exercise) = topic.selected_exercise().cloned() else {
            app.error = Some("请先选择一道练习".to_string());
            return;
        };
        app.loading = Some("AI 正在生成实验 prompt...".to_string());
        app.error = None;
        app.status = None;
        (topic_id, app.settings.clone(), concept, exercise)
    };

    spawn_forever(async move {
        let result = AiClient::default()
            .generate_experiment_prompt(&settings, &concept, &exercise)
            .await;
        match result {
            Ok(prompt) => {
                let mut app = APP_STATE.write();
                if let Some(topic) = app.topic_mut(topic_id) {
                    topic.experiment_prompts.push(prompt);
                    topic.updated_at = Utc::now();
                }
                app.loading = None;
                app.status = Some("实验 prompt 已生成".to_string());
                app.error = None;
            }
            Err(error) => {
                let mut app = APP_STATE.write();
                app.loading = None;
                app.error = Some(error.to_string());
            }
        }
        persist_signal();
    });
}

pub fn regenerate_exercises() {
    regenerate_from_topic();
}

pub fn persist_signal() {
    let snapshot = APP_STATE.read().clone();
    if let Err(error) = snapshot.save() {
        APP_STATE.write().error = Some(format!("保存本地历史失败：{error}"));
    }
}

fn storage_path() -> Option<PathBuf> {
    ProjectDirs::from("dev", "LearningDrillLab", "LearningDrillLab")
        .map(|dirs| dirs.config_dir().join("state.json"))
}

fn normalized_title(ai_title: Option<&str>, concept_title: &str) -> Option<String> {
    let source = ai_title
        .filter(|title| !title.trim().is_empty())
        .unwrap_or(concept_title);
    let cleaned = source
        .trim()
        .trim_matches('"')
        .trim_matches('“')
        .trim_matches('”')
        .trim();

    if cleaned.is_empty() {
        return None;
    }

    let mut title: String = cleaned.chars().take(24).collect();
    if cleaned.chars().count() > 24 {
        title.push_str("...");
    }
    Some(title)
}

fn title_from_input(input: &str) -> String {
    let trimmed = input.trim();
    let mut title: String = trimmed.chars().take(28).collect();
    if trimmed.chars().count() > 28 {
        title.push_str("...");
    }
    title
}
