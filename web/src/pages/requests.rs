use icons::{Copy, FolderOpen, Inbox, Lock, Pause, Pencil, Play, Plus, Trash2};
use leptos::ev::SubmitEvent;
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::Deserialize;
use serde_json::{Value, json};
use wasm_bindgen::JsValue;

use crate::api;
use crate::components::ui::badge::{Badge, BadgeVariant};
use crate::components::ui::button::{Button, ButtonSize, ButtonVariant};
use crate::components::ui::card::Card;
use crate::components::ui::checkbox::Checkbox;
use crate::components::ui::date_picker::DateTimePicker;
use crate::components::ui::empty::{Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant, EmptyTitle};
use crate::components::ui::input::{Input, InputType};
use crate::components::ui::label::Label;
use crate::components::ui::textarea::Textarea;
use crate::pages::drive::{Folder, folder_path};
use crate::widgets::{Choice, ListSkeleton, Modal, copy, fmt_date, fmt_size, fmt_time, parse_local, qr_svg, use_ui};

#[derive(Clone, Deserialize, PartialEq)]
pub struct UploadLink {
    pub id: String,
    pub folder_id: Option<String>,
    pub token: String,
    pub title: String,
    pub description: String,
    pub locked: i64,
    pub expires: Option<i64>,
    pub active: i64,
    pub files: i64,
    pub bytes: i64,
    pub last_upload: Option<i64>,
}

impl UploadLink {
    fn url(&self) -> String {
        link_url(&self.token)
    }

    fn body(&self, active: bool) -> Value {
        json!({
            "title": self.title, "description": self.description, "folder": self.folder_id,
            "expires": self.expires, "password": null, "active": active,
        })
    }

    fn status(&self) -> (&'static str, BadgeVariant) {
        let now = (js_sys::Date::now() / 1000.0) as i64;
        match self.expires {
            _ if self.active == 0 => ("Paused", BadgeVariant::Muted),
            Some(t) if t <= now => ("Expired", BadgeVariant::Warning),
            _ => ("Active", BadgeVariant::Success),
        }
    }
}

fn link_url(token: &str) -> String {
    api::absolute(&format!("/u/{token}"))
}

fn local_input(ts: i64) -> String {
    let d = js_sys::Date::new(&JsValue::from_f64(ts as f64 * 1000.0));
    format!("{:04}-{:02}-{:02}T{:02}:{:02}", d.get_full_year(), d.get_month() + 1, d.get_date(), d.get_hours(), d.get_minutes())
}

fn folder_options(folders: &[Folder]) -> Vec<(String, String)> {
    let mut options: Vec<(String, String)> = folders.iter().map(|f| (f.id.clone(), folder_path(folders, Some(&f.id)))).collect();
    options.sort_by_key(|(_, path)| path.to_lowercase());
    options.insert(0, (String::new(), "My Drive".into()));
    options
}

