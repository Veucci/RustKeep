use std::io::{Cursor, Read};

use calamine::Reader;
use image::{DynamicImage, ImageFormat};
use lopdf::{Document, Object, Stream, dictionary};
use resvg::{tiny_skia, usvg};
use serde_json::{Map, Value};
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

use crate::export;
use crate::markdown::{self, Images};
use crate::util::{Res, bad, esc};

const IMAGES: &[&str] = &["png", "jpg", "webp", "gif", "bmp", "ico", "tiff", "tga", "qoi", "pdf"];
const DOCUMENTS: &[&str] = &["pdf", "docx", "html", "md", "txt"];
const TABLES: &[&str] = &["xlsx", "csv", "json"];
const AUDIO: &[&str] = &["wav"];
const MAX_PIXELS: u32 = 64_000_000;
const FONT: &str = "Liberation Sans";

const MIME: &[(&str, &str)] = &[
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("webp", "image/webp"),
    ("gif", "image/gif"),
    ("bmp", "image/bmp"),
    ("ico", "image/x-icon"),
    ("tiff", "image/tiff"),
    ("tga", "image/x-tga"),
    ("qoi", "image/qoi"),
    ("pdf", "application/pdf"),
    ("docx", "application/vnd.openxmlformats-officedocument.wordprocessingml.document"),
    ("html", "text/html"),
    ("md", "text/markdown"),
    ("txt", "text/plain"),
    ("xlsx", "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"),
    ("csv", "text/csv"),
    ("json", "application/json"),
    ("wav", "audio/wav"),
];

#[derive(Clone, Copy)]
enum Source {
    Image,
    Svg,
    Document,
    Table,
    Audio,
    Unsupported,
}

enum Text {
    Markdown(String),
    Plain(String),
}

fn alias(ext: &str) -> String {
    match ext.to_ascii_lowercase().as_str() {
        "jpeg" => "jpg".into(),
        "tif" => "tiff".into(),
        "markdown" => "md".into(),
        other => other.into(),
    }
}

fn source(ext: &str) -> (Source, &'static [&'static str]) {
    match ext {
        "png" | "jpg" | "webp" | "gif" | "bmp" | "ico" | "tiff" | "tga" | "qoi" => (Source::Image, IMAGES),
        "svg" => (Source::Svg, IMAGES),
        "md" | "txt" | "docx" | "pdf" => (Source::Document, DOCUMENTS),
        "xlsx" | "xls" | "ods" | "csv" | "json" => (Source::Table, TABLES),
        "mp3" | "flac" | "ogg" | "oga" | "m4a" | "aac" => (Source::Audio, AUDIO),
        _ => (Source::Unsupported, &[]),
    }
}

pub fn targets(ext: &str) -> Vec<&'static str> {
    let ext = alias(ext);
    source(&ext).1.iter().copied().filter(|t| *t != ext).collect()
}

pub fn mime(ext: &str) -> &'static str {
    MIME.iter().find(|(e, _)| *e == ext).map_or("application/octet-stream", |(_, m)| m)
}

pub fn is_markdown(ext: &str) -> bool {
    alias(ext) == "md"
}

pub fn run(bytes: Vec<u8>, title: &str, from: &str, to: &str, images: Images) -> Res<Vec<u8>> {
    let from = alias(from);
    match source(&from).0 {
        Source::Image => encode_image(decode_image(&bytes, &from)?, to),
        Source::Svg => encode_image(rasterize(&bytes)?, to),
        Source::Document => write_text(read_text(&bytes, &from)?, title, to, images),
        Source::Table => write_table(read_table(bytes, &from)?, to),
        Source::Audio => to_wav(bytes, &from),
        Source::Unsupported => Err(bad("This conversion is not supported")),
    }
}

fn decode_image(bytes: &[u8], from: &str) -> Res<DynamicImage> {
    let format = ImageFormat::from_extension(from).ok_or_else(|| bad("Unknown image format"))?;
    Ok(image::load_from_memory_with_format(bytes, format)?)
}

