pub fn learning_system_prompt() -> &'static str {
    r#"You are Learning Drill Lab, an AI tutor for memorizing programming language structures.
Return only valid JSON. Do not wrap JSON in markdown fences.
Teach in Chinese. Optimize for technical correctness, clear mental models, and fair practice.
Never invent failure modes to force an exercise. If you are not sure a premise is true, design an observation, prediction, or construction task instead."#
}

pub fn follow_up_system_prompt() -> &'static str {
    r#"You are Learning Drill Lab, an AI tutor for memorizing programming language structures.
Teach in Chinese. The learner may challenge, correct, or ask follow-up questions about your previous explanation, exercises, or reviews.
Answer directly and technically. If the learner's challenge is correct, acknowledge it and correct the earlier explanation or judgment.
Use Markdown when it improves clarity. Do not return JSON for this follow-up chat."#
}

pub fn explain_and_generate_prompt(user_topic: &str) -> String {
    format!(
        r#"The learner wants to study this programming topic:

{user_topic}

Return JSON with this exact shape:
{{
  "topic_title": "a concise normalized Chinese title for this learning topic, no more than 18 Chinese characters",
  "explanation": "rich structured explanation in Chinese for the left chat panel. Use markdown-style headings, bullet lists, tables, and short code blocks when useful. The explanation must be detailed enough that the learner can attempt the exercises without needing unstated API knowledge. Include: what problem this construct solves, prerequisites and vocabulary, syntax/API anatomy, mental model, step-by-step mechanics, minimal runnable examples, relevant type/shape/lifetime/state changes, common mistakes, debugging cues, memory hooks, and how the drills map to the concept.",
  "concept": {{
    "title": "short title",
    "language": "programming language or Unknown",
    "summary": "one paragraph summary in Chinese",
    "key_points": ["point 1", "point 2", "point 3"]
  }},
  "exercises": [
    {{
      "kind": "FillBlank | FixBug | WriteFromScratch | PredictCompileResult",
      "title": "exercise title",
      "prompt": "question in Chinese. Do not repeat starter_code here.",
      "starter_code": "code or empty string",
      "expected_answer": "reference answer",
      "hints": ["hint 1"],
      "difficulty": "easy | medium | hard"
    }}
  ]
}}

Teaching contract:
- Teach before testing. Any API, rule, operator, syntax form, runtime behavior, type/shape/state transition, or debugging cue required by an exercise must be explained first.
- Explain mechanisms, not labels. Naming a method, function, keyword, or rule is not teaching it; explain what problem it solves, how it behaves, inputs/outputs, constraints, a small example, and common misuse.
- Exercises may transfer or combine ideas, but they must stay within the explanation's taught surface area. A learner should not need unstated library trivia to solve them.

Exercise contract:
- Generate exactly 4 exercises.
- Choose kinds from FillBlank, FixBug, WriteFromScratch, and PredictCompileResult. Do not force one of each kind.
- Use FixBug only when the starter_code contains a real, specific, verifiable bug. If uncertainty remains, use prediction, construction, or assertion-based tasks instead.
- Any prompt claim about failure, compilation, runtime errors, type/shape/lifetime behavior, output values, or API effects must be grounded in actual language/library semantics. Do not guess.
- Hints are optional guidance, not hidden requirements. If a hinted operation is required, the prompt and starter_code must make that requirement technically necessary or explicitly stated.
- Each expected_answer must answer the prompt directly and be consistent with starter_code.

Important: if an exercise has code, put that code only in starter_code. The prompt field must contain prose instructions only and must not include markdown code fences."#
    )
}

