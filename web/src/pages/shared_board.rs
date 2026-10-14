use icons::{Check, Download, Flag, ListChecks};
use leptos::prelude::*;
use leptos_router::hooks::use_params_map;
use serde::Deserialize;

use crate::api;
use crate::components::ui::badge::{Badge, BadgeVariant};
use crate::components::ui::card::Card;
use crate::components::ui::theme_toggle::ThemeToggle;
use crate::md;
use crate::pages::drive::{FileItem, Kind, KindIcon, Preview};
use crate::pages::projects::{DueChip, priority_class, priority_label};
use crate::widgets::{ListSkeleton, LogoTile, Modal, Options, Segmented, fmt_size};

const TABS: Options = &[("board", "Board"), ("files", "Files")];
const ROW: &str = "flex gap-3 items-center px-4 py-2.5 w-full text-left border-b last:border-b-0 hover:bg-muted/50";

#[derive(Clone, Deserialize)]
struct Lane {
    id: String,
    name: String,
}

#[derive(Clone, Deserialize)]
struct BoardTask {
    id: String,
    column_id: String,
    title: String,
    description: String,
    due: Option<String>,
    priority: i64,
}

#[derive(Clone, Deserialize)]
struct Sub {
    task_id: String,
    title: String,
    done: i64,
    description: String,
}

#[derive(Clone, Deserialize)]
struct SharedProject {
    name: String,
    description: String,
    columns: Vec<Lane>,
    tasks: Vec<BoardTask>,
    subtasks: Vec<Sub>,
    files: Vec<FileItem>,
}

impl SharedProject {
    fn subtasks_of(&self, task: &str) -> Vec<Sub> {
        self.subtasks.iter().filter(|s| s.task_id == task).cloned().collect()
    }

    fn is_done(&self, card: &BoardTask) -> bool {
        self.columns.last().is_some_and(|c| c.id == card.column_id)
    }
}

#[component]
pub fn SharedBoard() -> impl IntoView {
    let token = use_params_map().read_untracked().get("token").unwrap_or_default();
    let base = format!("/api/public/projects/{token}");
    let project = LocalResource::new({
        let base = base.clone();
        move || {
            let base = base.clone();
            async move { api::get::<SharedProject>(&base).await.ok() }
        }
    });
    let base = StoredValue::new(base);
    view! {
        <div class="flex flex-col gap-4 p-4 mx-auto max-w-screen-2xl md:p-8">
            <div class="flex gap-2 items-center">
                <LogoTile class="size-8" />
                <span class="font-semibold tracking-tight">"RustKeep"</span>
                <span class="flex-1 text-sm text-muted-foreground">"Shared board"</span>
                <ThemeToggle />
            </div>
            {move || match project.get() {
                None => view! { <Card class="py-0"><ListSkeleton rows=3 /></Card> }.into_any(),
                Some(None) => view! { <p class="text-muted-foreground">"This link is invalid or has expired."</p> }.into_any(),
                Some(Some(p)) => view! { <BoardView project=p base=base.get_value() /> }.into_any(),
            }}
        </div>
    }
}

#[component]
fn BoardView(project: SharedProject, base: String) -> impl IntoView {
    let tab = RwSignal::new("board".to_owned());
    let (name, description) = (project.name.clone(), project.description.clone());
    let project = StoredValue::new(project);
    view! {
        <div class="flex flex-col gap-1">
            <h1 class="text-2xl font-semibold tracking-tight">{name}</h1>
            <p class="text-sm text-muted-foreground">{description}</p>
        </div>
        <Segmented options=TABS value=tab.into() on_change=Callback::new(move |t| tab.set(t)) />
        {move || match tab.get().as_str() {
            "files" => view! { <SharedFiles files=project.with_value(|p| p.files.clone()) base=base.clone() /> }.into_any(),
            _ => view! { <Lanes project /> }.into_any(),
        }}
    }
}

#[component]
fn Lanes(project: StoredValue<SharedProject>) -> impl IntoView {
    let open = RwSignal::new(false);
    let selected = RwSignal::new(None::<BoardTask>);
    let show = Callback::new(move |card: BoardTask| {
        selected.set(Some(card));
        open.set(true);
    });
    view! {
        <div class="flex overflow-x-auto gap-4 items-start px-1 pb-4 -mx-1 snap-x snap-mandatory sm:snap-none">
            {project.with_value(|p| p.columns.iter().map(|lane| {
                let cards: Vec<BoardTask> = p.tasks.iter().filter(|t| t.column_id == lane.id).cloned().collect();
                let count = cards.len();
                view! {
                    <div class="flex flex-col gap-2 p-2 w-72 rounded-xl border shrink-0 snap-start scroll-ml-1 bg-muted/40">
                        <div class="flex gap-1 items-center px-1 min-h-8">
                            <span class="flex-1 text-sm font-semibold truncate">{lane.name.clone()}</span>
                            <Badge variant=BadgeVariant::Secondary class="tabular-nums">{count}</Badge>
                        </div>
                        {cards.into_iter().map(|card| view! { <SharedCard card project show /> }).collect_view()}
                    </div>
                }
            }).collect_view())}
        </div>
        <Modal open title="Task" class="max-w-[min(48rem,calc(100%-2rem))]">
            {move || selected.get().map(|card| view! { <TaskDetail card project /> })}
        </Modal>
    }
}

