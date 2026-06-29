use crate::app::{
    regenerate_exercises, request_experiment, select_exercise, submit_answer, update_answer_input,
    APP_STATE,
};
use crate::ui::markdown::MarkdownContent;
use dioxus::prelude::*;

#[component]
#[allow(non_snake_case)]
pub fn DrillPanel() -> Element {
    let snapshot = APP_STATE.read().clone();
    let topic = snapshot.active_topic().cloned();

    rsx! {
        div { class: "drill-panel",
            if let Some(topic) = topic {
                if let Some(concept) = &topic.concept {
                    div { class: "section",
                        h2 { "{concept.title}" }
                        p { class: "muted", "{concept.language}" }
                        MarkdownContent {
                            content: concept.summary.clone(),
                            class: "markdown-body".to_string(),
                        }
                        if !concept.key_points.is_empty() {
                            MarkdownContent {
                                content: markdown_list(&concept.key_points),
                                class: "markdown-body".to_string(),
                            }
                        }
                    }
                } else {
                    div { class: "section",
                        h2 { "当前知识点" }
                        p { class: "muted", "还没有生成知识点。请先在左侧输入学习目标并发送。" }
                    }
                }

                div { class: "section",
                    div { class: "exercise-tabs",
                        for exercise in topic.exercises.iter() {
                            {
                                let exercise_id = exercise.id;
                                let class_name = if Some(exercise.id) == topic.selected_exercise_id {
                                    "exercise-tab selected"
                                } else {
                                    "exercise-tab"
                                };
                                rsx! {
                                    button {
                                        class: "{class_name}",
                                        onclick: move |_| select_exercise(exercise_id),
                                        "{exercise.kind.label()}"
                                    }
                                }
                            }
                        }
                    }
                    if !topic.exercises.is_empty() {
                        div { class: "composer-row",
                            button {
                                disabled: snapshot.is_busy(),
                                onclick: move |_| regenerate_exercises(),
                                "重新生成练习"
                            }
                        }
                    }
                }

                if let Some(exercise) = topic.selected_exercise() {
                    {
                        let display_prompt = clean_exercise_prompt(&exercise.prompt, &exercise.starter_code);
                        rsx! {
                    div { class: "section",
                        h2 { "{exercise.title}" }
                        p { class: "muted", "{exercise.kind.label()} · {exercise.difficulty}" }
                        MarkdownContent {
                            content: display_prompt,
                            class: "markdown-body".to_string(),
                        }
                        if !exercise.starter_code.trim().is_empty() {
                            pre { class: "code-block", "{exercise.starter_code}" }
                        }
                        if !exercise.hints.is_empty() {
                            h3 { "提示" }
                            MarkdownContent {
                                content: markdown_list(&exercise.hints),
                                class: "markdown-body".to_string(),
                            }
                        }
                    }
                        }
                    }

                    div { class: "section",
                        h3 { "你的答案" }
                        textarea {
                            class: "answer-box code-input",
                            value: "{snapshot.answer_input}",
                            placeholder: "在这里写答案或代码...",
                            spellcheck: "false",
                            autocomplete: "off",
                            autocorrect: "off",
                            autocapitalize: "off",
                            translate: "no",
                            wrap: "off",
                            oninput: move |event| update_answer_input(event.value())
                        }
                        div { class: "composer-row",
                            button {
                                disabled: snapshot.is_busy(),
                                onclick: move |_| request_experiment(),
                                "请求实验"
                            }
                            button {
                                class: "primary",
                                disabled: snapshot.is_busy(),
                                onclick: move |_| submit_answer(),
                                "提交答案"
                            }
                        }
                    }

                    if let Some(review) = topic.latest_review_for_selected() {
                        {
                            let verdict = if review.is_correct { "正确" } else { "需要修正" };
                            rsx! {
                                div { class: "section",
                                    h3 { "Review 结果" }
                                    div { class: "review-box",
                                        p { strong { "结论：" } "{verdict} · {review.score}/100" }
                                        MarkdownContent {
                                            content: review.summary.clone(),
                                            class: "markdown-body".to_string(),
                                        }
                                        if !review.mistakes.is_empty() {
                                            h3 { "问题" }
                                            MarkdownContent {
                                                content: markdown_list(&review.mistakes),
                                                class: "markdown-body".to_string(),
                                            }
                                        }
                                        h3 { "修正答案" }
                                        pre { class: "code-block", "{review.corrected_answer}" }
                                        if !review.next_steps.is_empty() {
                                            h3 { "下一步练习" }
                                            MarkdownContent {
                                                content: markdown_list(&review.next_steps),
                                                class: "markdown-body".to_string(),
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    if let Some(prompt) = topic.latest_experiment_prompt() {
                        div { class: "section",
                            h3 { "实验 Prompt" }
                            div { class: "experiment-box",
                                p { strong { "{prompt.title}" } }
                                pre { class: "code-block", "{prompt.prompt}" }
                            }
                        }
                    }
                } else {
                    div { class: "section",
                        h2 { "练习区" }
                        p { class: "muted", "AI 生成练习后会显示在这里。" }
                    }
                }
            }
        }
    }
}

fn clean_exercise_prompt(prompt: &str, starter_code: &str) -> String {
    let starter_code = starter_code.trim();
    let mut cleaned = prompt.trim().to_string();

    if starter_code.is_empty() {
        return cleaned;
    }

    cleaned = cleaned.replace(starter_code, "");
    cleaned = strip_fenced_code_blocks(&cleaned);
    collapse_blank_lines(&cleaned)
}

fn markdown_list(items: &[String]) -> String {
    let mut markdown = String::new();

    for item in items {
        if !markdown.is_empty() {
            markdown.push('\n');
        }
        markdown.push_str("- ");
        markdown.push_str(&item.trim().replace('\n', "\n  "));
    }

    markdown
}

fn strip_fenced_code_blocks(text: &str) -> String {
    let mut output = Vec::new();
    let mut in_fence = false;

    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }

        if !in_fence {
            output.push(line);
        }
    }

    output.join("\n")
}

fn collapse_blank_lines(text: &str) -> String {
    let mut output = Vec::new();
    let mut previous_blank = false;

    for line in text.lines() {
        let blank = line.trim().is_empty();
        if blank && previous_blank {
            continue;
        }
        output.push(line.trim_end());
        previous_blank = blank;
    }

    output.join("\n").trim().to_string()
}
