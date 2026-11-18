use gloo_net::http::Request;
use icons::{ChevronLeft, ChevronRight, CodeXml, Download, Eye, File, FileArchive, FileAudio, FileImage, FileText, FileVideo, Pencil, RotateCw, X};
use leptos::ev;
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::Deserialize;
use serde_json::Value;
use web_sys::RequestCache;

use crate::api;
use crate::pages::editor::FileEditor;
use crate::widgets::{Ui, fmt_size, use_ui};

#[derive(Clone, Default, Deserialize, PartialEq)]
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
    #[serde(default)]
    pub upload_link_id: Option<String>,
    #[serde(default)]
    pub uploader: Option<String>,
    #[serde(default)]
    pub upload_note: Option<String>,
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
    #[serde(default)]
    pub share_uploads: i64,
}

const TEXT_EXTENSIONS: [&str; 23] = [
    "md", "markdown", "txt", "log", "csv", "json", "toml", "yaml", "yml", "ini", "env", "xml", "svg", "webmanifest", "html", "htm", "css", "js", "mjs", "ts",
    "rs", "py", "sh",
];

fn extension(name: &str) -> String {
    name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default()
}

pub fn editable(name: &str) -> bool {
    TEXT_EXTENSIONS.contains(&extension(name).as_str())
}

pub fn is_markdown(name: &str) -> bool {
    matches!(extension(name).as_str(), "md" | "markdown")
}