#[component]
pub fn RequestList(links: LocalResource<Vec<UploadLink>>, folders: Signal<Vec<Folder>>, go: Callback<String>) -> impl IntoView {
    let ui = use_ui();
    let editing = RwSignal::new(None::<Option<UploadLink>>);
    let toggle = move |l: UploadLink| {
        spawn_local(async move {
            if ui.run(api::put::<Value>(&format!("/api/upload-links/{}", l.id), &l.body(l.active == 0))).await.is_some() {
                links.refetch();
            }
        });
    };
    let remove = move |l: UploadLink| {
        let message = format!("\"{}\" will be deleted and its link stops working. Files already received stay in your drive.", l.title);
        ui.confirm_delete(message, move || {
            let id = l.id.clone();
            spawn_local(async move {
                if ui.run(api::del(&format!("/api/upload-links/{id}"))).await.is_some() {
                    links.refetch();
                }
            });
        });
    };
    view! {
        <div class="flex flex-wrap gap-2 justify-between items-center min-h-[2.75rem]">
            <p class="text-sm text-muted-foreground">"Anyone with a request link can upload files into the folder you pick, without seeing its contents."</p>
            <Button on:click=move |_| editing.set(Some(None))>
                <Plus />
                "New request"
            </Button>
        </div>
        {move || match links.get() {
            None => view! { <Card class="overflow-hidden gap-0 py-0"><ListSkeleton rows=3 /></Card> }.into_any(),
            Some(list) if list.is_empty() => view! {
                <Empty class="py-16">
                    <EmptyHeader>
                        <EmptyMedia variant=EmptyMediaVariant::Icon><Inbox /></EmptyMedia>
                        <EmptyTitle>"No file requests"</EmptyTitle>
                        <EmptyDescription>"Create a request link to collect files from anyone into one of your folders."</EmptyDescription>
                    </EmptyHeader>
                </Empty>
            }
            .into_any(),
            Some(list) => view! {
                <Card class="overflow-hidden gap-0 py-0">
                    {list.into_iter().map(|l| {
                        let (label, variant) = l.status();
                        let place = folders.with(|f| folder_path(f, l.folder_id.as_deref()));
                        let received = match l.last_upload {
                            Some(t) => format!("{} file(s), {} · last on {}", l.files, fmt_size(l.bytes), fmt_date(t)),
                            None => "No files yet".to_owned(),
                        };
                        let expiry = l.expires.map(|t| format!("Expires {}", fmt_time(t)));
                        let item = StoredValue::new(l.clone());
                        let paused = l.active == 0;
                        view! {
                            <div class="flex flex-wrap gap-3 items-center py-3 px-4 border-b last:border-b-0">
                                <div class="flex flex-col flex-1 gap-1 min-w-56">
                                    <div class="flex gap-2 items-center">
                                        <span class="font-medium truncate">{l.title.clone()}</span>
                                        <Badge variant>{label}</Badge>
                                        <Show when=move || l.locked == 1>
                                            <span title="Password protected"><Lock class="size-3.5 text-muted-foreground" /></span>
                                        </Show>
                                    </div>
                                    <button
                                        type="button"
                                        class="flex gap-1.5 items-center text-sm text-left text-muted-foreground w-fit hover:text-foreground"
                                        on:click=move |_| go.run(item.with_value(|l| l.folder_id.clone().unwrap_or_default()))
                                    >
                                        <FolderOpen class="size-3.5 shrink-0" />
                                        <span class="truncate">{place}</span>
                                    </button>
                                    <span class="text-xs text-muted-foreground">{received}{expiry.map(|e| format!(" · {e}"))}</span>
                                </div>
                                <div class="flex gap-1 items-center">
                                    <Button variant=ButtonVariant::Ghost size=ButtonSize::IconSm attr:title="Copy link" on:click=move |_| copy(ui, &item.with_value(UploadLink::url))>
                                        <Copy />
                                    </Button>
                                    <Button variant=ButtonVariant::Ghost size=ButtonSize::IconSm attr:title="Edit" on:click=move |_| editing.set(Some(Some(item.get_value())))>
                                        <Pencil />
                                    </Button>
                                    <Button
                                        variant=ButtonVariant::Ghost
                                        size=ButtonSize::IconSm
                                        attr:title=if paused { "Resume" } else { "Pause" }
                                        on:click=move |_| toggle(item.get_value())
                                    >
                                        {if paused { view! { <Play /> }.into_any() } else { view! { <Pause /> }.into_any() }}
                                    </Button>
                                    <Button variant=ButtonVariant::Ghost size=ButtonSize::IconSm class="text-destructive" attr:title="Delete" on:click=move |_| remove(item.get_value())>
                                        <Trash2 />
                                    </Button>
                                </div>
                            </div>
                        }
                    }).collect_view()}
                </Card>
            }
            .into_any(),
        }}
        {move || editing.get().map(|link| {
            let open = RwSignal::new(true);
            Effect::new(move |_| {
                if !open.get() {
                    editing.set(None);
                }
            });
            view! { <RequestDialog open folders link folder=None on_saved=Callback::new(move |()| links.refetch()) /> }
        })}
    }
}

