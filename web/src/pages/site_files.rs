use std::cmp::Reverse;
use std::time::Duration;

use icons::{
    ChevronRight, Copy, Download, EllipsisVertical, ExternalLink, Eye, FilePen, FilePlus, Folder as FolderIcon, FolderInput, FolderOpen, Info,
    LayoutGrid, List, Pencil, RefreshCw, Trash2, Upload, X,
};
use leptos::ev;
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use web_sys::{DragEvent, MouseEvent};

use crate::api;
use crate::components::ui::button::{Button, ButtonSize, ButtonVariant};
use crate::components::ui::card::Card;
use crate::components::ui::checkbox::Checkbox;
use crate::components::ui::empty::{Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant, EmptyTitle};
use crate::pages::drive::{FileItem, Kind, KindIcon, Preview, editable};
use crate::pages::files::{DRAG_TYPE, LayoutButton, MENU_BTN, MenuItem, NameDialog, Row, has_files, menu_position, save_file};
use crate::pages::sites::{self, Deploy, Editing, SiteCtx, deploy, drop_entries, dropped, file_url, public_path};
use crate::widgets::{ListSkeleton, Modal, Options, SearchBox, SortSelect, Ui, copy, fmt_date, fmt_size, fmt_time, has, query_state, use_ui};

const SORTS: Options =
    &[("name", "Name A-Z"), ("newest", "Newest"), ("oldest", "Oldest"), ("modified", "Last modified"), ("size", "Largest"), ("small", "Smallest")];

#[derive(Clone, Deserialize, PartialEq)]
pub(crate) struct SiteFile {
    pub path: String,
    pub size: i64,
    #[serde(default)]
    pub mime: String,
    #[serde(default)]
    pub created: i64,
    pub updated: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct Move {
    from: String,
    to: String,
}

fn is_dir(key: &str) -> bool {
    key.ends_with('/')
}

fn parent(key: &str) -> &str {
    key.trim_end_matches('/').rfind('/').map_or("", |i| &key[..=i])
}

fn base(key: &str) -> &str {
    &key[parent(key).len()..]
}

fn folder_key(at: &str) -> String {
    let at = at.trim_matches('/');
    if at.is_empty() { String::new() } else { format!("{at}/") }
}

fn inside<'a>(files: &'a [SiteFile], key: &'a str) -> impl Iterator<Item = &'a SiteFile> {
    files.iter().filter(move |f| f.path.strip_prefix(key).is_some_and(|rest| is_dir(key) || rest.is_empty()))
}

fn paths(files: &[SiteFile], keys: &[String]) -> Vec<String> {
    let mut list: Vec<String> = keys.iter().flat_map(|k| inside(files, k)).map(|f| f.path.clone()).collect();
    list.sort();
    list.dedup();
    list
}

fn relocate(files: &[SiteFile], key: &str, to: &str) -> Vec<Move> {
    inside(files, key).map(|f| Move { from: f.path.clone(), to: format!("{to}{}", &f.path[key.len()..]) }).filter(|m| m.from != m.to).collect()
}

fn move_into(files: &[SiteFile], keys: &[String], dir: &str) -> Vec<Move> {
    let mut list: Vec<Move> =
        keys.iter().filter(|k| !(is_dir(k) && dir.starts_with(k.as_str()))).flat_map(|k| relocate(files, k, &format!("{dir}{}", base(k)))).collect();
    list.sort_by(|a, b| a.from.cmp(&b.from));
    list.dedup_by(|a, b| a.from == b.from);
    list
}

fn renamed(key: &str, name: &str) -> String {
    let slash = if is_dir(key) { "/" } else { "" };
    format!("{}{}{slash}", parent(key), name.trim_matches('/'))
}

fn trail(dir: &str) -> Vec<(String, String)> {
    dir.match_indices('/').map(|(i, _)| (base(&dir[..=i]).trim_end_matches('/').to_owned(), dir[..=i].to_owned())).collect()
}

#[derive(Clone, Debug, PartialEq)]
struct Entry {
    key: String,
    name: String,
    mime: String,
    size: i64,
    created: i64,
    updated: i64,
    count: usize,
}

impl Entry {
    fn of(files: &[SiteFile], key: String) -> Entry {
        let list: Vec<&SiteFile> = inside(files, &key).collect();
        let mime = list.first().filter(|_| !is_dir(&key)).map(|f| f.mime.clone()).unwrap_or_default();
        Entry {
            name: base(&key).trim_end_matches('/').to_owned(),
            mime,
            size: list.iter().map(|f| f.size).sum(),
            created: list.iter().map(|f| f.created).min().unwrap_or_default(),
            updated: list.iter().map(|f| f.updated).max().unwrap_or_default(),
            count: list.len(),
            key,
        }
    }

    fn is_dir(&self) -> bool {
        is_dir(&self.key)
    }

    fn item(&self) -> FileItem {
        FileItem { id: self.key.clone(), name: self.name.clone(), mime: self.mime.clone(), size: self.size, created: self.created, ..Default::default() }
    }

