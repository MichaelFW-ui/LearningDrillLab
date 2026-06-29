use dioxus::prelude::*;
use pulldown_cmark::{html, CowStr, Event, LinkType, Options, Parser, Tag, TagEnd};

#[component]
#[allow(non_snake_case)]
pub fn MarkdownContent(content: String, class: String) -> Element {
    let html = markdown_to_html(&content);
    let class_name = format!("{class} markdown-content");

    rsx! {
        div {
            class: "{class_name}",
            dangerous_inner_html: "{html}",
        }
    }
}

fn markdown_to_html(content: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_MATH);

    let events = Parser::new_ext(content, options).filter_map(sanitize_event);
    let mut output = String::new();
    html::push_html(&mut output, events);
    output
}

fn sanitize_event(event: Event<'_>) -> Option<Event<'static>> {
    match event.into_static() {
        Event::Start(tag) => sanitize_start_tag(tag).map(Event::Start),
        Event::End(TagEnd::HtmlBlock) => None,
        Event::Html(html) | Event::InlineHtml(html) => Some(Event::Text(html)),
        Event::InlineMath(math) => Some(Event::Html(CowStr::from(format!(
            r#"<span class="math math-inline">\({}\)</span>"#,
            escape_html_text(&math)
        )))),
        Event::DisplayMath(math) => Some(Event::Html(CowStr::from(format!(
            r#"<div class="math math-display">\[{}\]</div>"#,
            escape_html_text(&math)
        )))),
        other => Some(other),
    }
}

fn sanitize_start_tag(mut tag: Tag<'static>) -> Option<Tag<'static>> {
    match &mut tag {
        Tag::HtmlBlock => return None,
        Tag::Link {
            link_type,
            dest_url,
            ..
        } => {
            if !is_safe_link(*link_type, dest_url) {
                *dest_url = CowStr::from("#");
            }
        }
        Tag::Image { dest_url, .. } => {
            if !is_safe_url(dest_url) {
                *dest_url = CowStr::from("#");
            }
        }
        _ => {}
    }

    Some(tag)
}

fn is_safe_link(link_type: LinkType, href: &str) -> bool {
    match link_type {
        LinkType::Email => !href.contains(|ch: char| ch.is_control() || ch.is_whitespace()),
        _ => is_safe_url(href),
    }
}

fn is_safe_url(url: &str) -> bool {
    let trimmed = url.trim();
    trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
        || trimmed.starts_with("mailto:")
        || trimmed.starts_with('#')
}

fn escape_html_text(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());

    for ch in text.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(ch),
        }
    }

    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_common_markdown() {
        let html = markdown_to_html("这是 **加粗**、*斜体* 和 `code`。");

        assert!(html.contains("<strong>加粗</strong>"));
        assert!(html.contains("<em>斜体</em>"));
        assert!(html.contains("<code>code</code>"));
    }

    #[test]
    fn renders_tables() {
        let html = markdown_to_html("| 名称 | 说明 |\n| --- | --- |\n| Box | 堆分配 |\n");

        assert!(html.contains("<table>"));
        assert!(html.contains("<th>名称</th>"));
        assert!(html.contains("<td>Box</td>"));
    }

    #[test]
    fn renders_math_for_mathjax() {
        let html = markdown_to_html("行内 $a+b$。\n\n$$x^2 < y$$");

        assert!(html.contains(r#"<span class="math math-inline">\(a+b\)</span>"#));
        assert!(html.contains(r#"<div class="math math-display">\[x^2 &lt; y\]</div>"#));
    }

    #[test]
    fn raw_html_is_escaped() {
        let html = markdown_to_html("<script>alert(1)</script>");

        assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
        assert!(!html.contains("<script>"));
    }

    #[test]
    fn unsafe_links_are_neutralized() {
        let html = markdown_to_html("[bad](javascript:alert(1))");

        assert!(html.contains(r##"<a href="#">bad</a>"##));
    }
}
