use icons::{ArrowRight, Bell, FileText, Folder, Notebook, Plus, SquareKanban};
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::A;
use leptos_router::hooks::use_navigate;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::api;
use crate::components::ui::button::{Button, ButtonSize, ButtonVariant};
use crate::components::ui::card::{Card, CardContent, CardDescription, CardHeader, CardTitle};
use crate::components::ui::progress::Progress;
use crate::components::ui::skeleton::Skeleton;
use crate::pages::files::FileItem;
use crate::pages::notes::NoteItem;
use crate::pages::projects::ProjectItem;
use crate::pages::reminders::Reminder;
use crate::widgets::{ListSkeleton, PageHeader, fmt_size, fmt_time, use_ui};

#[derive(Clone, Deserialize)]
struct Stats {
    notes: i64,
    archived: i64,
    trash: i64,
    files: i64,
    storage: i64,
    reminders: i64,
    vault: i64,
}

#[derive(Clone, Deserialize)]
struct Summary {
    stats: Stats,
    notes: Vec<NoteItem>,
    reminders: Vec<Reminder>,
    files: Vec<FileItem>,
    projects: Vec<ProjectItem>,
}

#[component]
fn StatCard(#[prop(into)] title: String, value: String, #[prop(into)] hint: String, children: Children) -> impl IntoView {
    view! {
        <Card class="gap-2 lift">
            <CardHeader class="flex flex-row justify-between items-center sm:flex">
                <CardDescription class="font-medium text-foreground">{title}</CardDescription>
                <div class="text-muted-foreground [&_svg]:size-4">{children()}</div>
            </CardHeader>
            <CardContent class="flex flex-col gap-1">
                <span class="text-3xl font-semibold tracking-tight tabular-nums">{value}</span>
                <span class="text-xs text-muted-foreground">{hint}</span>
            </CardContent>
        </Card>
    }
}

#[component]
fn Panel(#[prop(into)] title: String, #[prop(into)] description: String, href: &'static str, children: Children) -> impl IntoView {
    view! {
        <Card class="gap-4">
            <CardHeader class="flex flex-row justify-between items-start sm:flex">
                <div class="flex flex-col gap-1.5">
                    <CardTitle>{title}</CardTitle>
                    <CardDescription>{description}</CardDescription>
                </div>
                <Button variant=ButtonVariant::Ghost size=ButtonSize::Sm href=api::url(href)>
                    "View all"
                    <ArrowRight />
                </Button>
            </CardHeader>
            <CardContent class="flex flex-col">{children()}</CardContent>
        </Card>
    }
}

fn empty_line(text: &'static str) -> AnyView {
    view! { <p class="py-6 text-sm text-center text-muted-foreground">{text}</p> }.into_any()
}

fn stats_view(s: Stats, projects: &[ProjectItem]) -> impl IntoView {
    let open: i64 = projects.iter().map(|p| p.tasks - p.done).sum();
    view! {
        <StatCard title="Notes" value=s.notes.to_string() hint=format!("{} archived, {} in trash", s.archived, s.trash)>
            <Notebook />
        </StatCard>
        <StatCard title="Projects" value=projects.len().to_string() hint=format!("{open} open tasks")>
            <SquareKanban />
        </StatCard>
        <StatCard title="Files" value=s.files.to_string() hint=format!("{} stored", fmt_size(s.storage))>
            <Folder />
        </StatCard>
        <StatCard title="Reminders" value=s.reminders.to_string() hint=format!("{} secrets in vault", s.vault)>
            <Bell />
        </StatCard>
    }
}

fn notes_view(notes: Vec<NoteItem>) -> AnyView {
    if notes.is_empty() {
        return empty_line("No notes yet.");
    }
    notes
        .into_iter()
        .map(|n| {
            let title = if n.title.is_empty() { "Untitled".to_owned() } else { n.title };
            view! {
                <A href=api::url(&format!("/notes/{}", n.id)) attr:class="flex gap-3 items-center p-2 -mx-2 rounded-lg transition-colors hover:bg-accent">
                    <div class="flex justify-center items-center rounded-md size-8 bg-muted shrink-0">
                        <FileText class="size-4 text-muted-foreground" />
                    </div>
                    <span class="flex-1 text-sm font-medium truncate">{title}</span>
                    <span class="text-xs text-muted-foreground shrink-0">{fmt_time(n.updated)}</span>
                </A>
            }
        })
        .collect_view()
        .into_any()
}