pub fn is_html(name: &str) -> bool {
    matches!(extension(name).as_str(), "html" | "htm")
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
        let ext = extension(&f.name);
        let top = f.mime.split('/').next().unwrap_or_default();
        match (top, ext.as_str()) {
            ("image", _) | (_, "png" | "jpg" | "jpeg" | "gif" | "webp" | "avif" | "bmp" | "svg" | "ico") => Kind::Image,
            ("video", _) | (_, "mp4" | "webm" | "mov" | "mkv" | "m4v" | "ogv") => Kind::Video,
            ("audio", _) | (_, "mp3" | "wav" | "ogg" | "oga" | "flac" | "m4a" | "aac" | "opus") => Kind::Audio,
            (_, "pdf") => Kind::Pdf,
            (top, ext) if top == "text" || TEXT_EXTENSIONS.contains(&ext) => Kind::Text,
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

pub fn folder_path(folders: &[Folder], id: Option<&str>) -> String {
    let names: Vec<String> = trail(folders, id).into_iter().map(|f| f.name).collect();
    if names.is_empty() { "My Drive".into() } else { format!("My Drive / {}", names.join(" / ")) }
}

const TEXT_LIMIT: i64 = 2 << 20;
const TOP_BTN: &str = "flex justify-center items-center rounded-full transition-colors size-9 hover:bg-white/15";
const TEXT_BTN: &str = "flex justify-center items-center px-4 h-9 text-sm font-medium rounded-full transition-colors hover:bg-white/15";
const SAVE_BTN: &str = "flex justify-center items-center px-4 h-9 text-sm font-medium text-black bg-white rounded-full transition-colors hover:bg-white/90";
const SIDE_BTN: &str =
    "flex absolute top-1/2 z-10 justify-center items-center rounded-full transition-colors -translate-y-1/2 size-10 bg-black/40 hover:bg-white/20";
const SANDBOX: &str = "allow-scripts allow-forms allow-popups allow-modals";

#[derive(Deserialize)]
struct PageUrl {
    url: String,
}

#[component]
fn TextView(text: LocalResource<Option<String>>) -> impl IntoView {
    view! {
        <pre class="overflow-auto p-4 w-full max-w-4xl h-full font-mono text-sm whitespace-pre-wrap break-words rounded-lg bg-background text-foreground">
            {move || text.get().map(|t| t.unwrap_or_else(|| "Could not load this file.".into())).unwrap_or_else(|| "Loading...".into())}
        </pre>
    }
}

fn media(f: &FileItem, src: String, text: LocalResource<Option<String>>) -> AnyView {
    let name = f.name.clone();
    match Kind::of(f) {
        Kind::Image => view! { <img src=src alt=name class="object-contain max-w-full max-h-full rounded-md" /> }.into_any(),
        Kind::Video => view! { <video src=src controls autoplay playsinline class="max-w-full max-h-full bg-black rounded-md" /> }.into_any(),
        Kind::Audio => view! { <audio src=src controls autoplay class="w-full max-w-lg" /> }.into_any(),
        Kind::Pdf => view! { <iframe src=src title=name class="w-full max-w-5xl h-full bg-white rounded-md" /> }.into_any(),
        Kind::Text if f.size <= TEXT_LIMIT => view! { <TextView text /> }.into_any(),
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

async fn load_text(src: Option<String>) -> Option<String> {
    let res = Request::get(&src?).cache(RequestCache::NoCache).send().await.ok().filter(|r| r.ok())?;
    res.text().await.ok()
}

fn discard_then(ui: Ui, dirty: bool, action: impl Fn() + 'static) {
    if dirty {
        ui.confirm("Your unsaved changes will be lost.", "Discard", action);
    } else {
        action();
    }
}

#[component]
pub fn Preview(
    files: Signal<Vec<FileItem>>,
    current: RwSignal<Option<String>>,
    src: Callback<String, String>,
    #[prop(optional)] save_path: Option<Callback<String, String>>,
    #[prop(optional)] page_path: Option<Callback<String, String>>,
    #[prop(optional)] on_saved: Option<Callback<()>>,
) -> impl IntoView {
    let ui = use_ui();
    let index = move || current.get().and_then(|id| files.with(|l| l.iter().position(|f| f.id == id)));
    let file = Memo::new(move |_| index().and_then(|i| files.with(|l| l.get(i).cloned())));
    let version = RwSignal::new(0u32);
    let code = RwSignal::new(false);
    let editing = RwSignal::new(false);
    let draft = RwSignal::new(String::new());
    let original = StoredValue::new(String::new());
    let text = LocalResource::new(move || {
        version.track();
        load_text(file.get().filter(|f| editable(&f.name) && f.size <= TEXT_LIMIT).map(|f| src.run(f.id)))
    });
    let page = LocalResource::new(move || {
        let n = version.get();
        let path = page_path.zip(file.get()).filter(|(_, f)| is_html(&f.name)).map(|(path, f)| path.run(f.id));
        async move { ui.run(api::get::<PageUrl>(&path?)).await.map(|p| (n, api::url(&p.url))) }
    });
    Effect::new(move |_| {
        current.track();
        editing.set(false);
        code.set(false);
    });
    let step = move |by: isize| {
        let Some(i) = untrack(index) else { return };
        files.with_untracked(|l| {
            let next = (i as isize + by).rem_euclid(l.len().max(1) as isize) as usize;
            current.set(l.get(next).map(|f| f.id.clone()));
        });
    };
    let dirty = move || editing.get_untracked() && draft.with_untracked(|d| original.with_value(|o| d != o));
    let cancel = move || discard_then(ui, dirty(), move || editing.set(false));
    let close = move || discard_then(ui, dirty(), move || current.set(None));
    let escape = move || if editing.get_untracked() { cancel() } else { current.set(None) };
    let start_edit = move || {
        let Some(Some(body)) = text.get_untracked() else { return };
        original.set_value(body.clone());
        draft.set(body);
        editing.set(true);
    };
    let save = move || {
        let (Some(path), Some(f)) = (save_path, file.get_untracked()) else { return };
        let body = draft.get_untracked();
        spawn_local(async move {
            if ui.run(api::put_text::<Value>(&path.run(f.id), &body)).await.is_none() {
                return;
            }
            ui.notify("File saved");
            editing.set(false);
            version.update(|v| *v += 1);
            if let Some(saved) = on_saved {
                saved.run(());
            }
        });
    };
    let keys = window_event_listener(ev::keydown, move |e| {
        if current.get_untracked().is_none() || e.default_prevented() {
            return;
        }
        let arrows = !editing.get_untracked() && !media_focused();
        match e.key().as_str() {
            "Escape" => escape(),
            "ArrowLeft" if arrows => step(-1),
            "ArrowRight" if arrows => step(1),
            _ => {}
        }
    });
    on_cleanup(move || keys.remove());
    let many = move || !editing.get() && files.with(|l| l.len() > 1);
    let html_page = move || page_path.is_some() && !editing.get() && file.with(|f| f.as_ref().is_some_and(|f| is_html(&f.name)));
    let can_edit = move || save_path.is_some() && !editing.get() && text.with(|t| matches!(t, Some(Some(_))));
    let toggle = move |on: bool| if on { format!("{TOP_BTN} bg-white/20") } else { TOP_BTN.to_owned() };
    let body = move |f: FileItem, url: String| {
        if editing.get() {
            return view! { <FileEditor markdown=is_markdown(&f.name) text=draft on_save=Callback::new(move |()| save()) class="w-full max-w-4xl h-full bg-background" /> }
                .into_any();
        }
        if !html_page() || code.get() {
            return media(&f, url, text);
        }
        view! {
            <For
                each=move || page.get().flatten()
                key=Clone::clone
                children=move |(_, url): (u32, String)| view! { <iframe src=url title=f.name.clone() sandbox=SANDBOX class="w-full h-full bg-white rounded-md" /> }
            />
        }
        .into_any()
    };

    view! {
        {move || file.get().map(|f| {
            let url = src.run(f.id.clone());
            let shown = f.clone();
            let link = url.clone();
            view! {
                <div class="flex fixed inset-0 flex-col text-white z-[60] bg-black/90 animate-in fade-in-0 duration-200">
                    <div class="flex gap-3 items-center px-4 h-14 shrink-0">
                        <KindIcon kind=Kind::of(&f) class="size-5 shrink-0" />
                        <span class="flex-1 font-medium truncate">{f.name.clone()}</span>
                        <span class="hidden text-sm sm:inline text-white/60">{fmt_size(f.size)}</span>
                        <Show when=html_page>
                            <div class="flex gap-0.5 items-center p-0.5 rounded-full bg-white/10">
                                <button class=move || toggle(!code.get()) title="Preview" on:click=move |_| code.set(false)><Eye class="size-5" /></button>
                                <button class=move || toggle(code.get()) title="Code" on:click=move |_| code.set(true)><CodeXml class="size-5" /></button>
                            </div>
                            <button class=TOP_BTN title="Reload" on:click=move |_| version.update(|v| *v += 1)><RotateCw class="size-5" /></button>
                        </Show>
                        <Show when=can_edit>
                            <button class=TOP_BTN title="Edit" on:click=move |_| start_edit()><Pencil class="size-5" /></button>
                        </Show>
                        <Show when=move || editing.get()>
                            <button class=TEXT_BTN on:click=move |_| cancel()>"Cancel"</button>
                            <button class=SAVE_BTN title="Save (Ctrl+S)" on:click=move |_| save()>"Save"</button>
                        </Show>
                        <a href=link download=f.name.clone() class=TOP_BTN title="Download"><Download class="size-5" /></a>
                        <button class=TOP_BTN title="Close" on:click=move |_| close()><X class="size-5" /></button>
                    </div>
                    <div class="flex relative flex-1 justify-center items-center p-4 min-h-0 sm:px-16">
                        <Show when=many>
                            <button class=format!("{SIDE_BTN} left-2") title="Previous" on:click=move |_| step(-1)><ChevronLeft class="size-6" /></button>
                            <button class=format!("{SIDE_BTN} right-2") title="Next" on:click=move |_| step(1)><ChevronRight class="size-6" /></button>
                        </Show>
                        {move || body(shown.clone(), url.clone())}
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
        Folder { id: id.into(), name: id.into(), parent_id: parent.map(Into::into), created: 0, starred: 0, share_token: None, share_expires: None, share_uploads: 0 }
    }

    fn file(name: &str, mime: &str) -> FileItem {
        FileItem { id: name.into(), name: name.into(), mime: mime.into(), size: 0, created: 0, share_token: None, share_expires: None, archived: 0, folder_id: None, starred: 0, upload_link_id: None, uploader: None, upload_note: None }
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
        assert!(Kind::of(&file("site.webmanifest", "")) == Kind::Text);
        assert!(Kind::of(&file("logo.svg", "")) == Kind::Image);
    }

    #[test]
    fn text_files_pick_their_editor() {
        assert!(editable("deck/index.HTML") && is_html("deck/index.HTML") && !is_markdown("deck/index.HTML"));
        assert!(editable("notes.markdown") && is_markdown("notes.markdown"));
        assert!(editable(".env") && editable("site.webmanifest") && editable("logo.svg"));
        assert!(!editable("img/logo.png") && !editable("README") && !is_html("page.html.png"));
    }
}