fn rasterize(bytes: &[u8]) -> Res<DynamicImage> {
    let mut opt = usvg::Options { font_family: FONT.into(), ..Default::default() };
    let db = opt.fontdb_mut();
    export::FONTS.iter().for_each(|f| db.load_font_data(f.to_vec()));
    db.set_serif_family(FONT);
    db.set_sans_serif_family(FONT);
    db.set_monospace_family(FONT);
    let tree = usvg::Tree::from_data(bytes, &opt)?;
    let size = tree.size().to_int_size();
    let mut pixmap = tiny_skia::Pixmap::new(size.width(), size.height())
        .filter(|_| size.width().saturating_mul(size.height()) <= MAX_PIXELS)
        .ok_or_else(|| bad("Image is too large"))?;
    resvg::render(&tree, tiny_skia::Transform::default(), &mut pixmap.as_mut());
    let rgba = pixmap.pixels().iter().flat_map(|p| {
        let c = p.demultiply();
        [c.red(), c.green(), c.blue(), c.alpha()]
    });
    let img = image::RgbaImage::from_raw(size.width(), size.height(), rgba.collect()).ok_or_else(|| bad("Image is too large"))?;
    Ok(DynamicImage::ImageRgba8(img))
}

fn encode_image(img: DynamicImage, to: &str) -> Res<Vec<u8>> {
    if to == "pdf" {
        return image_pdf(&img);
    }
    let format = ImageFormat::from_extension(to).ok_or_else(|| bad("Unknown image format"))?;
    let img = match format {
        ImageFormat::Jpeg => on_white(&img),
        ImageFormat::Ico => DynamicImage::ImageRgba8(img.thumbnail(256, 256).to_rgba8()),
        _ => DynamicImage::ImageRgba8(img.to_rgba8()),
    };
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, format)?;
    Ok(out.into_inner())
}

fn on_white(img: &DynamicImage) -> DynamicImage {
    let mut rgba = img.to_rgba8();
    for p in rgba.pixels_mut() {
        let alpha = u16::from(p[3]);
        p.0 = p.0.map(|c| ((u16::from(c) * alpha + 255 * (255 - alpha)) / 255) as u8);
    }
    DynamicImage::ImageRgb8(DynamicImage::ImageRgba8(rgba).to_rgb8())
}

fn image_pdf(img: &DynamicImage) -> Res<Vec<u8>> {
    let mut jpeg = Cursor::new(Vec::new());
    on_white(img).write_to(&mut jpeg, ImageFormat::Jpeg)?;
    let (w, h) = (i64::from(img.width()), i64::from(img.height()));
    let mut doc = Document::with_version("1.5");
    let pages = doc.new_object_id();
    let picture = dictionary! {
        "Type" => "XObject", "Subtype" => "Image", "Width" => w, "Height" => h,
        "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8, "Filter" => "DCTDecode",
    };
    let picture = doc.add_object(Stream::new(picture, jpeg.into_inner()));
    let content = doc.add_object(Stream::new(dictionary! {}, format!("q {w} 0 0 {h} 0 0 cm /Im0 Do Q").into_bytes()));
    let page = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages, "Contents" => content,
        "MediaBox" => vec![0.into(), 0.into(), w.into(), h.into()],
        "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => picture } },
    });
    doc.objects.insert(pages, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 }));
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    doc.trailer.set("Root", catalog);
    let mut out = Vec::new();
    doc.save_to(&mut out)?;
    Ok(out)
}

fn read_text(bytes: &[u8], from: &str) -> Res<Text> {
    let utf8 = || String::from_utf8_lossy(bytes).into_owned();
    match from {
        "md" => Ok(Text::Markdown(utf8())),
        "txt" => Ok(Text::Plain(utf8())),
        "docx" => Ok(Text::Plain(docx_text(bytes)?)),
        _ => Ok(Text::Plain(pdf_text(bytes)?)),
    }
}