    fn kind(&self) -> Kind {
        if self.is_dir() { Kind::Other } else { Kind::of(&self.item()) }
    }
}

fn listing(files: &[SiteFile], dir: &str) -> Vec<Entry> {
    let mut keys: Vec<String> = files
        .iter()
        .filter_map(|f| f.path.strip_prefix(dir))
        .map(|rest| rest.split_once('/').map_or_else(|| format!("{dir}{rest}"), |(head, _)| format!("{dir}{head}/")))
        .collect();
    keys.sort();
    keys.dedup();
    keys.into_iter().map(|k| Entry::of(files, k)).collect()
}

fn search(files: &[SiteFile], q: &str) -> Vec<Entry> {
    files.iter().filter(|f| has(&f.path, q)).map(|f| Entry { name: f.path.clone(), ..Entry::of(files, f.path.clone()) }).collect()
}

fn sort_entries(list: &mut [Entry], sort: &str) {
    match sort {
        "newest" => list.sort_by_key(|e| Reverse(e.created)),
        "oldest" => list.sort_by_key(|e| e.created),
        "modified" => list.sort_by_key(|e| Reverse(e.updated)),
        "size" => list.sort_by_key(|e| Reverse(e.size)),
        "small" => list.sort_by_key(|e| e.size),
        _ => list.sort_by_key(|e| e.name.to_lowercase()),
    }
    list.sort_by_key(|e| !e.is_dir());
}

#[derive(Clone)]
enum Dialog {
    Rename(String),
    Move(Vec<String>),
}

#[derive(Clone, Copy)]
struct Manager {
    ui: Ui,
    site: SiteCtx,
    dir: Memo<String>,
    go: Callback<String>,
    selected: RwSignal<Vec<String>>,
    details: RwSignal<Option<String>>,
    preview: RwSignal<Option<String>>,
    menu: RwSignal<Option<(String, i32, i32)>>,
    dialog: RwSignal<Option<Dialog>>,
    loaded: RwSignal<Option<f64>>,
}

impl Manager {
    fn files(self) -> Vec<SiteFile> {
        self.site.files.get().unwrap_or_default()
    }

    fn entry(self, key: &str) -> Option<Entry> {
        let files = self.files();
        inside(&files, key).next().is_some().then(|| Entry::of(&files, key.to_owned()))
    }

    fn entries(self, keys: &[String]) -> Vec<Entry> {
        untrack(|| keys.iter().filter_map(|k| self.entry(k)).collect())
    }

    fn raw(self, path: &str) -> String {
        api::url(&file_url(self.site.id, path))
    }

    fn public(self, path: &str) -> String {
        api::url(&public_path(&self.site.name.get_untracked(), path))
    }

    fn open(self, e: &Entry) {
        if e.is_dir() {
            self.go.run(e.key.clone());
        } else {
            self.preview.set(Some(e.key.clone()));
        }
    }

    fn edit(self, e: &Entry) {
        self.site.editing.set(Some(Editing::Existing(e.key.clone())));
    }

    fn post(self, action: &'static str, body: Value) {
        let path = format!("/api/sites/{}/{action}", self.site.id);
        spawn_local(async move {
            if self.ui.run(api::post::<Value>(&path, &body)).await.is_some() {
                self.selected.set(Vec::new());
                self.details.set(None);
                self.site.refresh();
            }
        });
    }

    fn apply(self, moves: Vec<Move>) {
        if !moves.is_empty() {
            self.post("move", json!({ "moves": moves }));
        }
    }

    fn rename(self, key: &str, name: &str) {
        self.apply(relocate(&untrack(|| self.files()), key, &renamed(key, name)));
    }

    fn move_to(self, keys: &[String], dir: &str) {
        self.apply(move_into(&untrack(|| self.files()), keys, dir));
    }

    fn remove(self, entries: Vec<Entry>) {
        let what = match entries.as_slice() {
            [one] => format!("\"{}\"", one.name),
            many => format!("{} items", many.len()),
        };
        let note = if entries.iter().any(Entry::is_dir) { " Folders are deleted with everything inside." } else { "" };
        let keys: Vec<String> = entries.into_iter().map(|e| e.key).collect();
        self.ui.confirm_delete(format!("{what} will be deleted permanently.{note}"), move || {
            self.post("delete", json!({ "paths": paths(&untrack(|| self.files()), &keys) }));
        });
    }

    fn upload(self, picked: sites::Upload, dir: String) {
        let label = picked.label();
        deploy(self.site.id, &picked, false, &dir, self.loaded, move |res| match res {
            Ok(()) => {
                self.ui.notify(format!("Uploaded {label}"));
                self.site.refresh();
            }
            Err(e) => self.ui.fail(e),
        });
    }

    fn drop_into(self, dir: String, ev: &DragEvent) {
        ev.prevent_default();
        ev.stop_propagation();
        let Some(dt) = ev.data_transfer() else { return };
        let keys = dt.get_data(DRAG_TYPE).unwrap_or_default();
        if !keys.is_empty() {
            let keys: Vec<String> = keys.split('\n').map(str::to_owned).collect();
            return self.move_to(&keys, &dir);
        }
        let entries = drop_entries(&dt);
        let files = dt.files().map(|l| api::files_of(&l)).unwrap_or_default();
        spawn_local(async move {
            if let Some(picked) = dropped(entries, files).await {
                self.upload(picked, dir);
            }
        });
    }

