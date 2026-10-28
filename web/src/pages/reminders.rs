use std::cmp::Reverse;

use icons::{Archive, ArchiveRestore, Bell, Check, FileText, Pencil, Trash2};
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::A;
use serde::Deserialize;
use serde_json::{Value, json};
use wasm_bindgen::JsValue;

use crate::api;
use crate::id::Id;
use crate::md;
use crate::components::ui::badge::{Badge, BadgeVariant};
use crate::components::ui::button::{Button, ButtonSize, ButtonVariant};
use crate::components::ui::card::{Card, CardContent};
use crate::components::ui::date_picker::DateTimePicker;
use crate::components::ui::empty::{Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant, EmptyTitle};
use crate::components::ui::input::Input;
use crate::components::ui::label::Label;
use crate::pages::calendar::local_text;
use crate::pages::editor::RichEditor;
use crate::widgets::{
    ListSkeleton, Modal, Options, PageHeader, SearchBox, Segmented, SortSelect, Toolbar, fmt_time, has, parse_local, query_state,
    use_ui,
};

const SHOW: Options = &[("upcoming", "Upcoming"), ("done", "Completed"), ("archived", "Archived"), ("all", "All")];
const SORTS: Options = &[("soonest", "Soonest first"), ("latest", "Latest first"), ("title", "Title A-Z")];

#[derive(Clone, Deserialize)]
pub struct Reminder {
    pub id: Id,
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub remind_at: i64,
    pub note_id: Option<String>,
    pub sent: i64,
    #[serde(default)]
    pub archived: i64,
}

fn arrange(mut list: Vec<Reminder>, q: &str, show: &str, sort: &str) -> Vec<Reminder> {
    list.retain(|r| {
        let shown = match show {
            "upcoming" => r.archived == 0 && r.sent == 0,
            "done" => r.archived == 0 && r.sent == 1,
            "archived" => r.archived == 1,
            _ => true,
        };
        shown && (has(&r.title, q) || has(&r.description, q))
    });
    match sort {
        "latest" => list.sort_by_key(|r| Reverse(r.remind_at)),
        "title" => list.sort_by_key(|r| r.title.to_lowercase()),
        _ => list.sort_by_key(|r| r.remind_at),
    }
    list
}

#[component]
pub fn Reminders() -> impl IntoView {
    let reload = RwSignal::new(0u32);
    crate::widgets::focus_if_new();
    view! {
        <div class="flex flex-col gap-6 mx-auto max-w-4xl page-enter">
            <PageHeader title="Reminders" description="Delivered to your email address at the chosen time." />
            <Card>
                <CardContent>
                    <ReminderForm on_done=Callback::new(move |_| reload.update(|n| *n += 1)) />
                </CardContent>
            </Card>
            <ReminderList reload />
        </div>
    }
}

#[component]
pub fn ReminderForm(
    #[prop(optional)] reminder: Option<Reminder>,
    #[prop(optional)] note_id: Option<String>,
    #[prop(into, optional)] title: String,
    on_done: Callback<()>,
) -> impl IntoView {
    let ui = use_ui();
    let id = reminder.as_ref().map(|r| r.id);
    let title = RwSignal::new(reminder.as_ref().map_or(title, |r| r.title.clone()));
    let description = RwSignal::new(reminder.as_ref().map(|r| r.description.clone()).unwrap_or_default());
    let original = reminder.as_ref().map(|r| (local_text(r.remind_at), r.remind_at));
    let at = RwSignal::new(original.clone().map(|(text, _)| text).unwrap_or_default());
    let busy = RwSignal::new(false);
    let save = Callback::new(move |()| {
        if busy.get_untracked() {
            return;
        }
        let text = at.get_untracked();
        let unchanged = original.as_ref().filter(|(shown, _)| *shown == text).map(|(_, ts)| *ts);
        let Some(remind_at) = unchanged.or_else(|| parse_local(&text)) else {
            ui.notify("Pick a date and time");
            return;
        };
        let utc_offset = -js_sys::Date::new(&JsValue::from_f64(remind_at as f64 * 1000.0)).get_timezone_offset() as i64;
        let body = json!({
            "title": title.get_untracked(),
            "description": description.get_untracked(),
            "remind_at": remind_at,
            "utc_offset": utc_offset,
            "note_id": note_id,
        });
        busy.set(true);
        spawn_local(async move {
            let saved = match id {
                Some(id) => ui.run(api::put::<Value>(&format!("/api/reminders/{id}"), &body)).await,
                None => ui.run(api::post::<Value>("/api/reminders", &body)).await,
            };
            busy.set(false);
            if saved.is_none() {
                return;
            }
            if id.is_none() {
                title.set(String::new());
                at.set(String::new());
                description.set(String::new());
            }
            ui.notify(if id.is_some() { "Reminder updated" } else { "Reminder added" });
            on_done.run(());
        });
    });
    view! {
        <div class="flex flex-col gap-3">
            <div class="flex flex-col gap-3 sm:flex-row sm:items-end">
                <div class="flex flex-col flex-1 gap-2">
                    <Label>"Title"</Label>
                    <Input
                        id=if id.is_some() { "edit-item" } else { "new-item" }
                        bind_value=title
                        on:keydown=move |ev: web_sys::KeyboardEvent| {
                            if ev.key() == "Enter" && !ev.is_composing() && !ev.repeat() {
                                save.run(());
                            }
                        }
                    />
                </div>
                <div class="flex flex-col gap-2">
                    <Label>"When"</Label>
                    <DateTimePicker bind_value=at class="sm:w-56" />
                </div>
            </div>
            <div class="flex flex-col gap-2">
                <Label>"Description (optional)"</Label>
                <RichEditor body=description on_change=Callback::new(|_| ()) compact=true min_height="min-h-24" placeholder="Add details..." />
            </div>
            <div class="flex justify-end">
                <Button attr:disabled=move || busy.get() on:click=move |_| save.run(())>{if id.is_some() { "Save" } else { "Remind me" }}</Button>
            </div>
        </div>
    }
}

