use icons::{ArrowLeft, Copy, ExternalLink, FileArchive, Files, FolderUp, Globe, Lock, Plus, QrCode, Settings, Trash2, Upload, X};
use leptos::ev::SubmitEvent;
use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::A;
use leptos_router::hooks::{use_navigate, use_params_map};
use serde::Deserialize;
use serde_json::{Value, json};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{DataTransfer, DragEvent, File, FileSystemDirectoryEntry, FileSystemEntry, FileSystemFileEntry, FormData};

use crate::api;
use crate::components::ui::badge::{Badge, BadgeVariant};
use crate::components::ui::button::{Button, ButtonLink, ButtonSize, ButtonVariant};
use crate::components::ui::card::Card;
use crate::components::ui::checkbox::Checkbox;
use crate::components::ui::date_picker::DateTimePicker;
use crate::components::ui::empty::{Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant, EmptyTitle};
use crate::components::ui::input::{Input, InputType};
use crate::components::ui::input_group::{InputGroup, InputGroupAddon};
use crate::components::ui::label::Label;
use crate::components::ui::progress::Progress;
use crate::components::ui::spinner::Spinner;
use crate::components::ui::switch::Switch;
use crate::id::Id;
use crate::pages::drive::is_markdown;
use crate::pages::editor::FileEditor;
use crate::pages::requests::local_input;
use crate::pages::site_files::{SiteFile, SiteFiles};
use crate::widgets::{ListSkeleton, Modal, PageHeader, copy, fmt_size, fmt_time, parse_local, qr_svg, use_ui};

const NAME_RULE: &str = "1 to 63 lowercase letters, digits and hyphens";

#[derive(Clone, Deserialize, PartialEq)]
struct Site {
    id: Id,
    name: String,
    enabled: i64,
    locked: i64,
    expires: Option<i64>,
    views: i64,
    files: i64,
    bytes: i64,
    updated: i64,
}

impl Site {
    fn status(&self) -> (&'static str, BadgeVariant) {
        let now = (js_sys::Date::now() / 1000.0) as i64;
        match self.expires {
            _ if self.enabled == 0 => ("Disabled", BadgeVariant::Muted),
            Some(t) if t <= now => ("Expired", BadgeVariant::Warning),
            _ => ("Live", BadgeVariant::Success),
        }
    }

    fn meta(&self) -> String {
        let expiry = self.expires.map(|t| format!(" · expires {}", fmt_time(t))).unwrap_or_default();
        format!("{} file(s), {} · {} view(s) · updated {}{expiry}", self.files, fmt_size(self.bytes), self.views, fmt_time(self.updated))
    }
}

#[derive(Clone, Deserialize)]
struct NewSite {
    id: Id,
    name: String,
}

fn site_link(name: &str) -> String {
    api::absolute(&format!("/site/{name}/"))
}

pub(crate) fn file_url(id: Id, path: &str) -> String {
    format!("/api/sites/{id}/file?path={}", js_sys::encode_uri_component(path))
}

pub(crate) fn public_path(name: &str, path: &str) -> String {
    let encoded: Vec<String> = path.split('/').map(|part| js_sys::encode_uri_component(part).into()).collect();
    format!("/site/{name}/{}", encoded.join("/"))
}

fn slug(raw: &str) -> String {
    raw.to_lowercase()
        .chars()
        .map(|c| if c.is_whitespace() || c == '_' || c == '.' { '-' } else { c })
        .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-')
        .take(63)
        .collect()
}

fn is_zip(file: &File) -> bool {
    file.name().to_lowercase().ends_with(".zip")
}

#[derive(Clone)]
pub(crate) struct Picked {
    path: String,
    file: File,
}

impl Picked {
    fn relative(file: &File) -> Self {
        let path = js_sys::Reflect::get(file, &JsValue::from_str("webkitRelativePath"))
            .ok()
            .and_then(|v| v.as_string())
            .filter(|p| !p.is_empty())
            .unwrap_or_else(|| file.name());
        Self { path, file: file.clone() }
    }
}

#[derive(Clone)]
pub(crate) enum Upload {
    Files(Vec<Picked>),
    Zip(File),
}

impl Upload {
    fn files(list: Vec<Picked>) -> Option<Self> {
        (!list.is_empty()).then_some(Self::Files(list))
    }

    fn size(&self) -> f64 {
        match self {
            Self::Zip(file) => file.size(),
            Self::Files(list) => list.iter().map(|p| p.file.size()).sum(),
        }
    }

