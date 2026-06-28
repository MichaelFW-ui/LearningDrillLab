use crate::app::{
    begin_rename_topic, cancel_rename_topic, commit_rename_topic, delete_topic, new_topic,
    send_learning_request, set_topic_sort, switch_topic, update_chat_input, update_rename_input,
    AppState, ChatRole, TopicSort,
};
use dioxus::prelude::*;

#[component]
#[allow(non_snake_case)]
pub fn ChatPanel(state: Signal<AppState>) -> Element {
    let snapshot = state.read().clone();
    let active_id = snapshot.active_topic_id;
    let topics = snapshot.sorted_topics();

    rsx! {
        div { class: "chat-panel",
            div { class: "panel-header",
                div { class: "composer-row",
                    button {
                        class: "primary",
                        onclick: move |_| new_topic(state),
                        "开新话题"
                    }
                    select {
                        value: "{sort_value(snapshot.topic_sort)}",
                        onchange: move |event| set_topic_sort(state, sort_from_value(&event.value())),
                        option {
                            value: "updated",
                            selected: snapshot.topic_sort == TopicSort::UpdatedDesc,
                            "最近更新"
                        }
                        option {
                            value: "created",
                            selected: snapshot.topic_sort == TopicSort::CreatedDesc,
                            "最近创建"
                        }
                        option {
                            value: "title",
                            selected: snapshot.topic_sort == TopicSort::TitleAsc,
                            "标题 A-Z"
                        }
                    }
                }
                span { class: "muted", "历史话题" }
            }

            div { class: "topic-list",
                for topic in topics.iter() {
                    {
                        let topic_id = topic.id;
                        let open_class = if Some(topic.id) == active_id { "topic-open selected" } else { "topic-open" };
                        let updated_at = topic.updated_at.format("%Y-%m-%d %H:%M").to_string();
                        let is_renaming = snapshot.renaming_topic_id == Some(topic.id);
                        if is_renaming {
                            rsx! {
                                div { class: "topic-rename",
                                    input {
                                        value: "{snapshot.rename_input}",
                                        oninput: move |event| update_rename_input(state, event.value())
                                    }
                                    button {
                                        class: "topic-tool",
                                        onclick: move |_| commit_rename_topic(state),
                                        "保存"
                                    }
                                    button {
                                        class: "topic-tool",
                                        onclick: move |_| cancel_rename_topic(state),
                                        "取消"
                                    }
                                }
                            }
                        } else {
                            rsx! {
                                div { class: "topic-item",
                                    button {
                                        class: "{open_class}",
                                        onclick: move |_| switch_topic(state, topic_id),
                                        span { class: "topic-title", "{topic.title}" }
                                        span { class: "topic-meta", "{updated_at}" }
                                    }
                                    button {
                                        class: "topic-tool",
                                        title: "重命名",
                                        onclick: move |_| begin_rename_topic(state, topic_id),
                                        "改"
                                    }
                                    button {
                                        class: "topic-tool",
                                        title: "删除",
                                        onclick: move |_| delete_topic(state, topic_id),
                                        "删"
                                    }
                                }
                            }
                        }
                    }
                }
            }

            div { class: "messages",
                if let Some(topic) = snapshot.active_topic() {
                    if topic.messages.is_empty() {
                        div { class: "message assistant",
                            "输入一个想学习的编程语言知识点，例如 Rust 所有权借用、TypeScript 泛型约束、Python 装饰器。AI 会讲解并生成练习。"
                        }
                    }
                    for message in topic.messages.iter() {
                        {
                            let class_name = match message.role {
                                ChatRole::User => "message user",
                                ChatRole::Assistant => "message assistant",
                            };
                            rsx! {
                                if message.role == ChatRole::Assistant {
                                    RichMessage { content: message.content.clone() }
                                } else {
                                    div { class: "{class_name}", "{message.content}" }
                                }
                            }
                        }
                    }
                }
            }

            div { class: "composer",
                textarea {
                    value: "{snapshot.chat_input}",
                    placeholder: "输入想学习的知识点...",
                    oninput: move |event| update_chat_input(state, event.value())
                }
                div { class: "composer-row",
                    button {
                        class: "primary",
                        disabled: snapshot.is_busy(),
                        onclick: move |_| send_learning_request(state),
                        "发送"
                    }
                }
            }
        }
    }
}

