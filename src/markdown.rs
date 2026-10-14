use std::collections::HashMap;
use std::io::Cursor;
use std::sync::LazyLock;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use image::ImageFormat;
use pulldown_cmark::{Alignment, BlockQuoteKind, CodeBlockKind, CowStr, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use resvg::usvg;
use syntect::highlighting::ThemeSet;
use syntect::html::{ClassStyle, ClassedHTMLGenerator, css_for_theme_with_class_style};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;
use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime};
use typst_layout::PagedDocument;
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};

use crate::export::FONTS;
use crate::util::{Res, bad, esc};

pub const OPTIONS: Options = Options::ENABLE_TABLES
    .union(Options::ENABLE_STRIKETHROUGH)
    .union(Options::ENABLE_TASKLISTS)
    .union(Options::ENABLE_FOOTNOTES)
    .union(Options::ENABLE_GFM);

const MONO: [&[u8]; 2] = [include_bytes!("../assets/fonts/DejaVuSansMono.ttf"), include_bytes!("../assets/fonts/DejaVuSansMono-Bold.ttf")];
const HIGHLIGHT: ClassStyle = ClassStyle::SpacedPrefixed { prefix: "hl-" };
const MAX_IMAGES: usize = 50;

struct Picture {
    mime: &'static str,
    bytes: Vec<u8>,
}

pub type Images = HashMap<String, Vec<u8>>;
type Pictures = HashMap<String, Picture>;

fn picture(bytes: Vec<u8>) -> Option<Picture> {
    let Ok(format) = image::guess_format(&bytes) else {
        usvg::Tree::from_data(&bytes, &usvg::Options::default()).ok()?;
        return Some(Picture { mime: "image/svg+xml", bytes });
    };
    let decoded = image::load_from_memory_with_format(&bytes, format).ok()?;
    if matches!(format, ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Gif) {
        return Some(Picture { mime: format.to_mime_type(), bytes });
    }
    let mut png = Cursor::new(Vec::new());
    decoded.write_to(&mut png, ImageFormat::Png).ok()?;
    Some(Picture { mime: "image/png", bytes: png.into_inner() })
}

fn pictures(images: Images) -> Pictures {
    images.into_iter().filter_map(|(source, bytes)| picture(bytes).map(|p| (source, p))).collect()
}

fn attribute<'a>(html: &'a str, name: &str) -> Option<&'a str> {
    let start = html.find(&format!("{name}=\""))? + name.len() + 2;
    html[start..].split('"').next()
}

fn html_image(html: &str) -> Option<&str> {
    html.trim_start().to_ascii_lowercase().starts_with("<img").then(|| attribute(html, "src"))?
}

pub fn image_sources(markdown: &str) -> Vec<String> {
    let mut sources: Vec<String> = Vec::new();
    for event in Parser::new_ext(markdown, OPTIONS) {
        let source = match &event {
            Event::Start(Tag::Image { dest_url, .. }) => Some(dest_url.as_ref()),
            Event::Html(h) | Event::InlineHtml(h) => html_image(h),
            _ => None,
        };
        source.filter(|s| !sources.iter().any(|known| known == s)).into_iter().for_each(|s| sources.push(s.to_owned()));
    }
    sources.truncate(MAX_IMAGES);
    sources
}

fn mermaid(code: &str) -> Option<String> {
    let options = mermaid_rs_renderer::RenderOptions { theme: mermaid_rs_renderer::Theme::mermaid_default(), ..Default::default() };
    let svg = mermaid_rs_renderer::render_with_options(code, options).ok()?;
    picture(svg.clone().into_bytes()).map(|_| svg)
}

fn code_language(kind: &CodeBlockKind) -> String {
    match kind {
        CodeBlockKind::Fenced(info) => info.split([' ', ',', '{']).next().unwrap_or_default().to_ascii_lowercase(),
        CodeBlockKind::Indented => String::new(),
    }
}

