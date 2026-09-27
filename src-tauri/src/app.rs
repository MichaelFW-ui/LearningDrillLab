use crate::domain::concept::Concept;
use crate::domain::exercise::{Attempt, Exercise, ExperimentPrompt};
use chrono::{DateTime, Utc};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiSettings {
    #[serde(default = "default_base_url")]
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub selected_model: String,
    #[serde(default)]
    pub available_models: Vec<String>,
    #[serde(default)]
    pub bocha_api_key: String,
    #[serde(default)]
    pub tavily_api_key: String,
    #[serde(default = "default_tavily_base_url")]
    pub tavily_base_url: String,
    #[serde(default)]
    pub jina_api_key: String,
    #[serde(default)]
    pub sandbox_base_url: String,
    #[serde(default)]
    pub sandbox_api_key: String,
}

impl Default for ApiSettings {
    fn default() -> Self {
        Self {
            base_url: default_base_url(),
            api_key: String::new(),
            selected_model: String::new(),
            available_models: Vec::new(),
            bocha_api_key: String::new(),
            tavily_api_key: String::new(),
            tavily_base_url: default_tavily_base_url(),
            jina_api_key: String::new(),
            sandbox_base_url: String::new(),
            sandbox_api_key: String::new(),
        }
    }
}

fn default_base_url() -> String {
    "https://api.openai.com/v1".to_string()
}

fn default_tavily_base_url() -> String {
    "https://api.tavily.com".to_string()
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum TopicSort {
    #[default]
    UpdatedDesc,
    CreatedDesc,
    TitleAsc,
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
    #[serde(default)]
    pub experiment_runs: Vec<crate::sandbox::ExperimentRun>,
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
            experiment_runs: Vec::new(),
        }
    }

    pub fn selected_exercise(&self) -> Option<&Exercise> {
        let selected = self.selected_exercise_id?;
        self.exercises
            .iter()
            .find(|exercise| exercise.id == selected)
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
    pub status: Option<String>,
}

impl Default for AppState {
    fn default() -> Self {
        let mut state = Self {
            settings: ApiSettings::default(),
            topics: Vec::new(),
            active_topic_id: None,
            topic_sort: TopicSort::UpdatedDesc,
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

        let primary = fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str::<Self>(&raw).ok());
        let recovered = primary.is_none();
        let Some(mut state) = primary.or_else(|| {
            fs::read_to_string(path.with_extension("json.bak"))
                .ok()
                .and_then(|raw| serde_json::from_str::<Self>(&raw).ok())
        }) else {
            return Self::default();
        };
        state.status = Some(if recovered {
            "主数据文件不可用，已从备份恢复历史记录".to_string()
        } else {
            "已加载本地历史记录".to_string()
        });
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
        let bytes = serde_json::to_vec_pretty(self)?;
        let temporary = path.with_extension("json.tmp");
        fs::write(&temporary, bytes)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))?;
        }
        let primary_is_valid = fs::read_to_string(&path)
            .ok()
            .is_some_and(|raw| serde_json::from_str::<Self>(&raw).is_ok());
        if primary_is_valid {
            fs::copy(&path, path.with_extension("json.bak"))?;
        }
        fs::rename(temporary, path)?;
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_state_without_new_fields_loads_and_preserves_ids() {
        let original = AppState::default();
        let topic_id = original.active_topic_id.unwrap();
        let mut value = serde_json::to_value(&original).unwrap();
        value["settings"]
            .as_object_mut()
            .unwrap()
            .remove("sandbox_base_url");
        value["settings"]
            .as_object_mut()
            .unwrap()
            .remove("sandbox_api_key");
        value["topics"][0]
            .as_object_mut()
            .unwrap()
            .remove("experiment_runs");
        let restored: AppState = serde_json::from_value(value).unwrap();
        assert_eq!(restored.active_topic_id, Some(topic_id));
        assert_eq!(restored.topics[0].id, topic_id);
        assert!(restored.settings.sandbox_base_url.is_empty());
        assert!(restored.topics[0].experiment_runs.is_empty());
    }
}

pub fn storage_path() -> Option<PathBuf> {
    ProjectDirs::from("dev", "LearningDrillLab", "LearningDrillLab")
        .map(|dirs| dirs.config_dir().join("state.json"))
}
