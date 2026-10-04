use std::cmp::Reverse;

use icons::{Archive, ArchiveRestore, FileText, Pin, PinOff, Plus, Share2, Trash2};
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::A;
use leptos_router::hooks::use_navigate;
use serde::Deserialize;
use serde_json::json;

use crate::api;
use crate::id::Id;
use crate::components::ui::badge::{Badge, BadgeVariant};
use crate::components::ui::button::{Button, ButtonSize, ButtonVariant};
use crate::components::ui::card::Card;
use crate::components::ui::empty::{Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant, EmptyTitle};
use crate::widgets::{
    ListSkeleton, Options, PageHeader, PinGate, SearchBox, Segmented, SortSelect, Toolbar, create_note, fmt_time,
    query_state, use_ui,
};

#[derive(Clone, Copy, PartialEq)]
pub enum View {
    Active,
    Archived,
    Trash,
    Secret,
    Project(Id),
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
            View::Archived => "Archived notes",
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

    fn project(self) -> Option<Id> {
        if let View::Project(id) = self { Some(id) } else { None }
    }

    fn key(self) -> &'static str {
        match self {
            View::Archived => "archived",
            View::Trash => "trash",
            View::Secret => "secret",
            _ => "active",
        }
    }
}

const VIEWS: Options = &[("active", "Notes"), ("archived", "Archived"), ("trash", "Trash"), ("secret", "Secret")];
const SHOW: Options = &[("all", "All"), ("pinned", "Pinned"), ("shared", "Shared")];
const SORTS: Options =
    &[("updated", "Last edited"), ("oldest", "Oldest edit"), ("title", "Title A-Z"), ("title_desc", "Title Z-A")];

#[derive(Clone, Deserialize)]
pub struct NoteItem {
    pub id: String,
    pub title: String,
    pub updated: i64,
    pub share_token: Option<String>,
    #[serde(default)]
    pub archived: i64,
    #[serde(default)]
    pub pinned: i64,
}

fn view_href(key: &str) -> &'static str {
    match key {
        "archived" => "/archive?tab=notes",
        "trash" => "/trash",
        "secret" => "/secret",
        _ => "/notes",
    }
}

fn arrange(mut list: Vec<NoteItem>, show: &str, sort: &str) -> Vec<NoteItem> {
    list.retain(|n| match show {
        "pinned" => n.pinned == 1,
        "shared" => n.share_token.is_some(),
        _ => true,
    });
    match sort {
        "oldest" => list.sort_by_key(|n| n.updated),
        "title" => list.sort_by_key(|n| n.title.to_lowercase()),
        "title_desc" => list.sort_by_key(|n| Reverse(n.title.to_lowercase())),
        _ => list.sort_by_key(|n| Reverse(n.updated)),
    }
    list.sort_by_key(|n| Reverse(n.pinned));
    list
}

#[component]
pub fn NotesList(view: View, #[prop(optional)] embedded: bool) -> impl IntoView {
    if view == View::Secret {
        view! { <PinGate><NotesInner view embedded /></PinGate> }.into_any()
    } else {
        view! { <NotesInner view embedded /> }.into_any()
    }
}