    pub(crate) fn label(&self) -> String {
        match self {
            Self::Zip(file) => format!("ZIP archive {}", file.name()),
            Self::Files(list) => format!("{} file(s)", list.len()),
        }
    }

    fn form(&self) -> FormData {
        let form = FormData::new().expect("FormData");
        match self {
            Self::Zip(file) => {
                let _ = form.append_with_blob_and_filename("zip", file, &file.name());
            }
            Self::Files(list) => {
                for p in list {
                    let _ = form.append_with_str("path", &p.path);
                    let _ = form.append_with_blob_and_filename("file", &p.file, &p.file.name());
                }
            }
        }
        form
    }
}

async fn settle(start: impl FnOnce(&js_sys::Function, &js_sys::Function) -> Result<(), JsValue>) -> Option<JsValue> {
    let mut start = Some(start);
    let promise = js_sys::Promise::new(&mut |resolve, reject| {
        if let Some(Err(e)) = start.take().map(|s| s(&resolve, &reject)) {
            let _ = reject.call1(&JsValue::NULL, &e);
        }
    });
    JsFuture::from(promise).await.ok()
}

async fn entry_file(entry: &FileSystemFileEntry) -> Option<File> {
    let value = settle(|ok, err| {
        entry.file_with_callback_and_callback(ok, err);
        Ok(())
    })
    .await;
    value.map(JsCast::unchecked_into)
}

async fn read_dir(dir: &FileSystemDirectoryEntry) -> Vec<FileSystemEntry> {
    let reader = dir.create_reader();
    let mut entries = Vec::new();
    loop {
        let batch = settle(|ok, err| reader.read_entries_with_callback_and_callback(ok, err)).await;
        let batch: Vec<FileSystemEntry> = batch.map(|v| js_sys::Array::from(&v).iter().map(JsCast::unchecked_into).collect()).unwrap_or_default();
        if batch.is_empty() {
            return entries;
        }
        entries.extend(batch);
    }
}

async fn walk(mut stack: Vec<FileSystemEntry>) -> Vec<Picked> {
    let mut found = Vec::new();
    while let Some(entry) = stack.pop() {
        if entry.is_directory() {
            stack.extend(read_dir(entry.unchecked_ref()).await);
            continue;
        }
        if let Some(file) = entry_file(entry.unchecked_ref()).await {
            found.push(Picked { path: entry.full_path().trim_start_matches('/').to_owned(), file });
        }
    }
    found
}

pub(crate) fn drop_entries(dt: &DataTransfer) -> Vec<FileSystemEntry> {
    let items = dt.items();
    (0..items.length()).filter_map(|i| items.get(i)?.webkit_get_as_entry().ok().flatten()).collect()
}

pub(crate) async fn dropped(entries: Vec<FileSystemEntry>, files: Vec<File>) -> Option<Upload> {
    if let [file] = files.as_slice()
        && is_zip(file)
    {
        return Some(Upload::Zip(file.clone()));
    }
    if entries.is_empty() {
        return Upload::files(files.iter().map(Picked::relative).collect());
    }
    Upload::files(walk(entries).await)
}

pub(crate) fn deploy(id: Id, upload: &Upload, replace: bool, dir: &str, loaded: RwSignal<Option<f64>>, done: impl FnOnce(api::ApiResult<()>) + 'static) {
    loaded.set(Some(0.0));
    let path = format!("/api/sites/{id}/files?replace={replace}&dir={}", js_sys::encode_uri_component(dir));
    let finish = move |res| {
        loaded.set(None);
        done(res);
    };
    api::post_with_progress(&path, &upload.form(), move |bytes| loaded.set(Some(bytes)), finish);
}

#[derive(Clone, Copy)]
struct SiteForm {
    name: RwSignal<String>,
    enabled: RwSignal<bool>,
    locked: RwSignal<bool>,
    password: RwSignal<String>,
    expires: RwSignal<String>,
    was_locked: bool,
}

impl SiteForm {
    fn new(site: Option<&Site>) -> Self {
        let was_locked = site.is_some_and(|s| s.locked == 1);
        Self {
            name: RwSignal::new(site.map(|s| s.name.clone()).unwrap_or_default()),
            enabled: RwSignal::new(site.is_none_or(|s| s.enabled == 1)),
            locked: RwSignal::new(was_locked),
            password: RwSignal::new(String::new()),
            expires: RwSignal::new(site.and_then(|s| s.expires).map(local_input).unwrap_or_default()),
            was_locked,
        }
    }

