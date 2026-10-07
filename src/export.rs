use std::io::{Cursor, Write};

use genpdf::elements::{Break, LinearLayout, Paragraph};
use genpdf::fonts::{FontData, FontFamily};
use genpdf::style::Style;
use genpdf::{Element, Margins, SimplePageDecorator};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use zip::write::SimpleFileOptions;

use crate::util::{Res, esc};

#[derive(Default, Clone, Copy)]
pub struct Marks {
    bold: bool,
    italic: bool,
    strike: bool,
    code: bool,
}

pub struct Span {
    text: String,
    marks: Marks,
}

#[derive(Clone)]
pub enum Kind {
    Heading(u8),
    Paragraph,
    Item { depth: usize, marker: String },
    Quote,
    Code,
}

pub struct Block {
    kind: Kind,
    spans: Vec<Span>,
}

#[derive(Default)]
struct Reader {
    blocks: Vec<Block>,
    current: Option<Block>,
    marks: Marks,
    lists: Vec<Option<u64>>,
    marker: Option<String>,
    quote: usize,
}

const OPTIONS: Options = Options::ENABLE_TABLES.union(Options::ENABLE_STRIKETHROUGH).union(Options::ENABLE_TASKLISTS);

impl Reader {
    fn flush(&mut self) {
        if let Some(span) = self.current.as_mut().and_then(|b| b.spans.last_mut()) {
            span.text.truncate(span.text.trim_end_matches('\n').len());
        }
        self.blocks.extend(self.current.take());
    }

    fn open(&mut self, kind: Kind) {
        self.flush();
        self.current = Some(Block { kind, spans: Vec::new() });
    }

    fn context(&mut self) -> Kind {
        if !self.lists.is_empty() {
            return Kind::Item { depth: self.lists.len(), marker: self.marker.take().unwrap_or_default() };
        }
        if self.quote > 0 { Kind::Quote } else { Kind::Paragraph }
    }

    fn push(&mut self, text: &str, code: bool) {
        if self.current.is_none() {
            let kind = self.context();
            self.open(kind);
        }
        let marks = Marks { code: code || self.marks.code, ..self.marks };
        self.current.iter_mut().for_each(|b| b.spans.push(Span { text: text.to_owned(), marks }));
    }

    fn next_marker(&mut self) -> String {
        match self.lists.last_mut() {
            Some(Some(n)) => {
                *n += 1;
                format!("{}.", *n - 1)
            }
            _ => "\u{2022}".into(),
        }
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Heading { level, .. } => self.open(Kind::Heading(level as u8)),
            Tag::Paragraph => {
                let kind = self.context();
                self.open(kind);
            }
            Tag::CodeBlock(_) => self.open(Kind::Code),
            Tag::List(first) => self.lists.push(first),
            Tag::Item => {
                self.flush();
                self.marker = Some(self.next_marker());
            }
            Tag::BlockQuote(_) => self.quote += 1,
            Tag::TableCell if self.current.as_ref().is_some_and(|b| !b.spans.is_empty()) => self.push(" | ", false),
            tag => self.set_mark(&tag, true),
        }
    }

    fn set_mark(&mut self, tag: &Tag, on: bool) {
        match tag {
            Tag::Strong => self.marks.bold = on,
            Tag::Emphasis => self.marks.italic = on,
            Tag::Strikethrough => self.marks.strike = on,
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::List(_) => {
                self.flush();
                self.lists.pop();
            }
            TagEnd::BlockQuote(_) => self.quote = self.quote.saturating_sub(1),
            TagEnd::Strong => self.marks.bold = false,
            TagEnd::Emphasis => self.marks.italic = false,
            TagEnd::Strikethrough => self.marks.strike = false,
            TagEnd::Heading(_) | TagEnd::Paragraph | TagEnd::CodeBlock | TagEnd::Item | TagEnd::TableRow | TagEnd::TableHead => self.flush(),
            _ => {}
        }
    }

    fn event(&mut self, event: Event) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(t) | Event::Html(t) | Event::InlineHtml(t) => self.push(&t, false),
            Event::Code(t) => self.push(&t, true),
            Event::SoftBreak => self.push(" ", false),
            Event::HardBreak => self.push("\n", false),
            Event::TaskListMarker(done) => self.marker = Some(if done { "[x]" } else { "[ ]" }.into()),
            _ => {}
        }
    }
}

