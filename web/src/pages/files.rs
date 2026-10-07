use std::cmp::Reverse;

use icons::{
    Archive, ArchiveRestore, ChevronRight, Copy, Download, EllipsisVertical, Eye, Folder as FolderIcon, FolderInput,
    FolderOpen, FolderPlus, Info, LayoutGrid, Link, List, Pencil, Share2, Star, StarOff, Trash2, Upload, X,
};
use leptos::ev::{self, SubmitEvent};
use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::Serialize;
use serde_json::Value;
use wasm_bindgen::JsValue;
use web_sys::{DragEvent, File, MouseEvent};

use crate::api;
use crate::components::ui::button::{Button, ButtonSize, ButtonVariant};
use crate::components::ui::card::Card;
use crate::components::ui::checkbox::Checkbox;
use crate::components::ui::empty::{Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant, EmptyTitle};
use crate::components::ui::input::Input;
use crate::id::Id;
use crate::pages::drive::{FileItem, Folder, Kind, KindIcon, Preview, trail};
use crate::widgets::{
    ListSkeleton, Modal, Options, PageHeader, SearchBox, Segmented, ShareDialog, SortSelect, Ui, copy, fmt_date, fmt_size,
    fmt_time, has, query_state, use_ui,
};

const TABS: Options = &[("", "My Drive"), ("recent", "Recent"), ("starred", "Starred"), ("shared", "Shared"), ("archived", "Archived")];
const PROJECT_TABS: Options = &[("", "Files"), ("archived", "Archived")];
const SORTS: Options =
    &[("name", "Name A-Z"), ("newest", "Newest"), ("oldest", "Oldest"), ("size", "Largest"), ("small", "Smallest")];
const DRAG_TYPE: &str = "application/x-rustkeep-items";
const MENU_BTN: &str = "flex gap-3 items-center px-3 w-full h-9 text-sm text-left rounded-md hover:bg-accent";

fn raw_url(id: &str) -> String {
    api::url(&format!("/api/files/{id}/raw"))
}

#[derive(Clone, PartialEq)]
enum Entry {
    Dir(Folder),
    Doc(FileItem),
}

#[derive(Clone, Serialize)]
struct Edit {
    name: String,
    parent: Option<String>,
    starred: bool,
}

impl Entry {
    fn id(&self) -> &str {
        match self {
            Entry::Dir(f) => &f.id,
            Entry::Doc(f) => &f.id,
        }
    }

    fn name(&self) -> &str {
        match self {
            Entry::Dir(f) => &f.name,
            Entry::Doc(f) => &f.name,
        }
    }

    fn starred(&self) -> bool {
        match self {
            Entry::Dir(f) => f.starred == 1,
            Entry::Doc(f) => f.starred == 1,
        }
    }

    fn token(&self) -> Option<String> {
        match self {
            Entry::Dir(f) => f.share_token.clone(),
            Entry::Doc(f) => f.share_token.clone(),
        }
    }

    fn api(&self) -> String {
        match self {
            Entry::Dir(f) => format!("/api/folders/{}", f.id),
            Entry::Doc(f) => format!("/api/files/{}", f.id),
        }
    }

    fn edit(&self) -> Edit {
        let parent = match self {
            Entry::Dir(f) => f.parent_id.clone(),
            Entry::Doc(f) => f.folder_id.clone(),
        };
        Edit { name: self.name().to_owned(), parent, starred: self.starred() }
    }

    fn is_dir(&self) -> bool {
        matches!(self, Entry::Dir(_))
    }
}

#[derive(Clone, PartialEq)]
enum Scope {
    Folder(Option<String>),
    Recent,
    Starred,
    Shared,
    Archived,
    Project,
}

impl Scope {
    fn new(at: &str, project: bool) -> Scope {
        match at {
            "recent" => Scope::Recent,
            "starred" => Scope::Starred,
            "shared" => Scope::Shared,
            "archived" => Scope::Archived,
            _ if project => Scope::Project,
            "" => Scope::Folder(None),
            id => Scope::Folder(Some(id.to_owned())),
        }
    }

    fn tab(&self) -> &'static str {
        match self {
            Scope::Recent => "recent",
            Scope::Starred => "starred",
            Scope::Shared => "shared",
            Scope::Archived => "archived",
            _ => "",
        }
    }

    fn holds_file(&self, f: &FileItem, searching: bool) -> bool {
        match self {
            Scope::Archived => f.archived == 1,
            _ if f.archived == 1 => false,
            Scope::Folder(cur) => searching || f.folder_id == *cur,
            Scope::Starred => f.starred == 1,
            Scope::Shared => f.share_token.is_some(),
            Scope::Recent | Scope::Project => true,
        }
    }

    fn holds_folder(&self, f: &Folder, searching: bool) -> bool {
        match self {
            Scope::Folder(cur) => searching || f.parent_id == *cur,
            Scope::Starred => f.starred == 1,
            Scope::Shared => f.share_token.is_some(),
            _ => false,
        }
    }

    fn empty_text(&self, searching: bool) -> (&'static str, &'static str) {
        match self {
            _ if searching => ("No results", "Nothing matches your search."),
            Scope::Folder(_) | Scope::Project => ("This folder is empty", "Drop files here or use the Upload button."),
            Scope::Recent => ("No recent files", "Files you upload show up here."),
            Scope::Starred => ("Nothing starred", "Star files and folders to find them quickly."),
            Scope::Shared => ("Nothing shared", "Items with a public link show up here."),
            Scope::Archived => ("No archived files", "Archived files are kept here until you restore them."),
        }
    }
}