#[component]
pub fn ReminderList(#[prop(optional)] reload: Option<RwSignal<u32>>, #[prop(optional)] archive_only: bool) -> impl IntoView {
    let ui = use_ui();
    let items = LocalResource::new(move || {
        if let Some(r) = reload {
            r.track();
        }
        async move { ui.run(api::get::<Vec<Reminder>>("/api/reminders")).await }
    });
    let q = RwSignal::new(String::new());
    let (show, set_show) = query_state("show", "upcoming");
    let show = if archive_only { Signal::stored("archived".to_owned()) } else { show };
    let (sort, set_sort) = query_state("sort", "soonest");
    let refresh = Callback::new(move |_| items.refetch());
    let editing = RwSignal::new(None::<Reminder>);
    let edit_open = RwSignal::new(false);
    let edit = Callback::new(move |reminder| {
        editing.set(Some(reminder));
        edit_open.set(true);
    });
    let saved = Callback::new(move |_| {
        edit_open.set(false);
        items.refetch();
    });
    let rows = move || items.get().flatten().map(|list| arrange(list, &q.get(), &show.get(), &sort.get()));

    view! {
        <Toolbar>
            <div class="flex flex-wrap flex-1 gap-2 items-center min-w-0 max-w-full basis-full sm:basis-0">
                <SearchBox value=q placeholder="Search reminders  /" />
                <Show when=move || !archive_only>
                    <Segmented options=SHOW value=show on_change=set_show />
                </Show>
            </div>
            <SortSelect options=SORTS value=sort on_change=set_sort />
        </Toolbar>
        <Card class="overflow-hidden gap-0 py-0 divide-y">
            {move || match rows() {
                None => view! { <ListSkeleton rows=3 /> }.into_any(),
                Some(list) if list.is_empty() => view! {
                    <Empty class="m-4 border-0">
                        <EmptyHeader>
                            <EmptyMedia variant=EmptyMediaVariant::Icon><Bell /></EmptyMedia>
                            <EmptyTitle>"No reminders here"</EmptyTitle>
                            <EmptyDescription>"Nothing matches this view."</EmptyDescription>
                        </EmptyHeader>
                    </Empty>
                }
                .into_any(),
                Some(list) => view! {
                    <For
                        each=move || list.clone()
                        key=|r| (r.id, r.title.clone(), r.description.clone(), r.remind_at, r.sent, r.archived)
                        let(reminder)
                    >
                        <ReminderRow reminder reload=refresh edit />
                    </For>
                }
                .into_any(),
            }}
        </Card>
        <Modal open=edit_open title="Edit reminder" class="max-w-[min(40rem,calc(100%-2rem))]">
            {move || editing.get().map(|reminder| view! { <ReminderForm reminder on_done=saved /> })}
        </Modal>
    }
}

