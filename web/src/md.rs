use pulldown_cmark::{CowStr, Event, Options, Parser, Tag, TagEnd, html};

fn scheme(u: &str) -> Option<String> {
    let (scheme, _) = u.split_once(':')?;
    (!scheme.contains(['/', '?', '#'])).then(|| scheme.trim().to_ascii_lowercase())
}

fn safe_link(u: CowStr<'_>) -> CowStr<'_> {
    match scheme(&u).as_deref() {
        None | Some("http" | "https" | "mailto") => u,
        _ => "#".into(),
    }
}

fn safe_image(u: CowStr<'_>) -> CowStr<'_> {
    if scheme(&u).is_none() && !u.trim_start().starts_with("//") { u } else { "#".into() }
}

const OPTIONS: Options = Options::ENABLE_TABLES.union(Options::ENABLE_STRIKETHROUGH).union(Options::ENABLE_TASKLISTS);

pub fn first_image(src: &str) -> Option<String> {
    Parser::new_ext(src, OPTIONS).find_map(|ev| match ev {
        Event::Start(Tag::Image { dest_url, .. }) => Some(safe_image(dest_url).into_string()).filter(|u| u != "#"),
        _ => None,
    })
}

pub fn plain(src: &str) -> String {
    let mut out = String::new();
    let mut in_image = false;
    for ev in Parser::new_ext(src, OPTIONS) {
        match ev {
            Event::Start(Tag::Image { .. }) => in_image = true,
            Event::End(TagEnd::Image) => in_image = false,
            Event::Text(t) | Event::Code(t) if !in_image => out.push_str(&t),
            Event::SoftBreak | Event::HardBreak | Event::End(_) => out.push(' '),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn escape_attr(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#39;")
}

fn blocked_image(dest: &str, parser: &mut Parser) -> Event<'static> {
    let alt: String = parser
        .by_ref()
        .take_while(|e| !matches!(e, Event::End(TagEnd::Image)))
        .filter_map(|e| match e {
            Event::Text(t) | Event::Code(t) => Some(t.into_string()),
            _ => None,
        })
        .collect();
    Event::Html(format!("<img src=\"#\" data-src=\"{}\" alt=\"{}\">", escape_attr(dest), escape_attr(&alt)).into())
}

pub fn render(src: &str) -> String {
    let mut parser = Parser::new_ext(src, OPTIONS);
    let mut events = Vec::new();
    while let Some(ev) = parser.next() {
        events.push(match ev {
            Event::Html(h) | Event::InlineHtml(h) => Event::Text(h),
            Event::Start(Tag::Link { link_type, dest_url, title, id }) => {
                Event::Start(Tag::Link { link_type, dest_url: safe_link(dest_url), title, id })
            }
            Event::Start(Tag::Image { dest_url, .. }) if safe_image(dest_url.clone()).as_ref() == "#" => {
                blocked_image(&dest_url, &mut parser)
            }
            Event::Start(Tag::Image { link_type, dest_url, title, id }) => {
                Event::Start(Tag::Image { link_type, dest_url: safe_image(dest_url), title, id })
            }
            e => e,
        });
    }
    let mut out = String::new();
    html::push_html(&mut out, events.into_iter());
    out.replace("<a href=", "<a rel=\"external\" href=")
}

#[derive(Default)]
pub struct Element {
    pub tag: String,
    pub attrs: Vec<(String, String)>,
    pub checked: bool,
    pub children: Vec<Node>,
}

pub enum Node {
    Text(String),
    Element(Element),
}

impl Element {
    fn attr(&self, name: &str) -> &str {
        self.attrs.iter().find(|(k, _)| k == name).map_or("", |(_, v)| v.as_str())
    }

    fn image_src(&self) -> &str {
        Some(self.attr("data-src")).filter(|s| !s.is_empty()).unwrap_or_else(|| self.attr("src"))
    }
}

const BLOCKS: [&str; 19] = [
    "p", "div", "h1", "h2", "h3", "h4", "h5", "h6", "ul", "ol", "li", "blockquote", "pre", "hr", "table", "thead", "tbody",
    "tr", "figure",
];

fn element(n: &Node) -> Option<&Element> {
    match n {
        Node::Element(e) => Some(e),
        Node::Text(_) => None,
    }
}

fn is_block(n: &Node) -> bool {
    element(n).is_some_and(|e| BLOCKS.contains(&e.tag.as_str()))
}

pub fn to_markdown(nodes: &[Node]) -> String {
    blocks(nodes).join("\n\n")
}

fn blocks(nodes: &[Node]) -> Vec<String> {
    let mut out = Vec::new();
    let mut run = Vec::new();
    for n in nodes {
        if !is_block(n) {
            run.push(n);
            continue;
        }
        out.extend(paragraph(&run));
        run.clear();
        out.extend(element(n).and_then(block));
    }
    out.extend(paragraph(&run));
    out
}

fn paragraph(run: &[&Node]) -> Option<String> {
    let text: String = run.iter().map(|n| inline_node(n)).collect();
    let text = text.trim();
    (!text.is_empty()).then(|| text.lines().map(escape_line_start).collect::<Vec<_>>().join("\n"))
}

fn block(e: &Element) -> Option<String> {
    let text = match e.tag.as_str() {
        "ul" | "ol" => list(e),
        "blockquote" => prefix_lines(&to_markdown(&e.children), "> ", ">"),
        "pre" => code_block(e),
        "hr" => "---".to_owned(),
        "table" => table(e),
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            format!("{} {}", "#".repeat(e.tag[1..].parse().unwrap_or(1)), inline(&e.children).trim().replace('\n', " "))
        }
        _ => to_markdown(&e.children),
    };
    (!text.trim().trim_start_matches('#').trim().is_empty() || e.tag == "hr").then_some(text)
}

fn list(e: &Element) -> String {
    let mut number: usize = e.attr("start").parse().unwrap_or(1);
    let mut lines = Vec::new();
    for child in e.children.iter().filter_map(element) {
        let item = || blocks(&child.children).join("\n");
        let (marker, body) = match (child.tag.as_str(), e.tag.as_str()) {
            ("li", "ol") => (format!("{number}. "), item()),
            ("li", _) => ("- ".to_owned(), item()),
            _ => ("  ".to_owned(), block(child).unwrap_or_default()),
        };
        number += usize::from(child.tag == "li");
        lines.push(indent(&body, &marker));
    }
    lines.retain(|l| !l.trim().is_empty());
    lines.join("\n")
}

fn indent(body: &str, marker: &str) -> String {
    let pad = " ".repeat(marker.len());
    let mut out = String::new();
    for (i, line) in body.lines().enumerate() {
        let lead = if i == 0 { marker } else if line.is_empty() { "" } else { &pad };
        out.push_str(&format!("{}{lead}{line}", if i == 0 { "" } else { "\n" }));
    }
    if out.is_empty() { marker.trim_end().to_owned() } else { out }
}

fn prefix_lines(body: &str, prefix: &str, empty: &str) -> String {
    body.lines().map(|l| if l.is_empty() { empty.to_owned() } else { format!("{prefix}{l}") }).collect::<Vec<_>>().join("\n")
}

fn code_block(e: &Element) -> String {
    let lang = e.children.iter().filter_map(element).find_map(|c| c.attr("class").strip_prefix("language-")).unwrap_or("");
    format!("```{lang}\n{}\n```", raw_text(&e.children).trim_end_matches('\n'))
}

fn rows(e: &Element) -> Vec<&Element> {
    e.children.iter().filter_map(element).flat_map(|c| if c.tag == "tr" { vec![c] } else { rows(c) }).collect()
}

fn table(e: &Element) -> String {
    let rows: Vec<Vec<String>> = rows(e)
        .into_iter()
        .map(|r| r.children.iter().filter_map(element).map(|c| inline(&c.children).trim().replace('\n', " ")).collect())
        .collect();
    let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
    let line = |cells: &[String]| format!("| {} |", (0..cols).map(|i| cells.get(i).map_or("", String::as_str)).collect::<Vec<_>>().join(" | "));
    let mut out: Vec<String> = rows.iter().map(|r| line(r)).collect();
    if !out.is_empty() {
        out.insert(1, line(&vec!["---".to_owned(); cols]));
    }
    out.join("\n")
}

fn raw_text(nodes: &[Node]) -> String {
    nodes
        .iter()
        .map(|n| match n {
            Node::Text(t) => t.replace('\u{a0}', " "),
            Node::Element(e) if e.tag == "br" => "\n".to_owned(),
            Node::Element(e) => raw_text(&e.children),
        })
        .collect()
}

fn inline(nodes: &[Node]) -> String {
    nodes.iter().map(inline_node).collect()
}

fn delimiter(tag: &str) -> Option<&'static str> {
    match tag {
        "strong" | "b" => Some("**"),
        "em" | "i" => Some("*"),
        "s" | "del" | "strike" => Some("~~"),
        _ => None,
    }
}

fn inline_node(n: &Node) -> String {
    let e = match n {
        Node::Text(t) => return escape(t),
        Node::Element(e) => e,
    };
    if let Some(d) = delimiter(&e.tag) {
        return wrap(d, &inline(&e.children));
    }
    match e.tag.as_str() {
        "code" => format!("`{}`", raw_text(&e.children)),
        "a" if !e.attr("href").is_empty() => format!("[{}]({})", inline(&e.children), e.attr("href")),
        "img" => format!("![{}]({})", escape(e.attr("alt")), e.image_src()),
        "br" => "  \n".to_owned(),
        "input" => (if e.checked { "[x] " } else { "[ ] " }).to_owned(),
        _ => inline(&e.children),
    }
}

fn wrap(delimiter: &str, inner: &str) -> String {
    let core = inner.trim();
    if core.is_empty() {
        return inner.to_owned();
    }
    let lead = &inner[..inner.len() - inner.trim_start().len()];
    let tail = &inner[inner.trim_end().len()..];
    format!("{lead}{delimiter}{core}{delimiter}{tail}")
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\u{a0}' | '\n' => out.push(' '),
            '\\' | '*' | '_' | '`' | '[' | ']' | '<' | '>' | '~' | '|' | '&' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

fn escape_line_start(line: &str) -> String {
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    if line.starts_with(['#', '-', '+', '=']) {
        format!("\\{line}")
    } else if digits > 0 && line[digits..].starts_with(['.', ')']) {
        format!("{}\\{}", &line[..digits], &line[digits..])
    } else {
        line.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::{Element, Node, first_image, plain, render, to_markdown};

    fn t(s: &str) -> Node {
        Node::Text(s.to_owned())
    }

    fn el(tag: &str, children: Vec<Node>) -> Node {
        Node::Element(Element { tag: tag.to_owned(), children, ..Default::default() })
    }

    fn with(tag: &str, attrs: &[(&str, &str)], children: Vec<Node>) -> Node {
        let attrs = attrs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
        Node::Element(Element { tag: tag.to_owned(), attrs, children, ..Default::default() })
    }

    #[test]
    fn serializes_rich_text_to_markdown() {
        let doc = vec![
            el("h2", vec![t("Plan")]),
            t("\n"),
            el("p", vec![t("Hello "), el("b", vec![t("bold ")]), t("and "), el("i", vec![t("it")]), el("br", vec![]), t("next\u{a0}line")]),
            el("ul", vec![el("li", vec![t("one")]), el("li", vec![t("two"), el("ol", vec![el("li", vec![t("deep")])])])]),
            with("ol", &[("start", "3")], vec![el("li", vec![t("three")])]),
            el("blockquote", vec![el("p", vec![t("quoted")])]),
            el("pre", vec![with("code", &[("class", "language-rust")], vec![t("let x = 1;\n")])]),
            el("p", vec![with("a", &[("href", "https://x.dev")], vec![t("link")]), t(" "), with("img", &[("src", "/f/raw"), ("alt", "pic")], vec![])]),
            el("p", vec![t("1. not a list * 2 #tag")]),
            el("p", vec![el("br", vec![])]),
        ];
        assert_eq!(
            to_markdown(&doc),
            "## Plan\n\nHello **bold** and *it*  \nnext line\n\n- one\n- two\n  1. deep\n\n3. three\n\n> quoted\n\n```rust\nlet x = 1;\n```\n\n[link](https://x.dev) ![pic](/f/raw)\n\n1\\. not a list \\* 2 #tag"
        );
    }

    #[test]
    fn keeps_tasks_tables_and_blocked_images() {
        let mut done = Element { tag: "input".into(), checked: true, ..Default::default() };
        done.attrs.push(("type".into(), "checkbox".into()));
        let doc = vec![
            el("ul", vec![el("li", vec![Node::Element(done), t("shipped")])]),
            el("table", vec![el("thead", vec![el("tr", vec![el("th", vec![t("a")]), el("th", vec![t("b")])])]), el("tbody", vec![el("tr", vec![el("td", vec![t("1")]), el("td", vec![t("2")])])])]),
            el("p", vec![with("img", &[("src", "#"), ("data-src", "https://x.dev/i.png"), ("alt", "x")], vec![])]),
        ];
        assert_eq!(to_markdown(&doc), "- [x] shipped\n\n| a | b |\n| --- | --- |\n| 1 | 2 |\n\n![x](https://x.dev/i.png)");
        let html = render("![x](https://x.dev/i.png) ![y](/ok.png)");
        assert!(html.contains("src=\"#\" data-src=\"https://x.dev/i.png\" alt=\"x\""));
        assert!(html.contains("src=\"/ok.png\""));
    }

    #[test]
    fn finds_cover_and_plain_text() {
        let src = "Intro **bold**\n\n![shot](javascript:x) ![shot](/api/files/1/raw)\n\n- item";
        assert_eq!(first_image(src).as_deref(), Some("/api/files/1/raw"));
        assert_eq!(plain(src), "Intro bold item");
        assert_eq!(first_image("no image"), None);
    }

    #[test]
    fn strips_html_and_js_links() {
        let out = render("<script>alert(1)</script>\n\n[x](javascript:alert(1)) **b**");
        assert!(!out.contains("<script>"));
        assert!(out.contains("href=\"#\""));
        assert!(out.contains("<strong>b</strong>"));
    }

    #[test]
    fn allowlists_urls() {
        let out = render("[a](java&#9;script:x) [b](https://x.dev) [c](/notes/1) ![d](https://x.dev/i.png) ![e](//x.dev/i.png) ![f](/api/files/1/raw)");
        assert_eq!(out.matches("href=\"#\"").count(), 1);
        assert!(out.contains("href=\"https://x.dev\"") && out.contains("href=\"/notes/1\""));
        assert_eq!(out.matches("src=\"#\"").count(), 2);
        assert!(out.contains("src=\"/api/files/1/raw\""));
    }
}