    fn drag_start(self, key: &str, ev: &DragEvent) {
        let sel = self.selected.get_untracked();
        let keys = if sel.iter().any(|s| s == key) { sel.join("\n") } else { key.to_owned() };
        if let Some(dt) = ev.data_transfer() {
            let _ = dt.set_data(DRAG_TYPE, &keys);
            dt.set_effect_allowed("move");
        }
    }

    fn file_paths(self, entries: &[Entry]) -> Vec<String> {
        let keys: Vec<String> = entries.iter().map(|e| e.key.clone()).collect();
        paths(&untrack(|| self.files()), &keys)
    }

    fn download(self, entries: Vec<Entry>) {
        for (i, path) in (0u64..).zip(self.file_paths(&entries)) {
            let (url, name) = (self.raw(&path), base(&path).to_owned());
            set_timeout(move || save_file(&url, &name), Duration::from_millis(300 * i));
        }
    }

    fn copy_links(self, entries: Vec<Entry>) {
        let name = self.site.name.get_untracked();
        let links: Vec<String> = self.file_paths(&entries).iter().map(|p| api::absolute(&public_path(&name, p))).collect();
        copy(self.ui, &links.join("\n"));
    }

    fn toggle(self, key: &str) {
        self.selected.update(|s| match s.iter().position(|x| x == key) {
            Some(i) => {
                s.remove(i);
            }
            None => s.push(key.to_owned()),
        });
    }
}

fn use_manager() -> Manager {
    expect_context()
}

fn empty_text(searching: bool, root: bool) -> (&'static str, &'static str) {
    match (searching, root) {
        (true, _) => ("No results", "Nothing matches your search."),
        (false, true) => ("No files yet", "Drop files here or use the Upload button. An index.html is served as the home page."),
        (false, false) => ("This folder is empty", "Drop files here or use the Upload button."),
    }
}

#[component]
pub(crate) fn SiteFiles(site: SiteCtx) -> impl IntoView {
    let ui = use_ui();
    let q = RwSignal::new(String::new());
    let (at, set_at) = query_state("in", "");
    let (sort, set_sort) = query_state("sort", "name");
    let (layout, set_layout) = query_state("layout", "list");
    let dir = Memo::new(move |_| at.with(|a| folder_key(a)));
    let m = Manager {
        ui,
        site,
        dir,
        go: Callback::new(move |key: String| {
            q.set(String::new());
            set_at.run(key.trim_end_matches('/').to_owned());
        }),
        selected: RwSignal::new(Vec::new()),
        details: RwSignal::new(None),
        preview: RwSignal::new(None),
        menu: RwSignal::new(None),
        dialog: RwSignal::new(None),
        loaded: RwSignal::new(None),
    };
    provide_context(m);
    Effect::new(move |_| {
        dir.track();
        m.selected.set(Vec::new());
    });
    let searching = Signal::derive(move || !q.get().trim().is_empty());
    let shown = Memo::new(move |_| {
        let files = m.files();
        let mut list = if searching.get() { search(&files, &q.get()) } else { listing(&files, &dir.get()) };
        sort_entries(&mut list, &sort.get());
        list
    });
    let previewable = Signal::derive(move || shown.with(|l| l.iter().filter(|e| !e.is_dir()).map(Entry::item).collect()));
    let keys = window_event_listener(ev::keydown, move |e| {
        let busy = m.preview.get_untracked().is_some() || m.dialog.get_untracked().is_some() || site.editing.get_untracked().is_some();
        if e.key() != "Escape" || busy || site.deploying.get_untracked().is_some() {
            return;
        }
        m.menu.set(None);
        m.details.set(None);
        m.selected.set(Vec::new());
    });
    on_cleanup(move || keys.remove());
    let over = RwSignal::new(false);
    let grid = Signal::derive(move || layout.get() == "grid");
    let uploading = move || m.loaded.with(Option::is_some);

    view! {
        <div
            class="flex relative flex-col gap-4"
            on:dragenter=move |ev: DragEvent| {
                if has_files(&ev) {
                    over.set(true);
                }
            }
        >
            <div class="flex flex-wrap gap-2 justify-between items-center">
                <h2 class="text-lg font-semibold">"Files"</h2>
                <div class="flex flex-wrap gap-2 items-center">
                    <Button variant=ButtonVariant::Outline on:click=move |_| site.editing.set(Some(Editing::New(dir.get_untracked())))>
                        <FilePlus />
                        <span class="hidden sm:inline">"New file"</span>
                    </Button>
                    <Button
                        variant=ButtonVariant::Outline
                        attr:disabled=uploading
                        on:click=move |_| site.deploying.set(Some(Deploy { replace: false, dir: dir.get_untracked() }))
                    >
                        <Upload />
                        <span class="hidden sm:inline">{move || if uploading() { "Uploading..." } else { "Upload" }}</span>
                    </Button>
                    <Button on:click=move |_| site.deploying.set(Some(Deploy { replace: true, dir: String::new() }))>
                        <RefreshCw />
                        <span class="hidden sm:inline">"Redeploy"</span>
                    </Button>
                </div>
            </div>
            <Show when=move || m.selected.with(Vec::is_empty) fallback=|| view! { <BulkBar /> }>
                <div class="flex flex-wrap gap-2 justify-between items-center min-h-[2.75rem]">
                    <Crumbs searching />
                    <div class="flex flex-wrap gap-2 items-center">
                        <SearchBox value=q placeholder="Search files  /" />
                        <SortSelect options=SORTS value=sort on_change=set_sort />
                        <div class="inline-flex gap-1 items-center p-1 rounded-lg bg-muted">
                            <LayoutButton value="list" current=layout on_change=set_layout title="List view"><List /></LayoutButton>
                            <LayoutButton value="grid" current=layout on_change=set_layout title="Grid view"><LayoutGrid /></LayoutButton>
                        </div>
                    </div>
                </div>
            </Show>
            {move || {
                let list = shown.get();
                if site.files.get().is_none() {
                    return view! { <Card class="overflow-hidden gap-0 py-0"><ListSkeleton rows=3 /></Card> }.into_any();
                }
                if list.is_empty() {
                    let (title, text) = empty_text(searching.get(), dir.with(String::is_empty));
                    return view! {
                        <Empty class="py-16">
                            <EmptyHeader>
                                <EmptyMedia variant=EmptyMediaVariant::Icon><FolderOpen /></EmptyMedia>
                                <EmptyTitle>{title}</EmptyTitle>
                                <EmptyDescription>{text}</EmptyDescription>
                            </EmptyHeader>
                        </Empty>
                    }
                    .into_any();
                }
                if grid.get() { view! { <GridView entries=list /> }.into_any() } else { view! { <ListView entries=list /> }.into_any() }
            }}
            <StorageLine />
            <Show when=move || over.get()>
                <div
                    class="flex absolute inset-0 z-30 flex-col gap-2 justify-center items-center rounded-xl border-2 border-dashed backdrop-blur-sm border-primary bg-background/80 animate-in fade-in-0"
                    on:dragover=move |ev: DragEvent| ev.prevent_default()
                    on:dragleave=move |_| over.set(false)
                    on:drop=move |ev: DragEvent| {
                        over.set(false);
                        m.drop_into(dir.get_untracked(), &ev);
                    }
                >
                    <Upload class="size-8 pointer-events-none" />
                    <span class="font-medium pointer-events-none">"Drop files to upload them here"</span>
                </div>
            </Show>
            <Preview
                files=previewable
                current=m.preview
                src=Callback::new(move |key: String| m.raw(&key))
                save_path=Callback::new(move |key: String| file_url(site.id, &key))
                page_path=Callback::new(move |key: String| format!("/api/sites/{}/preview?path={}", site.id, js_sys::encode_uri_component(&key)))
                on_saved=Callback::new(move |()| site.refresh())
            />
            <ItemMenu />
            <Details />
            <Dialogs />
        </div>
    }
}

#[component]
fn Crumbs(searching: Signal<bool>) -> impl IntoView {
    let m = use_manager();
    view! {
        <nav class="flex flex-wrap gap-1 items-center min-w-0 text-lg font-medium">
            {move || {
                if searching.get() {
                    return view! { <span class="px-2">"Search results"</span> }.into_any();
                }
                view! {
                    <Crumb target=String::new() name=m.site.name.get() />
                    {trail(&m.dir.get()).into_iter().map(|(name, key)| view! {
                        <ChevronRight class="size-4 text-muted-foreground shrink-0" />
                        <Crumb target=key name />
                    }).collect_view()}
                }
                .into_any()
            }}
        </nav>
    }
}

#[component]
fn Crumb(target: String, name: String) -> impl IntoView {
    let m = use_manager();
    let hover = RwSignal::new(false);
    let key = StoredValue::new(target);
    view! {
        <button
            type="button"
            class="py-1 px-2 rounded-md transition-colors hover:bg-muted max-w-56 truncate"
            class=("bg-accent", move || hover.get())
            on:click=move |_| m.go.run(key.get_value())
            on:dragover=move |ev: DragEvent| {
                ev.prevent_default();
                hover.set(true);
            }
            on:dragleave=move |_| hover.set(false)
            on:drop=move |ev: DragEvent| {
                hover.set(false);
                m.drop_into(key.get_value(), &ev);
            }
        >
            {name}
        </button>
    }
}

#[component]
fn BulkBar() -> impl IntoView {
    let m = use_manager();
    let picked = move || m.entries(&m.selected.get());
    let single = move || match picked().as_slice() {
        [one] => Some(one.clone()),
        _ => None,
    };
    let text = move || single().is_some_and(|e| !e.is_dir() && editable(&e.key));
    view! {
        <div class="flex flex-wrap gap-1 items-center px-2 rounded-xl border min-h-[2.75rem] bg-muted/60 animate-in fade-in-0">
            <Button variant=ButtonVariant::Ghost size=ButtonSize::IconSm attr:title="Clear selection" on:click=move |_| m.selected.set(Vec::new())>
                <X />
            </Button>
            <span class="mr-2 text-sm font-medium">{move || format!("{} selected", m.selected.with(Vec::len))}</span>
            <Show when=move || single().is_some()>
                <Button
                    variant=ButtonVariant::Ghost
                    size=ButtonSize::Sm
                    on:click=move |_| m.dialog.set(untrack(single).map(|e| Dialog::Rename(e.key)))
                >
                    <Pencil />
                    "Rename"
                </Button>
            </Show>
            <Show when=text>
                <Button variant=ButtonVariant::Ghost size=ButtonSize::Sm on:click=move |_| untrack(single).iter().for_each(|e| m.edit(e))>
                    <FilePen />
                    "Edit"
                </Button>
            </Show>
            <Button variant=ButtonVariant::Ghost size=ButtonSize::Sm on:click=move |_| m.dialog.set(Some(Dialog::Move(m.selected.get_untracked())))>
                <FolderInput />
                "Move"
            </Button>
            <Button variant=ButtonVariant::Ghost size=ButtonSize::Sm on:click=move |_| m.download(untrack(picked))>
                <Download />
                "Download"
            </Button>
            <Button variant=ButtonVariant::Ghost size=ButtonSize::Sm on:click=move |_| m.copy_links(untrack(picked))>
                <Copy />
                "Copy links"
            </Button>
            <Button variant=ButtonVariant::Ghost size=ButtonSize::Sm class="text-destructive" on:click=move |_| m.remove(untrack(picked))>
                <Trash2 />
                "Delete"
            </Button>
        </div>
    }
}

const LIST_COLUMNS: &str =
    "grid-cols-[1.25rem_minmax(0,1fr)_2rem] sm:grid-cols-[1.25rem_minmax(0,1fr)_9rem_6rem_2rem] lg:grid-cols-[1.25rem_minmax(0,1fr)_9rem_9rem_6rem_2rem]";

#[component]
fn ListView(entries: Vec<Entry>) -> impl IntoView {
    let m = use_manager();
    let keys = StoredValue::new(entries.iter().map(|e| e.key.clone()).collect::<Vec<_>>());
    let all = move || keys.with_value(|k| !k.is_empty() && m.selected.with(|s| k.iter().all(|key| s.contains(key))));
    let toggle_all = move |_| {
        let next = if untrack(all) { Vec::new() } else { keys.get_value() };
        m.selected.set(next);
    };
    view! {
        <Card class="overflow-hidden gap-0 py-0">
            <div class=format!("grid items-center gap-3 px-3 h-10 text-xs font-medium border-b bg-muted/50 text-muted-foreground {LIST_COLUMNS}")>
                <Checkbox aria_label="Select all" checked=Signal::derive(all) on_checked_change=Callback::new(toggle_all) />
                <span>"Name"</span>
                <span class="hidden lg:block">"Created"</span>
                <span class="hidden sm:block">"Modified"</span>
                <span class="hidden text-right sm:block">"Size"</span>
                <span />
            </div>
            {entries.into_iter().map(|entry| view! { <Item entry grid=false /> }).collect_view()}
        </Card>
    }
}

#[component]
fn GridView(entries: Vec<Entry>) -> impl IntoView {
    let section = "text-sm font-medium text-muted-foreground";
    let (dirs, docs): (Vec<Entry>, Vec<Entry>) = entries.into_iter().partition(Entry::is_dir);
    let (any_dir, any_doc) = (!dirs.is_empty(), !docs.is_empty());
    view! {
        <Show when=move || any_dir>
            <h3 class=section>"Folders"</h3>
        </Show>
        <div class="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-4">
            {dirs.into_iter().map(|entry| view! { <Item entry grid=true /> }).collect_view()}
        </div>
        <Show when=move || any_doc>
            <h3 class=section>"Files"</h3>
        </Show>
        <div class="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-4">
            {docs.into_iter().map(|entry| view! { <Item entry grid=true /> }).collect_view()}
        </div>
    }
}

fn icon(e: &Entry, class: &'static str) -> AnyView {
    if e.is_dir() {
        return view! { <FolderIcon class=format!("{class} fill-current text-amber-500") /> }.into_any();
    }
    view! { <KindIcon kind=e.kind() class=format!("{class} text-muted-foreground") /> }.into_any()
}

fn thumb(m: Manager, e: &Entry) -> AnyView {
    match e.kind() {
        Kind::Image => view! { <img src=m.raw(&e.key) loading="lazy" alt="" class="object-cover size-full" /> }.into_any(),
        Kind::Video => view! { <video src=format!("{}#t=0.5", m.raw(&e.key)) preload="metadata" muted class="object-cover pointer-events-none size-full" /> }.into_any(),
        _ => icon(e, "size-12"),
    }
}

#[component]
fn Item(entry: Entry, grid: bool) -> impl IntoView {
    let m = use_manager();
    let e = StoredValue::new(entry.clone());
    let key = StoredValue::new(entry.key.clone());
    let hover = RwSignal::new(false);
    let picked = move || m.selected.with(|s| s.contains(&key.get_value()));
    let open = move |ev: MouseEvent| {
        if ev.ctrl_key() || ev.meta_key() || !m.selected.with_untracked(Vec::is_empty) {
            return m.toggle(&key.get_value());
        }
        m.open(&e.get_value());
    };
    let show_menu = move |ev: MouseEvent| {
        ev.prevent_default();
        ev.stop_propagation();
        m.menu.set(Some((key.get_value(), ev.client_x(), ev.client_y())));
    };
    let is_dir = entry.is_dir();
    let drop_target = move |ev: DragEvent| {
        if is_dir {
            ev.prevent_default();
            hover.set(true);
        }
    };
    let on_drop = move |ev: DragEvent| {
        hover.set(false);
        if is_dir {
            m.drop_into(key.get_value(), &ev);
        }
    };
    let check = view! {
        <Checkbox
            aria_label="Select"
            checked=Signal::derive(picked)
            on_checked_change=Callback::new(move |_| m.toggle(&key.get_value()))
            on:click=|ev: MouseEvent| ev.stop_propagation()
        />
    };
    let more = view! {
        <button type="button" class="flex justify-center items-center rounded-md size-8 hover:bg-accent shrink-0" title="More actions" on:click=show_menu>
            <EllipsisVertical class="size-4" />
        </button>
    };
    let base = if grid {
        "flex overflow-hidden relative flex-col rounded-xl border transition-all cursor-pointer select-none group bg-card hover:shadow-md".to_owned()
    } else {
        format!("grid items-center gap-3 px-3 py-1.5 border-b last:border-b-0 transition-colors cursor-pointer select-none group hover:bg-muted/50 {LIST_COLUMNS}")
    };
    let body = if grid {
        view! {
            <Show when=move || !is_dir>
                <div class="flex overflow-hidden justify-center items-center border-b aspect-video bg-muted/60">{thumb(m, &e.get_value())}</div>
            </Show>
            <div class="flex gap-2 items-center py-2 pr-1 pl-3 min-w-0">
                {icon(&entry, "size-4 shrink-0")}
                <span class="flex-1 text-sm font-medium truncate" title=entry.name.clone()>{entry.name.clone()}</span>
                {more}
            </div>
            <div class="absolute top-2 left-2 sm:opacity-0 sm:group-hover:opacity-100 sm:pointer-coarse:opacity-100" class=("sm:opacity-100", picked)>{check}</div>
        }
        .into_any()
    } else {
        view! {
            {check}
            <div class="flex gap-3 items-center py-1 min-w-0">
                <div class="flex justify-center items-center rounded-lg size-9 bg-muted shrink-0">{icon(&entry, "size-4")}</div>
                <span class="text-sm font-medium truncate" title=entry.name.clone()>{entry.name.clone()}</span>
            </div>
            <span class="hidden text-sm lg:block text-muted-foreground">{fmt_date(entry.created)}</span>
            <span class="hidden text-sm sm:block text-muted-foreground">{fmt_date(entry.updated)}</span>
            <span class="hidden text-sm tabular-nums text-right sm:block text-muted-foreground">{fmt_size(entry.size)}</span>
            {more}
        }
        .into_any()
    };
    view! {
        <div
            class=base
            class=("bg-accent", move || picked() || hover.get())
            class=("ring-2", move || grid && (picked() || hover.get()))
            class=("ring-primary", move || grid && (picked() || hover.get()))
            draggable="true"
            on:click=open
            on:contextmenu=show_menu
            on:dragstart=move |ev: DragEvent| m.drag_start(&key.get_value(), &ev)
            on:dragover=drop_target
            on:dragleave=move |_| hover.set(false)
            on:drop=on_drop
        >
            {body}
        </div>
    }
}

#[component]
fn ItemMenu() -> impl IntoView {
    let m = use_manager();
    let target = move || m.menu.get().and_then(|(key, x, y)| untrack(|| m.entry(&key)).map(|e| (e, x, y)));
    view! {
        {move || target().map(|(entry, x, y)| {
            let is_dir = entry.is_dir();
            let text = !is_dir && editable(&entry.key);
            let (raw, public, name) = (m.raw(&entry.key), m.public(&entry.key), entry.name.clone());
            let e = StoredValue::new(entry);
            let act = move |f: fn(Manager, Entry)| Callback::new(move |_| f(m, e.get_value()));
            view! {
                <div class="fixed inset-0 z-40" on:click=move |_| m.menu.set(None) on:contextmenu=move |ev: MouseEvent| { ev.prevent_default(); m.menu.set(None); } />
                <div
                    class="overflow-y-auto fixed z-50 p-1 w-56 rounded-lg border shadow-lg max-h-[calc(100vh-2rem)] bg-popover text-popover-foreground pop-in [&_svg]:size-4 [&_svg]:text-muted-foreground"
                    style=menu_position(x, y, 400)
                    on:click=move |_| m.menu.set(None)
                >
                    <MenuItem label=if is_dir { "Open" } else { "Preview" } action=act(|m, e| m.open(&e))>
                        {if is_dir { view! { <FolderOpen /> }.into_any() } else { view! { <Eye /> }.into_any() }}
                    </MenuItem>
                    <Show when=move || !is_dir>
                        <a class=MENU_BTN href=public.clone() target="_blank" rel="noopener">
                            <ExternalLink />
                            "Open in new tab"
                        </a>
                        <Show when=move || text>
                            <MenuItem label="Edit" action=act(|m, e| m.edit(&e))><FilePen /></MenuItem>
                        </Show>
                        <a class=MENU_BTN href=raw.clone() download=name.clone()>
                            <Download />
                            "Download"
                        </a>
                        <MenuItem label="Copy link" action=act(|m, e| m.copy_links(vec![e]))><Copy /></MenuItem>
                    </Show>
                    <div class="my-1 h-px bg-border" />
                    <MenuItem label="Rename" action=act(|m, e| m.dialog.set(Some(Dialog::Rename(e.key))))><Pencil /></MenuItem>
                    <MenuItem label="Move to" action=act(|m, e| m.dialog.set(Some(Dialog::Move(vec![e.key]))))><FolderInput /></MenuItem>
                    <MenuItem label="Details" action=act(|m, e| m.details.set(Some(e.key)))><Info /></MenuItem>
                    <div class="my-1 h-px bg-border" />
                    <MenuItem label="Delete" danger=true action=act(|m, e| m.remove(vec![e]))><Trash2 /></MenuItem>
                </div>
            }
        })}
    }
}

fn facts(m: Manager, e: &Entry) -> AnyView {
    let (path, created, updated) = (e.key.clone(), fmt_time(e.created), fmt_time(e.updated));
    let size = format!("{} ({} bytes)", fmt_size(e.size), e.size);
    if e.is_dir() {
        let contents = format!("{} file(s)", e.count);
        return view! {
            <Row label="Type">"Folder"</Row>
            <Row label="Path">{path}</Row>
            <Row label="Contents">{contents}</Row>
            <Row label="Total size">{size}</Row>
            <Row label="Oldest created">{created}</Row>
            <Row label="Last modified">{updated}</Row>
        }
        .into_any();
    }
    let mime = if e.mime.is_empty() { "unknown" } else { &e.mime };
    let kind = format!("{} ({mime})", e.kind().label());
    let link = api::absolute(&public_path(&m.site.name.get_untracked(), &e.key));
    view! {
        <Row label="Type">{kind}</Row>
        <Row label="Path">{path}</Row>
        <Row label="Size">{size}</Row>
        <Row label="Created">{created}</Row>
        <Row label="Modified">{updated}</Row>
        <Row label="Public link">
            <a href=link.clone() target="_blank" rel="noopener" class="break-all hover:underline underline-offset-4">{link.clone()}</a>
        </Row>
    }
    .into_any()
}

#[component]
fn Details() -> impl IntoView {
    let m = use_manager();
    let entry = move || m.details.get().and_then(|key| m.entry(&key));
    view! {
        {move || entry().map(|e| {
            let shown = facts(m, &e);
            let key = StoredValue::new(e.key.clone());
            let opener = e.clone();
            view! {
                <div class="fixed inset-0 z-40 bg-black/30 animate-in fade-in-0" on:click=move |_| m.details.set(None) />
                <aside class="flex overflow-y-auto fixed inset-y-0 right-0 z-50 flex-col gap-5 p-5 w-full border-l shadow-xl sm:w-96 bg-background animate-in slide-in-from-right duration-300">
                    <div class="flex gap-3 items-center">
                        {icon(&e, "size-5 shrink-0")}
                        <h3 class="flex-1 font-semibold break-all">{e.name.clone()}</h3>
                        <Button variant=ButtonVariant::Ghost size=ButtonSize::IconSm attr:title="Close" on:click=move |_| m.details.set(None)><X /></Button>
                    </div>
                    <div class="flex overflow-hidden justify-center items-center rounded-xl border aspect-video bg-muted/60">{thumb(m, &e)}</div>
                    <div class="flex flex-wrap gap-2">
                        <Button size=ButtonSize::Sm on:click=move |_| { m.details.set(None); m.open(&opener); }>
                            <Eye />
                            "Open"
                        </Button>
                        <Button variant=ButtonVariant::Outline size=ButtonSize::Sm on:click=move |_| m.dialog.set(Some(Dialog::Rename(key.get_value())))>
                            <Pencil />
                            "Rename"
                        </Button>
                        <Button variant=ButtonVariant::Outline size=ButtonSize::Sm on:click=move |_| m.dialog.set(Some(Dialog::Move(vec![key.get_value()])))>
                            <FolderInput />
                            "Move"
                        </Button>
                    </div>
                    <div class="flex flex-col gap-4">{shown}</div>
                </aside>
            }
        })}
    }
}

#[component]
fn Dialogs() -> impl IntoView {
    let m = use_manager();
    view! {
        {move || m.dialog.get().map(|dialog| {
            let open = RwSignal::new(true);
            Effect::new(move |_| {
                if !open.get() {
                    m.dialog.set(None);
                }
            });
            match dialog {
                Dialog::Rename(key) => {
                    let initial = base(&key).trim_end_matches('/').to_owned();
                    view! { <NameDialog open title="Rename" initial on_submit=Callback::new(move |name: String| m.rename(&key, &name)) /> }.into_any()
                }
                Dialog::Move(keys) => view! { <MoveDialog open keys /> }.into_any(),
            }
        })}
    }
}

#[component]
fn MoveDialog(open: RwSignal<bool>, keys: Vec<String>) -> impl IntoView {
    let m = use_manager();
    let at = RwSignal::new(String::new());
    let keys = StoredValue::new(keys);
    let children = move || {
        let files = m.files();
        let here = at.get();
        listing(&files, &here).into_iter().filter(|e| e.is_dir() && !keys.with_value(|k| k.contains(&e.key))).collect::<Vec<_>>()
    };
    let confirm = move |_| {
        m.move_to(&keys.get_value(), &at.get_untracked());
        open.set(false);
    };
    let crumb = "py-1 px-2 rounded-md hover:bg-muted truncate max-w-40";
    view! {
        <Modal open title="Move to">
            <div class="flex flex-wrap gap-0.5 items-center text-sm">
                <button type="button" class=crumb on:click=move |_| at.set(String::new())>{move || m.site.name.get()}</button>
                {move || trail(&at.get()).into_iter().map(|(name, key)| view! {
                    <ChevronRight class="size-3.5 text-muted-foreground" />
                    <button type="button" class=crumb on:click=move |_| at.set(key.clone())>{name}</button>
                }).collect_view()}
            </div>
            <div class="overflow-y-auto max-h-72 rounded-lg border divide-y min-h-32">
                {move || {
                    let list = children();
                    if list.is_empty() {
                        return view! { <p class="p-4 text-sm text-center text-muted-foreground">"No folders here"</p> }.into_any();
                    }
                    list.into_iter().map(|e| view! {
                        <button type="button" class="flex gap-3 items-center px-3 w-full h-10 text-sm text-left hover:bg-muted" on:click=move |_| at.set(e.key.clone())>
                            <FolderIcon class="text-amber-500 fill-current size-4 shrink-0" />
                            <span class="flex-1 truncate">{e.name.clone()}</span>
                            <ChevronRight class="size-4 text-muted-foreground" />
                        </button>
                    }).collect_view().into_any()
                }}
            </div>
            <div class="flex gap-2 justify-end">
                <button type="button" class="px-4 h-9 text-sm font-medium rounded-md hover:bg-accent" on:click=move |_| open.set(false)>"Cancel"</button>
                <Button on:click=confirm>"Move here"</Button>
            </div>
        </Modal>
    }
}

#[component]
fn StorageLine() -> impl IntoView {
    let m = use_manager();
    let text = move || {
        let files = m.files();
        format!("{} file(s), {}", files.len(), fmt_size(files.iter().map(|f| f.size).sum()))
    };
    view! { <p class="text-xs text-muted-foreground">{text}</p> }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, size: i64, created: i64, updated: i64) -> SiteFile {
        SiteFile { path: path.into(), size, mime: String::new(), created, updated }
    }