fn reminders_view(items: Vec<Reminder>) -> AnyView {
    if items.is_empty() {
        return empty_line("Nothing scheduled.");
    }
    items
        .into_iter()
        .map(|r| {
            view! {
                <div class="flex gap-3 items-center py-2">
                    <div class="flex justify-center items-center rounded-md size-8 bg-muted shrink-0">
                        <Bell class="size-4 text-muted-foreground" />
                    </div>
                    <div class="flex flex-col min-w-0">
                        <span class="text-sm font-medium truncate">{r.title}</span>
                        <span class="text-xs text-muted-foreground">{fmt_time(r.remind_at)}</span>
                    </div>
                </div>
            }
        })
        .collect_view()
        .into_any()
}

fn projects_view(items: Vec<ProjectItem>) -> AnyView {
    if items.is_empty() {
        return empty_line("No projects yet.");
    }
    items
        .into_iter()
        .take(5)
        .map(|p| {
            let pct = if p.tasks == 0 { 0.0 } else { p.done as f64 * 100.0 / p.tasks as f64 };
            view! {
                <A href=api::url(&format!("/projects/{}", p.id)) attr:class="flex flex-col gap-2 p-2 -mx-2 rounded-lg transition-colors hover:bg-accent">
                    <div class="flex justify-between text-sm">
                        <span class="font-medium truncate">{p.name}</span>
                        <span class="text-muted-foreground tabular-nums">{format!("{}/{}", p.done, p.tasks)}</span>
                    </div>
                    <Progress value=pct />
                </A>
            }
        })
        .collect_view()
        .into_any()
}

fn files_view(items: Vec<FileItem>) -> AnyView {
    if items.is_empty() {
        return empty_line("No files uploaded.");
    }
    items
        .into_iter()
        .map(|f| {
            view! {
                <a
                    href=api::url(&format!("/api/files/{}/raw", f.id))
                    target="_blank"
                    class="flex gap-3 items-center p-2 -mx-2 rounded-lg transition-colors hover:bg-accent"
                >
                    <div class="flex justify-center items-center rounded-md size-8 bg-muted shrink-0">
                        <Folder class="size-4 text-muted-foreground" />
                    </div>
                    <span class="flex-1 text-sm font-medium truncate">{f.name}</span>
                    <span class="text-xs text-muted-foreground shrink-0">{fmt_size(f.size)}</span>
                </a>
            }
        })
        .collect_view()
        .into_any()
}

#[component]
pub fn Dashboard() -> impl IntoView {
    let ui = use_ui();
    let navigate = use_navigate();
    let summary = LocalResource::new(move || async move { ui.run(api::get::<Summary>("/api/dashboard")).await });
    let greeting = move || ui.me.with(|m| m.as_ref().map(|m| format!("Welcome back, {}", m.name)).unwrap_or_else(|| "Welcome back".into()));
    let new_note = move |_| {
        let navigate = navigate.clone();
        spawn_local(async move {
            if let Some(v) = ui.run(api::post::<Value>("/api/notes", &json!({}))).await {
                navigate(&format!("/notes/{}", v["id"].as_str().unwrap_or_default()), Default::default());
            }
        });
    };
    let data = move || summary.get().flatten();

    view! {
        <div class="flex flex-col gap-6 mx-auto max-w-7xl page-enter">
            <PageHeader title=greeting description="Here is what is happening in your workspace.">
                <Button variant=ButtonVariant::Outline href=api::url("/files")>
                    <Folder />
                    "Upload"
                </Button>
                <Button on:click=new_note>
                    <Plus />
                    "New note"
                </Button>
            </PageHeader>
            <div class="grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
                {move || match data() {
                    Some(s) => stats_view(s.stats, &s.projects).into_any(),
                    None => (0..4).map(|_| view! { <Skeleton class="h-32 rounded-xl" /> }).collect_view().into_any(),
                }}
            </div>
            <div class="grid gap-4 lg:grid-cols-7">
                <div class="lg:col-span-4">
                    <Panel title="Recent notes" description="Your latest edits" href="/notes">
                        {move || data().map_or_else(|| view! { <ListSkeleton /> }.into_any(), |s| notes_view(s.notes))}
                    </Panel>
                </div>
                <div class="lg:col-span-3">
                    <Panel title="Upcoming reminders" description="Delivered by email" href="/reminders">
                        {move || data().map_or_else(|| view! { <ListSkeleton rows=3 /> }.into_any(), |s| reminders_view(s.reminders))}
                    </Panel>
                </div>
            </div>
            <div class="grid gap-4 lg:grid-cols-2">
                <Panel title="Projects" description="Progress across your boards" href="/projects">
                    {move || data().map_or_else(|| view! { <ListSkeleton rows=3 /> }.into_any(), |s| projects_view(s.projects))}
                </Panel>
                <Panel title="Recent files" description="Latest uploads" href="/files">
                    {move || data().map_or_else(|| view! { <ListSkeleton rows=3 /> }.into_any(), |s| files_view(s.files))}
                </Panel>
            </div>
        </div>
    }
}