fn collect_text<'a>(events: impl Iterator<Item = Event<'a>>) -> String {
    events
        .take_while(|e| !matches!(e, Event::End(TagEnd::CodeBlock | TagEnd::Image)))
        .filter_map(|e| match e {
            Event::Text(t) | Event::Code(t) => Some(t.into_string()),
            _ => None,
        })
        .collect()
}

fn quote(kind: Option<BlockQuoteKind>) -> &'static str {
    match kind {
        None => "#quote(block: true)[",
        Some(BlockQuoteKind::Note) => "#alert(\"Note\", \"#0969da\")[",
        Some(BlockQuoteKind::Tip) => "#alert(\"Tip\", \"#1a7f37\")[",
        Some(BlockQuoteKind::Important) => "#alert(\"Important\", \"#8250df\")[",
        Some(BlockQuoteKind::Warning) => "#alert(\"Warning\", \"#9a6700\")[",
        Some(BlockQuoteKind::Caution) => "#alert(\"Caution\", \"#cf222e\")[",
    }
}

static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(two_face::syntax::extra_newlines);

static HIGHLIGHT_CSS: LazyLock<String> = LazyLock::new(|| {
    let themes = ThemeSet::load_defaults();
    let css = |name: &str| themes.themes.get(name).and_then(|t| css_for_theme_with_class_style(t, HIGHLIGHT).ok()).unwrap_or_default();
    format!("{}\n@media (prefers-color-scheme: dark) {{\n{}\n}}", css("InspiredGitHub"), css("base16-ocean.dark"))
});

fn highlight(language: &str, code: &str) -> String {
    let Some(syntax) = SYNTAXES.find_syntax_by_token(language).filter(|_| !language.is_empty()) else {
        return format!("<pre><code>{}</code></pre>\n", esc(code));
    };
    let mut generator = ClassedHTMLGenerator::new_with_class_style(syntax, &SYNTAXES, HIGHLIGHT);
    for line in LinesWithEndings::from(code) {
        if generator.parse_html_for_line_which_includes_newline(line).is_err() {
            return format!("<pre><code>{}</code></pre>\n", esc(code));
        }
    }
    format!("<pre class=\"hl-code\"><code class=\"language-{}\">{}</code></pre>\n", esc(language), generator.finalize())
}

fn html_code_block(kind: &CodeBlockKind, code: &str) -> String {
    let language = code_language(kind);
    match (language.as_str(), mermaid(code)) {
        ("mermaid", Some(svg)) => format!("<figure class=\"diagram\">{svg}</figure>\n"),
        _ => highlight(&language, code),
    }
}

fn data_uri(picture: &Picture) -> String {
    format!("data:{};base64,{}", picture.mime, B64.encode(&picture.bytes))
}

fn embed_html_images(html: &str, images: &Pictures) -> String {
    match html_image(html).and_then(|src| images.get(src).map(|p| (src, p))) {
        Some((src, picture)) => html.replacen(&format!("src=\"{src}\""), &format!("src=\"{}\"", data_uri(picture)), 1),
        None => html.to_owned(),
    }
}

pub fn html(title: &str, markdown: &str, images: Images) -> String {
    let images = pictures(images);
    let mut parser = Parser::new_ext(markdown, OPTIONS);
    let mut events = Vec::new();
    while let Some(event) = parser.next() {
        events.push(match event {
            Event::Start(Tag::CodeBlock(kind)) => Event::Html(html_code_block(&kind, &collect_text(parser.by_ref())).into()),
            Event::Start(Tag::Image { link_type, dest_url, title, id }) => {
                let dest_url = images.get(dest_url.as_ref()).map_or(dest_url, |p| data_uri(p).into());
                Event::Start(Tag::Image { link_type, dest_url, title, id })
            }
            Event::Html(h) => Event::Html(embed_html_images(&h, &images).into()),
            Event::InlineHtml(h) => Event::InlineHtml(embed_html_images(&h, &images).into()),
            event => event,
        });
    }
    let mut body = String::new();
    pulldown_cmark::html::push_html(&mut body, events.into_iter());
    page(title, &body)
}