fn status(r: &Reminder) -> AnyView {
    let now = (js_sys::Date::now() / 1000.0) as i64;
    match (r.archived, r.sent) {
        (1, _) => view! { <Badge variant=BadgeVariant::Secondary>"Archived"</Badge> }.into_any(),
        (_, 1) => view! { <Badge variant=BadgeVariant::Secondary>"Completed"</Badge> }.into_any(),
        _ if r.remind_at < now => view! { <Badge variant=BadgeVariant::Destructive>"Due"</Badge> }.into_any(),
        _ => view! { <Badge variant=BadgeVariant::Outline>"Pending"</Badge> }.into_any(),
    }
}

#[component]
fn ReminderRow(reminder: Reminder, reload: Callback<()>, edit: Callback<Reminder>) -> impl IntoView {
    let ui = use_ui();
    let id = reminder.id;
    let act = move |action: &'static str| ui.act(format!("/api/reminders/{id}/action/{action}"), move || reload.run(()));
    let remove = move |_| {
        ui.confirm_delete("This reminder will be deleted.", move || {
            spawn_local(async move {
                if ui.run(api::del(&format!("/api/reminders/{id}"))).await.is_some() {
                    reload.run(());
                }
            });
        });
    };
    let pending = reminder.sent == 0;
    let archived = reminder.archived == 1;
    let badge = status(&reminder);
    let summary = md::plain(&reminder.description);
    let stored = StoredValue::new(reminder.clone());

    view! {
        <div class="flex flex-wrap gap-3 items-center py-3 px-4 transition-colors group hover:bg-muted/50">
            <div class="flex justify-center items-center rounded-lg size-9 bg-muted shrink-0">
                <Bell class="size-4 text-muted-foreground" />
            </div>
            <div class="flex flex-col flex-1 min-w-48">
                <span class="font-medium truncate" class=("line-through", !pending) class=("text-muted-foreground", !pending)>{reminder.title}</span>
                <span class="text-xs text-muted-foreground">{fmt_time(reminder.remind_at)}</span>
                {(!summary.is_empty()).then(|| view! { <span class="text-xs text-muted-foreground line-clamp-1">{summary}</span> })}
            </div>
            {reminder.note_id.map(|n| view! {
                <Button variant=ButtonVariant::Ghost size=ButtonSize::Sm href=api::url(&format!("/notes/{n}"))>
                    <FileText />
                    "Open note"
                </Button>
            })}
            {badge}
            <div class="flex gap-0.5 items-center ml-auto opacity-70 transition-opacity group-hover:opacity-100 pointer-coarse:opacity-100">
                <Show when=move || pending && !archived>
                    <Button variant=ButtonVariant::Ghost size=ButtonSize::IconSm attr:title="Mark as done" on:click=move |_| act("done")>
                        <Check />
                    </Button>
                </Show>
                <Button variant=ButtonVariant::Ghost size=ButtonSize::IconSm attr:title="Edit" on:click=move |_| edit.run(stored.get_value())>
                    <Pencil />
                </Button>
                <Button
                    variant=ButtonVariant::Ghost
                    size=ButtonSize::IconSm
                    attr:title=if archived { "Unarchive" } else { "Archive" }
                    on:click=move |_| act(if archived { "unarchive" } else { "archive" })
                >
                    {if archived { view! { <ArchiveRestore /> }.into_any() } else { view! { <Archive /> }.into_any() }}
                </Button>
                <Button variant=ButtonVariant::Ghost size=ButtonSize::IconSm attr:title="Delete" on:click=remove>
                    <Trash2 />
                </Button>
            </div>
        </div>
    }
}

pub fn reminder_link(r: &Reminder) -> String {
    r.note_id.as_ref().map_or_else(|| api::url("/reminders"), |n| api::url(&format!("/notes/{n}")))
}

#[component]
pub fn ReminderLink(reminder: Reminder) -> impl IntoView {
    view! {
        <A href=reminder_link(&reminder) attr:class="flex gap-3 items-center p-2 -mx-2 rounded-lg transition-colors hover:bg-accent">
            <div class="flex justify-center items-center rounded-md size-8 bg-muted shrink-0">
                <Bell class="size-4 text-muted-foreground" />
            </div>
            <div class="flex flex-col min-w-0">
                <span class="text-sm font-medium truncate">{reminder.title}</span>
                <span class="text-xs text-muted-foreground">{fmt_time(reminder.remind_at)}</span>
            </div>
        </A>
    }
}
