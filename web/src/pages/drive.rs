use gloo_net::http::Request;
use icons::{ChevronLeft, ChevronRight, Download, File, FileArchive, FileAudio, FileImage, FileText, FileVideo, X};
use leptos::ev;
use leptos::prelude::*;
use serde::Deserialize;

use crate::widgets::fmt_size;

#[derive(Clone, Deserialize, PartialEq)]
pub struct FileItem {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub mime: String,
    pub size: i64,
    pub created: i64,
    #[serde(default)]
    pub share_token: Option<String>,
    #[serde(default)]
    pub share_expires: Option<i64>,
    #[serde(default)]
    pub archived: i64,
    #[serde(default)]
    pub folder_id: Option<String>,
    #[serde(default)]
    pub starred: i64,
}

#[derive(Clone, Deserialize, PartialEq)]
pub struct Folder {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    #[serde(default)]
    pub created: i64,
    #[serde(default)]
    pub starred: i64,
    #[serde(default)]
    pub share_token: Option<String>,
    #[serde(default)]
    pub share_expires: Option<i64>,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Image,
    Video,
    Audio,
    Pdf,
    Text,
    Archive,
    Other,
}

impl Kind {
    pub fn of(f: &FileItem) -> Kind {
        let ext = f.name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
        let top = f.mime.split('/').next().unwrap_or_default();
        match (top, ext.as_str()) {
            ("image", _) | (_, "png" | "jpg" | "jpeg" | "gif" | "webp" | "avif" | "bmp" | "svg" | "ico") => Kind::Image,
            ("video", _) | (_, "mp4" | "webm" | "mov" | "mkv" | "m4v" | "ogv") => Kind::Video,
            ("audio", _) | (_, "mp3" | "wav" | "ogg" | "oga" | "flac" | "m4a" | "aac" | "opus") => Kind::Audio,
            (_, "pdf") => Kind::Pdf,
            ("text", _) | (_, "md" | "txt" | "json" | "csv" | "log" | "toml" | "yaml" | "yml" | "xml" | "rs" | "js" | "ts" | "py" | "sh" | "css" | "html") => {
                Kind::Text
            }
            (_, "zip" | "tar" | "gz" | "tgz" | "7z" | "rar" | "xz" | "bz2") => Kind::Archive,
            _ if f.mime == "application/pdf" => Kind::Pdf,
            _ => Kind::Other,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Image => "Image",
            Kind::Video => "Video",
            Kind::Audio => "Audio",
            Kind::Pdf => "PDF document",
            Kind::Text => "Text document",
            Kind::Archive => "Archive",
            Kind::Other => "File",
        }
    }
}

#[component]
pub fn KindIcon(kind: Kind, #[prop(into, optional)] class: String) -> impl IntoView {
    match kind {
        Kind::Image => view! { <FileImage class /> }.into_any(),
        Kind::Video => view! { <FileVideo class /> }.into_any(),
        Kind::Audio => view! { <FileAudio class /> }.into_any(),
        Kind::Pdf | Kind::Text => view! { <FileText class /> }.into_any(),
        Kind::Archive => view! { <FileArchive class /> }.into_any(),
        Kind::Other => view! { <File class /> }.into_any(),
    }
}

pub fn trail(folders: &[Folder], from: Option<&str>) -> Vec<Folder> {
    let mut out: Vec<Folder> = Vec::new();
    let mut next = from.map(str::to_owned);
    while let Some(id) = next.take().filter(|_| out.len() < 64) {
        let Some(f) = folders.iter().find(|f| f.id == id) else { break };
        next = f.parent_id.clone();
        out.push(f.clone());
    }
    out.reverse();
    out
}

const TEXT_LIMIT: i64 = 2 << 20;
const TOP_BTN: &str = "flex justify-center items-center rounded-full transition-colors size-9 hover:bg-white/15";
const SIDE_BTN: &str =
    "flex absolute top-1/2 z-10 justify-center items-center rounded-full transition-colors -translate-y-1/2 size-10 bg-black/40 hover:bg-white/20";

#[component]
fn TextView(src: String) -> impl IntoView {
    let text = LocalResource::new(move || {
        let src = src.clone();
        async move {
            let res = Request::get(&src).send().await.ok()?;
            res.text().await.ok()
        }
    });
    view! {
        <pre class="overflow-auto p-4 w-full max-w-4xl h-full font-mono text-sm whitespace-pre-wrap break-words rounded-lg bg-background text-foreground">
            {move || text.get().map(|t| t.unwrap_or_else(|| "Could not load this file.".into())).unwrap_or_else(|| "Loading...".into())}
        </pre>
    }
}

