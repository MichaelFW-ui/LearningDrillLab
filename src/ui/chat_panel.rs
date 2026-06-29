use crate::app::{
    begin_rename_topic, cancel_rename_topic, commit_rename_topic, delete_topic,
    follow_up_from_chat_input, generate_from_chat_input, new_topic, regenerate_from_topic,
    set_topic_sort, switch_topic, update_chat_input, update_rename_input, ChatRole, TopicSort,
    APP_STATE,
};
use crate::ui::markdown::MarkdownContent;
use dioxus::prelude::*;

#[component]
#[allow(non_snake_case)]
pub fn ChatPanel() -> Element {
    let snapshot = APP_STATE.read().clone();
    let active_id = snapshot.active_topic_id;
    let topics = snapshot.sorted_topics();
    let has_learning_content = snapshot
        .active_topic()
        .map(|topic| {
            topic.concept.is_some()
                || !topic.exercises.is_empty()
                || topic.messages.iter().any(|message| {
                    message.role == ChatRole::Assistant && message.raw_response.is_some()
                })
        })
        .unwrap_or(false);
    let chat_placeholder = if has_learning_content {
        "追问或质疑当前讲解..."
    } else {
        "输入想学习的知识点..."
    };
    let send_label = if has_learning_content {
        "追问"
    } else {
        "生成练习"
    };

    rsx! {
        div { class: "chat-panel",
            div { class: "panel-header",
                div { class: "composer-row",
                    button {
                        class: "primary",
                        onclick: move |_| new_topic(),
                        "开新话题"
                    }
                    select {
                        value: "{sort_value(snapshot.topic_sort)}",
                        onchange: move |event| set_topic_sort(sort_from_value(&event.value())),
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
                                        oninput: move |event| update_rename_input(event.value())
                                    }
                                    button {
                                        class: "topic-tool",
                                        onclick: move |_| commit_rename_topic(),
                                        "保存"
                                    }
                                    button {
                                        class: "topic-tool",
                                        onclick: move |_| cancel_rename_topic(),
                                        "取消"
                                    }
                                }
                            }
                        } else {
                            rsx! {
                                div { class: "topic-item",
                                    button {
                                        class: "{open_class}",
                                        onclick: move |_| switch_topic(topic_id),
                                        span { class: "topic-title", "{topic.title}" }
                                        span { class: "topic-meta", "{updated_at}" }
                                    }
                                    button {
                                        class: "topic-tool",
                                        title: "重命名",
                                        onclick: move |_| begin_rename_topic(topic_id),
                                        "改"
                                    }
                                    button {
                                        class: "topic-tool",
                                        title: "删除",
                                        onclick: move |_| delete_topic(topic_id),
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
                                MarkdownContent {
                                    content: message.content.clone(),
                                    class: class_name.to_string(),
                                }
                            }
                        }
                    }
                }
            }

            div { class: "composer",
                textarea {
                    value: "{snapshot.chat_input}",
                    placeholder: "{chat_placeholder}",
                    oninput: move |event| update_chat_input(event.value())
                }
                div { class: "composer-row",
                    if has_learning_content {
                        button {
                            disabled: snapshot.is_busy(),
                            onclick: move |_| regenerate_from_topic(),
                            "重新生成练习"
                        }
                    }
                    if has_learning_content {
                        button {
                            class: "primary",
                            disabled: snapshot.is_busy(),
                            onclick: move |_| follow_up_from_chat_input(),
                            "{send_label}"
                        }
                    } else {
                        button {
                            class: "primary",
                            disabled: snapshot.is_busy(),
                            onclick: move |_| generate_from_chat_input(),
                            "{send_label}"
                        }
                    }
                }
            }
        }
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