#[component]
#[allow(non_snake_case)]
fn RichMessage(content: String) -> Element {
    let blocks = parse_rich_blocks(&content);

    rsx! {
        div { class: "message assistant rich-message",
            for block in blocks.iter() {
                {
                    match block {
                        RichBlock::Heading(text) => rsx! { h3 { "{text}" } },
                        RichBlock::Paragraph(text) => rsx! { p { "{text}" } },
                        RichBlock::Bullets(items) => rsx! {
                            ul {
                                for item in items.iter() {
                                    li { "{item}" }
                                }
                            }
                        },
                        RichBlock::Code(code) => rsx! { pre { "{code}" } },
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RichBlock {
    Heading(String),
    Paragraph(String),
    Bullets(Vec<String>),
    Code(String),
}

fn parse_rich_blocks(content: &str) -> Vec<RichBlock> {
    let mut blocks = Vec::new();
    let mut paragraph = Vec::new();
    let mut bullets = Vec::new();
    let mut code = Vec::new();
    let mut in_code = false;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("```") {
            if in_code {
                blocks.push(RichBlock::Code(code.join("\n")));
                code.clear();
                in_code = false;
            } else {
                flush_paragraph(&mut blocks, &mut paragraph);
                flush_bullets(&mut blocks, &mut bullets);
                in_code = true;
            }
            continue;
        }

        if in_code {
            code.push(line.to_string());
            continue;
        }

        if trimmed.is_empty() {
            flush_paragraph(&mut blocks, &mut paragraph);
            flush_bullets(&mut blocks, &mut bullets);
            continue;
        }

        if let Some(heading) = heading_text(trimmed) {
            flush_paragraph(&mut blocks, &mut paragraph);
            flush_bullets(&mut blocks, &mut bullets);
            blocks.push(RichBlock::Heading(heading.to_string()));
            continue;
        }

        if let Some(item) = bullet_text(trimmed) {
            flush_paragraph(&mut blocks, &mut paragraph);
            bullets.push(item.to_string());
            continue;
        }

        flush_bullets(&mut blocks, &mut bullets);
        paragraph.push(trimmed.to_string());
    }

    if in_code && !code.is_empty() {
        blocks.push(RichBlock::Code(code.join("\n")));
    }
    flush_paragraph(&mut blocks, &mut paragraph);
    flush_bullets(&mut blocks, &mut bullets);

    if blocks.is_empty() {
        blocks.push(RichBlock::Paragraph(content.trim().to_string()));
    }

    blocks
}

fn heading_text(line: &str) -> Option<&str> {
    line.strip_prefix("### ")
        .or_else(|| line.strip_prefix("## "))
        .or_else(|| line.strip_prefix("# "))
}

fn bullet_text(line: &str) -> Option<&str> {
    line.strip_prefix("- ")
        .or_else(|| line.strip_prefix("* "))
        .or_else(|| line.strip_prefix("• "))
}

fn flush_paragraph(blocks: &mut Vec<RichBlock>, paragraph: &mut Vec<String>) {
    if !paragraph.is_empty() {
        blocks.push(RichBlock::Paragraph(paragraph.join(" ")));
        paragraph.clear();
    }
}

fn flush_bullets(blocks: &mut Vec<RichBlock>, bullets: &mut Vec<String>) {
    if !bullets.is_empty() {
        blocks.push(RichBlock::Bullets(std::mem::take(bullets)));
    }
}

fn sort_value(sort: TopicSort) -> &'static str {
    match sort {
        TopicSort::UpdatedDesc => "updated",
        TopicSort::CreatedDesc => "created",
        TopicSort::TitleAsc => "title",
    }
}

fn sort_from_value(value: &str) -> TopicSort {
    match value {
        "created" => TopicSort::CreatedDesc,
        "title" => TopicSort::TitleAsc,
        _ => TopicSort::UpdatedDesc,
    }
}
