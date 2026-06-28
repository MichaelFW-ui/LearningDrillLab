use crate::app::{
    begin_rename_topic, cancel_rename_topic, commit_rename_topic, delete_topic,
    follow_up_from_chat_input, generate_from_chat_input, new_topic, regenerate_from_topic,
    set_topic_sort, switch_topic, update_chat_input, update_rename_input, APP_STATE, ChatRole,
    TopicSort,
};
use dioxus::prelude::*;
use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag};

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

#[component]
#[allow(non_snake_case)]
fn RichMessage(content: String) -> Element {
    let nodes = parse_markdown(&content);

    rsx! {
        div { class: "message assistant rich-message",
            for node in nodes.iter() {
                { render_markdown_node(node) }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum MarkdownNode {
    Paragraph(Vec<MarkdownNode>),
    Heading {
        level: u8,
        children: Vec<MarkdownNode>,
    },
    Text(String),
    SoftBreak,
    HardBreak,
    Rule,
    Emphasis(Vec<MarkdownNode>),
    Strong(Vec<MarkdownNode>),
    Strikethrough(Vec<MarkdownNode>),
    InlineCode(String),
    CodeBlock {
        language: Option<String>,
        code: String,
    },
    List {
        ordered: bool,
        start: Option<u64>,
        items: Vec<MarkdownNode>,
    },
    Item(Vec<MarkdownNode>),
    BlockQuote(Vec<MarkdownNode>),
    Link {
        href: String,
        title: String,
        children: Vec<MarkdownNode>,
    },
    Table {
        alignments: Vec<Alignment>,
        children: Vec<MarkdownNode>,
    },
    TableHead(Vec<MarkdownNode>),
    TableRow(Vec<MarkdownNode>),
    TableCell(Vec<MarkdownNode>),
    TaskListMarker(bool),
}

#[derive(Debug)]
enum MarkdownFrameKind {
    Paragraph,
    Heading(u8),
    Emphasis,
    Strong,
    Strikethrough,
    CodeBlock(Option<String>),
    List { ordered: bool, start: Option<u64> },
    Item,
    BlockQuote,
    Link { href: String, title: String },
    Table(Vec<Alignment>),
    TableHead,
    TableRow,
    TableCell,
}

#[derive(Debug)]
struct MarkdownFrame {
    kind: MarkdownFrameKind,
    children: Vec<MarkdownNode>,
}

fn parse_markdown(content: &str) -> Vec<MarkdownNode> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);

    let mut roots = Vec::new();
    let mut stack = Vec::new();

    for event in Parser::new_ext(content, options) {
        match event {
            Event::Start(tag) => stack.push(MarkdownFrame {
                kind: frame_kind(tag),
                children: Vec::new(),
            }),
            Event::End(_) => {
                if let Some(frame) = stack.pop() {
                    push_node(&mut roots, &mut stack, frame.into_node());
                }
            }
            Event::Text(text) => {
                push_node(&mut roots, &mut stack, MarkdownNode::Text(text.to_string()));
            }
            Event::Code(code) => {
                push_node(
                    &mut roots,
                    &mut stack,
                    MarkdownNode::InlineCode(code.to_string()),
                );
            }
            Event::Html(html) | Event::InlineHtml(html) => {
                push_node(&mut roots, &mut stack, MarkdownNode::Text(html.to_string()));
            }
            Event::SoftBreak => push_node(&mut roots, &mut stack, MarkdownNode::SoftBreak),
            Event::HardBreak => push_node(&mut roots, &mut stack, MarkdownNode::HardBreak),
            Event::Rule => push_node(&mut roots, &mut stack, MarkdownNode::Rule),
            Event::TaskListMarker(checked) => {
                push_node(
                    &mut roots,
                    &mut stack,
                    MarkdownNode::TaskListMarker(checked),
                );
            }
            Event::InlineMath(math) => {
                push_node(
                    &mut roots,
                    &mut stack,
                    MarkdownNode::InlineCode(math.to_string()),
                );
            }
            Event::DisplayMath(math) => {
                push_node(
                    &mut roots,
                    &mut stack,
                    MarkdownNode::CodeBlock {
                        language: Some("math".to_string()),
                        code: math.to_string(),
                    },
                );
            }
            Event::FootnoteReference(reference) => {
                push_node(
                    &mut roots,
                    &mut stack,
                    MarkdownNode::Text(format!("[{reference}]")),
                );
            }
        }
    }

    while let Some(frame) = stack.pop() {
        push_node(&mut roots, &mut stack, frame.into_node());
    }

    roots
}

fn frame_kind(tag: Tag<'_>) -> MarkdownFrameKind {
    match tag {
        Tag::Paragraph => MarkdownFrameKind::Paragraph,
        Tag::Heading { level, .. } => MarkdownFrameKind::Heading(heading_level(level)),
        Tag::BlockQuote(_) => MarkdownFrameKind::BlockQuote,
        Tag::CodeBlock(kind) => MarkdownFrameKind::CodeBlock(code_language(kind)),
        Tag::List(start) => MarkdownFrameKind::List {
            ordered: start.is_some(),
            start,
        },
        Tag::Item => MarkdownFrameKind::Item,
        Tag::Emphasis => MarkdownFrameKind::Emphasis,
        Tag::Strong => MarkdownFrameKind::Strong,
        Tag::Strikethrough => MarkdownFrameKind::Strikethrough,
        Tag::Link {
            dest_url, title, ..
        } => MarkdownFrameKind::Link {
            href: dest_url.to_string(),
            title: title.to_string(),
        },
        Tag::Image {
            dest_url, title, ..
        } => MarkdownFrameKind::Link {
            href: dest_url.to_string(),
            title: title.to_string(),
        },
        Tag::Table(alignments) => MarkdownFrameKind::Table(alignments),
        Tag::TableHead => MarkdownFrameKind::TableHead,
        Tag::TableRow => MarkdownFrameKind::TableRow,
        Tag::TableCell => MarkdownFrameKind::TableCell,
        Tag::FootnoteDefinition(_) => MarkdownFrameKind::Paragraph,
        Tag::HtmlBlock => MarkdownFrameKind::Paragraph,
        Tag::DefinitionList => MarkdownFrameKind::List {
            ordered: false,
            start: None,
        },
        Tag::DefinitionListTitle | Tag::DefinitionListDefinition => MarkdownFrameKind::Item,
        Tag::MetadataBlock(_) => MarkdownFrameKind::Paragraph,
        Tag::Superscript | Tag::Subscript => MarkdownFrameKind::Emphasis,
    }
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn code_language(kind: CodeBlockKind<'_>) -> Option<String> {
    match kind {
        CodeBlockKind::Fenced(language) if !language.trim().is_empty() => {
            Some(language.to_string())
        }
        _ => None,
    }
}

fn push_node(roots: &mut Vec<MarkdownNode>, stack: &mut [MarkdownFrame], node: MarkdownNode) {
    if let Some(parent) = stack.last_mut() {
        parent.children.push(node);
    } else {
        roots.push(node);
    }
}

impl MarkdownFrame {
    fn into_node(self) -> MarkdownNode {
        match self.kind {
            MarkdownFrameKind::Paragraph => MarkdownNode::Paragraph(self.children),
            MarkdownFrameKind::Heading(level) => MarkdownNode::Heading {
                level,
                children: self.children,
            },
            MarkdownFrameKind::Emphasis => MarkdownNode::Emphasis(self.children),
            MarkdownFrameKind::Strong => MarkdownNode::Strong(self.children),
            MarkdownFrameKind::Strikethrough => MarkdownNode::Strikethrough(self.children),
            MarkdownFrameKind::CodeBlock(language) => MarkdownNode::CodeBlock {
                language,
                code: collect_text(&self.children),
            },
            MarkdownFrameKind::List { ordered, start } => MarkdownNode::List {
                ordered,
                start,
                items: self.children,
            },
            MarkdownFrameKind::Item => MarkdownNode::Item(self.children),
            MarkdownFrameKind::BlockQuote => MarkdownNode::BlockQuote(self.children),
            MarkdownFrameKind::Link { href, title } => MarkdownNode::Link {
                href,
                title,
                children: self.children,
            },
            MarkdownFrameKind::Table(alignments) => MarkdownNode::Table {
                alignments,
                children: self.children,
            },
            MarkdownFrameKind::TableHead => MarkdownNode::TableHead(self.children),
            MarkdownFrameKind::TableRow => MarkdownNode::TableRow(self.children),
            MarkdownFrameKind::TableCell => MarkdownNode::TableCell(self.children),
        }
    }
}

fn collect_text(nodes: &[MarkdownNode]) -> String {
    let mut text = String::new();
    for node in nodes {
        match node {
            MarkdownNode::Text(value) | MarkdownNode::InlineCode(value) => text.push_str(value),
            MarkdownNode::SoftBreak | MarkdownNode::HardBreak => text.push('\n'),
            MarkdownNode::Emphasis(children)
            | MarkdownNode::Strong(children)
            | MarkdownNode::Strikethrough(children)
            | MarkdownNode::Paragraph(children)
            | MarkdownNode::Item(children)
            | MarkdownNode::BlockQuote(children)
            | MarkdownNode::TableHead(children)
            | MarkdownNode::TableRow(children)
            | MarkdownNode::TableCell(children) => text.push_str(&collect_text(children)),
            MarkdownNode::Heading { children, .. } => text.push_str(&collect_text(children)),
            MarkdownNode::CodeBlock { code, .. } => text.push_str(code),
            MarkdownNode::List { items, .. } => text.push_str(&collect_text(items)),
            MarkdownNode::Link { children, .. } => text.push_str(&collect_text(children)),
            MarkdownNode::Table { children, .. } => text.push_str(&collect_text(children)),
            MarkdownNode::Rule | MarkdownNode::TaskListMarker(_) => {}
        }
    }
    text
}

fn render_markdown_node(node: &MarkdownNode) -> Element {
    match node {
        MarkdownNode::Paragraph(children) => rsx! {
            p {
                for child in children.iter() {
                    { render_markdown_node(child) }
                }
            }
        },
        MarkdownNode::Heading { level, children } => {
            let class_name = format!("md-heading md-h{level}");
            rsx! {
                h3 { class: "{class_name}",
                    for child in children.iter() {
                        { render_markdown_node(child) }
                    }
                }
            }
        }
        MarkdownNode::Text(text) => rsx! { "{text}" },
        MarkdownNode::SoftBreak => rsx! { " " },
        MarkdownNode::HardBreak => rsx! { br {} },
        MarkdownNode::Rule => rsx! { hr {} },
        MarkdownNode::Emphasis(children) => rsx! {
            em {
                for child in children.iter() {
                    { render_markdown_node(child) }
                }
            }
        },
        MarkdownNode::Strong(children) => rsx! {
            strong {
                for child in children.iter() {
                    { render_markdown_node(child) }
                }
            }
        },
        MarkdownNode::Strikethrough(children) => rsx! {
            del {
                for child in children.iter() {
                    { render_markdown_node(child) }
                }
            }
        },
        MarkdownNode::InlineCode(code) => rsx! { code { "{code}" } },
        MarkdownNode::CodeBlock { language, code } => {
            let language_label = language.as_deref().unwrap_or("");
            rsx! {
                div { class: "code-block",
                    if !language_label.is_empty() {
                        div { class: "code-language", "{language_label}" }
                    }
                    pre { code { "{code}" } }
                }
            }
        }
        MarkdownNode::List {
            ordered,
            start,
            items,
        } => {
            if *ordered {
                let start_attr = start.unwrap_or(1).to_string();
                rsx! {
                    ol { start: "{start_attr}",
                        for item in items.iter() {
                            { render_markdown_node(item) }
                        }
                    }
                }
            } else {
                rsx! {
                    ul {
                        for item in items.iter() {
                            { render_markdown_node(item) }
                        }
                    }
                }
            }
        }
        MarkdownNode::Item(children) => rsx! {
            li {
                for child in children.iter() {
                    { render_markdown_node(child) }
                }
            }
        },
        MarkdownNode::BlockQuote(children) => rsx! {
            blockquote {
                for child in children.iter() {
                    { render_markdown_node(child) }
                }
            }
        },
        MarkdownNode::Link {
            href,
            title,
            children,
        } => {
            let safe_href = safe_link_href(href);
            rsx! {
                a {
                    href: "{safe_href}",
                    title: "{title}",
                    target: "_blank",
                    rel: "noreferrer noopener",
                    for child in children.iter() {
                        { render_markdown_node(child) }
                    }
                }
            }
        }
        MarkdownNode::Table {
            alignments: _,
            children,
        } => rsx! {
            div { class: "table-scroll",
                table {
                    for child in children.iter() {
                        { render_markdown_node(child) }
                    }
                }
            }
        },
        MarkdownNode::TableHead(children) => rsx! {
            thead {
                for child in children.iter() {
                    { render_markdown_node(child) }
                }
            }
        },
        MarkdownNode::TableRow(children) => rsx! {
            tr {
                for child in children.iter() {
                    { render_markdown_node(child) }
                }
            }
        },
        MarkdownNode::TableCell(children) => rsx! {
            td {
                for child in children.iter() {
                    { render_markdown_node(child) }
                }
            }
        },
        MarkdownNode::TaskListMarker(checked) => rsx! {
            input {
                r#type: "checkbox",
                checked: *checked,
                disabled: true,
            }
        },
    }
}

fn safe_link_href(href: &str) -> String {
    let trimmed = href.trim();
    if trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
        || trimmed.starts_with("mailto:")
    {
        trimmed.to_string()
    } else {
        "#".to_string()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_inline_markdown() {
        let nodes = parse_markdown("这是 **加粗**、*斜体* 和 `code`。");

        assert!(contains_node(&nodes, |node| matches!(
            node,
            MarkdownNode::Strong(_)
        )));
        assert!(contains_node(&nodes, |node| matches!(
            node,
            MarkdownNode::Emphasis(_)
        )));
        assert!(contains_node(&nodes, |node| matches!(
            node,
            MarkdownNode::InlineCode(code) if code == "code"
        )));
    }

    #[test]
    fn parses_tables() {
        let nodes = parse_markdown("| 名称 | 说明 |\n| --- | --- |\n| Box | 堆分配 |\n");

        assert!(contains_node(&nodes, |node| matches!(
            node,
            MarkdownNode::Table { .. }
        )));
        assert!(contains_node(&nodes, |node| matches!(
            node,
            MarkdownNode::TableHead(_)
        )));
        assert!(contains_node(&nodes, |node| matches!(
            node,
            MarkdownNode::TableCell(_)
        )));
    }

    #[test]
    fn raw_html_is_kept_as_text() {
        let nodes = parse_markdown("<script>alert(1)</script>");

        assert_eq!(collect_text(&nodes), "<script>alert(1)</script>");
    }

    fn contains_node(nodes: &[MarkdownNode], predicate: fn(&MarkdownNode) -> bool) -> bool {
        nodes.iter().any(|node| {
            predicate(node)
                || match node {
                    MarkdownNode::Paragraph(children)
                    | MarkdownNode::Emphasis(children)
                    | MarkdownNode::Strong(children)
                    | MarkdownNode::Strikethrough(children)
                    | MarkdownNode::Item(children)
                    | MarkdownNode::BlockQuote(children)
                    | MarkdownNode::TableHead(children)
                    | MarkdownNode::TableRow(children)
                    | MarkdownNode::TableCell(children) => contains_node(children, predicate),
                    MarkdownNode::Heading { children, .. } => contains_node(children, predicate),
                    MarkdownNode::List { items, .. } => contains_node(items, predicate),
                    MarkdownNode::Link { children, .. } => contains_node(children, predicate),
                    MarkdownNode::Table { children, .. } => contains_node(children, predicate),
                    MarkdownNode::Text(_)
                    | MarkdownNode::SoftBreak
                    | MarkdownNode::HardBreak
                    | MarkdownNode::Rule
                    | MarkdownNode::InlineCode(_)
                    | MarkdownNode::CodeBlock { .. }
                    | MarkdownNode::TaskListMarker(_) => false,
                }
        })
    }
}