#[component]
fn SharedCard(card: BoardTask, project: StoredValue<SharedProject>, show: Callback<BoardTask>) -> impl IntoView {
    let done = project.with_value(|p| p.is_done(&card));
    let subs = project.with_value(|p| p.subtasks_of(&card.id));
    let progress = (!subs.is_empty()).then(|| format!("{}/{}", subs.iter().filter(|s| s.done == 1).count(), subs.len()));
    let description = md::plain(&card.description);
    let cover = md::first_image(&card.description);
    let priority = card.priority;
    let open_card = card.clone();
    view! {
        <button
            type="button"
            class="flex flex-col gap-2 p-3 text-sm text-left rounded-lg border shadow-xs bg-card lift"
            on:click=move |_| show.run(open_card.clone())
        >
            {cover.map(|src| view! { <img src=src alt="" loading="lazy" class="object-cover w-full h-32 rounded-md bg-muted" /> })}
            <span class="font-medium leading-snug" class=("line-through", done) class=("text-muted-foreground", done)>{card.title}</span>
            <Show when={
                let empty = description.is_empty();
                move || !empty
            }>
                <p class="text-xs text-muted-foreground line-clamp-3">{description.clone()}</p>
            </Show>
            <div class="flex flex-wrap gap-2 items-center text-xs text-muted-foreground">
                <Show when=move || priority != 0>
                    <span class=format!("inline-flex gap-1 items-center py-0.5 px-1.5 rounded-md {}", priority_class(priority))>
                        <Flag class="size-3" />
                        {priority_label(priority)}
                    </span>
                </Show>
                {card.due.map(|d| view! { <DueChip due=d done /> })}
                {progress.map(|s| view! {
                    <span class="inline-flex gap-1 items-center py-0.5 px-1.5 rounded-md bg-muted">
                        <ListChecks class="size-3" />
                        {s}
                    </span>
                })}
            </div>
        </button>
    }
}

#[component]
fn TaskDetail(card: BoardTask, project: StoredValue<SharedProject>) -> impl IntoView {
    let done = project.with_value(|p| p.is_done(&card));
    let subs = project.with_value(|p| p.subtasks_of(&card.id));
    let priority = card.priority;
    view! {
        <div class="flex flex-col gap-4 min-w-0">
            <h2 class="text-xl font-semibold leading-snug">{card.title}</h2>
            <div class="flex flex-wrap gap-2 items-center text-xs text-muted-foreground">
                <Show when=move || priority != 0>
                    <span class=format!("inline-flex gap-1 items-center py-0.5 px-1.5 rounded-md {}", priority_class(priority))>
                        <Flag class="size-3" />
                        {priority_label(priority)}
                    </span>
                </Show>
                {card.due.map(|d| view! { <DueChip due=d done /> })}
            </div>
            <Show when={
                let empty = card.description.trim().is_empty();
                move || !empty
            }>
                <div class="p-4 rounded-xl border md bg-card" inner_html=md::render(&card.description) />
            </Show>
            {(!subs.is_empty()).then(|| view! {
                <div class="flex flex-col gap-2">
                    <span class="text-sm font-medium">"Subtasks"</span>
                    {subs.into_iter().map(|s| view! { <SubtaskItem sub=s /> }).collect_view()}
                </div>
            })}
        </div>
    }
}

#[component]
fn SubtaskItem(sub: Sub) -> impl IntoView {
    let done = sub.done == 1;
    let details = (!sub.description.trim().is_empty()).then(|| md::render(&sub.description));
    view! {
        <div class="flex flex-col gap-2 py-2 px-3 rounded-lg border bg-card">
            <div class="flex gap-2 items-center text-sm">
                <span class="flex justify-center items-center rounded-sm border size-4 shrink-0" class=("bg-primary", done) class=("text-primary-foreground", done)>
                    <Show when=move || done><Check class="size-3" /></Show>
                </span>
                <span class=("line-through", done) class=("text-muted-foreground", done)>{sub.title}</span>
            </div>
            {details.map(|html| view! { <div class="text-sm md" inner_html=html /> })}
        </div>
    }
}

#[component]
fn SharedFiles(files: Vec<FileItem>, base: String) -> impl IntoView {
    let files = Signal::stored(files);
    let preview = RwSignal::new(None::<String>);
    let raw = StoredValue::new(move |id: &str| api::url(&format!("{base}/files/{id}/raw")));
    view! {
        <Card class="overflow-hidden gap-0 py-0">
            {move || files.get().into_iter().map(|f| {
                let id = f.id.clone();
                view! {
                    <div class=ROW>
                        <button type="button" class="flex flex-1 gap-3 items-center min-w-0 text-left" on:click=move |_| preview.set(Some(id.clone()))>
                            <KindIcon kind=Kind::of(&f) class="size-5 text-muted-foreground shrink-0" />
                            <span class="text-sm font-medium truncate">{f.name.clone()}</span>
                        </button>
                        <span class="text-sm tabular-nums text-muted-foreground shrink-0">{fmt_size(f.size)}</span>
                        <a href=raw.with_value(|r| r(&f.id)) download=f.name.clone() title="Download" class="flex justify-center items-center rounded-md size-8 hover:bg-accent shrink-0">
                            <Download class="size-4" />
                        </a>
                    </div>
                }
            }).collect_view()}
            <Show when=move || files.with(Vec::is_empty)>
                <p class="p-8 text-sm text-center text-muted-foreground">"No files in this project."</p>
            </Show>
        </Card>
        <Preview files current=preview src=Callback::new(move |id: String| raw.with_value(|r| r(&id))) />
    }
}