fn sort_files(list: &mut [FileItem], sort: &str) {
    match sort {
        "newest" => list.sort_by_key(|f| Reverse(f.created)),
        "oldest" => list.sort_by_key(|f| f.created),
        "size" => list.sort_by_key(|f| Reverse(f.size)),
        "small" => list.sort_by_key(|f| f.size),
        _ => list.sort_by_key(|f| f.name.to_lowercase()),
    }
}

fn sort_folders(list: &mut [Folder], sort: &str) {
    match sort {
        "newest" => list.sort_by_key(|f| Reverse(f.created)),
        "oldest" => list.sort_by_key(|f| f.created),
        _ => list.sort_by_key(|f| f.name.to_lowercase()),
    }
}

fn subtree(folders: &[Folder], root: &str) -> Vec<String> {
    let mut ids = vec![root.to_owned()];
    let mut i = 0;
    while let Some(id) = ids.get(i).cloned() {
        ids.extend(folders.iter().filter(|f| f.parent_id.as_deref() == Some(&id)).map(|f| f.id.clone()));
        i += 1;
    }
    ids
}

fn has_files(ev: &DragEvent) -> bool {
    ev.data_transfer().is_some_and(|dt| dt.types().includes(&JsValue::from_str("Files"), 0))
}

#[derive(Clone)]
enum Dialog {
    NewFolder,
    Rename(String),
    Move(Vec<String>),
    Share(String),
}

#[derive(Clone, Copy)]
struct Drive {
    ui: Ui,
    project: Option<Id>,
    files: LocalResource<Vec<FileItem>>,
    folders: LocalResource<Vec<Folder>>,
    scope: Memo<Scope>,
    go: Callback<String>,
    selected: RwSignal<Vec<String>>,
    details: RwSignal<Option<String>>,
    preview: RwSignal<Option<String>>,
    menu: RwSignal<Option<(String, i32, i32)>>,
    dialog: RwSignal<Option<Dialog>>,
    busy: RwSignal<bool>,
}

impl Drive {
    fn all_folders(self) -> Vec<Folder> {
        self.folders.get().unwrap_or_default()
    }

    fn entry(self, id: &str) -> Option<Entry> {
        let dir = self.all_folders().into_iter().find(|f| f.id == id).map(Entry::Dir);
        dir.or_else(|| self.files.get().unwrap_or_default().into_iter().find(|f| f.id == id).map(Entry::Doc))
    }

    fn entries(self, ids: &[String]) -> Vec<Entry> {
        untrack(|| ids.iter().filter_map(|id| self.entry(id)).collect())
    }

    fn current(self) -> Option<String> {
        match self.scope.get_untracked() {
            Scope::Folder(cur) => cur,
            _ => None,
        }
    }

    fn reload(self) {
        self.files.refetch();
        self.folders.refetch();
    }

    fn open(self, e: &Entry) {
        match e {
            Entry::Dir(f) => self.go.run(f.id.clone()),
            Entry::Doc(f) => self.preview.set(Some(f.id.clone())),
        }
    }

    fn save(self, entries: Vec<Entry>, change: impl Fn(&mut Edit) + 'static) {
        spawn_local(async move {
            for e in entries {
                let mut edit = e.edit();
                change(&mut edit);
                if self.ui.run(api::put::<Value>(&e.api(), &edit)).await.is_none() {
                    break;
                }
            }
            self.reload();
        });
    }

    fn create_folder(self, name: String) {
        let body = Edit { name, parent: self.current(), starred: false };
        spawn_local(async move {
            if self.ui.run(api::post::<Value>("/api/folders", &body)).await.is_some() {
                self.folders.refetch();
            }
        });
    }

    fn archive(self, entries: Vec<Entry>, on: bool) {
        let action = if on { "archive" } else { "unarchive" };
        spawn_local(async move {
            for e in entries.iter().filter(|e| !e.is_dir()) {
                let path = format!("{}/action/{action}", e.api());
                if self.ui.run(api::post::<Value>(&path, &())).await.is_none() {
                    break;
                }
            }
            self.selected.set(Vec::new());
            self.files.refetch();
        });
    }

    fn remove(self, entries: Vec<Entry>) {
        let what = match entries.as_slice() {
            [one] => format!("\"{}\"", one.name()),
            many => format!("{} items", many.len()),
        };
        let note = if entries.iter().any(Entry::is_dir) { " Folders are deleted with everything inside." } else { "" };
        self.ui.confirm_delete(format!("{what} will be deleted permanently.{note}"), move || {
            let entries = entries.clone();
            spawn_local(async move {
                for e in &entries {
                    if self.ui.run(api::del(&e.api())).await.is_none() {
                        break;
                    }
                }
                self.selected.set(Vec::new());
                self.details.set(None);
                self.reload();
            });
        });
    }

    fn upload(self, list: Vec<File>, folder: Option<String>) {
        if list.is_empty() {
            return;
        }
        self.busy.set(true);
        spawn_local(async move {
            if let Some(saved) = self.ui.run(api::upload(list, self.project, folder)).await {
                self.ui.notify(format!("Uploaded {} file(s)", saved.len()));
                self.files.refetch();
            }
            self.busy.set(false);
        });
    }

    fn drop_into(self, target: Option<String>, ev: &DragEvent) {
        ev.prevent_default();
        ev.stop_propagation();
        let Some(dt) = ev.data_transfer() else { return };
        let ids = dt.get_data(DRAG_TYPE).unwrap_or_default();
        if ids.is_empty() {
            self.upload(dt.files().map(|l| api::files_of(&l)).unwrap_or_default(), target);
            return;
        }
        let ids: Vec<String> = ids.split(',').filter(|id| Some(*id) != target.as_deref()).map(str::to_owned).collect();
        self.selected.set(Vec::new());
        self.save(self.entries(&ids), move |e| e.parent = target.clone());
    }