    fn body(self) -> Result<Value, &'static str> {
        let pw = self.password.get_untracked();
        let password = match (self.locked.get_untracked(), pw.is_empty()) {
            (false, _) if self.was_locked => Some(String::new()),
            (false, _) => None,
            (true, true) if self.was_locked => None,
            (true, true) => return Err("Enter a password or turn off password protection"),
            (true, false) => Some(pw),
        };
        Ok(json!({
            "name": self.name.get_untracked(), "enabled": self.enabled.get_untracked(),
            "password": password, "expires": parse_local(&self.expires.get_untracked()),
        }))
    }
}

#[component]
pub fn Sites() -> impl IntoView {
    let ui = use_ui();
    let sites = LocalResource::new(move || async move { ui.run(api::get::<Vec<Site>>("/api/sites")).await.unwrap_or_default() });
    let creating = RwSignal::new(false);
    view! {
        <div class="flex flex-col gap-6 mx-auto max-w-6xl page-enter">
            <PageHeader title="Sites" description="Publish static websites made of HTML, CSS and JavaScript files.">
                <Button on:click=move |_| creating.set(true)>
                    <Plus />
                    "New site"
                </Button>
            </PageHeader>
            {move || match sites.get() {
                None => view! { <Card class="overflow-hidden gap-0 py-0"><ListSkeleton rows=3 /></Card> }.into_any(),
                Some(list) if list.is_empty() => view! {
                    <Empty class="py-16">
                        <EmptyHeader>
                            <EmptyMedia variant=EmptyMediaVariant::Icon><Globe /></EmptyMedia>
                            <EmptyTitle>"No sites"</EmptyTitle>
                            <EmptyDescription>"Upload a folder, files or a ZIP with an index.html to get a shareable link."</EmptyDescription>
                        </EmptyHeader>
                    </Empty>
                }
                .into_any(),
                Some(list) => view! {
                    <Card class="overflow-hidden gap-0 py-0">
                        <For each=move || list.clone() key=|s| s.id let(site)>
                            <SiteRow site />
                        </For>
                    </Card>
                }
                .into_any(),
            }}
            <Show when=move || creating.get()>
                <NewSiteDialog open=creating on_created=Callback::new(move |()| sites.refetch()) />
            </Show>
        </div>
    }
}

#[component]
fn SiteRow(site: Site) -> impl IntoView {
    let ui = use_ui();
    let (label, variant) = site.status();
    let link = site_link(&site.name);
    let copy_link = link.clone();
    let manage = api::url(&format!("/sites/{}", site.id));
    let locked = site.locked == 1;
    let meta = site.meta();
    view! {
        <div class="flex flex-wrap gap-3 items-center py-3 px-4 border-b last:border-b-0">
            <div class="flex flex-col flex-1 gap-1 min-w-56">
                <div class="flex flex-wrap gap-2 items-center">
                    <A href=manage.clone() attr:class="font-medium truncate hover:underline underline-offset-4">{site.name}</A>
                    <Badge variant>{label}</Badge>
                    <Show when=move || locked>
                        <Badge variant=BadgeVariant::Outline><Lock class="mr-1 size-3" />"Password"</Badge>
                    </Show>
                </div>
                <a href=link.clone() target="_blank" rel="noopener" class="flex gap-1.5 items-center text-sm text-muted-foreground w-fit hover:text-foreground">
                    <Globe class="size-3.5 shrink-0" />
                    <span class="truncate">{link.clone()}</span>
                </a>
                <span class="text-xs text-muted-foreground">{meta}</span>
            </div>
            <div class="flex gap-1 items-center">
                <Button variant=ButtonVariant::Ghost size=ButtonSize::IconSm attr:title="Copy link" on:click=move |_| copy(ui, &copy_link)>
                    <Copy />
                </Button>
                <ButtonLink variant=ButtonVariant::Ghost size=ButtonSize::IconSm href=link attr:title="Open site" attr:target="_blank" attr:rel="noopener">
                    <ExternalLink />
                </ButtonLink>
                <ButtonLink variant=ButtonVariant::Ghost size=ButtonSize::IconSm href=manage attr:title="Manage">
                    <Settings />
                </ButtonLink>
            </div>
        </div>
    }
}

