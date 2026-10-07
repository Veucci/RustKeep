use icons::{ArrowRight, Bell, CalendarClock, Flag, Folder, FolderKanban, Notebook, Pin, Plus, SquareKanban, TriangleAlert, Upload};
use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::use_navigate;
use serde::Deserialize;
use serde_json::json;

use crate::api;
use crate::id::Id;
use crate::components::ui::button::{Button, ButtonSize, ButtonVariant};
use crate::components::ui::card::{Card, CardContent, CardDescription, CardHeader, CardTitle};
use crate::components::ui::progress::Progress;
use crate::components::ui::skeleton::Skeleton;
use crate::pages::drive::FileItem;
use crate::pages::notes::NoteItem;
use crate::pages::projects::{ProjectItem, percent};
use crate::pages::reminders::{Reminder, ReminderLink};
use crate::widgets::{ListSkeleton, PageHeader, create_note, days_until, due_label, fmt_size, fmt_time, use_ui};

#[derive(Clone, Deserialize)]
struct Stats {
    notes: i64,
    archived: i64,
    trash: i64,
    files: i64,
    storage: i64,
    reminders: i64,
    vault: i64,
    overdue: i64,
}

#[derive(Clone, Deserialize)]
struct DueTask {
    id: Id,
    title: String,
    due: String,
    priority: i64,
    project_id: Id,
    project: String,
}

#[derive(Clone, Deserialize)]
struct Summary {
    stats: Stats,
    notes: Vec<NoteItem>,
    reminders: Vec<Reminder>,
    files: Vec<FileItem>,
    projects: Vec<ProjectItem>,
    tasks: Vec<DueTask>,
}

#[component]
fn StatCard(
    #[prop(into)] title: String,
    value: String,
    #[prop(into)] hint: String,
    href: &'static str,
    #[prop(optional)] alert: bool,
    children: Children,
) -> impl IntoView {
    view! {
        <A href=api::url(href) attr:class="block group">
            <Card class="gap-2 py-4 h-full sm:py-6 lift">
                <CardHeader class="flex flex-row justify-between items-center px-4 sm:flex sm:px-6">
                    <CardDescription class="font-medium text-foreground">{title}</CardDescription>
                    <div class="text-muted-foreground [&_svg]:size-4 group-hover:text-foreground">{children()}</div>
                </CardHeader>
                <CardContent class="flex flex-col gap-1 px-4 sm:px-6">
                    <span class="text-3xl font-semibold tracking-tight tabular-nums">{value}</span>
                    <span class="text-xs" class=("text-destructive", alert) class=("text-muted-foreground", !alert)>{hint}</span>
                </CardContent>
            </Card>
        </A>
    }
}