    fn drag_start(self, id: &str, ev: &DragEvent) {
        let sel = self.selected.get_untracked();
        let ids = if sel.iter().any(|s| s == id) { sel.join(",") } else { id.to_owned() };
        if let Some(dt) = ev.data_transfer() {
            let _ = dt.set_data(DRAG_TYPE, &ids);
            dt.set_effect_allowed("move");
        }
    }

    fn toggle(self, id: &str) {
        self.selected.update(|s| match s.iter().position(|x| x == id) {
            Some(i) => {
                s.remove(i);
            }
            None => s.push(id.to_owned()),
        });
    }
}

fn use_drive() -> Drive {
    expect_context()
}

#[component]
pub fn FilesPage() -> impl IntoView {
    view! {
        <div class="flex flex-col gap-6 mx-auto max-w-6xl page-enter">
            <PageHeader title="Files" description="Organize, preview and share your files with folders and links." />
            <FileManager />
        </div>
    }
}

#[component]
pub fn FileManager(#[prop(optional)] project: Option<Id>, #[prop(optional)] archive_only: bool) -> impl IntoView {
    let ui = use_ui();
    let q = RwSignal::new(String::new());
    let (at, set_at) = query_state("in", "");
    let (sort, set_sort) = query_state("sort", "name");
    let (layout, set_layout) = query_state("layout", "list");
    let path = project.map_or("/api/files".to_owned(), |p| format!("/api/files?project={p}"));
    let files = LocalResource::new(move || {
        let path = path.clone();
        async move { ui.run(api::get::<Vec<FileItem>>(&path)).await.unwrap_or_default() }
    });
    let folders = LocalResource::new(move || async move { ui.run(api::get::<Vec<Folder>>("/api/folders")).await.unwrap_or_default() });
    let scope = Memo::new(move |_| if archive_only { Scope::Archived } else { Scope::new(&at.get(), project.is_some()) });
    let d = Drive {
        ui,
        project,
        files,
        folders,
        scope,
        go: Callback::new(move |id: String| {
            q.set(String::new());
            set_at.run(id);
        }),
        selected: RwSignal::new(Vec::new()),
        details: RwSignal::new(None),
        preview: RwSignal::new(None),
        menu: RwSignal::new(None),
        dialog: RwSignal::new(None),
        busy: RwSignal::new(false),
    };
    provide_context(d);
    Effect::new(move |_| {
        scope.track();
        d.selected.set(Vec::new());
    });
    if project.is_none() && !archive_only {
        Effect::new(move |_| {
            let name = trail(&d.all_folders(), d.scope.with(|s| match s { Scope::Folder(cur) => cur.clone(), _ => None }).as_deref()).pop();
            ui.crumb.set(None);
            if let Some(f) = name {
                ui.set_crumb(f.name);
            }
        });
    }
    let searching = Signal::derive(move || !q.get().trim().is_empty());
    let shown_folders = Memo::new(move |_| {
        let mut list: Vec<Folder> =
            d.all_folders().into_iter().filter(|f| has(&f.name, &q.get()) && scope.with(|s| s.holds_folder(f, searching.get()))).collect();
        sort_folders(&mut list, &sort.get());
        list
    });
    let shown_files = Memo::new(move |_| {
        let mut list: Vec<FileItem> = files
            .get()
            .unwrap_or_default()
            .into_iter()
            .filter(|f| has(&f.name, &q.get()) && scope.with(|s| s.holds_file(f, searching.get())))
            .collect();
        let recent = scope.with(|s| *s == Scope::Recent);
        let order = if recent { "newest".to_owned() } else { sort.get() };
        sort_files(&mut list, &order);
        list
    });
    let tab = Signal::derive(move || scope.with(Scope::tab).to_owned());
    let keys = window_event_listener(ev::keydown, move |e| {
        if e.key() != "Escape" || d.preview.get_untracked().is_some() || d.dialog.get_untracked().is_some() {
            return;
        }
        d.menu.set(None);
        d.details.set(None);
        d.selected.set(Vec::new());
    });
    on_cleanup(move || keys.remove());

    let input = NodeRef::<html::Input>::new();
    let on_pick = move |_| {
        let Some(el) = input.get() else { return };
        let picked = el.files().map(|l| api::files_of(&l)).unwrap_or_default();
        el.set_value("");
        d.upload(picked, d.current());
    };
    let over = RwSignal::new(false);
    let can_upload = !archive_only;
    let can_folder = project.is_none() && !archive_only;
    let tabs = if project.is_some() { PROJECT_TABS } else { TABS };
    let grid = Signal::derive(move || layout.get() == "grid");

    view! {
        <div
            class="flex relative flex-col gap-4"
            on:dragenter=move |ev: DragEvent| {
                if can_upload && has_files(&ev) {
                    over.set(true);
                }
            }
        >
            <div class="flex flex-wrap gap-2 justify-between items-center">
                <Show when=move || !archive_only fallback=|| view! { <span /> }>
                    <Segmented options=tabs value=tab on_change=set_at />
                </Show>
                <div class="flex gap-2 items-center">
                    <Show when=move || can_folder>
                        <Button variant=ButtonVariant::Outline on:click=move |_| d.dialog.set(Some(Dialog::NewFolder))>
                            <FolderPlus />
                            <span class="hidden sm:inline">"New folder"</span>
                        </Button>
                    </Show>
                    <Show when=move || can_upload>
                        <Button on:click=move |_| { if let Some(el) = input.get() { el.click(); } } attr:disabled=move || d.busy.get()>
                            <Upload />
                            {move || if d.busy.get() { "Uploading..." } else { "Upload" }}
                        </Button>
                        <input type="file" multiple class="hidden" node_ref=input on:change=on_pick />
                    </Show>
                </div>
            </div>
            <Show when=move || d.selected.with(Vec::is_empty) fallback=|| view! { <BulkBar /> }>
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
                let (dirs, docs) = (shown_folders.get(), shown_files.get());
                if files.get().is_none() || folders.get().is_none() {
                    return view! { <Card class="overflow-hidden gap-0 py-0"><ListSkeleton rows=3 /></Card> }.into_any();
                }
                if dirs.is_empty() && docs.is_empty() {
                    let (title, text) = scope.with(|s| s.empty_text(searching.get()));
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
                if grid.get() {
                    view! { <GridView dirs docs /> }.into_any()
                } else {
                    view! { <ListView dirs docs /> }.into_any()
                }
            }}
            <StorageLine />
            <Show when=move || over.get()>
                <div
                    class="flex absolute inset-0 z-30 flex-col gap-2 justify-center items-center rounded-xl border-2 border-dashed backdrop-blur-sm border-primary bg-background/80 animate-in fade-in-0"
                    on:dragover=move |ev: DragEvent| ev.prevent_default()
                    on:dragleave=move |_| over.set(false)
                    on:drop=move |ev: DragEvent| {
                        over.set(false);
                        d.drop_into(d.current(), &ev);
                    }
                >
                    <Upload class="size-8 pointer-events-none" />
                    <span class="font-medium pointer-events-none">"Drop files to upload them here"</span>
                </div>
            </Show>
            <Preview files=shown_files.into() current=d.preview src=Callback::new(|id: String| raw_url(&id)) />
            <ItemMenu />
            <Details />
            <Dialogs />
        </div>
    }
}