pub fn parse(markdown: &str) -> Vec<Block> {
    let mut reader = Reader::default();
    Parser::new_ext(markdown, OPTIONS).for_each(|e| reader.event(e));
    reader.flush();
    reader.blocks
}

fn docx_run(span: &Span, size: Option<u32>) -> String {
    let m = span.marks;
    let props = [
        (m.bold || size.is_some(), "<w:b/>".to_owned()),
        (m.italic, "<w:i/>".to_owned()),
        (m.strike, "<w:strike/>".to_owned()),
        (m.code, "<w:rFonts w:ascii=\"Courier New\" w:hAnsi=\"Courier New\"/>".to_owned()),
        (size.is_some(), format!("<w:sz w:val=\"{}\"/>", size.unwrap_or_default())),
    ];
    let props: String = props.into_iter().filter(|(on, _)| *on).map(|(_, p)| p).collect();
    let text = span.text.split('\n').map(|t| format!("<w:t xml:space=\"preserve\">{}</w:t>", esc(t))).collect::<Vec<_>>().join("<w:br/>");
    format!("<w:r><w:rPr>{props}</w:rPr>{text}</w:r>")
}

fn docx_paragraph(block: &Block) -> String {
    let (props, size, prefix) = match &block.kind {
        Kind::Heading(level) => (String::new(), Some([40, 32, 28][(*level as usize).clamp(1, 3) - 1]), String::new()),
        Kind::Paragraph => (String::new(), None, String::new()),
        Kind::Item { depth, marker } => (format!("<w:ind w:left=\"{}\" w:hanging=\"360\"/>", depth * 360), None, format!("{marker} ")),
        Kind::Quote => ("<w:pBdr><w:left w:val=\"single\" w:sz=\"12\" w:space=\"8\" w:color=\"BBBBBB\"/></w:pBdr><w:ind w:left=\"360\"/>".into(), None, String::new()),
        Kind::Code => ("<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"F2F2F2\"/>".into(), None, String::new()),
    };
    let code = matches!(block.kind, Kind::Code);
    let lead = (!prefix.is_empty()).then(|| docx_run(&Span { text: prefix, marks: Marks::default() }, None));
    let runs: String = block.spans.iter().map(|s| docx_run(&Span { text: s.text.clone(), marks: Marks { code: code || s.marks.code, ..s.marks } }, size)).collect();
    format!("<w:p><w:pPr>{props}</w:pPr>{}{runs}</w:p>", lead.unwrap_or_default())
}

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/></Types>"#;
const ROOT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
const DOCUMENT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#;
const STYLES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Calibri" w:hAnsi="Calibri" w:cs="Calibri"/><w:sz w:val="22"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="120" w:line="276" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults></w:styles>"#;

pub fn docx(title: &str, markdown: &str) -> Res<Vec<u8>> {
    let heading = Block { kind: Kind::Heading(1), spans: vec![Span { text: title.to_owned(), marks: Marks::default() }] };
    let body: String = std::iter::once(heading).chain(parse(markdown)).map(|b| docx_paragraph(&b)).collect();
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{body}<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1134" w:right="1134" w:bottom="1134" w:left="1134" w:header="0" w:footer="0" w:gutter="0"/></w:sectPr></w:body></w:document>"#
    );
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let parts = [
        ("[Content_Types].xml", CONTENT_TYPES),
        ("_rels/.rels", ROOT_RELS),
        ("word/_rels/document.xml.rels", DOCUMENT_RELS),
        ("word/styles.xml", STYLES),
        ("word/document.xml", document.as_str()),
    ];
    for (name, content) in parts {
        zip.start_file(name, SimpleFileOptions::default())?;
        zip.write_all(content.as_bytes())?;
    }
    Ok(zip.finish()?.into_inner())
}