fn media(f: &FileItem, src: String) -> AnyView {
    let name = f.name.clone();
    match Kind::of(f) {
        Kind::Image => view! { <img src=src alt=name class="object-contain max-w-full max-h-full rounded-md" /> }.into_any(),
        Kind::Video => view! { <video src=src controls autoplay playsinline class="max-w-full max-h-full bg-black rounded-md" /> }.into_any(),
        Kind::Audio => view! { <audio src=src controls autoplay class="w-full max-w-lg" /> }.into_any(),
        Kind::Pdf => view! { <iframe src=src title=name class="w-full max-w-5xl h-full bg-white rounded-md" /> }.into_any(),
        Kind::Text if f.size <= TEXT_LIMIT => view! { <TextView src /> }.into_any(),
        kind => view! {
            <div class="flex flex-col gap-4 items-center p-8 text-center rounded-xl bg-white/10">
                <KindIcon kind class="size-14 text-white/70" />
                <p class="text-sm text-white/80">"No preview available for this file."</p>
                <a href=src download=name class="py-2 px-4 text-sm font-medium text-black bg-white rounded-md hover:bg-white/90">"Download"</a>
            </div>
        }
        .into_any(),
    }
}

fn media_focused() -> bool {
    document().active_element().is_some_and(|el| matches!(el.tag_name().as_str(), "VIDEO" | "AUDIO" | "INPUT" | "TEXTAREA"))
}

#[component]
pub fn Preview(files: Signal<Vec<FileItem>>, current: RwSignal<Option<String>>, src: Callback<String, String>) -> impl IntoView {
    let index = move || current.get().and_then(|id| files.with(|l| l.iter().position(|f| f.id == id)));
    let step = move |by: isize| {
        let Some(i) = untrack(index) else { return };
        files.with_untracked(|l| {
            let next = (i as isize + by).rem_euclid(l.len().max(1) as isize) as usize;
            current.set(l.get(next).map(|f| f.id.clone()));
        });
    };
    let keys = window_event_listener(ev::keydown, move |e| {
        if current.get_untracked().is_none() || (e.key().starts_with("Arrow") && media_focused()) {
            return;
        }
        match e.key().as_str() {
            "Escape" => current.set(None),
            "ArrowLeft" => step(-1),
            "ArrowRight" => step(1),
            _ => {}
        }
    });
    on_cleanup(move || keys.remove());
    let many = move || files.with(|l| l.len() > 1);

    view! {
        {move || index().and_then(|i| files.with(|l| l.get(i).cloned())).map(|f| {
            let url = src.run(f.id.clone());
            view! {
                <div class="flex fixed inset-0 flex-col text-white z-[60] bg-black/90 animate-in fade-in-0 duration-200">
                    <div class="flex gap-3 items-center px-4 h-14 shrink-0">
                        <KindIcon kind=Kind::of(&f) class="size-5 shrink-0" />
                        <span class="flex-1 font-medium truncate">{f.name.clone()}</span>
                        <span class="hidden text-sm sm:inline text-white/60">{fmt_size(f.size)}</span>
                        <a href=url.clone() download=f.name.clone() class=TOP_BTN title="Download"><Download class="size-5" /></a>
                        <button class=TOP_BTN title="Close" on:click=move |_| current.set(None)><X class="size-5" /></button>
                    </div>
                    <div class="flex relative flex-1 justify-center items-center p-4 min-h-0 sm:px-16">
                        <Show when=many>
                            <button class=format!("{SIDE_BTN} left-2") title="Previous" on:click=move |_| step(-1)><ChevronLeft class="size-6" /></button>
                            <button class=format!("{SIDE_BTN} right-2") title="Next" on:click=move |_| step(1)><ChevronRight class="size-6" /></button>
                        </Show>
                        {media(&f, url)}
                    </div>
                </div>
            }
        })}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(id: &str, parent: Option<&str>) -> Folder {
        Folder { id: id.into(), name: id.into(), parent_id: parent.map(Into::into), created: 0, starred: 0, share_token: None, share_expires: None }
    }

    fn file(name: &str, mime: &str) -> FileItem {
        FileItem { id: name.into(), name: name.into(), mime: mime.into(), size: 0, created: 0, share_token: None, share_expires: None, archived: 0, folder_id: None, starred: 0 }
    }

    #[test]
    fn trail_walks_up_and_survives_cycles() {
        let tree = [dir("a", None), dir("b", Some("a")), dir("c", Some("b"))];
        assert_eq!(trail(&tree, Some("c")).iter().map(|f| f.id.as_str()).collect::<Vec<_>>(), ["a", "b", "c"]);
        assert!(trail(&tree, None).is_empty());
        let looped = [dir("x", Some("y")), dir("y", Some("x"))];
        assert_eq!(trail(&looped, Some("x")).len(), 64);
    }

    #[test]
    fn kind_uses_mime_then_extension() {
        assert!(Kind::of(&file("clip.mov", "video/quicktime")) == Kind::Video);
        assert!(Kind::of(&file("clip.mkv", "application/octet-stream")) == Kind::Video);
        assert!(Kind::of(&file("doc", "application/pdf")) == Kind::Pdf);
        assert!(Kind::of(&file("a.bin", "")) == Kind::Other);
    }
}