#[component]
fn SiteLink(link: String, close: Callback<()>) -> impl IntoView {
    let ui = use_ui();
    let copy_link = link.clone();
    view! {
        <div class="self-center p-3 bg-white rounded-lg border size-48 [&_svg]:size-full" inner_html=qr_svg(&link) />
        <Input readonly=true bind_value=RwSignal::new(link.clone()) />
        <div class="flex flex-wrap gap-2 justify-end">
            <Button variant=ButtonVariant::Ghost on:click=move |_| close.run(())>"Done"</Button>
            <ButtonLink variant=ButtonVariant::Outline href=link attr:target="_blank" attr:rel="noopener">
                <ExternalLink />
                "Open"
            </ButtonLink>
            <Button on:click=move |_| copy(ui, &copy_link)>
                <Copy />
                "Copy link"
            </Button>
        </div>
    }
}

#[component]
fn NewSiteDialog(open: RwSignal<bool>, on_created: Callback<()>) -> impl IntoView {
    let ui = use_ui();
    let form = SiteForm::new(None);
    let picked = RwSignal::new_local(None::<Upload>);
    let loaded = RwSignal::new(None::<f64>);
    let created = RwSignal::new(None::<String>);
    let submit = Callback::new(move |ev: SubmitEvent| {
        ev.prevent_default();
        let Some(upload) = picked.get_untracked() else { return ui.notify("Choose the files of your site") };
        let body = match form.body() {
            Ok(body) => body,
            Err(msg) => return ui.notify(msg),
        };
        loaded.set(Some(0.0));
        spawn_local(async move {
            let Some(site) = ui.run(api::post::<NewSite>("/api/sites", &body)).await else { return loaded.set(None) };
            on_created.run(());
            deploy(site.id, &upload, true, "", loaded, move |res| {
                on_created.run(());
                match res {
                    Ok(()) => created.set(Some(site.name)),
                    Err(e) => {
                        ui.notify(format!("The site was created, but the upload failed: {}", e.msg));
                        open.set(false);
                    }
                }
            });
        });
    });
    let sending = move || loaded.with(Option::is_some);
    view! {
        <Modal open title="New site" class="max-w-[min(36rem,calc(100%-2rem))]">
            {move || match created.get() {
                Some(name) => view! {
                    <p class="text-sm text-muted-foreground">"Your site is live. Share the link or let people scan the QR code."</p>
                    <SiteLink link=site_link(&name) close=Callback::new(move |()| open.set(false)) />
                }
                .into_any(),
                None => view! {
                    <form class="flex flex-col gap-4" on:submit=move |ev| submit.run(ev)>
                        <NameField form />
                        <div class="flex flex-col gap-2">
                            <Label>"Files"</Label>
                            <Picker picked loaded />
                        </div>
                        <OptionFields form />
                        <div class="flex gap-2 justify-end">
                            <Button attr:r#type="button" variant=ButtonVariant::Ghost on:click=move |_| open.set(false)>"Cancel"</Button>
                            <Button attr:disabled=sending>{move || if sending() { "Uploading..." } else { "Create site" }}</Button>
                        </div>
                    </form>
                }
                .into_any(),
            }}
        </Modal>
    }
}

#[component]
fn NameField(form: SiteForm, #[prop(optional)] original: Option<String>) -> impl IntoView {
    let name = form.name;
    Effect::new(move |_| {
        let clean = name.with(|n| slug(n));
        if name.with_untracked(|n| *n != clean) {
            name.set(clean);
        }
    });
    let creating = original.is_none();
    let renamed = move || original.as_ref().is_some_and(|o| name.with(|n| n != o));
    let prefix = api::absolute("/site/");
    view! {
        <div class="flex flex-col gap-2">
            <Label>"Site name"</Label>
            {if creating {
                view! {
                    <InputGroup class="overflow-hidden">
                        <InputGroupAddon class="self-stretch pr-3 min-w-0 border-r bg-muted max-w-[65%]">
                            <span class="truncate">{prefix}</span>
                        </InputGroupAddon>
                        <Input
                            attr:data-slot="input-group-control"
                            class="flex-1 bg-transparent rounded-none border-0 shadow-none pl-3! focus-visible:ring-0 dark:bg-transparent"
                            bind_value=name
                            required=true
                            autocomplete="off"
                            placeholder="my-site"
                        />
                    </InputGroup>
                }
                .into_any()
            } else {
                view! { <Input bind_value=name required=true autocomplete="off" placeholder="my-site" /> }.into_any()
            }}
            <p class="text-xs text-muted-foreground">{NAME_RULE}</p>
            <Show when=renamed>
                <p class="text-xs text-destructive">"Renaming changes the link. The old link stops working."</p>
            </Show>
        </div>
    }
}