#[component]
fn NotesInner(view: View, embedded: bool) -> impl IntoView {
    let ui = use_ui();
    let navigate = use_navigate();
    let q = RwSignal::new(String::new());
    let (show, set_show) = query_state("show", "all");
    let (sort, set_sort) = query_state("sort", "updated");
    let notes = LocalResource::new(move || {
        let path = format!("/api/notes?{}&q={}", view.query(), js_sys::encode_uri_component(&q.get()));
        async move { ui.run(api::get::<Vec<NoteItem>>(&path)).await }
    });
    let reload = Callback::new(move |_| notes.refetch());
    let nav = navigate.clone();
    let create = Callback::new(move |_| create_note(ui, nav.clone(), json!({ "secret": view == View::Secret, "project_id": view.project() })));
    let switch = Callback::new(move |key: String| navigate(view_href(&key), Default::default()));
    let can_create = matches!(view, View::Active | View::Secret | View::Project(_));
    let standalone = !embedded && view.project().is_none();
    let wrapper = if standalone { "flex flex-col gap-6 mx-auto max-w-5xl page-enter" } else { "flex flex-col gap-4" };
    let rows = move || notes.get().flatten().map(|list| arrange(list, &show.get(), &sort.get()));

    view! {
        <div class=wrapper>
            <Show when=move || standalone>
                <PageHeader title=view.title() description=view.description()>
                    <Show when=move || can_create>
                        <Button on:click=move |_| create.run(()) attr:title="New note">
                            <Plus />
                            "New note"
                        </Button>
                    </Show>
                </PageHeader>
                <Segmented options=VIEWS value=Signal::derive(move || view.key().to_owned()) on_change=switch />
            </Show>
            <Toolbar>
                <div class="flex flex-wrap flex-1 gap-2 items-center min-w-0 max-w-full">
                    <SearchBox value=q placeholder="Search notes  /" />
                    <Show when=move || view != View::Trash>
                        <Segmented options=SHOW value=show on_change=set_show />
                    </Show>
                </div>
                <div class="flex gap-2 items-center">
                    <SortSelect options=SORTS value=sort on_change=set_sort />
                    <Show when=move || can_create && !standalone>
                        <Button on:click=move |_| create.run(())>
                            <Plus />
                            "New note"
                        </Button>
                    </Show>
                </div>
            </Toolbar>
            <Card class="overflow-hidden gap-0 py-0">
                {move || match rows() {
                    None => view! { <ListSkeleton /> }.into_any(),
                    Some(list) if list.is_empty() => view! {
                        <Empty class="m-4 border-0">
                            <EmptyHeader>
                                <EmptyMedia variant=EmptyMediaVariant::Icon><FileText /></EmptyMedia>
                                <EmptyTitle>"Nothing here"</EmptyTitle>
                                <EmptyDescription>"No notes match this view. Try another filter or create a note."</EmptyDescription>
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
fn RowButton(#[prop(into)] title: String, on_click: Callback<()>, children: Children) -> impl IntoView {
    view! {
        <Button variant=ButtonVariant::Ghost size=ButtonSize::IconSm attr:title=title on:click=move |_| on_click.run(())>
            {children()}
        </Button>
    }
}

#[component]
fn NoteRow(note: NoteItem, view: View, reload: Callback<()>) -> impl IntoView {
    let ui = use_ui();
    let id = StoredValue::new(note.id.clone());
    let act = move |action: &'static str| {
        Callback::new(move |_| ui.act(format!("/api/notes/{}/action/{action}", id.get_value()), move || reload.run(())))
    };
    let purge = Callback::new(move |_| {
        if !window().confirm_with_message("Delete this note permanently?").unwrap_or(false) {
            return;
        }
        spawn_local(async move {
            if ui.run(api::del(&format!("/api/notes/{}", id.get_value()))).await.is_some() {
                reload.run(());
            }
        });
    });
    let title = if note.title.is_empty() { "Untitled".to_owned() } else { note.title };
    let shared = note.share_token.is_some();
    let pinned = note.pinned == 1;
    let archived = note.archived == 1;
    let live = matches!(view, View::Active | View::Project(_) | View::Archived);
    let in_list = matches!(view, View::Active | View::Project(_));

    view! {
        <div class="flex gap-3 items-center py-3 px-4 transition-colors group hover:bg-muted/50">
            <A href=api::url(&format!("/notes/{}", note.id)) attr:class="flex flex-1 gap-3 items-center min-w-0">
                <div class="flex justify-center items-center rounded-lg transition-colors size-9 bg-muted shrink-0 group-hover:bg-background">
                    <FileText class="size-4 text-muted-foreground" />
                </div>
                <div class="flex flex-col min-w-0">
                    <span class="flex gap-1.5 items-center font-medium">
                        <Show when=move || pinned>
                            <Pin class="size-3.5 text-primary shrink-0" />
                        </Show>
                        <span class="truncate">{title}</span>
                    </span>
                    <span class="text-xs text-muted-foreground">{format!("Edited {}", fmt_time(note.updated))}</span>
                </div>
            </A>
            <Show when=move || shared>
                <span class="hidden gap-1 items-center text-xs sm:flex text-muted-foreground">
                    <Share2 class="size-3.5" />
                    "Shared"
                </span>
            </Show>
            <Show when=move || archived && view != View::Archived>
                <Badge variant=BadgeVariant::Secondary>"Archived"</Badge>
            </Show>
            <div class="flex gap-0.5 items-center opacity-70 transition-opacity group-hover:opacity-100">
                <Show when=move || live>
                    <RowButton title=if pinned { "Unpin" } else { "Pin to top" } on_click=act(if pinned { "unpin" } else { "pin" })>
                        {if pinned { view! { <PinOff /> }.into_any() } else { view! { <Pin /> }.into_any() }}
                    </RowButton>
                </Show>
                <Show when=move || in_list && !archived>
                    <RowButton title="Archive" on_click=act("archive")><Archive /></RowButton>
                </Show>
                <Show when=move || live && archived>
                    <RowButton title="Unarchive" on_click=act("unarchive")><ArchiveRestore /></RowButton>
                </Show>
                <Show when=move || live>
                    <RowButton title="Move to trash" on_click=act("trash")><Trash2 /></RowButton>
                </Show>
                <Show when=move || view == View::Trash>
                    <Button variant=ButtonVariant::Ghost size=ButtonSize::Sm on:click=move |_| act("restore").run(())>
                        <ArchiveRestore />
                        "Restore"
                    </Button>
                    <Button variant=ButtonVariant::Ghost size=ButtonSize::Sm on:click=move |_| purge.run(())>
                        <Trash2 />
                        "Delete"
                    </Button>
                </Show>
            </div>
        </div>
    }
}
