use icons::{ArchiveRestore, FileText, LockKeyhole, Notebook, Plus, Search, Share2, Trash2};
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::A;
use leptos_router::hooks::use_navigate;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::api;
use crate::components::ui::button::{Button, ButtonSize, ButtonVariant};
use crate::components::ui::card::Card;
use crate::components::ui::empty::{Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant, EmptyTitle};
use crate::components::ui::input::Input;
use crate::widgets::{ListSkeleton, PageHeader, PinGate, fmt_time, use_ui};

#[derive(Clone, Copy, PartialEq)]
pub enum View {
    Active,
    Archived,
    Trash,
    Secret,
    Project(i64),
}

impl View {
    fn query(self) -> String {
        match self {
            View::Active => "view=active".into(),
            View::Archived => "view=archived".into(),
            View::Trash => "view=trash".into(),
            View::Secret => "view=secret".into(),
            View::Project(id) => format!("view=project&project={id}"),
        }
    }

    fn title(self) -> &'static str {
        match self {
            View::Active => "Notes",
            View::Archived => "Archive",
            View::Trash => "Trash",
            View::Secret => "Secret notes",
            View::Project(_) => "Project notes",
        }
    }

    fn description(self) -> &'static str {
        match self {
            View::Active => "Markdown notes, saved as you type.",
            View::Archived => "Notes you put away for later.",
            View::Trash => "Restore notes or delete them for good.",
            View::Secret => "Encrypted notes behind your PIN.",
            View::Project(_) => "Notes linked to this project.",
        }
    }

    fn project(self) -> Option<i64> {
        if let View::Project(id) = self { Some(id) } else { None }
    }
}

#[derive(Clone, Deserialize)]
pub struct NoteItem {
    pub id: String,
    pub title: String,
    pub updated: i64,
    pub share_token: Option<String>,
}

#[component]
pub fn NotesList(view: View) -> impl IntoView {
    if view == View::Secret {
        view! { <PinGate><NotesInner view /></PinGate> }.into_any()
    } else {
        view! { <NotesInner view /> }.into_any()
    }
}

#[component]
fn SwitchArea(view: View) -> impl IntoView {
    match view {
        View::Active => view! {
            <Button variant=ButtonVariant::Outline size=ButtonSize::Icon attr:title="Secret notes" href=api::url("/secret")>
                <LockKeyhole />
            </Button>
        }
        .into_any(),
        View::Secret => view! {
            <Button variant=ButtonVariant::Outline href=api::url("/notes")>
                <Notebook />
                "All notes"
            </Button>
        }
        .into_any(),
        _ => ().into_any(),
    }
}

#[component]
fn NotesInner(view: View) -> impl IntoView {
    let ui = use_ui();
    let navigate = use_navigate();
    let q = RwSignal::new(String::new());
    let notes = LocalResource::new(move || {
        let path = format!("/api/notes?{}&q={}", view.query(), js_sys::encode_uri_component(&q.get()));
        async move { ui.run(api::get::<Vec<NoteItem>>(&path)).await.unwrap_or_default() }
    });
    let reload = Callback::new(move |_| notes.refetch());
    let create = move |_| {
        let navigate = navigate.clone();
        spawn_local(async move {
            let body = json!({ "secret": view == View::Secret, "project_id": view.project() });
            if let Some(v) = ui.run(api::post::<Value>("/api/notes", &body)).await {
                navigate(&format!("/notes/{}", v["id"].as_str().unwrap_or_default()), Default::default());
            }
        });
    };
    let can_create = matches!(view, View::Active | View::Secret | View::Project(_));
    let wrapper = if view.project().is_some() { "flex flex-col gap-4" } else { "flex flex-col gap-6 mx-auto max-w-5xl page-enter" };

    view! {
        <div class=wrapper>
            <PageHeader title=view.title() description=view.description()>
                <div class="relative w-full sm:w-64">
                    <Search class="absolute left-2.5 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
                    <Input class="pl-8" placeholder="Search notes" bind_value=q />
                </div>
                <SwitchArea view />
                <Show when=move || can_create>
                    <Button on:click=create.clone()>
                        <Plus />
                        "New note"
                    </Button>
                </Show>
            </PageHeader>
            <Card class="overflow-hidden gap-0 py-0">
                {move || match notes.get() {
                    None => view! { <ListSkeleton /> }.into_any(),
                    Some(list) if list.is_empty() => view! {
                        <Empty class="m-4 border-0">
                            <EmptyHeader>
                                <EmptyMedia variant=EmptyMediaVariant::Icon><FileText /></EmptyMedia>
                                <EmptyTitle>"Nothing here yet"</EmptyTitle>
                                <EmptyDescription>"Notes you create will show up in this list."</EmptyDescription>
                            </EmptyHeader>
                        </Empty>
                    }
                    .into_any(),
                    Some(list) => view! {
                        <div class="divide-y">
                            {list.into_iter().map(|n| view! { <NoteRow note=n view reload /> }).collect_view()}
                        </div>
                    }
                    .into_any(),
                }}
            </Card>
        </div>
    }
}

#[component]
fn NoteRow(note: NoteItem, view: View, reload: Callback<()>) -> impl IntoView {
    let ui = use_ui();
    let id = StoredValue::new(note.id.clone());
    let act = move |suffix: &'static str, delete: bool| {
        let path = format!("/api/notes/{}{suffix}", id.get_value());
        spawn_local(async move {
            let res = if delete { api::del(&path).await } else { api::post::<Value>(&path, &()).await };
            if ui.run(std::future::ready(res)).await.is_some() {
                reload.run(());
            }
        });
    };
    let title = if note.title.is_empty() { "Untitled".to_owned() } else { note.title };
    let shared = note.share_token.is_some();

    view! {
        <div class="flex gap-3 items-center py-3 px-4 transition-colors group hover:bg-muted/50">
            <A href=api::url(&format!("/notes/{}", note.id)) attr:class="flex flex-1 gap-3 items-center min-w-0">
                <div class="flex justify-center items-center rounded-lg transition-colors size-9 bg-muted shrink-0 group-hover:bg-background">
                    <FileText class="size-4 text-muted-foreground" />
                </div>
                <div class="flex flex-col min-w-0">
                    <span class="font-medium truncate">{title}</span>
                    <span class="text-xs text-muted-foreground">{format!("Edited {}", fmt_time(note.updated))}</span>
                </div>
            </A>
            <Show when=move || shared>
                <span class="flex gap-1 items-center text-xs text-muted-foreground">
                    <Share2 class="size-3.5" />
                    "Shared"
                </span>
            </Show>
            <Show when=move || view == View::Archived>
                <Button variant=ButtonVariant::Ghost size=ButtonSize::Sm on:click=move |_| act("/action/unarchive", false)>
                    <ArchiveRestore />
                    "Unarchive"
                </Button>
            </Show>
            <Show when=move || view == View::Trash>
                <Button variant=ButtonVariant::Ghost size=ButtonSize::Sm on:click=move |_| act("/action/restore", false)>
                    <ArchiveRestore />
                    "Restore"
                </Button>
                <Button
                    variant=ButtonVariant::Ghost
                    size=ButtonSize::Sm
                    on:click=move |_| {
                        if window().confirm_with_message("Delete this note permanently?").unwrap_or(false) {
                            act("", true);
                        }
                    }
                >
                    <Trash2 />
                    "Delete"
                </Button>
            </Show>
        </div>
    }
}