#[component]
fn OptionFields(form: SiteForm) -> impl IntoView {
    let was_locked = form.was_locked;
    view! {
        <label class="flex gap-3 justify-between items-center p-3 rounded-lg border cursor-pointer">
            <span class="flex flex-col gap-0.5">
                <span class="text-sm font-medium">"Enabled"</span>
                <span class="text-xs text-muted-foreground">"Turn off to take the site offline without deleting it."</span>
            </span>
            <Switch checked=form.enabled aria_label="Enabled" />
        </label>
        <label class="flex gap-2 items-center text-sm font-medium">
            <Checkbox checked=form.locked on_checked_change=Callback::new(move |v| form.locked.set(v)) aria_label="Require a password" />
            "Require a password"
        </label>
        <Show when=move || form.locked.get()>
            <Input
                r#type=InputType::Password
                bind_value=form.password
                autocomplete="new-password"
                placeholder=if was_locked { "Leave empty to keep the current password" } else { "Password" }
            />
        </Show>
        <div class="flex flex-col gap-2">
            <Label>"Expiration date (optional)"</Label>
            <DateTimePicker bind_value=form.expires />
        </div>
    }
}

#[component]
fn Picker(picked: RwSignal<Option<Upload>, LocalStorage>, loaded: RwSignal<Option<f64>>) -> impl IntoView {
    let over = RwSignal::new(false);
    let folder_input = NodeRef::<html::Input>::new();
    let files_input = NodeRef::<html::Input>::new();
    let zip_input = NodeRef::<html::Input>::new();
    let choose = move |input: NodeRef<html::Input>| {
        if let Some(el) = input.get() {
            el.click();
        }
    };
    let take = move |input: NodeRef<html::Input>| {
        let Some(el) = input.get() else { return Vec::new() };
        let list = el.files().map(|l| api::files_of(&l)).unwrap_or_default();
        el.set_value("");
        list
    };
    let drop = move |ev: DragEvent| {
        ev.prevent_default();
        over.set(false);
        let Some(dt) = ev.data_transfer() else { return };
        let entries = drop_entries(&dt);
        let files = dt.files().map(|l| api::files_of(&l)).unwrap_or_default();
        spawn_local(async move { picked.set(dropped(entries, files).await) });
    };
    let sending = move || loaded.with(Option::is_some);
    let percent = Signal::derive(move || {
        let total = picked.with(|p| p.as_ref().map_or(0.0, Upload::size));
        loaded.get().unwrap_or(0.0) / total.max(1.0) * 100.0
    });
    view! {
        <div
            class="flex flex-col gap-3 justify-center items-center p-6 text-sm text-center rounded-xl border-2 border-dashed transition-colors text-muted-foreground"
            class=("border-primary", move || over.get())
            class=("bg-muted/50", move || over.get())
            on:dragover=move |ev: DragEvent| {
                ev.prevent_default();
                over.set(true);
            }
            on:dragleave=move |_| over.set(false)
            on:drop=drop
        >
            <Upload class="size-6" />
            <div class="flex flex-col gap-1">
                <span class="font-medium text-foreground">"Drop a folder, files or a ZIP here"</span>
                <span class="text-xs">"The index.html file is served as the home page."</span>
            </div>
            <div class="flex flex-wrap gap-2 justify-center">
                <Button attr:r#type="button" variant=ButtonVariant::Outline size=ButtonSize::Sm attr:disabled=sending on:click=move |_| choose(folder_input)>
                    <FolderUp />
                    "Choose folder"
                </Button>
                <Button attr:r#type="button" variant=ButtonVariant::Outline size=ButtonSize::Sm attr:disabled=sending on:click=move |_| choose(files_input)>
                    <Files />
                    "Choose files"
                </Button>
                <Button attr:r#type="button" variant=ButtonVariant::Outline size=ButtonSize::Sm attr:disabled=sending on:click=move |_| choose(zip_input)>
                    <FileArchive />
                    "Choose ZIP"
                </Button>
            </div>
        </div>
        <input
            type="file"
            multiple
            prop:webkitdirectory=true
            class="hidden"
            node_ref=folder_input
            on:change=move |_| picked.set(Upload::files(take(folder_input).iter().map(Picked::relative).collect()))
        />
        <input
            type="file"
            multiple
            class="hidden"
            node_ref=files_input
            on:change=move |_| picked.set(Upload::files(take(files_input).iter().map(Picked::relative).collect()))
        />
        <input
            type="file"
            accept=".zip,application/zip"
            class="hidden"
            node_ref=zip_input
            on:change=move |_| picked.set(take(zip_input).into_iter().next().map(Upload::Zip))
        />
        {move || picked.with(|p| p.as_ref().map(|u| (u.label(), u.size()))).map(|(label, size)| view! {
            <div class="flex flex-col gap-1.5">
                <div class="flex gap-3 items-center text-sm">
                    <Files class="size-4 text-muted-foreground shrink-0" />
                    <span class="flex-1 truncate">{label}</span>
                    <span class="tabular-nums text-muted-foreground shrink-0">{fmt_size(size as i64)}</span>
                    <Show when=move || !sending()>
                        <Button attr:r#type="button" variant=ButtonVariant::Ghost size=ButtonSize::IconXs attr:title="Remove" on:click=move |_| picked.set(None)>
                            <X />
                        </Button>
                    </Show>
                </div>
                <Show when=sending>
                    <Progress value=percent />
                </Show>
            </div>
        })}
    }
}