fn docx_text(bytes: &[u8]) -> Res<String> {
    let mut xml = String::new();
    zip::ZipArchive::new(Cursor::new(bytes))?.by_name("word/document.xml")?.read_to_string(&mut xml)?;
    let mut out = String::new();
    for part in xml.split('<').skip(1) {
        let (tag, text) = part.split_once('>').unwrap_or((part, ""));
        match tag.split(' ').next().unwrap_or_default().trim_end_matches('/') {
            "w:t" => out.push_str(&unescape(text)),
            "/w:p" => out.push_str("\n\n"),
            "w:br" => out.push('\n'),
            "w:tab" => out.push('\t'),
            _ => {}
        }
    }
    Ok(out)
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

fn pdf_text(bytes: &[u8]) -> Res<String> {
    let doc = Document::load_mem(bytes)?;
    let pages: Vec<u32> = doc.get_pages().into_keys().collect();
    let text = doc.extract_text(&pages)?;
    let lines: Vec<String> = text.lines().map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")).collect();
    Ok(lines.join("\n").trim().to_owned())
}

fn write_text(text: Text, title: &str, to: &str, images: Images) -> Res<Vec<u8>> {
    match (to, text) {
        ("pdf", Text::Markdown(md)) => markdown::pdf(title, &md, images, false),
        ("pdf", Text::Plain(t)) => markdown::plain_pdf(title, &t),
        ("html", Text::Markdown(md)) => Ok(markdown::html(title, &md, images).into_bytes()),
        ("html", Text::Plain(t)) => Ok(markdown::page(title, &paragraphs(&t)).into_bytes()),
        ("docx", Text::Markdown(md)) => export::docx_blocks(&export::parse(&md)),
        ("docx", Text::Plain(t)) => export::docx_blocks(&export::plain(&t)),
        (_, Text::Markdown(md)) => Ok(export::text(&export::parse(&md)).into_bytes()),
        (_, Text::Plain(t)) => Ok(t.into_bytes()),
    }
}

fn paragraphs(text: &str) -> String {
    export::plain(text).iter().map(|b| format!("<p>{}</p>\n", esc(&export::text(std::slice::from_ref(b))).replace('\n', "<br>"))).collect()
}

fn read_table(bytes: Vec<u8>, from: &str) -> Res<Vec<Vec<String>>> {
    match from {
        "csv" => csv::ReaderBuilder::new()
            .has_headers(false)
            .flexible(true)
            .from_reader(bytes.as_slice())
            .records()
            .map(|r| Ok(r?.iter().map(str::to_owned).collect()))
            .collect(),
        "json" => json_rows(serde_json::from_slice(&bytes)?),
        _ => {
            let mut book = calamine::open_workbook_auto_from_rs(Cursor::new(bytes))?;
            let sheet = book.worksheet_range_at(0).ok_or_else(|| bad("The workbook has no sheets"))??;
            Ok(sheet.rows().map(|row| row.iter().map(ToString::to_string).collect()).collect())
        }
    }
}

fn cell(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn json_rows(v: Value) -> Res<Vec<Vec<String>>> {
    let Value::Array(items) = v else { return Err(bad("JSON must be an array of objects or arrays")) };
    if items.iter().all(Value::is_array) {
        return Ok(items.iter().map(|row| row.as_array().into_iter().flatten().map(cell).collect()).collect());
    }
    let mut header: Vec<String> = Vec::new();
    for key in items.iter().filter_map(Value::as_object).flat_map(Map::keys) {
        if !header.contains(key) {
            header.push(key.clone());
        }
    }
    let rows = items.iter().map(|item| header.iter().map(|k| item.get(k).map(cell).unwrap_or_default()).collect());
    Ok(std::iter::once(header.clone()).chain(rows).collect())
}

fn write_table(rows: Vec<Vec<String>>, to: &str) -> Res<Vec<u8>> {
    match to {
        "csv" => {
            let mut w = csv::WriterBuilder::new().flexible(true).from_writer(Vec::new());
            rows.iter().try_for_each(|r| w.write_record(r))?;
            Ok(w.into_inner().map_err(|e| e.into_error())?)
        }
        "json" => {
            let (header, body) = rows.split_first().map_or((&[][..], &[][..]), |(h, b)| (h.as_slice(), b));
            let objects: Vec<Map<String, Value>> =
                body.iter().map(|r| header.iter().zip(r).map(|(k, v)| (k.clone(), Value::String(v.clone()))).collect()).collect();
            Ok(serde_json::to_vec_pretty(&objects)?)
        }
        _ => xlsx(&rows),
    }
}

fn xlsx(rows: &[Vec<String>]) -> Res<Vec<u8>> {
    let mut book = rust_xlsxwriter::Workbook::new();
    let sheet = book.add_worksheet();
    for (r, row) in (0u32..).zip(rows) {
        for (c, value) in (0u16..).zip(row) {
            match value.parse::<f64>() {
                Ok(n) if n.is_finite() => sheet.write_number(r, c, n)?,
                _ => sheet.write_string(r, c, value)?,
            };
        }
    }
    Ok(book.save_to_buffer()?)
}

fn to_wav(bytes: Vec<u8>, from: &str) -> Res<Vec<u8>> {
    let stream = MediaSourceStream::new(Box::new(Cursor::new(bytes)), Default::default());
    let mut hint = Hint::new();
    hint.with_extension(from);
    let mut format = symphonia::default::get_probe().probe(&hint, stream, FormatOptions::default(), MetadataOptions::default())?;
    let track = format.default_track(TrackType::Audio).ok_or_else(|| bad("No audio track found"))?;
    let params = track.codec_params.as_ref().and_then(|p| p.audio()).ok_or_else(|| bad("No audio track found"))?;
    let mut decoder = symphonia::default::get_codecs().make_audio_decoder(params, &AudioDecoderOptions::default())?;
    let track_id = track.id;
    let (mut samples, mut spec) = (Vec::<i16>::new(), (0u16, 0u32));
    let mut chunk = Vec::new();
    while let Some(packet) = format.next_packet()? {
        if packet.track_id != track_id {
            continue;
        }
        let Ok(buf) = decoder.decode(&packet) else { continue };
        spec = (buf.spec().channels().count() as u16, buf.spec().rate());
        buf.copy_to_vec_interleaved(&mut chunk);
        samples.extend_from_slice(&chunk);
    }
    if spec.0 == 0 {
        return Err(bad("No audio could be decoded"));
    }
    Ok(wav(spec.0, spec.1, &samples))
}

fn wav(channels: u16, rate: u32, samples: &[i16]) -> Vec<u8> {
    let data = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * u32::from(channels) * 2).to_le_bytes());
    out.extend_from_slice(&(channels * 2).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    samples.iter().for_each(|s| out.extend_from_slice(&s.to_le_bytes()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png() -> Vec<u8> {
        let img = DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(4, 3, image::Rgba([200, 10, 10, 128])));
        let mut out = Cursor::new(Vec::new());
        img.write_to(&mut out, ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn lists_targets_without_the_source_format() {
        assert!(targets("JPEG").contains(&"png") && !targets("jpeg").contains(&"jpg"));
        assert_eq!(targets("md"), ["pdf", "docx", "html", "txt"]);
        assert_eq!(targets("xls"), ["xlsx", "csv", "json"]);
        assert!(targets("mp4").is_empty() && targets("").is_empty());
    }

    #[test]
    fn converts_images() {
        for to in IMAGES {
            let out = run(png(), "pic", "png", to, Images::new()).unwrap_or_else(|_| panic!("png to {to}"));
            assert!(!out.is_empty());
        }
        assert!(run(png(), "pic", "png", "pdf", Images::new()).unwrap().starts_with(b"%PDF"));
        let jpg = image::load_from_memory(&run(png(), "pic", "png", "jpg", Images::new()).unwrap()).unwrap().to_rgb8();
        assert_eq!(jpg.dimensions(), (4, 3));
        assert!(jpg.get_pixel(0, 0)[1] > 100, "transparent areas are flattened onto white");
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="5"><rect width="10" height="5" fill="red"/><text y="4">Hi</text></svg>"#;
        assert!(run(svg.to_vec(), "s", "svg", "png", Images::new()).is_ok_and(|b| b.starts_with(b"\x89PNG")));
    }

    #[test]
    fn converts_documents() {
        let md = b"# Title\n\n- a & b\n".to_vec();
        assert!(run(md.clone(), "n", "md", "pdf", Images::new()).is_ok_and(|b| b.starts_with(b"%PDF")));
        assert!(run(md.clone(), "n", "md", "html", Images::new()).is_ok_and(|b| String::from_utf8(b).unwrap().contains("<li>a &amp; b</li>")));
        let docx = run(md, "n", "md", "docx", Images::new()).unwrap();
        assert_eq!(run(docx, "n", "docx", "txt", Images::new()).unwrap(), "Title\n\n\u{2022} a & b\n\n".as_bytes());
        let pdf = run(b"hello pdf".to_vec(), "n", "txt", "pdf", Images::new()).unwrap();
        let t = String::from_utf8(run(pdf, "n", "pdf", "txt", Images::new()).unwrap()).unwrap();
        assert!(t.starts_with("hello pdf"));
    }

    #[test]
    fn converts_tables() {
        let json = run(b"name,age\nAda,36\n".to_vec(), "t", "csv", "json", Images::new()).unwrap();
        assert_eq!(serde_json::from_slice::<Value>(&json).unwrap(), serde_json::json!([{ "name": "Ada", "age": "36" }]));
        let csv = run(br#"[{"a":1,"b":null},{"b":"x, y"}]"#.to_vec(), "t", "json", "csv", Images::new()).unwrap();
        assert_eq!(csv, b"a,b\n1,\n,\"x, y\"\n");
        let xlsx = run(b"a,b\n1,two\n".to_vec(), "t", "csv", "xlsx", Images::new()).unwrap();
        assert_eq!(run(xlsx, "t", "xlsx", "csv", Images::new()).unwrap(), b"a,b\n1,two\n");
    }

    #[test]
    fn converts_audio_to_wav() {
        let pcm: Vec<i16> = (0..800).map(|i| (i * 40) as i16).collect();
        let source = wav(2, 8000, &pcm);
        assert_eq!(to_wav(source.clone(), "wav").unwrap(), source);
        assert!(run(b"not audio".to_vec(), "a", "mp3", "wav", Images::new()).is_err());
    }
}