#[component]
fn LayoutButton(value: &'static str, current: Signal<String>, on_change: Callback<String>, title: &'static str, children: Children) -> impl IntoView {
    let class = move || {
        let state = if current.get() == value { "bg-background text-foreground shadow-sm" } else { "text-muted-foreground hover:text-foreground" };
        format!("flex justify-center items-center rounded-md transition-all size-7 [&_svg]:size-4 {state}")
    };
    view! { <button type="button" class=class title=title on:click=move |_| on_change.run(value.to_owned())>{children()}</button> }
}

#[component]
fn Crumbs(searching: Signal<bool>) -> impl IntoView {
    let d = use_drive();
    let label = move || match d.scope.get() {
        _ if searching.get() => "Search results",
        Scope::Recent => "Recent",
        Scope::Starred => "Starred",
        Scope::Shared => "Shared with a link",
        Scope::Archived => "Archived",
        Scope::Project => "Project files",
        Scope::Folder(_) => "",
    };
    let path = move || match d.scope.get() {
        Scope::Folder(cur) => trail(&d.all_folders(), cur.as_deref()),
        _ => Vec::new(),
    };
    view! {
        <nav class="flex flex-wrap gap-1 items-center min-w-0 text-lg font-medium">
            {move || match label() {
                "" => view! {
                    <Crumb target=None name="My Drive".to_owned() />
                    {path().into_iter().map(|f| view! {
                        <ChevronRight class="size-4 text-muted-foreground shrink-0" />
                        <Crumb target=Some(f.id) name=f.name />
                    }).collect_view()}
                }
                .into_any(),
                text => view! { <span class="px-2">{text}</span> }.into_any(),
            }}
        </nav>
    }
}

#[component]
fn Crumb(target: Option<String>, name: String) -> impl IntoView {
    let d = use_drive();
    let hover = RwSignal::new(false);
    let id = StoredValue::new(target);
    view! {
        <button
            type="button"
            class="py-1 px-2 rounded-md transition-colors hover:bg-muted max-w-56 truncate"
            class=("bg-accent", move || hover.get())
            on:click=move |_| d.go.run(id.get_value().unwrap_or_default())
            on:dragover=move |ev: DragEvent| {
                ev.prevent_default();
                hover.set(true);
            }
            on:dragleave=move |_| hover.set(false)
            on:drop=move |ev: DragEvent| {
                hover.set(false);
                d.drop_into(id.get_value(), &ev);
            }
        >
            {name}
        </button>
    }
}

#[component]
fn BulkBar() -> impl IntoView {
    let d = use_drive();
    let picked = move || d.entries(&d.selected.get());
    let archived = move || d.scope.with(|s| *s == Scope::Archived);
    let any_file = move || picked().iter().any(|e| !e.is_dir());
    let all_starred = move || picked().iter().all(Entry::starred);
    view! {
        <div class="flex flex-wrap gap-1 items-center px-2 rounded-xl border min-h-[2.75rem] bg-muted/60 animate-in fade-in-0">
            <Button variant=ButtonVariant::Ghost size=ButtonSize::IconSm attr:title="Clear selection" on:click=move |_| d.selected.set(Vec::new())>
                <X />
            </Button>
            <span class="mr-2 text-sm font-medium">{move || format!("{} selected", d.selected.with(Vec::len))}</span>
            <Button variant=ButtonVariant::Ghost size=ButtonSize::Sm on:click=move |_| d.dialog.set(Some(Dialog::Move(d.selected.get_untracked())))>
                <FolderInput />
                "Move"
            </Button>
            <Button
                variant=ButtonVariant::Ghost
                size=ButtonSize::Sm
                on:click=move |_| {
                    let on = !untrack(all_starred);
                    d.save(untrack(picked), move |e| e.starred = on);
                }
            >
                <Star />
                {move || if all_starred() { "Unstar" } else { "Star" }}
            </Button>
            <Show when=any_file>
                <Button variant=ButtonVariant::Ghost size=ButtonSize::Sm on:click=move |_| d.archive(untrack(picked), !untrack(archived))>
                    {move || if archived() { view! { <ArchiveRestore /> "Restore" }.into_any() } else { view! { <Archive /> "Archive" }.into_any() }}
                </Button>
            </Show>
            <Button variant=ButtonVariant::Ghost size=ButtonSize::Sm class="text-destructive" on:click=move |_| d.remove(untrack(picked))>
                <Trash2 />
                "Delete"
            </Button>
        </div>
    }
}

