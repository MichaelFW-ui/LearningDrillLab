pub fn learning_system_prompt() -> &'static str {
    r#"You are Learning Drill Lab, an AI tutor for memorizing programming language structures.
Return only valid JSON. Do not wrap JSON in markdown fences.
Teach in Chinese. The chat explanation should be structured, practical, and rich enough for memory training.
Exercises should test syntax, compilation behavior, and implementation recall."#
}

pub fn explain_and_generate_prompt(user_topic: &str) -> String {
    format!(
        r#"The learner wants to study this programming topic:

{user_topic}

Return JSON with this exact shape:
{{
  "topic_title": "a concise normalized Chinese title for this learning topic, no more than 18 Chinese characters",
  "explanation": "rich structured explanation in Chinese for the left chat panel. Use markdown-style headings, bullet lists, and short code blocks when useful. Include: what problem this construct solves, syntax anatomy, mental model, minimal examples, common mistakes, memory hooks, and how the following drills map to the concept.",
  "concept": {{
    "title": "short title",
    "language": "programming language or Unknown",
    "summary": "one paragraph summary in Chinese",
    "key_points": ["point 1", "point 2", "point 3"]
  }},
  "exercises": [
    {{
      "kind": "FillBlank",
      "title": "exercise title",
      "prompt": "question in Chinese. Do not repeat starter_code here.",
      "starter_code": "code or empty string",
      "expected_answer": "reference answer",
      "hints": ["hint 1"],
      "difficulty": "easy"
    }},
    {{
      "kind": "FixBug",
      "title": "exercise title",
      "prompt": "question in Chinese. Do not repeat starter_code here.",
      "starter_code": "buggy code",
      "expected_answer": "fixed answer",
      "hints": ["hint 1"],
      "difficulty": "medium"
    }},
    {{
      "kind": "WriteFromScratch",
      "title": "exercise title",
      "prompt": "question in Chinese. Do not repeat starter_code here.",
      "starter_code": "",
      "expected_answer": "reference code",
      "hints": ["hint 1"],
      "difficulty": "medium"
    }},
    {{
      "kind": "PredictCompileResult",
      "title": "exercise title",
      "prompt": "question in Chinese. Do not repeat starter_code here.",
      "starter_code": "code to inspect",
      "expected_answer": "compile/runtime prediction",
      "hints": ["hint 1"],
      "difficulty": "medium"
    }}
  ]
}}

Important: if an exercise has code, put that code only in starter_code. The prompt field must contain prose instructions only and must not include markdown code fences."#
    )
}

pub fn review_prompt(exercise_json: &str, answer: &str) -> String {
    format!(
        r#"Review the learner answer for this exercise.

Exercise JSON:
{exercise_json}

Learner answer:
{answer}

Return JSON with this exact shape:
{{
  "is_correct": true,
  "score": 0,
  "summary": "short review in Chinese",
  "mistakes": ["mistake or misconception"],
  "corrected_answer": "corrected answer or improved code",
  "next_steps": ["next drill suggestion"]
}}

The score must be an integer from 0 to 100."#
    )
}

pub fn experiment_prompt(concept_json: &str, exercise_json: &str) -> String {
    format!(
        r#"Create a prompt that the learner could send to an external code agent for a local experiment.

Concept JSON:
{concept_json}

Exercise JSON:
{exercise_json}

Return JSON with this exact shape:
{{
  "title": "short experiment title in Chinese",
  "prompt": "detailed experiment prompt in Chinese. It should ask the code agent to create a tiny runnable project or file that demonstrates the concept, with clear expected observations."
}}"#
    )
}