#[component]
pub fn SiteDetail() -> impl IntoView {
    let ui = use_ui();
    let params = use_params_map();
    let id = Memo::new(move |_| params.read().get("id").unwrap_or_default());
    let sites = LocalResource::new(move || async move { ui.run(api::get::<Vec<Site>>("/api/sites")).await });
    let site = Memo::new(move |_| sites.get().flatten().map(|list| list.into_iter().find(|s| s.id.as_str() == id.get())));
    let found = Memo::new(move |_| site.with(|s| s.as_ref().map(|s| s.as_ref().map(|s| s.id))));
    let reload = Callback::new(move |()| sites.refetch());
    view! {
        {move || found.get().map(|found| match found {
            Some(id) => view! { <SiteView id site=Signal::derive(move || site.get().flatten()) reload /> }.into_any(),
            None => view! {
                <div class="flex flex-col gap-3 items-center p-16 text-center">
                    <p class="text-lg font-semibold">"Site not found"</p>
                    <ButtonLink href=api::url("/sites")>"All sites"</ButtonLink>
                </div>
            }
            .into_any(),
        })}
    }
}

#[derive(Clone)]
pub(crate) enum Editing {
    New(String),
    Existing(String),
}

#[derive(Clone)]
pub(crate) struct Deploy {
    pub replace: bool,
    pub dir: String,
}

#[derive(Clone, Copy)]
pub(crate) struct SiteCtx {
    pub id: Id,
    pub name: Memo<String>,
    pub files: LocalResource<Vec<SiteFile>>,
    pub editing: RwSignal<Option<Editing>>,
    pub deploying: RwSignal<Option<Deploy>>,
    reload: Callback<()>,
}

impl SiteCtx {
    pub fn refresh(self) {
        self.files.refetch();
        self.reload.run(());
    }
}

#[component]
fn SiteView(id: Id, site: Signal<Option<Site>>, reload: Callback<()>) -> impl IntoView {
    let ui = use_ui();
    let files = LocalResource::new(move || async move { ui.run(api::get::<Vec<SiteFile>>(&format!("/api/sites/{id}/files"))).await.unwrap_or_default() });
    let name = Memo::new(move |_| site.with(|s| s.as_ref().map(|s| s.name.clone()).unwrap_or_default()));
    let ctx = SiteCtx { id, name, files, editing: RwSignal::new(None), deploying: RwSignal::new(None), reload };
    let settings = RwSignal::new(false);
    let qr_open = RwSignal::new(false);
    Effect::new(move |_| ui.set_crumb(name.get()));
    view! {
        <div class="flex flex-col gap-6 mx-auto max-w-6xl page-enter">
            <div class="flex flex-wrap gap-2 items-center">
                <ButtonLink variant=ButtonVariant::Ghost size=ButtonSize::Sm href=api::url("/sites")>
                    <ArrowLeft />
                    "All sites"
                </ButtonLink>
            </div>
            {move || site.get().map(|site| view! { <SiteHeader site settings qr_open /> })}
            <SiteFiles site=ctx />
        </div>
        <Modal open=qr_open title="QR code">
            {move || view! { <SiteLink link=site_link(&name.get()) close=Callback::new(move |()| qr_open.set(false)) /> }}
        </Modal>
        {move || settings.get().then(|| site.get_untracked().map(|site| view! { <SettingsDialog site open=settings reload /> }))}
        {move || ctx.deploying.get().map(|job| view! { <DeployDialog ctx job /> })}
        {move || ctx.editing.get().map(|editing| view! { <EditorDialog ctx editing /> })}
    }
}