#[component]
fn ListView(dirs: Vec<Folder>, docs: Vec<FileItem>) -> impl IntoView {
    let d = use_drive();
    let ids: Vec<String> = dirs.iter().map(|f| f.id.clone()).chain(docs.iter().map(|f| f.id.clone())).collect();
    let ids = StoredValue::new(ids);
    let all = move || ids.with_value(|ids| !ids.is_empty() && d.selected.with(|s| ids.iter().all(|id| s.contains(id))));
    let toggle_all = move |_| {
        let next = if untrack(all) { Vec::new() } else { ids.get_value() };
        d.selected.set(next);
    };
    view! {
        <Card class="overflow-hidden gap-0 py-0">
            <div class="grid items-center gap-3 px-3 h-10 text-xs font-medium border-b grid-cols-[1.25rem_minmax(0,1fr)_2rem] sm:grid-cols-[1.25rem_minmax(0,1fr)_9rem_6rem_2rem] bg-muted/50 text-muted-foreground">
                <Checkbox aria_label="Select all" checked=Signal::derive(all) on_checked_change=Callback::new(toggle_all) />
                <span>"Name"</span>
                <span class="hidden sm:block">"Created"</span>
                <span class="hidden text-right sm:block">"Size"</span>
                <span />
            </div>
            {dirs.into_iter().map(|f| view! { <Item entry=Entry::Dir(f) grid=false /> }).collect_view()}
            {docs.into_iter().map(|f| view! { <Item entry=Entry::Doc(f) grid=false /> }).collect_view()}
        </Card>
    }
}

#[component]
fn GridView(dirs: Vec<Folder>, docs: Vec<FileItem>) -> impl IntoView {
    let section = "text-sm font-medium text-muted-foreground";
    view! {
        <Show when={
            let any = !dirs.is_empty();
            move || any
        }>
            <h3 class=section>"Folders"</h3>
        </Show>
        <div class="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-4">
            {dirs.into_iter().map(|f| view! { <Item entry=Entry::Dir(f) grid=true /> }).collect_view()}
        </div>
        <Show when={
            let any = !docs.is_empty();
            move || any
        }>
            <h3 class=section>"Files"</h3>
        </Show>
        <div class="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-4">
            {docs.into_iter().map(|f| view! { <Item entry=Entry::Doc(f) grid=true /> }).collect_view()}
        </div>
    }
}

fn icon(e: &Entry, class: &'static str) -> AnyView {
    match e {
        Entry::Dir(_) => view! { <FolderIcon class=format!("{class} fill-current text-amber-500") /> }.into_any(),
        Entry::Doc(f) => view! { <KindIcon kind=Kind::of(f) class=format!("{class} text-muted-foreground") /> }.into_any(),
    }
}

fn thumb(e: &Entry) -> AnyView {
    let Entry::Doc(f) = e else { return icon(e, "size-12") };
    match Kind::of(f) {
        Kind::Image => view! { <img src=raw_url(&f.id) loading="lazy" alt="" class="object-cover size-full" /> }.into_any(),
        Kind::Video => view! { <video src=format!("{}#t=0.5", raw_url(&f.id)) preload="metadata" muted class="object-cover pointer-events-none size-full" /> }.into_any(),
        _ => icon(e, "size-12"),
    }
}

fn badges(e: &Entry) -> impl IntoView + use<> {
    let archived = matches!(e, Entry::Doc(f) if f.archived == 1);
    view! {
        <Show when={
            let s = e.starred();
            move || s
        }>
            <Star class="text-amber-500 fill-current size-3.5 shrink-0" />
        </Show>
        <Show when={
            let s = e.token().is_some();
            move || s
        }>
            <span title="Shared with a link"><Link class="size-3.5 text-muted-foreground shrink-0" /></span>
        </Show>
        <Show when=move || archived>
            <span title="Archived"><Archive class="size-3.5 text-muted-foreground shrink-0" /></span>
        </Show>
    }
}