    fn site() -> Vec<SiteFile> {
        vec![file("index.html", 10, 5, 50), file("css/app.css", 20, 3, 30), file("css/vendor/a.css", 5, 7, 70), file("app.css", 1, 1, 1)]
    }

    fn keys(list: &[Entry]) -> Vec<&str> {
        list.iter().map(|e| e.key.as_str()).collect()
    }

    #[test]
    fn listing_groups_paths_into_virtual_folders() {
        let files = site();
        let root = listing(&files, "");
        assert_eq!(keys(&root), ["app.css", "css/", "index.html"]);
        let css = root.iter().find(|e| e.key == "css/").cloned().unwrap_or_else(|| panic!("css folder"));
        assert_eq!((css.name.as_str(), css.size, css.created, css.updated, css.count), ("css", 25, 3, 70, 2));
        assert_eq!(keys(&listing(&files, "css/")), ["css/app.css", "css/vendor/"]);
        assert!(listing(&files, "missing/").is_empty());
    }

    #[test]
    fn folders_sort_before_files() {
        let mut list = listing(&site(), "");
        sort_entries(&mut list, "size");
        assert_eq!(keys(&list), ["css/", "index.html", "app.css"]);
    }

    #[test]
    fn folder_moves_expand_to_every_file_inside() {
        let files = site();
        let moved = move_into(&files, &["css/".to_owned(), "index.html".to_owned()], "public/");
        let pairs: Vec<(&str, &str)> = moved.iter().map(|m| (m.from.as_str(), m.to.as_str())).collect();
        assert_eq!(pairs, [("css/app.css", "public/css/app.css"), ("css/vendor/a.css", "public/css/vendor/a.css"), ("index.html", "public/index.html")]);
        assert!(move_into(&files, &["css/".to_owned()], "css/vendor/").is_empty());
        assert!(move_into(&files, &["css/app.css".to_owned()], "css/").is_empty());
    }