#[component]
fn SiteHeader(site: Site, settings: RwSignal<bool>, qr_open: RwSignal<bool>) -> impl IntoView {
    let ui = use_ui();
    let navigate = use_navigate();
    let id = site.id;
    let (label, variant) = site.status();
    let locked = site.locked == 1;
    let link = site_link(&site.name);
    let copy_link = link.clone();
    let message = format!("\"{}\" and all its files will be deleted. Its link stops working.", site.name);
    let remove = move |_| {
        let navigate = navigate.clone();
        ui.confirm_delete(message.clone(), move || {
            let navigate = navigate.clone();
            spawn_local(async move {
                if ui.run(api::del(&format!("/api/sites/{id}"))).await.is_some() {
                    navigate("/sites", Default::default());
                }
            });
        });
    };
    view! {
        <div class="flex flex-wrap gap-3 items-start">
            <div class="flex flex-col flex-1 gap-1 min-w-64">
                <div class="flex flex-wrap gap-2 items-center">
                    <h1 class="text-2xl font-semibold tracking-tight break-all">{site.name.clone()}</h1>
                    <Badge variant>{label}</Badge>
                    <Show when=move || locked>
                        <Badge variant=BadgeVariant::Outline><Lock class="mr-1 size-3" />"Password"</Badge>
                    </Show>
                </div>
                <a href=link.clone() target="_blank" rel="noopener" class="text-sm break-all text-muted-foreground w-fit hover:text-foreground">{link.clone()}</a>
                <span class="text-xs text-muted-foreground">{site.meta()}</span>
            </div>
            <Button variant=ButtonVariant::Outline attr:title="Copy link" on:click=move |_| copy(ui, &copy_link)>
                <Copy />
                <span class="hidden sm:inline">"Copy link"</span>
            </Button>
            <ButtonLink variant=ButtonVariant::Outline href=link.clone() attr:title="Open site" attr:target="_blank" attr:rel="noopener">
                <ExternalLink />
                <span class="hidden sm:inline">"Open"</span>
            </ButtonLink>
            <Button variant=ButtonVariant::Outline attr:title="Show QR code" on:click=move |_| qr_open.set(true)>
                <QrCode />
                <span class="hidden sm:inline">"Show QR"</span>
            </Button>
            <Button variant=ButtonVariant::Outline attr:title="Site settings" on:click=move |_| settings.set(true)>
                <Settings />
                <span class="hidden sm:inline">"Settings"</span>
            </Button>
            <Button variant=ButtonVariant::Ghost size=ButtonSize::Icon attr:title="Delete site" on:click=remove>
                <Trash2 />
            </Button>
        </div>
    }
}

#[component]
fn SettingsDialog(site: Site, open: RwSignal<bool>, reload: Callback<()>) -> impl IntoView {
    let ui = use_ui();
    let form = SiteForm::new(Some(&site));
    let id = site.id;
    let submit = move |ev: SubmitEvent| {
        ev.prevent_default();
        let body = match form.body() {
            Ok(body) => body,
            Err(msg) => return ui.notify(msg),
        };
        spawn_local(async move {
            if ui.run(api::put::<Value>(&format!("/api/sites/{id}"), &body)).await.is_some() {
                ui.notify("Site saved");
                reload.run(());
                open.set(false);
            }
        });
    };
    view! {
        <Modal open title="Settings" class="max-w-[min(36rem,calc(100%-2rem))]">
            <form class="flex flex-col gap-4" on:submit=submit>
                <p class="text-sm text-muted-foreground">"Name, access and expiration of the site."</p>
                <NameField form original=site.name.clone() />
                <OptionFields form />
                <div class="flex gap-2 justify-end">
                    <Button attr:r#type="button" variant=ButtonVariant::Ghost on:click=move |_| open.set(false)>"Cancel"</Button>
                    <Button>"Save"</Button>
                </div>
            </form>
        </Modal>
    }
}

