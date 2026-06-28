use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReviewResult {
    pub id: Uuid,
    pub attempt_id: Option<Uuid>,
    pub is_correct: bool,
    pub score: u8,
    pub summary: String,
    pub mistakes: Vec<String>,
    pub corrected_answer: String,
    pub next_steps: Vec<String>,
    pub raw_response: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl ReviewResult {
    pub fn new(
        attempt_id: Option<Uuid>,
        is_correct: bool,
        score: u8,
        summary: impl Into<String>,
        mistakes: Vec<String>,
        corrected_answer: impl Into<String>,
        next_steps: Vec<String>,
        raw_response: Option<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            attempt_id,
            is_correct,
            score,
            summary: summary.into(),
            mistakes,
            corrected_answer: corrected_answer.into(),
            next_steps,
            raw_response,
            created_at: Utc::now(),
        }
    }
}