#[component]
fn Item(entry: Entry, grid: bool) -> impl IntoView {
    let d = use_drive();
    let e = StoredValue::new(entry.clone());
    let id = StoredValue::new(entry.id().to_owned());
    let hover = RwSignal::new(false);
    let picked = move || d.selected.with(|s| s.contains(&id.get_value()));
    let open = move |ev: MouseEvent| {
        if ev.ctrl_key() || ev.meta_key() || !d.selected.with_untracked(Vec::is_empty) {
            d.toggle(&id.get_value());
            return;
        }
        d.open(&e.get_value());
    };
    let context = move |ev: MouseEvent| {
        ev.prevent_default();
        d.menu.set(Some((id.get_value(), ev.client_x(), ev.client_y())));
    };
    let menu_btn = move |ev: MouseEvent| {
        ev.stop_propagation();
        d.menu.set(Some((id.get_value(), ev.client_x(), ev.client_y())));
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
            d.drop_into(Some(id.get_value()), &ev);
        }
    };
    let check = view! {
        <Checkbox
            aria_label="Select"
            checked=Signal::derive(picked)
            on_checked_change=Callback::new(move |_| d.toggle(&id.get_value()))
            on:click=|ev: MouseEvent| ev.stop_propagation()
        />
    };
    let more = view! {
        <button type="button" class="flex justify-center items-center rounded-md size-8 hover:bg-accent shrink-0" title="More actions" on:click=menu_btn>
            <EllipsisVertical class="size-4" />
        </button>
    };
    let (size, when) = match &entry {
        Entry::Doc(f) => (fmt_size(f.size), fmt_date(f.created)),
        Entry::Dir(f) => ("-".to_owned(), fmt_date(f.created)),
    };
    let base = if grid {
        "flex overflow-hidden relative flex-col rounded-xl border transition-all cursor-pointer select-none group bg-card hover:shadow-md"
    } else {
        "grid items-center gap-3 px-3 py-1.5 border-b last:border-b-0 transition-colors cursor-pointer select-none group grid-cols-[1.25rem_minmax(0,1fr)_2rem] sm:grid-cols-[1.25rem_minmax(0,1fr)_9rem_6rem_2rem] hover:bg-muted/50"
    };
    let body = if grid {
        view! {
            <Show when=move || !is_dir>
                <div class="flex overflow-hidden justify-center items-center border-b aspect-video bg-muted/60">{thumb(&e.get_value())}</div>
            </Show>
            <div class="flex gap-2 items-center py-2 pr-1 pl-3 min-w-0">
                {icon(&entry, "size-4 shrink-0")}
                <span class="flex-1 text-sm font-medium truncate" title=entry.name().to_owned()>{entry.name().to_owned()}</span>
                {badges(&entry)}
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
                <span class="text-sm font-medium truncate" title=entry.name().to_owned()>{entry.name().to_owned()}</span>
                {badges(&entry)}
            </div>
            <span class="hidden text-sm sm:block text-muted-foreground">{when}</span>
            <span class="hidden text-sm tabular-nums text-right sm:block text-muted-foreground">{size}</span>
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
            on:contextmenu=context
            on:dragstart=move |ev: DragEvent| d.drag_start(&id.get_value(), &ev)
            on:dragover=drop_target
            on:dragleave=move |_| hover.set(false)
            on:drop=on_drop
        >
            {body}
        </div>
    }
}

#[component]
fn MenuItem(#[prop(into)] label: String, action: Callback<()>, #[prop(optional)] danger: bool, children: Children) -> impl IntoView {
    let d = use_drive();
    let class = if danger { format!("{MENU_BTN} text-destructive") } else { MENU_BTN.to_owned() };
    view! {
        <button
            type="button"
            class=class
            on:click=move |_| {
                d.menu.set(None);
                action.run(());
            }
        >
            {children()}
            {label}
        </button>
    }
}

#[component]
fn ItemMenu() -> impl IntoView {
    let d = use_drive();
    let target = move || d.menu.get().and_then(|(id, x, y)| untrack(|| d.entry(&id)).map(|e| (e, x, y)));
    view! {
        {move || target().map(|(e, x, y)| {
            let w = window().inner_width().ok().and_then(|v| v.as_f64()).unwrap_or(1024.0) as i32;
            let h = window().inner_height().ok().and_then(|v| v.as_f64()).unwrap_or(768.0) as i32;
            let style = format!("left: {}px; top: {}px", x.min(w - 232).max(8), y.min(h - 420).max(8));
            let e = StoredValue::new(e);
            let act = move |f: fn(Drive, Entry)| Callback::new(move |_| f(d, e.get_value()));
            let is_dir = e.with_value(Entry::is_dir);
            let archived = e.with_value(|e| matches!(e, Entry::Doc(f) if f.archived == 1));
            let star_label = if e.with_value(Entry::starred) { "Remove from Starred" } else { "Add to Starred" };
            view! {
                <div class="fixed inset-0 z-40" on:click=move |_| d.menu.set(None) on:contextmenu=move |ev: MouseEvent| { ev.prevent_default(); d.menu.set(None); } />
                <div class="overflow-y-auto fixed z-50 p-1 w-56 rounded-lg border shadow-lg max-h-[calc(100vh-2rem)] bg-popover text-popover-foreground pop-in [&_svg]:size-4 [&_svg]:text-muted-foreground" style=style>
                    <MenuItem label=if is_dir { "Open" } else { "Preview" } action=act(|d, e| d.open(&e))>{if is_dir { view! { <FolderOpen /> }.into_any() } else { view! { <Eye /> }.into_any() }}</MenuItem>
                    <Show when=move || !is_dir>
                        <a class=MENU_BTN href=raw_url(e.with_value(|e| e.id().to_owned()).as_str()) download=e.with_value(|e| e.name().to_owned()) on:click=move |_| d.menu.set(None)>
                            <Download />
                            "Download"
                        </a>
                        <MenuItem label="Copy private link" action=act(|d, e| copy(d.ui, &api::absolute(&format!("/api/files/{}/raw", e.id()))))><Copy /></MenuItem>
                    </Show>
                    <div class="my-1 h-px bg-border" />
                    <MenuItem label="Share" action=act(|d, e| d.dialog.set(Some(Dialog::Share(e.id().to_owned())))) ><Share2 /></MenuItem>
                    <MenuItem label="Rename" action=act(|d, e| d.dialog.set(Some(Dialog::Rename(e.id().to_owned())))) ><Pencil /></MenuItem>
                    <MenuItem label="Move to" action=act(|d, e| d.dialog.set(Some(Dialog::Move(vec![e.id().to_owned()])))) ><FolderInput /></MenuItem>
                    <MenuItem label=star_label action=act(|d, e| { let on = !e.starred(); d.save(vec![e], move |x| x.starred = on); })>
                        {if star_label.starts_with("Add") { view! { <Star /> }.into_any() } else { view! { <StarOff /> }.into_any() }}
                    </MenuItem>
                    <MenuItem label="Details" action=act(|d, e| d.details.set(Some(e.id().to_owned())))><Info /></MenuItem>
                    <div class="my-1 h-px bg-border" />
                    <Show when=move || !is_dir>
                        <MenuItem label=if archived { "Restore" } else { "Archive" } action=act(|d, e| { let on = !matches!(&e, Entry::Doc(f) if f.archived == 1); d.archive(vec![e], on); })>
                            {if archived { view! { <ArchiveRestore /> }.into_any() } else { view! { <Archive /> }.into_any() }}
                        </MenuItem>
                    </Show>
                    <MenuItem label="Delete" danger=true action=act(|d, e| d.remove(vec![e]))><Trash2 /></MenuItem>
                </div>
            }
        })}
    }
}

