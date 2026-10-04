use pulldown_cmark::{CowStr, Event, Options, Parser, Tag, html};

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

pub fn render(src: &str) -> String {
    let opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let events = Parser::new_ext(src, opts).map(|ev| match ev {
        Event::Html(h) | Event::InlineHtml(h) => Event::Text(h),
        Event::Start(Tag::Link { link_type, dest_url, title, id }) => {
            Event::Start(Tag::Link { link_type, dest_url: safe_link(dest_url), title, id })
        }
        Event::Start(Tag::Image { link_type, dest_url, title, id }) => {
            Event::Start(Tag::Image { link_type, dest_url: safe_image(dest_url), title, id })
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

    #[test]
    fn allowlists_urls() {
        let out = render("[a](java&#9;script:x) [b](https://x.dev) [c](/notes/1) ![d](https://x.dev/i.png) ![e](//x.dev/i.png) ![f](/api/files/1/raw)");
        assert_eq!(out.matches("href=\"#\"").count(), 1);
        assert!(out.contains("href=\"https://x.dev\"") && out.contains("href=\"/notes/1\""));
        assert_eq!(out.matches("src=\"#\"").count(), 2);
        assert!(out.contains("src=\"/api/files/1/raw\""));
    }
}