pub fn audit_learning_response_prompt(user_topic: &str, candidate_json: &str) -> String {
    format!(
        r#"Audit and repair this generated learning package before it is shown to the learner.

Original learner topic:
{user_topic}

Candidate JSON:
{candidate_json}

Return JSON with this exact shape:
{{
  "audit": [
    {{
      "exercise_title": "candidate exercise title",
      "claim": "the concrete technical claim being checked, for example whether code errors, output value, type/shape change, API effect, compile behavior, or required fix",
      "verdict": "valid | false | uncertain | unsupported_by_explanation | misaligned",
      "action": "keep | rewrite | replace"
    }}
  ],
  "learning_response": {{
    "topic_title": "a concise normalized Chinese title",
    "explanation": "repaired rich explanation",
    "concept": {{
      "title": "short title",
      "language": "programming language or Unknown",
      "summary": "one paragraph summary in Chinese",
      "key_points": ["point 1", "point 2", "point 3"]
    }},
    "exercises": [
      {{
        "kind": "FillBlank | FixBug | WriteFromScratch | PredictCompileResult",
        "title": "exercise title",
        "prompt": "question in Chinese. Do not repeat starter_code here.",
        "starter_code": "code or empty string",
        "expected_answer": "reference answer",
        "hints": ["hint 1"],
        "difficulty": "easy | medium | hard"
      }}
    ]
  }}
}}

Audit contract:
- Treat the candidate as untrusted. Audit the explanation, exercise prompts, starter_code, hints, and expected answers as technical claims.
- For every candidate exercise, write at least one audit item before deciding whether to keep, rewrite, or replace it.
- A claim is not valid because it appears in the candidate. Validate it against language/library semantics, not against the candidate's explanation.
- Keep exactly 4 exercises, but freely rewrite, replace, or reorder flawed exercises.
- Reject false or uncertain premises. If the candidate claims code fails, compiles, panics, returns a value, changes a type/shape/state, or uses an API effect, that claim must follow from actual language/library semantics. If you cannot verify the claim from reliable knowledge, rewrite the exercise into a safer observation, prediction, construction, or assertion task.
- Do not preserve FixBug exercises unless the bug is real, specific, and visible from the starter_code and prompt. Otherwise change the kind.
- Check coverage: every operation or behavior needed by an exercise must be taught in explanation. Expand explanation or replace the exercise.
- Check alignment: hints must not smuggle extra requirements, and expected_answer must match the repaired prompt.
- If a prompt says "will error", "cannot", "must", or "requires", the audit item must name the exact semantic rule that makes that statement true. If you cannot name that rule confidently, the verdict is uncertain and the exercise must be rewritten or replaced.
- Prefer robust exercises with observable targets: predict output, print relevant facts, satisfy explicit assertions, implement a stated transformation, or compare behavior before/after a real change.
- Return only valid JSON. Do not include audit notes outside the JSON."#
    )
}

pub fn regenerate_exercises_prompt(
    concept_json: &str,
    explanation_context: &str,
    previous_exercises_json: &str,
) -> String {
    format!(
        r#"Generate a replacement exercise set for the existing learning topic.

Concept JSON:
{concept_json}

Existing explanation and follow-up context:
{explanation_context}

Previous exercises JSON:
{previous_exercises_json}

Return JSON with this exact shape:
{{
  "exercises": [
    {{
      "kind": "FillBlank | FixBug | WriteFromScratch | PredictCompileResult",
      "title": "exercise title",
      "prompt": "question in Chinese. Do not repeat starter_code here.",
      "starter_code": "code or empty string",
      "expected_answer": "reference answer",
      "hints": ["hint 1"],
      "difficulty": "easy | medium | hard"
    }}
  ]
}}

Exercise-only regeneration contract:
- Do not rewrite, reinterpret, or restate the user's original topic. The concept and existing explanation above are the fixed source of truth.
- Generate exactly 4 new exercises that practice the existing explanation. Do not return explanation, concept, or title fields.
- Choose kinds from FillBlank, FixBug, WriteFromScratch, and PredictCompileResult. Do not force one of each kind.
- Use FixBug only when the starter_code contains a real, specific, verifiable bug. If uncertainty remains, use prediction, construction, or assertion-based tasks instead.
- Any prompt claim about failure, compilation, runtime errors, type/shape/lifetime behavior, output values, or API effects must be grounded in actual language/library semantics. Do not guess.
- Hints are optional guidance, not hidden requirements. If a hinted operation is required, the prompt and starter_code must make that requirement technically necessary or explicitly stated.
- Each expected_answer must answer the prompt directly and be consistent with starter_code.
- Prefer exercises that differ from previous_exercises_json while staying within the existing explanation's taught surface area.

Important: if an exercise has code, put that code only in starter_code. The prompt field must contain prose instructions only and must not include markdown code fences."#
    )
}

