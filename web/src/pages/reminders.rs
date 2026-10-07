use std::cmp::Reverse;

use icons::{AlarmClock, Archive, ArchiveRestore, Bell, Check, FileText, Trash2};
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::A;
use serde::Deserialize;

use crate::api;
use crate::id::Id;
use crate::components::ui::badge::{Badge, BadgeVariant};
use crate::components::ui::button::{Button, ButtonSize, ButtonVariant};
use crate::components::ui::card::{Card, CardContent};
use crate::components::ui::empty::{Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant, EmptyTitle};
use crate::widgets::{
    ListSkeleton, Options, PageHeader, ReminderForm, SearchBox, Segmented, SortSelect, Toolbar, fmt_time, has, query_state,
    use_ui,
};

const SHOW: Options = &[("upcoming", "Upcoming"), ("done", "Completed"), ("archived", "Archived"), ("all", "All")];
const SORTS: Options = &[("soonest", "Soonest first"), ("latest", "Latest first"), ("title", "Title A-Z")];

#[derive(Clone, Deserialize)]
pub struct Reminder {
    pub id: Id,
    pub title: String,
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
        shown && has(&r.title, q)
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
                    <ReminderForm note_id=None title="" on_done=Callback::new(move |_| reload.update(|n| *n += 1)) />
                </CardContent>
            </Card>
            <ReminderList reload />
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
                Some(list) => list.into_iter().map(|r| view! { <ReminderRow reminder=r reload=refresh /> }).collect_view().into_any(),
            }}
        </Card>
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
fn ReminderRow(reminder: Reminder, reload: Callback<()>) -> impl IntoView {
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

    view! {
        <div class="flex flex-wrap gap-3 items-center py-3 px-4 transition-colors group hover:bg-muted/50">
            <div class="flex justify-center items-center rounded-lg size-9 bg-muted shrink-0">
                <Bell class="size-4 text-muted-foreground" />
            </div>
            <div class="flex flex-col flex-1 min-w-48">
                <span class="font-medium truncate" class=("line-through", !pending) class=("text-muted-foreground", !pending)>{reminder.title}</span>
                <span class="text-xs text-muted-foreground">{fmt_time(reminder.remind_at)}</span>
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
                <Show when=move || !archived>
                    <Button variant=ButtonVariant::Ghost size=ButtonSize::IconSm attr:title="Snooze one day" on:click=move |_| act("snooze")>
                        <AlarmClock />
                    </Button>
                </Show>
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
