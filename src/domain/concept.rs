use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Concept {
    pub id: Uuid,
    pub title: String,
    pub language: String,
    pub summary: String,
    pub key_points: Vec<String>,
}

impl Concept {
    pub fn new(
        title: impl Into<String>,
        language: impl Into<String>,
        summary: impl Into<String>,
        key_points: Vec<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            title: title.into(),
            language: language.into(),
            summary: summary.into(),
            key_points,
        }
    }
}