#[component]
fn DeployDialog(ctx: SiteCtx, job: Deploy) -> impl IntoView {
    let ui = use_ui();
    let open = RwSignal::new(true);
    Effect::new(move |_| {
        if !open.get() {
            ctx.deploying.set(None);
        }
    });
    let picked = RwSignal::new_local(None::<Upload>);
    let loaded = RwSignal::new(None::<f64>);
    let Deploy { replace, dir } = job;
    let folder = StoredValue::new(dir.clone());
    let (title, action, done) = if replace { ("Redeploy site", "Redeploy", "Site redeployed") } else { ("Upload files", "Upload", "Files uploaded") };
    let hint = match (replace, dir.trim_end_matches('/')) {
        (true, _) => "Every current file is replaced by the new upload.".to_owned(),
        (false, "") => "The files are added to the site. Files with the same path are overwritten.".to_owned(),
        (false, folder) => format!("The files are added to the folder {folder}. Files with the same path are overwritten."),
    };
    let submit = move |ev: SubmitEvent| {
        ev.prevent_default();
        let Some(upload) = picked.get_untracked() else { return ui.notify("Choose files to upload") };
        deploy(ctx.id, &upload, replace, &folder.get_value(), loaded, move |res| match res {
            Ok(()) => {
                ui.notify(done);
                ctx.refresh();
                open.set(false);
            }
            Err(e) => ui.fail(e),
        });
    };
    let sending = move || loaded.with(Option::is_some);
    view! {
        <Modal open title=title class="max-w-[min(36rem,calc(100%-2rem))]">
            <form class="flex flex-col gap-4" on:submit=submit>
                <p class="text-sm text-muted-foreground">{hint.clone()}</p>
                <Picker picked loaded />
                <div class="flex gap-2 justify-end">
                    <Button attr:r#type="button" variant=ButtonVariant::Ghost on:click=move |_| open.set(false)>"Cancel"</Button>
                    <Button attr:disabled=sending>{move || if sending() { "Uploading..." } else { action }}</Button>
                </div>
            </form>
        </Modal>
    }
}

#[component]
fn EditorDialog(ctx: SiteCtx, editing: Editing) -> impl IntoView {
    let ui = use_ui();
    let open = RwSignal::new(true);
    Effect::new(move |_| {
        if !open.get() {
            ctx.editing.set(None);
        }
    });
    let (is_new, start) = match editing {
        Editing::New(dir) => (true, dir),
        Editing::Existing(path) => (false, path),
    };
    let target = RwSignal::new(start.clone());
    let text = RwSignal::new(String::new());
    let ready = RwSignal::new(is_new);
    if !is_new {
        spawn_local(async move {
            if let Some(body) = ui.run(api::get_text(&file_url(ctx.id, &start))).await {
                text.set(body);
                ready.set(true);
            }
        });
    }
    let markdown = Memo::new(move |_| is_markdown(&target.get()));
    let save = move || {
        let path = target.get_untracked().trim().trim_start_matches('/').to_owned();
        if path.is_empty() {
            return ui.notify("Enter a file path");
        }
        spawn_local(async move {
            if ui.run(api::put_text::<Value>(&file_url(ctx.id, &path), &text.get_untracked())).await.is_some() {
                ui.notify("File saved");
                ctx.refresh();
                open.set(false);
            }
        });
    };
    view! {
        <Modal open title=if is_new { "New file" } else { "Edit file" } class="max-w-[min(64rem,calc(100%-2rem))]">
            <form
                class="flex flex-col gap-4"
                on:submit=move |ev: SubmitEvent| {
                    ev.prevent_default();
                    save();
                }
            >
                {if is_new {
                    view! {
                        <div class="flex flex-col gap-2">
                            <Label>"Path"</Label>
                            <Input bind_value=target required=true autofocus=true autocomplete="off" placeholder="about.html" />
                        </div>
                    }
                    .into_any()
                } else {
                    view! { <p class="text-sm break-all text-muted-foreground">{target.get_untracked()}</p> }.into_any()
                }}
                <Show when=move || ready.get() fallback=|| view! { <div class="flex justify-center py-16"><Spinner /></div> }>
                    {move || view! { <FileEditor markdown=markdown.get() text on_save=Callback::new(move |()| save()) class="h-[55vh] min-h-32" /> }}
                </Show>
                <div class="flex gap-2 justify-end">
                    <Button attr:r#type="button" variant=ButtonVariant::Ghost on:click=move |_| open.set(false)>"Cancel"</Button>
                    <Button attr:disabled=move || !ready.get()>"Save"</Button>
                </div>
            </form>
        </Modal>
    }
}

#[cfg(test)]
mod tests {
    use super::slug;

    #[test]
    fn slug_keeps_lowercase_letters_digits_and_hyphens() {
        assert_eq!(slug("My Site_2.0!"), "my-site-2-0");
        assert_eq!(slug(&"a".repeat(80)).len(), 63);
    }
}
