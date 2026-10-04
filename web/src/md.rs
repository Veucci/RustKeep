use pulldown_cmark::{CowStr, Event, Options, Parser, Tag, html};

fn safe_url(u: CowStr<'_>) -> CowStr<'_> {
    let l = u.trim().to_ascii_lowercase();
    if ["javascript:", "vbscript:", "data:"].iter().any(|s| l.starts_with(s)) { "#".into() } else { u }
}

pub fn render(src: &str) -> String {
    let opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let events = Parser::new_ext(src, opts).map(|ev| match ev {
        Event::Html(h) | Event::InlineHtml(h) => Event::Text(h),
        Event::Start(Tag::Link { link_type, dest_url, title, id }) => {
            Event::Start(Tag::Link { link_type, dest_url: safe_url(dest_url), title, id })
        }
        Event::Start(Tag::Image { link_type, dest_url, title, id }) => {
            Event::Start(Tag::Image { link_type, dest_url: safe_url(dest_url), title, id })
        }
        e => e,
    });
    let mut out = String::new();
    html::push_html(&mut out, events);
    out
}

#[cfg(test)]
mod tests {
    use super::render;

    #[test]
    fn strips_html_and_js_links() {
        let out = render("<script>alert(1)</script>\n\n[x](javascript:alert(1)) **b**");
        assert!(!out.contains("<script>"));
        assert!(out.contains("href=\"#\""));
        assert!(out.contains("<strong>b</strong>"));
    }
}