pub fn audit_exercises_prompt(
    concept_json: &str,
    explanation_context: &str,
    candidate_exercises_json: &str,
) -> String {
    format!(
        r#"Audit and repair this replacement exercise set before it is shown to the learner.

Concept JSON:
{concept_json}

Existing explanation and follow-up context:
{explanation_context}

Candidate exercises JSON:
{candidate_exercises_json}

Return JSON with this exact shape:
{{
  "audit": [
    {{
      "exercise_title": "candidate exercise title",
      "claim": "the concrete technical claim being checked, for example whether code errors, output value, type/shape change, API effect, compile behavior, or required fix",
      "verdict": "valid | false | uncertain | unsupported_by_explanation | misaligned",
      "action": "keep | rewrite | replace"
    }}
  ],
  "exercise_set": {{
    "exercises": [
      {{
        "kind": "FillBlank | FixBug | WriteFromScratch | PredictCompileResult",
        "title": "exercise title",
        "prompt": "question in Chinese. Do not repeat starter_code here.",
        "starter_code": "code or empty string",
        "expected_answer": "reference answer",
        "hints": ["hint 1"],
        "difficulty": "easy | medium | hard"
      }}
    ]
  }}
}}

Exercise audit contract:
- Treat the candidate exercises as untrusted technical claims.
- For every candidate exercise, write at least one audit item before deciding whether to keep, rewrite, or replace it.
- A claim is not valid because it appears in the candidate. Validate it against language/library semantics, not against the existing explanation.
- Keep exactly 4 exercises, but freely rewrite, replace, or reorder flawed exercises.
- Reject false or uncertain premises. If an exercise claims code fails, compiles, panics, returns a value, changes a type/shape/state, or uses an API effect, that claim must follow from actual language/library semantics. If you cannot verify the claim from reliable knowledge, rewrite the exercise into a safer observation, prediction, construction, or assertion task.
- Do not preserve FixBug exercises unless the bug is real, specific, and visible from the starter_code and prompt. Otherwise change the kind.
- Check coverage: every operation or behavior needed by an exercise must be taught in the existing explanation/context. Replace exercises that require untaught facts.
- Check alignment: hints must not smuggle extra requirements, and expected_answer must match the repaired prompt.
- If a prompt says "will error", "cannot", "must", or "requires", the audit item must name the exact semantic rule that makes that statement true. If you cannot name that rule confidently, the verdict is uncertain and the exercise must be rewritten or replaced.
- Do not return or modify explanation/concept/title. Return only the audit and exercise_set fields described above.
- Return only valid JSON."#
    )
}

pub fn review_prompt(exercise_json: &str, answer: &str) -> String {
    format!(
        r#"Review the learner answer for this exercise.

Exercise JSON:
{exercise_json}

Learner answer:
{answer}

Review contract:
- Be fair, technical, and non-punitive. The exercise is not automatically correct.
- First audit the exercise premise, starter_code, expected_answer, and hints against actual language/library semantics.
- If the learner challenges the exercise, evaluate the challenge as a technical answer. A correct challenge to a flawed exercise is correct.
- If the exercise is false, ambiguous, or more restrictive than its prompt justifies, set is_correct to true, give a high score, explain the flaw in summary, keep mistakes empty or minimal, and provide a corrected exercise or answer in corrected_answer.
- Hints are not requirements unless the prompt validly makes them requirements.
- Mark the learner wrong only when the exercise is valid and the learner's answer is technically wrong relative to that valid exercise.

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

pub fn follow_up_context_prompt(
    concept_json: &str,
    exercises_json: &str,
    attempts_json: &str,
) -> String {
    format!(
        r#"Current learning context:

Concept JSON:
{concept_json}

Exercises JSON:
{exercises_json}

Learner attempts and reviews JSON:
{attempts_json}

Use this context only to answer the learner's follow-up in the ongoing chat. Do not generate a new exercise set unless the learner explicitly asks for one."#
    )
}

pub fn experiment_prompt(concept_json: &str, exercise_json: &str) -> String {
    format!(
        r#"Create a prompt that the learner could send to an external code agent for a local experiment.

Concept JSON:
{concept_json}

Exercise JSON:
{exercise_json}

Experiment contract:
- Preserve the exact selected exercise because the learner may be challenging the exercise itself.
- Include the exercise title, prompt, starter_code, expected_answer, hints, and difficulty from Exercise JSON.
- Ask the code agent to reproduce the starter_code exactly before modifying it.
- Ask the code agent to verify each premise in the prompt against observed behavior. If the prompt claims failure or a specific output, the experiment must report whether that claim is true.
- Ask the code agent to print relevant observable facts for the domain, such as values, types, shapes, state changes, compiler diagnostics, runtime errors, or outputs.
- Ask the code agent to compare observations with expected_answer and hints, then classify the exercise as valid, flawed, or underspecified.
- Do not turn this into a general concept demo; the selected exercise is the object under test.

Return JSON with this exact shape:
{{
  "title": "short experiment title in Chinese",
  "prompt": "detailed experiment prompt in Chinese. It must faithfully include the exact selected exercise and ask a code agent to reproduce, validate, and if needed challenge the exercise with clear observed outputs."
}}"#
    )
}