#[component]
pub fn RequestDialog(
    open: RwSignal<bool>,
    folders: Signal<Vec<Folder>>,
    link: Option<UploadLink>,
    folder: Option<String>,
    on_saved: Callback<()>,
) -> impl IntoView {
    let ui = use_ui();
    let editing = link.as_ref().map(|l| l.id.clone());
    let is_edit = editing.is_some();
    let was_locked = link.as_ref().is_some_and(|l| l.locked == 1);
    let title = RwSignal::new(link.as_ref().map(|l| l.title.clone()).unwrap_or_default());
    let description = RwSignal::new(link.as_ref().map(|l| l.description.clone()).unwrap_or_default());
    let target = RwSignal::new(link.as_ref().map_or(folder, |l| l.folder_id.clone()).unwrap_or_default());
    let expires = RwSignal::new(link.as_ref().and_then(|l| l.expires).map(local_input).unwrap_or_default());
    let locked = RwSignal::new(was_locked);
    let password = RwSignal::new(String::new());
    let active = link.as_ref().is_none_or(|l| l.active == 1);
    let created = RwSignal::new(None::<String>);
    let options = Signal::derive(move || folders.with(|f| folder_options(f)));
    let submit = Callback::new(move |ev: SubmitEvent| {
        ev.prevent_default();
        let pw = password.get_untracked();
        let secret = match (locked.get_untracked(), pw.is_empty()) {
            (false, _) => Some(String::new()),
            (true, true) if was_locked => None,
            (true, true) => return ui.notify("Enter a password or turn off password protection"),
            (true, false) => Some(pw),
        };
        let folder = Some(target.get_untracked()).filter(|t| !t.is_empty());
        let body = json!({
            "title": title.get_untracked(), "description": description.get_untracked(), "folder": folder,
            "expires": parse_local(&expires.get_untracked()), "password": secret, "active": active,
        });
        let editing = editing.clone();
        spawn_local(async move {
            let saved = match &editing {
                Some(id) => ui.run(api::put::<Value>(&format!("/api/upload-links/{id}"), &body)).await,
                None => ui.run(api::post::<Value>("/api/upload-links", &body)).await,
            };
            let Some(saved) = saved else { return };
            on_saved.run(());
            match saved["token"].as_str() {
                Some(t) => created.set(Some(link_url(t))),
                None => {
                    ui.notify("Request updated");
                    open.set(false);
                }
            }
        });
    });
    let heading = if is_edit { "Edit file request" } else { "New file request" };
    view! {
        <Modal open title=heading class="overflow-visible!">
            {move || match created.get() {
                Some(url) => {
                    let copy_url = url.clone();
                    view! {
                        <p class="text-sm text-muted-foreground">"Share this link. Uploaded files go straight into the folder you picked."</p>
                        <div class="self-center p-3 bg-white rounded-lg border size-48 [&_svg]:size-full" inner_html=qr_svg(&url) />
                        <Input readonly=true bind_value=RwSignal::new(url) />
                        <div class="flex gap-2 justify-end">
                            <Button variant=ButtonVariant::Ghost on:click=move |_| open.set(false)>"Done"</Button>
                            <Button on:click=move |_| copy(ui, &copy_url)><Copy />"Copy link"</Button>
                        </div>
                    }
                    .into_any()
                }
                None => view! {
                    <form class="flex flex-col gap-4" on:submit=move |ev| submit.run(ev)>
                        <div class="flex flex-col gap-2">
                            <Label>"Title"</Label>
                            <Input bind_value=title required=true placeholder="Invoices for March" />
                        </div>
                        <div class="flex flex-col gap-2">
                            <Label>"Description (optional)"</Label>
                            <Textarea bind_value=description rows=3u32 placeholder="Shown to people on the upload page" />
                        </div>
                        <div class="flex flex-col gap-2">
                            <Label>"Save files to"</Label>
                            <Choice class="w-full" options value=target on_change=Callback::new(move |v: String| target.set(v)) />
                        </div>
                        <div class="flex flex-col gap-2">
                            <Label>"Expiration date (optional)"</Label>
                            <DateTimePicker bind_value=expires />
                        </div>
                        <label class="flex gap-2 items-center text-sm font-medium">
                            <Checkbox checked=locked on_checked_change=Callback::new(move |v| locked.set(v)) aria_label="Require a password" />
                            "Require a password"
                        </label>
                        <Show when=move || locked.get()>
                            <Input
                                r#type=InputType::Password
                                bind_value=password
                                autocomplete="new-password"
                                placeholder=if was_locked { "Leave empty to keep the current password" } else { "Password" }
                            />
                        </Show>
                        <div class="flex gap-2 justify-end">
                            <button type="button" class="px-4 h-9 text-sm font-medium rounded-md hover:bg-accent" on:click=move |_| open.set(false)>"Cancel"</button>
                            <Button>{if is_edit { "Save" } else { "Create link" }}</Button>
                        </div>
                    </form>
                }
                .into_any(),
            }}
        </Modal>
    }
}