#[component]
fn Row(label: &'static str, children: Children) -> impl IntoView {
    view! {
        <div class="flex flex-col gap-0.5">
            <span class="text-xs text-muted-foreground">{label}</span>
            <span class="text-sm break-words">{children()}</span>
        </div>
    }
}

fn location(d: Drive, parent: Option<&str>) -> String {
    let names: Vec<String> = trail(&d.all_folders(), parent).into_iter().map(|f| f.name).collect();
    if names.is_empty() { "My Drive".into() } else { format!("My Drive / {}", names.join(" / ")) }
}

fn sharing(token: &Option<String>, expires: Option<i64>) -> String {
    match (token, expires) {
        (None, _) => "Private, only you can access it".into(),
        (Some(_), None) => "Anyone with the link".into(),
        (Some(_), Some(t)) => format!("Anyone with the link until {}", fmt_time(t)),
    }
}

fn folder_facts(d: Drive, f: &Folder) -> AnyView {
    let folders = d.all_folders();
    let ids = subtree(&folders, &f.id);
    let files = d.files.get().unwrap_or_default();
    let inside: Vec<&FileItem> = files.iter().filter(|x| x.folder_id.as_ref().is_some_and(|p| ids.contains(p))).collect();
    let total: i64 = inside.iter().map(|x| x.size).sum();
    let contents = format!("{} folder(s), {} file(s)", ids.len() - 1, inside.len());
    let (place, created, shared) = (location(d, f.parent_id.as_deref()), fmt_time(f.created), sharing(&f.share_token, f.share_expires));
    view! {
        <Row label="Type">"Folder"</Row>
        <Row label="Contents">{contents}</Row>
        <Row label="Total size">{fmt_size(total)}</Row>
        <Row label="Location">{place}</Row>
        <Row label="Created">{created}</Row>
        <Row label="Sharing">{shared}</Row>
    }
    .into_any()
}

fn file_facts(d: Drive, f: &FileItem) -> AnyView {
    let kind = Kind::of(f);
    let status = if f.archived == 1 { "Archived" } else { "Active" };
    let mime = if f.mime.is_empty() { "unknown" } else { &f.mime };
    let kind = format!("{} ({mime})", kind.label());
    let size = format!("{} ({} bytes)", fmt_size(f.size), f.size);
    let (place, created, shared) = (location(d, f.folder_id.as_deref()), fmt_time(f.created), sharing(&f.share_token, f.share_expires));
    view! {
        <Row label="Type">{kind}</Row>
        <Row label="Size">{size}</Row>
        <Row label="Location">{place}</Row>
        <Row label="Uploaded">{created}</Row>
        <Row label="Status">{status}</Row>
        <Row label="Sharing">{shared}</Row>
    }
    .into_any()
}

#[component]
fn Details() -> impl IntoView {
    let d = use_drive();
    let entry = move || d.details.get().and_then(|id| d.entry(&id));
    view! {
        {move || entry().map(|e| {
            let id = e.id().to_owned();
            let facts = match &e {
                Entry::Dir(f) => folder_facts(d, f),
                Entry::Doc(f) => file_facts(d, f),
            };
            let opener = e.clone();
            let ids = StoredValue::new(id.clone());
            view! {
                <div class="fixed inset-0 z-40 bg-black/30 animate-in fade-in-0" on:click=move |_| d.details.set(None) />
                <aside class="flex overflow-y-auto fixed inset-y-0 right-0 z-50 flex-col gap-5 p-5 w-full border-l shadow-xl sm:w-96 bg-background animate-in slide-in-from-right duration-300">
                    <div class="flex gap-3 items-center">
                        {icon(&e, "size-5 shrink-0")}
                        <h3 class="flex-1 font-semibold break-all">{e.name().to_owned()}</h3>
                        <Button variant=ButtonVariant::Ghost size=ButtonSize::IconSm attr:title="Close" on:click=move |_| d.details.set(None)><X /></Button>
                    </div>
                    <div class="flex overflow-hidden justify-center items-center rounded-xl border aspect-video bg-muted/60">{thumb(&e)}</div>
                    <div class="flex flex-wrap gap-2">
                        <Button size=ButtonSize::Sm on:click=move |_| { d.details.set(None); d.open(&opener); }>
                            <Eye />
                            "Open"
                        </Button>
                        <Button variant=ButtonVariant::Outline size=ButtonSize::Sm on:click=move |_| d.dialog.set(Some(Dialog::Share(ids.get_value())))>
                            <Share2 />
                            "Share"
                        </Button>
                        <Button variant=ButtonVariant::Outline size=ButtonSize::Sm on:click=move |_| d.dialog.set(Some(Dialog::Rename(ids.get_value())))>
                            <Pencil />
                            "Rename"
                        </Button>
                        <Button variant=ButtonVariant::Outline size=ButtonSize::Sm on:click=move |_| d.dialog.set(Some(Dialog::Move(vec![ids.get_value()])))>
                            <FolderInput />
                            "Move"
                        </Button>
                    </div>
                    <div class="flex flex-col gap-4">{facts}</div>
                </aside>
            }
        })}
    }
}