#[component]
fn Panel(#[prop(into)] title: String, #[prop(into)] description: String, href: &'static str, children: Children) -> impl IntoView {
    view! {
        <Card class="gap-4 h-full">
            <CardHeader class="flex flex-row justify-between items-start sm:flex">
                <div class="flex flex-col gap-1.5">
                    <CardTitle>
                        <A href=api::url(href) attr:class="hover:underline underline-offset-4">{title}</A>
                    </CardTitle>
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

fn empty_line(text: &'static str, href: &'static str, action: &'static str) -> AnyView {
    view! {
        <div class="flex flex-col gap-2 items-center py-6 text-sm text-center text-muted-foreground">
            <p>{text}</p>
            <A href=api::url(href) attr:class="font-medium underline text-foreground underline-offset-4">{action}</A>
        </div>
    }
    .into_any()
}

const ROW: &str = "flex gap-3 items-center p-2 -mx-2 rounded-lg transition-colors hover:bg-accent";

fn stats_view(s: Stats, projects: &[ProjectItem]) -> impl IntoView {
    let open: i64 = projects.iter().map(|p| p.tasks - p.done).sum();
    let task_hint = if s.overdue > 0 { format!("{open} open tasks, {} overdue", s.overdue) } else { format!("{open} open tasks") };
    view! {
        <StatCard title="Notes" value=s.notes.to_string() hint=format!("{} archived, {} in trash", s.archived, s.trash) href="/notes">
            <Notebook />
        </StatCard>
        <StatCard title="Projects" value=projects.len().to_string() hint=task_hint href="/projects" alert={s.overdue > 0}>
            <SquareKanban />
        </StatCard>
        <StatCard title="Files" value=s.files.to_string() hint=format!("{} stored", fmt_size(s.storage)) href="/files">
            <Folder />
        </StatCard>
        <StatCard title="Reminders" value=s.reminders.to_string() hint=format!("{} secrets in vault", s.vault) href="/reminders">
            <Bell />
        </StatCard>
    }
}

fn due_class(days: i64) -> &'static str {
    match days {
        d if d < 0 => "text-destructive",
        0..=2 => "text-foreground font-medium",
        _ => "text-muted-foreground",
    }
}

fn tasks_view(items: Vec<DueTask>) -> AnyView {
    if items.is_empty() {
        return empty_line("No tasks due in the next two weeks.", "/projects", "Open projects");
    }
    items
        .into_iter()
        .map(|t| {
            let days = days_until(&t.due);
            let icon = if days < 0 { view! { <TriangleAlert class="size-4 text-destructive" /> }.into_any() } else { view! { <CalendarClock class="size-4 text-muted-foreground" /> }.into_any() };
            view! {
                <A href=api::url(&format!("/projects/{}?task={}", t.project_id, t.id)) attr:class=ROW>
                    <div class="flex justify-center items-center rounded-md size-8 bg-muted shrink-0">{icon}</div>
                    <div class="flex flex-col flex-1 min-w-0">
                        <span class="flex gap-1.5 items-center text-sm font-medium">
                            <span class="truncate">{t.title}</span>
                            <Show when=move || t.priority == 3>
                                <Flag class="size-3.5 text-destructive shrink-0" />
                            </Show>
                        </span>
                        <span class="text-xs truncate text-muted-foreground">{t.project}</span>
                    </div>
                    <span class=format!("text-xs shrink-0 {}", due_class(days)) title=t.due.clone()>{due_label(days)}</span>
                </A>
            }
        })
        .collect_view()
        .into_any()
}

fn notes_view(notes: Vec<NoteItem>) -> AnyView {
    if notes.is_empty() {
        return empty_line("No notes yet.", "/notes", "Go to notes");
    }
    notes
        .into_iter()
        .map(|n| {
            let title = if n.title.is_empty() { "Untitled".to_owned() } else { n.title };
            let pinned = n.pinned == 1;
            view! {
                <A href=api::url(&format!("/notes/{}", n.id)) attr:class=ROW>
                    <div class="flex justify-center items-center rounded-md size-8 bg-muted shrink-0">
                        {if pinned { view! { <Pin class="size-4 text-primary" /> }.into_any() } else { view! { <Notebook class="size-4 text-muted-foreground" /> }.into_any() }}
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
        return empty_line("Nothing scheduled.", "/reminders?new=1", "Add a reminder");
    }
    items.into_iter().map(|r| view! { <ReminderLink reminder=r /> }).collect_view().into_any()
}

fn projects_view(items: Vec<ProjectItem>) -> AnyView {
    if items.is_empty() {
        return empty_line("No projects yet.", "/projects?new=1", "Create a project");
    }
    items
        .into_iter()
        .take(5)
        .map(|p| {
            let pct = percent(&p);
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
        return empty_line("No files uploaded.", "/files", "Upload a file");
    }
    items
        .into_iter()
        .map(|f| {
            view! {
                <a href=api::url(&format!("/api/files/{}/raw", f.id)) target="_blank" class=ROW>
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
    let new_note = move |_| create_note(ui, navigate.clone(), json!({}));
    let data = move || summary.get().flatten();
    let loading = |rows: usize| view! { <ListSkeleton rows /> }.into_any();

    view! {
        <div class="flex flex-col gap-6 mx-auto max-w-7xl page-enter">
            <PageHeader title=greeting description="Here is what is happening in your workspace.">
                <Button variant=ButtonVariant::Outline href=api::url("/reminders?new=1")>
                    <Bell />
                    "Reminder"
                </Button>
                <Button variant=ButtonVariant::Outline href=api::url("/files")>
                    <Upload />
                    "Upload"
                </Button>
                <Button variant=ButtonVariant::Outline href=api::url("/projects?new=1")>
                    <FolderKanban />
                    "Project"
                </Button>
                <Button on:click=new_note>
                    <Plus />
                    "New note"
                </Button>
            </PageHeader>
            <div class="grid grid-cols-2 gap-3 sm:gap-4 xl:grid-cols-4">
                {move || match data() {
                    Some(s) => stats_view(s.stats, &s.projects).into_any(),
                    None => (0..4).map(|_| view! { <Skeleton class="h-32 rounded-xl" /> }).collect_view().into_any(),
                }}
            </div>
            <div class="grid grid-cols-1 gap-4 lg:grid-cols-7">
                <div class="lg:col-span-4">
                    <Panel title="Upcoming tasks" description="Overdue and due within 14 days, across all boards" href="/projects">
                        {move || data().map_or_else(|| loading(4), |s| tasks_view(s.tasks))}
                    </Panel>
                </div>
                <div class="lg:col-span-3">
                    <Panel title="Upcoming reminders" description="Delivered by email" href="/reminders">
                        {move || data().map_or_else(|| loading(3), |s| reminders_view(s.reminders))}
                    </Panel>
                </div>
            </div>
            <div class="grid grid-cols-1 gap-4 lg:grid-cols-3">
                <Panel title="Notes" description="Pinned and recently edited" href="/notes">
                    {move || data().map_or_else(|| loading(4), |s| notes_view(s.notes))}
                </Panel>
                <Panel title="Projects" description="Progress across your boards" href="/projects">
                    {move || data().map_or_else(|| loading(3), |s| projects_view(s.projects))}
                </Panel>
                <Panel title="Recent files" description="Latest uploads" href="/files">
                    {move || data().map_or_else(|| loading(3), |s| files_view(s.files))}
                </Panel>
            </div>
        </div>
    }
}