fn font(bytes: &[u8]) -> Res<FontData> {
    Ok(FontData::new(bytes.to_vec(), None)?)
}

fn fonts() -> Res<FontFamily<FontData>> {
    Ok(FontFamily {
        regular: font(include_bytes!("../assets/fonts/LiberationSans-Regular.ttf"))?,
        bold: font(include_bytes!("../assets/fonts/LiberationSans-Bold.ttf"))?,
        italic: font(include_bytes!("../assets/fonts/LiberationSans-Italic.ttf"))?,
        bold_italic: font(include_bytes!("../assets/fonts/LiberationSans-BoldItalic.ttf"))?,
    })
}

fn pdf_style(marks: Marks, base: Style) -> Style {
    let mut style = base;
    if marks.bold {
        style.set_bold();
    }
    if marks.italic || marks.code {
        style.set_italic();
    }
    style
}

fn lines(spans: &[Span]) -> Vec<Vec<(&str, Marks)>> {
    let mut out = vec![Vec::new()];
    for span in spans {
        for (i, part) in span.text.split('\n').enumerate() {
            if i > 0 {
                out.push(Vec::new());
            }
            out.last_mut().into_iter().for_each(|line| line.push((part, span.marks)));
        }
    }
    out
}

fn pdf_block(block: &Block) -> impl Element + use<> {
    let (base, indent, prefix) = match &block.kind {
        Kind::Heading(level) => (Style::new().bold().with_font_size([20, 16, 13][(*level as usize).clamp(1, 3) - 1]), 0.0, String::new()),
        Kind::Item { depth, marker } => (Style::new(), *depth as f64 * 5.0, format!("{marker} ")),
        Kind::Quote => (Style::new().italic(), 5.0, String::new()),
        Kind::Paragraph | Kind::Code => (Style::new(), 0.0, String::new()),
    };
    let mut layout = LinearLayout::vertical();
    for (i, line) in lines(&block.spans).into_iter().enumerate() {
        let mut p = Paragraph::default();
        if i == 0 && !prefix.is_empty() {
            p.push_styled(prefix.clone(), base);
        }
        line.into_iter().for_each(|(text, marks)| p.push_styled(text.to_owned(), pdf_style(marks, base)));
        layout.push(p);
    }
    layout.padded(Margins::trbl(0, 0, 2, indent))
}

pub fn pdf(title: &str, markdown: &str) -> Res<Vec<u8>> {
    let mut doc = genpdf::Document::new(fonts()?);
    doc.set_title(title);
    doc.set_minimal_conformance();
    doc.set_font_size(11);
    let mut decorator = SimplePageDecorator::new();
    decorator.set_margins(18);
    doc.set_page_decorator(decorator);
    doc.push(Paragraph::new(title).styled(Style::new().bold().with_font_size(22)));
    doc.push(Break::new(1));
    parse(markdown).iter().for_each(|b| doc.push(pdf_block(b)));
    let mut out = Vec::new();
    doc.render(&mut out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_markdown_blocks() {
        let blocks = parse("# Title\n\nHello **bold** \u{11f}\u{15f}\n\n1. one\n2. two\n\n- [x] done\n\n> quote");
        let kinds: Vec<String> = blocks
            .iter()
            .map(|b| match &b.kind {
                Kind::Heading(l) => format!("h{l}"),
                Kind::Paragraph => "p".into(),
                Kind::Item { marker, .. } => marker.clone(),
                Kind::Quote => "q".into(),
                Kind::Code => "code".into(),
            })
            .collect();
        assert_eq!(kinds, ["h1", "p", "1.", "2.", "[x]", "q"]);
        assert!(blocks[1].spans[1].marks.bold);
    }

    #[test]
    fn renders_documents() {
        let md = "# Heading \u{130}\u{131}\n\nSome **bold** and `code`\n\n```\na\nb\n```";
        assert!(docx("Note", md).is_ok_and(|b| b.starts_with(b"PK")));
        assert!(pdf("Note", md).is_ok_and(|b| b.starts_with(b"%PDF")));
    }
}