#[component]
fn Dialogs() -> impl IntoView {
    let d = use_drive();
    view! {
        {move || d.dialog.get().map(|dialog| {
            let open = RwSignal::new(true);
            Effect::new(move |_| {
                if !open.get() {
                    d.dialog.set(None);
                }
            });
            match dialog {
                Dialog::NewFolder => view! {
                    <NameDialog open title="New folder" initial=String::new() on_submit=Callback::new(move |name| d.create_folder(name)) />
                }
                .into_any(),
                Dialog::Rename(id) => {
                    let e = d.entries(std::slice::from_ref(&id));
                    let name = e.first().map(|e| e.name().to_owned()).unwrap_or_default();
                    view! { <NameDialog open title="Rename" initial=name on_submit=Callback::new(move |name: String| d.save(e.clone(), move |x| x.name = name.clone())) /> }.into_any()
                }
                Dialog::Move(ids) => view! { <MoveDialog open ids /> }.into_any(),
                Dialog::Share(id) => share_dialog(d, open, &id),
            }
        })}
    }
}

fn share_dialog(d: Drive, open: RwSignal<bool>, id: &str) -> AnyView {
    let Some(e) = untrack(|| d.entry(id)) else { return ().into_any() };
    let token = RwSignal::new(e.token());
    Effect::new(move |prev: Option<Option<String>>| {
        let now = token.get();
        if prev.is_some_and(|p| p != now) {
            d.reload();
        }
        now
    });
    let prefix = if e.is_dir() { "/d/" } else { "/api/public/files/" };
    view! { <ShareDialog open api_path=format!("{}/share", e.api()) link_prefix=prefix token /> }.into_any()
}

#[component]
fn NameDialog(open: RwSignal<bool>, title: &'static str, initial: String, on_submit: Callback<String>) -> impl IntoView {
    let name = RwSignal::new(initial);
    let field = NodeRef::<html::Input>::new();
    Effect::new(move |_| {
        if let Some(el) = field.get() {
            let _ = el.focus();
            el.select();
        }
    });
    let submit = move |ev: SubmitEvent| {
        ev.prevent_default();
        let value = name.get_untracked().trim().to_owned();
        if value.is_empty() {
            return;
        }
        on_submit.run(value);
        open.set(false);
    };
    view! {
        <Modal open title=title>
            <form class="flex flex-col gap-4" on:submit=submit>
                <Input bind_value=name node_ref=field required=true placeholder="Name" />
                <div class="flex gap-2 justify-end">
                    <button type="button" class="px-4 h-9 text-sm font-medium rounded-md hover:bg-accent" on:click=move |_| open.set(false)>"Cancel"</button>
                    <Button>"Save"</Button>
                </div>
            </form>
        </Modal>
    }
}

#[component]
fn MoveDialog(open: RwSignal<bool>, ids: Vec<String>) -> impl IntoView {
    let d = use_drive();
    let at = RwSignal::new(None::<String>);
    let ids = StoredValue::new(ids);
    let children = move || {
        let here = at.get();
        let mut list: Vec<Folder> =
            d.all_folders().into_iter().filter(|f| f.parent_id == here && !ids.with_value(|i| i.contains(&f.id))).collect();
        sort_folders(&mut list, "name");
        list
    };
    let confirm = move |_| {
        let target = at.get_untracked();
        d.selected.set(Vec::new());
        d.save(d.entries(&ids.get_value()), move |e| e.parent = target.clone());
        open.set(false);
    };
    let crumb = "py-1 px-2 rounded-md hover:bg-muted truncate max-w-40";
    view! {
        <Modal open title="Move to">
            <div class="flex flex-wrap gap-0.5 items-center text-sm">
                <button type="button" class=crumb on:click=move |_| at.set(None)>"My Drive"</button>
                {move || trail(&d.all_folders(), at.get().as_deref()).into_iter().map(|f| view! {
                    <ChevronRight class="size-3.5 text-muted-foreground" />
                    <button type="button" class=crumb on:click=move |_| at.set(Some(f.id.clone()))>{f.name.clone()}</button>
                }).collect_view()}
            </div>
            <div class="overflow-y-auto max-h-72 rounded-lg border divide-y min-h-32">
                {move || {
                    let list = children();
                    if list.is_empty() {
                        return view! { <p class="p-4 text-sm text-center text-muted-foreground">"No folders here"</p> }.into_any();
                    }
                    list.into_iter().map(|f| view! {
                        <button type="button" class="flex gap-3 items-center px-3 w-full h-10 text-sm text-left hover:bg-muted" on:click=move |_| at.set(Some(f.id.clone()))>
                            <FolderIcon class="text-amber-500 fill-current size-4 shrink-0" />
                            <span class="flex-1 truncate">{f.name.clone()}</span>
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
    let d = use_drive();
    let text = move || {
        let files = d.files.get().unwrap_or_default();
        let total: i64 = files.iter().map(|f| f.size).sum();
        format!("{} file(s), {} used", files.len(), fmt_size(total))
    };
    view! { <p class="text-xs text-muted-foreground">{text}</p> }
}