    #[test]
    fn rename_keeps_parent_and_kind() {
        let files = site();
        assert_eq!(renamed("css/vendor/", "lib"), "css/lib/");
        assert_eq!(renamed("css/app.css", "main.css"), "css/main.css");
        let moved = relocate(&files, "css/vendor/", &renamed("css/vendor/", "lib"));
        assert_eq!(moved, [Move { from: "css/vendor/a.css".into(), to: "css/lib/a.css".into() }]);
        assert_eq!(relocate(&files, "app.css", "main.css").len(), 1);
    }

    #[test]
    fn paths_expand_folders_without_prefix_collisions() {
        let files = site();
        assert_eq!(paths(&files, &["css/".to_owned(), "css/app.css".to_owned()]), ["css/app.css", "css/vendor/a.css"]);
        assert_eq!(paths(&files, &["app.css".to_owned()]), ["app.css"]);
    }

    #[test]
    fn trail_lists_each_folder_prefix() {
        assert_eq!(trail("css/vendor/"), [("css".to_owned(), "css/".to_owned()), ("vendor".to_owned(), "css/vendor/".to_owned())]);
        assert!(trail("").is_empty());
        assert_eq!(folder_key("/css/vendor/"), "css/vendor/");
        assert_eq!(folder_key(""), "");
    }
}
