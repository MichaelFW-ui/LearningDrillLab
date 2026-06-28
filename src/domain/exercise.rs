use crate::domain::review::ReviewResult;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ExerciseKind {
    FillBlank,
    FixBug,
    WriteFromScratch,
    PredictCompileResult,
}

impl ExerciseKind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::FillBlank => "填空题",
            Self::FixBug => "改错题",
            Self::WriteFromScratch => "从零写代码",
            Self::PredictCompileResult => "预测编译结果",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Exercise {
    pub id: Uuid,
    pub concept_id: Option<Uuid>,
    pub kind: ExerciseKind,
    pub title: String,
    pub prompt: String,
    pub starter_code: String,
    pub expected_answer: String,
    pub hints: Vec<String>,
    pub difficulty: String,
}

impl Exercise {
    pub fn new(
        concept_id: Option<Uuid>,
        kind: ExerciseKind,
        title: impl Into<String>,
        prompt: impl Into<String>,
        starter_code: impl Into<String>,
        expected_answer: impl Into<String>,
        hints: Vec<String>,
        difficulty: impl Into<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            concept_id,
            kind,
            title: title.into(),
            prompt: prompt.into(),
            starter_code: starter_code.into(),
            expected_answer: expected_answer.into(),
            hints,
            difficulty: difficulty.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Attempt {
    pub id: Uuid,
    pub exercise_id: Uuid,
    pub answer: String,
    pub review: Option<ReviewResult>,
    pub created_at: DateTime<Utc>,
}

impl Attempt {
    pub fn new(exercise_id: Uuid, answer: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            exercise_id,
            answer: answer.into(),
            review: None,
            created_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExperimentPrompt {
    pub id: Uuid,
    pub title: String,
    pub prompt: String,
    pub created_at: DateTime<Utc>,
}

impl ExperimentPrompt {
    pub fn new(title: impl Into<String>, prompt: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            title: title.into(),
            prompt: prompt.into(),
            created_at: Utc::now(),
        }
    }
}