const PAGE_CSS: &str = r#"
:root { color-scheme: light dark; --fg: #1f2328; --muted: #59636e; --bg: #ffffff; --subtle: #f6f8fa; --border: #d1d9e0; --link: #0969da; }
@media (prefers-color-scheme: dark) { :root { --fg: #e6edf3; --muted: #9198a1; --bg: #0d1117; --subtle: #151b23; --border: #3d444d; --link: #4493f8; } }
* { box-sizing: border-box; }
body { margin: 0; background: var(--bg); color: var(--fg); font: 16px/1.6 -apple-system, BlinkMacSystemFont, "Segoe UI", "Noto Sans", Helvetica, Arial, sans-serif; }
main { max-width: 900px; margin: 0 auto; padding: 40px 24px 64px; overflow-wrap: break-word; }
h1, h2, h3, h4, h5, h6 { margin: 1.5em 0 0.6em; font-weight: 600; line-height: 1.25; }
h1 { font-size: 2em; padding-bottom: 0.3em; border-bottom: 1px solid var(--border); }
h2 { font-size: 1.5em; padding-bottom: 0.3em; border-bottom: 1px solid var(--border); }
h3 { font-size: 1.25em; }
h6 { color: var(--muted); }
main > :first-child { margin-top: 0; }
p, ul, ol, table, pre, blockquote, figure, details { margin: 0 0 1em; }
a { color: var(--link); text-decoration: none; }
a:hover { text-decoration: underline; }
img { max-width: 100%; height: auto; }
hr { height: 4px; margin: 1.5em 0; border: 0; background: var(--border); }
ul, ol { padding-left: 2em; }
li + li { margin-top: 0.25em; }
li:has(> input[type=checkbox]) { list-style: none; }
li > input[type=checkbox] { margin: 0 0.4em 0 -1.4em; vertical-align: middle; }
code, pre { font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, "Liberation Mono", monospace; font-size: 85%; }
:not(pre) > code { padding: 0.2em 0.4em; border-radius: 6px; background: var(--subtle); border: 1px solid var(--border); }
pre { padding: 16px; overflow: auto; line-height: 1.45; border-radius: 6px; background: var(--subtle); border: 1px solid var(--border); }
pre code { font-size: 100%; background: none !important; }
blockquote { padding: 0 1em; color: var(--muted); border-left: 0.25em solid var(--border); }
blockquote[class^=markdown-alert] { color: var(--fg); border-left-color: var(--alert); }
blockquote[class^=markdown-alert]::before { display: block; padding-top: 0.5em; font-weight: 600; color: var(--alert); }
.markdown-alert-note { --alert: #0969da; } .markdown-alert-note::before { content: "Note"; }
.markdown-alert-tip { --alert: #1a7f37; } .markdown-alert-tip::before { content: "Tip"; }
.markdown-alert-important { --alert: #8250df; } .markdown-alert-important::before { content: "Important"; }
.markdown-alert-warning { --alert: #9a6700; } .markdown-alert-warning::before { content: "Warning"; }
.markdown-alert-caution { --alert: #cf222e; } .markdown-alert-caution::before { content: "Caution"; }
table { display: block; width: max-content; max-width: 100%; overflow: auto; border-collapse: collapse; }
th, td { padding: 6px 13px; border: 1px solid var(--border); }
th { font-weight: 600; background: var(--subtle); }
tbody tr:nth-child(2n) { background: var(--subtle); }
figure.diagram { margin: 1.5em 0; text-align: center; overflow-x: auto; }
figure.diagram svg { max-width: 100%; height: auto; border-radius: 6px; background: #ffffff; }
.footnote-definition { display: flex; gap: 0.5em; font-size: 0.875em; color: var(--muted); }
.footnote-definition p { margin: 0; }
sup.footnote-reference a { padding: 0 0.1em; }
"#;

pub fn page(title: &str, body: &str) -> String {
    format!(
        "<!doctype html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>{}</title>\n<style>{PAGE_CSS}{}</style>\n</head>\n<body>\n<main>\n{body}</main>\n</body>\n</html>\n",
        esc(title),
        *HIGHLIGHT_CSS
    )
}

const PREAMBLE: &str = r##"
#set page(paper: "a4", margin: (x: 2cm, y: 2.2cm), numbering: "1")
#set text(font: ("Liberation Sans", "DejaVu Sans Mono"), size: 10.5pt, fill: rgb("#1f2328"))
#set par(leading: 0.7em, spacing: 1.2em)
#show heading: set block(above: 1.6em, below: 0.9em)
#show heading.where(level: 1): set text(size: 20pt)
#show heading.where(level: 2): set text(size: 16pt)
#show heading.where(level: 3): set text(size: 13pt)
#show heading.where(level: 1): it => block(width: 100%, stroke: (bottom: 0.6pt + luma(215)), inset: (bottom: 0.35em), it)
#show heading.where(level: 2): it => block(width: 100%, stroke: (bottom: 0.6pt + luma(215)), inset: (bottom: 0.3em), it)
#show link: set text(fill: rgb("#0969da"))
#show raw: set text(font: "DejaVu Sans Mono", size: 8.8pt)
#show raw.where(block: false): box.with(fill: luma(240), inset: (x: 3pt), outset: (y: 2.5pt), radius: 3pt)
#show raw.where(block: true): block.with(width: 100%, fill: rgb("#f6f8fa"), inset: 10pt, radius: 5pt, stroke: 0.5pt + luma(218))
#show quote.where(block: true): it => block(width: 100%, stroke: (left: 3pt + luma(208)), inset: (left: 12pt, y: 4pt), text(fill: luma(90), it.body))
#set table(stroke: 0.5pt + luma(208), inset: (x: 8pt, y: 6pt), fill: (_, y) => if y == 0 { luma(240) } else if calc.even(y) { luma(250) })
#show table.cell.where(y: 0): strong
#show footnote.entry: set text(size: 8.5pt)
#let task(done) = box(baseline: 0.12em, width: 0.85em, height: 0.85em, radius: 2pt, stroke: 0.7pt + if done { rgb("#0969da") } else { luma(140) }, fill: if done { rgb("#0969da") } else { none }, if done { align(center + horizon, text(fill: white, size: 0.7em, weight: "bold", "✓")) })
#let picture(path, alt) = box(image(path, alt: alt))
#let diagram(path) = align(center, block(above: 1.4em, below: 1.4em, image(path)))
#let missing(alt) = box(stroke: 0.5pt + luma(190), radius: 3pt, inset: (x: 5pt, y: 3pt), text(size: 0.85em, fill: luma(110), "Image: " + alt))
#let alert(label, color, body) = block(width: 100%, stroke: (left: 3pt + rgb(color)), inset: (left: 12pt, y: 4pt), [#text(fill: rgb(color), weight: "bold", label) \ #body])
"##;

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars().filter(|c| !c.is_control()) {
        if c.is_ascii_punctuation() {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn string(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '\\' | '"' => out.extend(['\\', c]),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn footnote_marker(label: &str) -> String {
    format!("\u{1}{label}\u{2}")
}

fn align(alignment: &Alignment) -> &'static str {
    match alignment {
        Alignment::Center => "center",
        Alignment::Right => "right",
        Alignment::None | Alignment::Left => "left",
    }
}

fn opening(tag: &Tag) -> String {
    match tag {
        Tag::Heading { level, .. } => format!("#heading(level: {})[", *level as u8),
        Tag::BlockQuote(kind) => quote(*kind).into(),
        Tag::Item | Tag::TableCell => "[".into(),
        Tag::Emphasis => "#emph[".into(),
        Tag::Strong => "#strong[".into(),
        Tag::Strikethrough => "#strike[".into(),
        Tag::Link { dest_url, .. } if dest_url.is_empty() => "#[".into(),
        Tag::Link { dest_url, .. } => format!("#link({})[", string(dest_url)),
        Tag::TableHead => "table.header(".into(),
        _ => String::new(),
    }
}

fn closing(tag: &TagEnd) -> &'static str {
    match tag {
        TagEnd::Paragraph => "\n\n",
        TagEnd::Heading(_) | TagEnd::BlockQuote(_) => "]\n",
        TagEnd::Item | TagEnd::TableCell => "], ",
        TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link => "]",
        TagEnd::TableHead => "), ",
        TagEnd::Table | TagEnd::List(_) => ")\n",
        _ => "",
    }
}

struct Typst<'a> {
    out: String,
    files: Vec<(String, Vec<u8>)>,
    images: &'a Pictures,
    lists: Vec<(usize, bool)>,
    notes: HashMap<String, String>,
    pending: Vec<(String, String)>,
}

impl<'a> Typst<'a> {
    fn new(images: &'a Pictures) -> Self {
        Self { out: String::new(), files: Vec::new(), images, lists: Vec::new(), notes: HashMap::new(), pending: Vec::new() }
    }

    fn file(&mut self, bytes: Vec<u8>) -> String {
        let path = format!("/asset-{}", self.files.len());
        self.files.push((path.clone(), bytes));
        string(&path)
    }

    fn image(&mut self, url: &str, alt: &str) {
        let markup = match self.images.get(url) {
            Some(picture) => format!("#picture({}, {})", self.file(picture.bytes.clone()), string(alt)),
            None => format!("#missing({})", string(if alt.is_empty() { url } else { alt })),
        };
        self.out.push_str(&markup);
    }

    fn code_block(&mut self, kind: &CodeBlockKind, code: &str) {
        let language = code_language(kind);
        let markup = match (language.as_str(), mermaid(code)) {
            ("mermaid", Some(svg)) => format!("#diagram({})\n", self.file(svg.into_bytes())),
            _ => format!("#raw(block: true, lang: {}, {})\n", string(&language), string(code.trim_end_matches('\n'))),
        };
        self.out.push_str(&markup);
    }

    fn table(&mut self, alignments: &[Alignment]) {
        let align: Vec<&str> = alignments.iter().map(align).collect();
        self.out.push_str(&format!("#table(columns: {}, align: ({},), ", alignments.len().max(1), align.join(", ")));
    }

    fn list(&mut self, start: Option<u64>) {
        self.out.push_str(&start.map_or("#list(".into(), |n| format!("#enum(start: {n}, ")));
        self.lists.push((self.out.len(), start.is_some()));
    }

    fn task(&mut self, done: bool) {
        if let Some((position, marked)) = self.lists.last_mut().filter(|(_, marked)| !marked) {
            *marked = true;
            self.out.insert_str(*position, "marker: [], ");
        }
        self.out.push_str(&format!("#task({done}) "));
    }

    fn start<'e>(&mut self, tag: Tag<'e>, events: &mut dyn Iterator<Item = Event<'e>>) {
        match tag {
            Tag::CodeBlock(kind) => self.code_block(&kind, &collect_text(events)),
            Tag::Image { dest_url, .. } => self.image(&dest_url, &collect_text(events)),
            Tag::List(start) => self.list(start),
            Tag::Table(alignments) => self.table(&alignments),
            Tag::FootnoteDefinition(label) => self.pending.push((label.into_string(), std::mem::take(&mut self.out))),
            tag => self.out.push_str(&opening(&tag)),
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::List(_) => _ = self.lists.pop(),
            TagEnd::FootnoteDefinition => self.close_footnote(),
            _ => {}
        }
        self.out.push_str(closing(&tag));
    }

    fn close_footnote(&mut self) {
        if let Some((label, outer)) = self.pending.pop() {
            let body = std::mem::replace(&mut self.out, outer);
            self.notes.insert(label, body.trim_end().to_owned());
        }
    }

    fn html(&mut self, html: &str) {
        let lower = html.trim().to_ascii_lowercase();
        if lower.starts_with("<br") {
            self.out.push_str("#linebreak()");
        }
        if let Some(src) = html_image(html) {
            self.image(src, attribute(html, "alt").unwrap_or_default());
        }
    }

    fn leaf(&mut self, event: Event) {
        match event {
            Event::Text(text) => self.out.push_str(&escape(&text)),
            Event::Code(code) => self.out.push_str(&format!("#raw({})", string(&code))),
            Event::Html(html) | Event::InlineHtml(html) => self.html(&html),
            Event::SoftBreak => self.out.push(' '),
            Event::HardBreak => self.out.push_str("#linebreak()"),
            Event::Rule => self.out.push_str("#line(length: 100%, stroke: 0.8pt + luma(215))\n"),
            Event::TaskListMarker(done) => self.task(done),
            Event::FootnoteReference(label) => self.out.push_str(&footnote_marker(&label)),
            _ => {}
        }
    }

    fn write<'e>(&mut self, mut events: impl Iterator<Item = Event<'e>>) {
        while let Some(event) = events.next() {
            match event {
                Event::Start(tag) => self.start(tag, &mut events),
                Event::End(tag) => self.end(tag),
                event => self.leaf(event),
            }
        }
    }

    fn finish(self) -> (String, Vec<(String, Vec<u8>)>) {
        let out = self.notes.iter().fold(self.out, |out, (label, body)| out.replace(&footnote_marker(label), &format!("#footnote[{body}]")));
        (out, self.files)
    }
}

static FONT_SET: LazyLock<(LazyHash<FontBook>, Vec<Font>)> = LazyLock::new(|| {
    let fonts: Vec<Font> = FONTS.iter().chain(&MONO).flat_map(|data| Font::iter(Bytes::new(*data))).collect();
    (LazyHash::new(FontBook::from_fonts(&fonts)), fonts)
});

static LIBRARY: LazyLock<LazyHash<Library>> = LazyLock::new(|| LazyHash::new(Library::default()));

fn file_id(path: &str) -> FileId {
    let path = VirtualPath::new(path).unwrap_or_else(|_| VirtualPath::new("/main.typ").expect("valid path"));
    RootedPath::new(VirtualRoot::Project, path).intern()
}

struct Document {
    main: Source,
    files: HashMap<FileId, Bytes>,
}

impl World for Document {
    fn library(&self) -> &LazyHash<Library> {
        &LIBRARY
    }

    fn book(&self) -> &LazyHash<FontBook> {
        &FONT_SET.0
    }

    fn main(&self) -> FileId {
        self.main.id()
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main.id() { Ok(self.main.clone()) } else { Err(FileError::AccessDenied) }
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        self.files.get(&id).cloned().ok_or(FileError::AccessDenied)
    }

    fn font(&self, index: usize) -> Option<Font> {
        FONT_SET.1.get(index).cloned()
    }

    fn today(&self, _: Option<typst::foundations::Duration>) -> Option<Datetime> {
        None
    }
}

fn compile<'e>(title: &str, events: impl Iterator<Item = Event<'e>>, images: Images) -> Res<Vec<u8>> {
    let images = pictures(images);
    let mut writer = Typst::new(&images);
    writer.write(events);
    let (body, files) = writer.finish();
    let source = format!("#set document(title: {})\n{PREAMBLE}\n{body}", string(title));
    let files = files.into_iter().map(|(path, bytes)| (file_id(&path), Bytes::new(bytes))).collect();
    let world = Document { main: Source::new(file_id("/main.typ"), source), files };
    let document = typst::compile::<PagedDocument>(&world).output.map_err(|errors| {
        errors.iter().for_each(|e| eprintln!("error: pdf layout: {}", e.message));
        bad("The document could not be laid out")
    })?;
    typst_pdf::pdf(&document, &typst_pdf::PdfOptions::default()).map_err(|_| bad("The PDF could not be written"))
}

fn heading(title: &str) -> [Event<'_>; 3] {
    let tag = Tag::Heading { level: HeadingLevel::H1, id: None, classes: Vec::new(), attrs: Vec::new() };
    [Event::Start(tag), Event::Text(CowStr::Borrowed(title)), Event::End(TagEnd::Heading(HeadingLevel::H1))]
}

pub fn pdf(title: &str, markdown: &str, images: Images, with_title: bool) -> Res<Vec<u8>> {
    let lead = with_title.then(|| heading(title)).into_iter().flatten();
    compile(title, lead.chain(Parser::new_ext(markdown, OPTIONS)), images)
}

pub fn plain_pdf(title: &str, text: &str) -> Res<Vec<u8>> {
    let text = text.replace("\r\n", "\n");
    let paragraphs = text.split("\n\n").filter(|p| !p.trim().is_empty()).flat_map(|p| {
        let lines = p.trim_matches('\n').split('\n').map(|line| Event::Text(line.into()));
        let lines = lines.flat_map(|line| [Event::HardBreak, line]).skip(1);
        std::iter::once(Event::Start(Tag::Paragraph)).chain(lines).chain([Event::End(TagEnd::Paragraph)])
    });
    compile(title, paragraphs.collect::<Vec<_>>().into_iter(), Images::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# Title\n\nText with **bold**, *it*, ~~gone~~, `code` and [link](https://x.dev) #1 $5 a < b @me.\n\n\
        | Name | Qty |\n|:-----|----:|\n| Apple | 3 |\n| Pear | 10 |\n\n\
        - [x] done\n- [ ] todo\n  1. nested\n\n> [!WARNING]\n> Careful\n\n> plain quote\n\n\
        ```rust\nfn main() {}\n```\n\n```mermaid\nflowchart LR\n  A[Start] --> B{Ok?}\n  B -->|Yes| C[Done]\n```\n\n\
        Footnote here[^n].\n\n[^n]: The note.\n\n---\n\n![logo](logo.png) ![gone](missing.png)\n\n<img src=\"logo.png\" alt=\"html\">\n";

    fn images() -> Images {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="20" height="10" fill="red"/></svg>"#;
        Images::from([("logo.png".to_owned(), svg.to_vec()), ("missing.png".to_owned(), b"broken".to_vec())])
    }

    #[test]
    fn collects_image_sources() {
        assert_eq!(image_sources(SAMPLE), ["logo.png", "missing.png"]);
        assert!(picture(b"not an image".to_vec()).is_none());
    }

    #[test]
    fn writes_typst_markup() {
        let images = pictures(images());
        let mut writer = Typst::new(&images);
        writer.write(Parser::new_ext(SAMPLE, OPTIONS));
        let (out, files) = writer.finish();
        assert!(out.contains("#table(columns: 2, align: (left, right,), table.header([Name], [Qty], ), [Apple], [3], "));
        assert!(out.contains("#list(marker: [], [#task(true) done], [#task(false) todo#enum(start: 1, [nested], )"));
        assert!(out.contains("#alert(\"Warning\", \"#9a6700\")["));
        assert!(out.contains("#footnote[The note\\.]"));
        assert!(out.contains("\\#1 \\$5 a \\< b \\@me\\."));
        assert!(out.contains("#diagram(\"/asset-0\")"));
        assert!(out.contains("#missing(\"gone\")"));
        assert!(pdf("Links", "[empty]() [anchor](#top) [rel](docs/a.md)", Images::new(), false).is_ok());
        assert_eq!(files.len(), 3);
    }

    #[test]
    fn renders_pdf() {
        let pdf = pdf("Sample", SAMPLE, images(), true).unwrap();
        assert!(pdf.starts_with(b"%PDF"));
        assert!(plain_pdf("Plain", "one #[x]\nline\n\ntwo \\ \"q\"").unwrap().starts_with(b"%PDF"));
    }

    #[test]
    fn renders_html() {
        let page = html("Sample <x>", SAMPLE, images());
        assert!(page.contains("<title>Sample &lt;x&gt;</title>"));
        assert!(page.contains("<table>") && page.contains("<th style=\"text-align: left\">Name</th>"));
        assert!(page.contains("<figure class=\"diagram\"><svg"));
        assert!(page.contains("class=\"hl-code\""));
        assert!(page.contains("src=\"data:image/svg+xml;base64,"));
        assert!(page.contains("markdown-alert-warning"));
        assert!(!page.contains("src=\"logo.png\""));
    }
}
