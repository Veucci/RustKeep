use std::cmp::Reverse;

use icons::{
    Archive, ArchiveRestore, ArrowLeft, Calendar, ChevronLeft, ChevronRight, Download, Flag, FolderKanban, ListChecks,
    Pencil, Plus, Trash2,
};
use leptos::ev::SubmitEvent;
use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::A;
use leptos_router::hooks::{use_navigate, use_params_map};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use web_sys::DragEvent;

use crate::api::{self, AutoSave};
use crate::id::Id;
use crate::components::ui::badge::{Badge, BadgeVariant};
use crate::components::ui::button::{Button, ButtonSize, ButtonVariant};
use crate::components::ui::checkbox::Checkbox;
use crate::components::ui::card::{Card, CardContent, CardDescription, CardHeader, CardTitle};
use crate::components::ui::date_picker::DatePicker;
use crate::components::ui::empty::{Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant, EmptyTitle};
use crate::components::ui::input::Input;
use crate::components::ui::label::Label;
use crate::components::ui::progress::Progress;
use crate::components::ui::skeleton::Skeleton;
use crate::components::ui::textarea::Textarea;
use crate::md;
use crate::pages::editor::RichEditor;
use crate::pages::files::FileManager;
use crate::pages::notes::{NotesList, View};
use crate::widgets::{
    Choice, Modal, Options, PageHeader, SearchBox, Segmented, SortSelect, Toolbar, Ui, days_until, due_label,
    fmt_date, focus_if_new, has, owned, query_state, use_ui,
};

const STATUSES: [(&str, &str); 3] = [("active", "Active"), ("paused", "Paused"), ("completed", "Completed")];
const PRIORITIES: [(i64, &str); 4] = [(0, "No priority"), (1, "Low"), (2, "Medium"), (3, "High")];
const SHOW: Options =
    &[("all", "All"), ("active", "Active"), ("paused", "Paused"), ("completed", "Completed"), ("archived", "Archived")];
const SORTS: Options =
    &[("newest", "Newest"), ("oldest", "Oldest"), ("name", "Name A-Z"), ("progress", "Most progress"), ("open", "Most open tasks")];
const TABS: Options = &[("board", "Board"), ("notes", "Notes"), ("files", "Files")];
const DUE: Options = &[("all", "All tasks"), ("overdue", "Overdue"), ("soon", "Due in 7 days"), ("nodate", "No date")];
const ORDER: Options = &[("manual", "Manual order"), ("due", "Due date"), ("priority", "Priority"), ("newest", "Newest")];
const PAGE: usize = 20;

#[derive(Clone, Deserialize)]
pub struct ProjectItem {
    pub id: Id,
    pub name: String,
    pub description: String,
    pub status: String,
    pub tasks: i64,
    pub done: i64,
    #[serde(default)]
    pub created: i64,
    #[serde(default)]
    pub archived: i64,
}

#[derive(Clone, Deserialize)]
struct Column {
    id: Id,
    name: String,
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct Task {
    id: Id,
    column_id: Id,
    title: String,
    description: String,
    due: Option<String>,
    #[serde(default)]
    priority: i64,
    #[serde(default, skip_serializing)]
    created: i64,
    #[serde(default, skip_serializing)]
    subtasks: i64,
    #[serde(default, skip_serializing)]
    subtasks_done: i64,
}

#[derive(Clone, Deserialize)]
struct Subtask {
    id: Id,
    title: String,
    done: i64,
    #[serde(default)]
    description: String,
    due: Option<String>,
    #[serde(default)]
    priority: i64,
}

fn status_label(s: &str) -> &'static str {
    STATUSES.iter().find(|(k, _)| *k == s).map_or("Active", |(_, l)| *l)
}

pub fn percent(p: &ProjectItem) -> f64 {
    if p.tasks == 0 { 0.0 } else { p.done as f64 * 100.0 / p.tasks as f64 }
}

fn arrange(mut list: Vec<ProjectItem>, q: &str, show: &str, sort: &str) -> Vec<ProjectItem> {
    list.retain(|p| {
        let shown = match show {
            "all" => p.archived == 0,
            "archived" => p.archived == 1,
            status => p.archived == 0 && p.status == status,
        };
        shown && (has(&p.name, q) || has(&p.description, q))
    });
    match sort {
        "oldest" => list.sort_by_key(|p| p.created),
        "name" => list.sort_by_key(|p| p.name.to_lowercase()),
        "progress" => list.sort_by(|a, b| percent(b).total_cmp(&percent(a))),
        "open" => list.sort_by_key(|p| Reverse(p.tasks - p.done)),
        _ => list.sort_by_key(|p| Reverse(p.created)),
    }
    list
}

#[component]
pub fn Projects() -> impl IntoView {
    view! {
        <div class="flex flex-col gap-6 mx-auto max-w-6xl page-enter">
            <PageHeader title="Projects" description="Boards, notes and files grouped by project." />
            <NewProject />
            <ProjectList />
        </div>
    }
}

#[component]
fn NewProject() -> impl IntoView {
    let ui = use_ui();
    let navigate = use_navigate();
    let name = RwSignal::new(String::new());
    let description = RwSignal::new(String::new());
    let submit = move |ev: SubmitEvent| {
        ev.prevent_default();
        let navigate = navigate.clone();
        spawn_local(async move {
            let body = json!({ "name": name.get_untracked(), "description": description.get_untracked() });
            if let Some(v) = ui.run(api::post::<Value>("/api/projects", &body)).await {
                navigate(&format!("/projects/{}", v["id"].as_str().unwrap_or_default()), Default::default());
            }
        });
    };
    focus_if_new();

    view! {
        <Card>
            <CardContent>
                <form class="flex flex-col gap-2 sm:flex-row" on:submit=submit>
                    <Input id="new-item" bind_value=name placeholder="Project name" required=true />
                    <Input bind_value=description placeholder="Short description" />
                    <Button>
                        <Plus />
                        "Create project"
                    </Button>
                </form>
            </CardContent>
        </Card>
    }
}

#[component]
pub fn ProjectList(#[prop(optional)] archive_only: bool) -> impl IntoView {
    let ui = use_ui();
    let list = LocalResource::new(move || async move { ui.run(api::get::<Vec<ProjectItem>>("/api/projects")).await });
    let q = RwSignal::new(String::new());
    let (show, set_show) = query_state("show", "all");
    let show = if archive_only { Signal::stored("archived".to_owned()) } else { show };
    let (sort, set_sort) = query_state("sort", "newest");
    let reload = Callback::new(move |_| list.refetch());
    let rows = move || list.get().flatten().map(|items| arrange(items, &q.get(), &show.get(), &sort.get()));

    view! {
        <Toolbar>
            <div class="flex flex-wrap flex-1 gap-2 items-center min-w-0 max-w-full">
                <SearchBox value=q placeholder="Search projects  /" />
                <Show when=move || !archive_only>
                    <Segmented options=SHOW value=show on_change=set_show />
                </Show>
            </div>
            <SortSelect options=SORTS value=sort on_change=set_sort />
        </Toolbar>
        <div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
            {move || match rows() {
                None => (0..3).map(|_| view! { <Skeleton class="h-40 rounded-xl" /> }).collect_view().into_any(),
                Some(items) if items.is_empty() => view! {
                    <Empty class="sm:col-span-2 lg:col-span-3">
                        <EmptyHeader>
                            <EmptyMedia variant=EmptyMediaVariant::Icon><FolderKanban /></EmptyMedia>
                            <EmptyTitle>"No projects here"</EmptyTitle>
                            <EmptyDescription>"Create a project to get a board with To do, In progress and Done columns."</EmptyDescription>
                        </EmptyHeader>
                    </Empty>
                }
                .into_any(),
                Some(items) => items.into_iter().map(|p| view! { <ProjectCard project=p reload /> }).collect_view().into_any(),
            }}
        </div>
    }
}

#[component]
fn ProjectCard(project: ProjectItem, reload: Callback<()>) -> impl IntoView {
    let ui = use_ui();
    let pct = percent(&project);
    let id = project.id;
    let archived = project.archived == 1;
    let toggle = move |ev: leptos::ev::MouseEvent| {
        ev.prevent_default();
        ev.stop_propagation();
        let action = if archived { "unarchive" } else { "archive" };
        ui.act(format!("/api/projects/{id}/action/{action}"), move || reload.run(()));
    };
    view! {
        <A href=api::url(&format!("/projects/{id}")) attr:class="block group">
            <Card class="h-full lift">
                <CardHeader>
                    <div class="flex gap-2 justify-between items-start">
                        <div class="flex justify-center items-center rounded-lg size-9 bg-muted">
                            <FolderKanban class="size-4 text-muted-foreground" />
                        </div>
                        <div class="flex-1" />
                        <Button
                            variant=ButtonVariant::Ghost
                            size=ButtonSize::IconXs
                            class="opacity-0 transition-opacity group-hover:opacity-100"
                            attr:title=if archived { "Unarchive" } else { "Archive" }
                            on:click=toggle
                        >
                            {if archived { view! { <ArchiveRestore /> }.into_any() } else { view! { <Archive /> }.into_any() }}
                        </Button>
                        <Badge variant=BadgeVariant::Outline>{if archived { "Archived" } else { status_label(&project.status) }}</Badge>
                    </div>
                    <CardTitle class="mt-2">{project.name}</CardTitle>
                    <CardDescription class="line-clamp-2">{project.description}</CardDescription>
                </CardHeader>
                <CardContent class="flex flex-col gap-2 mt-auto">
                    <div class="flex justify-between text-xs text-muted-foreground">
                        <span>{format!("{} open", project.tasks - project.done)}</span>
                        <span class="tabular-nums">{format!("{}/{} done", project.done, project.tasks)}</span>
                    </div>
                    <Progress value=pct />
                </CardContent>
            </Card>
        </A>
    }
}

#[component]
pub fn ProjectDetail() -> impl IntoView {
    let ui = use_ui();
    let params = use_params_map();
    let id = move || params.read().get("id").unwrap_or_default();
    let project = LocalResource::new(move || {
        let path = format!("/api/projects/{}", id());
        async move { ui.run(api::get::<ProjectItem>(&path)).await }
    });
    view! { {move || project.get().flatten().map(|p| view! { <ProjectView project=p /> })} }
}

#[component]
fn ProjectView(project: ProjectItem) -> impl IntoView {
    let ui = use_ui();
    let navigate = use_navigate();
    let id = project.id;
    let name = RwSignal::new(project.name);
    let description = RwSignal::new(project.description);
    let status = RwSignal::new(project.status);
    let archived = RwSignal::new(project.archived == 1);
    let (tab, set_tab) = query_state("tab", "board");
    Effect::new(move |_| ui.set_crumb(name.get()));
    let save = move || {
        spawn_local(async move {
            let body = json!({ "name": name.get_untracked(), "description": description.get_untracked(), "status": status.get_untracked() });
            if ui.run(api::put::<Value>(&format!("/api/projects/{id}"), &body)).await.is_some() {
                ui.notify("Project saved");
            }
        });
    };
    let toggle_archive = move |_| {
        let action = if archived.get_untracked() { "unarchive" } else { "archive" };
        ui.act(format!("/api/projects/{id}/action/{action}"), move || {
            archived.update(|a| *a = !*a);
            ui.notify(if archived.get_untracked() { "Project archived" } else { "Project restored" });
        });
    };
    let remove = move |_| {
        let navigate = navigate.clone();
        ui.confirm_delete("This project and its board will be deleted. Notes and files are kept.", move || {
            let navigate = navigate.clone();
            spawn_local(async move {
                if ui.run(api::del(&format!("/api/projects/{id}"))).await.is_some() {
                    navigate("/projects", Default::default());
                }
            });
        });
    };

    view! {
        <div class="flex flex-col gap-6 mx-auto max-w-screen-2xl page-enter">
            <div class="flex flex-wrap gap-2 items-center">
                <Button variant=ButtonVariant::Ghost size=ButtonSize::Sm href=api::url("/projects")>
                    <ArrowLeft />
                    "All projects"
                </Button>
                <Show when=move || archived.get()>
                    <Badge variant=BadgeVariant::Secondary>"Archived"</Badge>
                </Show>
            </div>
            <div class="flex flex-wrap gap-3 items-start">
                <div class="flex flex-col flex-1 gap-1 min-w-64">
                    <Input
                        class="px-0 h-auto text-2xl font-semibold tracking-tight bg-transparent border-0 shadow-none md:text-2xl focus-visible:ring-0 dark:bg-transparent"
                        bind_value=name
                        on:change=move |_| save()
                    />
                    <Textarea
                        class="py-0 px-0 min-h-0 text-sm bg-transparent border-0 shadow-none text-muted-foreground focus-visible:ring-0 dark:bg-transparent"
                        placeholder="Add a description"
                        bind_value=description
                        on:change=move |_| save()
                    />
                </div>
                <Choice
                    options=owned(&STATUSES)
                    value=status
                    on_change=Callback::new(move |picked| {
                        status.set(picked);
                        save();
                    })
                />
                <Button variant=ButtonVariant::Outline attr:download="" href=api::url(&format!("/api/projects/{id}/export"))>
                    <Download />
                    "Export Excel"
                </Button>
                <Button variant=ButtonVariant::Outline on:click=toggle_archive>
                    {move || if archived.get() { view! { <ArchiveRestore /> "Unarchive" }.into_any() } else { view! { <Archive /> "Archive" }.into_any() }}
                </Button>
                <Button variant=ButtonVariant::Ghost size=ButtonSize::Icon attr:title="Delete project" on:click=remove>
                    <Trash2 />
                </Button>
            </div>
            <Segmented options=TABS value=tab on_change=set_tab />
            {move || match tab.get().as_str() {
                "notes" => view! { <div class="page-enter"><NotesList view=View::Project(id) /></div> }.into_any(),
                "files" => view! { <div class="flex flex-col gap-4 page-enter"><FileManager project=id /></div> }.into_any(),
                _ => view! { <Board project=id /> }.into_any(),
            }}
        </div>
    }
}

#[derive(Clone, Copy)]
struct BoardCtx {
    project: Id,
    columns: LocalResource<Vec<Column>>,
    tasks: RwSignal<Vec<Task>>,
    source: LocalResource<Option<Vec<Task>>>,
    dragging: RwSignal<Option<Id>>,
    over_task: RwSignal<Option<Id>>,
    over_column: RwSignal<Option<Id>>,
    form: TaskForm,
    ui: Ui,
}

fn reorder(list: &mut Vec<Task>, id: Id, column: Id, before: Option<Id>) {
    let Some(from) = list.iter().position(|t| t.id == id) else { return };
    let mut t = list.remove(from);
    t.column_id = column;
    let at = before.and_then(|b| list.iter().position(|x| x.id == b)).unwrap_or(list.len());
    list.insert(at, t);
}

impl BoardCtx {
    fn call(self, fut: impl Future<Output = api::ApiResult<Value>> + 'static, columns: bool) {
        spawn_local(async move {
            self.ui.run(fut).await;
            self.source.refetch();
            if columns {
                self.columns.refetch();
            }
        });
    }

    fn save_task(self, t: Task) {
        self.call(async move { api::put::<Value>(&format!("/api/tasks/{}", t.id), &t).await }, false);
    }

    fn end_drag(self) {
        self.dragging.set(None);
        self.over_task.set(None);
        self.over_column.set(None);
    }

    fn drop_at(self, column_id: Id, before: Option<Id>) {
        let dragged = self.dragging.get_untracked();
        self.end_drag();
        let Some(id) = dragged.filter(|id| before != Some(*id)) else { return };
        self.tasks.update(|list| reorder(list, id, column_id, before));
        let body = json!({ "column_id": column_id, "before": before });
        self.call(async move { api::post::<Value>(&format!("/api/tasks/{id}/move"), &body).await }, false);
    }

    fn last_column(self) -> Option<Id> {
        self.columns.get().unwrap_or_default().last().map(|c| c.id)
    }
}

#[derive(Clone, Copy)]
struct TaskForm {
    open: RwSignal<bool>,
    id: RwSignal<Id>,
    title: RwSignal<String>,
    description: RwSignal<String>,
    column: RwSignal<Id>,
    due: RwSignal<String>,
    priority: RwSignal<i64>,
}

impl TaskForm {
    fn new() -> Self {
        Self {
            open: RwSignal::new(false),
            id: RwSignal::new(Id::default()),
            title: RwSignal::new(String::new()),
            description: RwSignal::new(String::new()),
            column: RwSignal::new(Id::default()),
            due: RwSignal::new(String::new()),
            priority: RwSignal::new(0),
        }
    }

    fn edit(&self, t: Task) {
        self.id.set(t.id);
        self.title.set(t.title);
        self.description.set(t.description);
        self.column.set(t.column_id);
        self.due.set(t.due.unwrap_or_default());
        self.priority.set(t.priority);
        self.open.set(true);
    }

    fn task(&self) -> Task {
        let due = self.due.get_untracked();
        Task {
            id: self.id.get_untracked(),
            column_id: self.column.get_untracked(),
            title: self.title.get_untracked(),
            description: self.description.get_untracked(),
            due: (!due.is_empty()).then_some(due),
            priority: self.priority.get_untracked(),
            ..Default::default()
        }
    }
}

#[derive(Clone, Copy)]
struct Filters {
    q: RwSignal<String>,
    due: Signal<String>,
    order: Signal<String>,
}

impl Filters {
    fn keep(&self, t: &Task) -> bool {
        let days = t.due.as_deref().map(days_until);
        let due_ok = match self.due.get().as_str() {
            "overdue" => days.is_some_and(|d| d < 0),
            "soon" => days.is_some_and(|d| (0..=7).contains(&d)),
            "nodate" => days.is_none(),
            _ => true,
        };
        due_ok && (has(&t.title, &self.q.get()) || has(&t.description, &self.q.get()))
    }

    fn apply(&self, mut list: Vec<Task>) -> Vec<Task> {
        list.retain(|t| self.keep(t));
        match self.order.get().as_str() {
            "due" => list.sort_by_key(|t| t.due.clone().unwrap_or_else(|| "9999".into())),
            "priority" => list.sort_by_key(|t| Reverse(t.priority)),
            "newest" => list.sort_by_key(|t| Reverse(t.created)),
            _ => {}
        }
        list
    }
}

#[component]
fn Board(project: Id) -> impl IntoView {
    let ui = use_ui();
    let columns = LocalResource::new(move || async move {
        ui.run(api::get::<Vec<Column>>(&format!("/api/projects/{project}/columns"))).await.unwrap_or_default()
    });
    let source = LocalResource::new(move || async move { ui.run(api::get::<Vec<Task>>(&format!("/api/projects/{project}/tasks"))).await });
    let tasks = RwSignal::new(Vec::new());
    let ctx = BoardCtx {
        project,
        columns,
        tasks,
        source,
        dragging: RwSignal::new(None),
        over_task: RwSignal::new(None),
        over_column: RwSignal::new(None),
        form: TaskForm::new(),
        ui,
    };
    let (open_task, set_open_task) = query_state("task", "");
    Effect::new(move |_| {
        let Some(list) = source.get().flatten() else { return };
        let wanted = open_task.get_untracked().parse::<Id>().ok();
        if let Some(t) = wanted.and_then(|w| list.iter().find(|t| t.id == w)) {
            ctx.form.edit(t.clone());
            set_open_task.run(String::new());
        }
        tasks.set(list);
    });
    let (due, set_due) = query_state("due", "all");
    let (order, set_order) = query_state("order", "manual");
    let filters = Filters { q: RwSignal::new(String::new()), due, order };
    let new_column = RwSignal::new(String::new());
    let add_column = move |ev: SubmitEvent| {
        ev.prevent_default();
        let body = json!({ "name": new_column.get_untracked() });
        new_column.set(String::new());
        ctx.call(async move { api::post::<Value>(&format!("/api/projects/{project}/columns"), &body).await }, true);
    };

    view! {
        <Toolbar>
            <div class="flex flex-wrap flex-1 gap-2 items-center min-w-0 max-w-full">
                <SearchBox value=filters.q placeholder="Search tasks  /" />
                <Segmented options=DUE value=due on_change=set_due />
            </div>
            <SortSelect options=ORDER value=order on_change=set_order />
        </Toolbar>
        <div class="flex overflow-x-auto gap-4 items-start px-1 pb-4 -mx-1 page-enter">
            {move || {
                columns.get().unwrap_or_default().into_iter().map(|c| view! { <BoardColumn column=c ctx filters /> }).collect_view()
            }}
            <form
                class="flex flex-col gap-2 p-3 w-72 rounded-xl border border-dashed transition-colors shrink-0 hover:bg-muted/30"
                on:submit=add_column
            >
                <span class="text-sm font-medium text-muted-foreground">"Add column"</span>
                <Input class="bg-background" placeholder="Column name" bind_value=new_column required=true />
            </form>
        </div>
        <p class="text-xs text-muted-foreground">"Drag cards to reorder them or move them between columns. Click a card to edit it."</p>
        <TaskModal ctx />
    }
}

#[component]
fn ColumnHeader(column: Column, count: Signal<usize>, ctx: BoardCtx) -> impl IntoView {
    let id = column.id;
    let editing = RwSignal::new(false);
    let name = RwSignal::new(column.name.clone());
    let rename = move |ev: SubmitEvent| {
        ev.prevent_default();
        editing.set(false);
        let body = json!({ "name": name.get_untracked() });
        ctx.call(async move { api::put::<Value>(&format!("/api/columns/{id}"), &body).await }, true);
    };
    let shift = move |dir: &'static str| ctx.call(async move { api::post::<Value>(&format!("/api/columns/{id}/move/{dir}"), &()).await }, true);
    let remove = move |_| {
        ctx.ui.confirm_delete("This column and all of its tasks will be deleted.", move || {
            ctx.call(async move { api::del(&format!("/api/columns/{id}")).await }, true);
        });
    };

    view! {
        <div class="flex gap-1 items-center px-1 group/col min-h-8">
            <Show
                when=move || editing.get()
                fallback=move || {
                    view! {
                        <span class="flex-1 text-sm font-semibold truncate" on:dblclick=move |_| editing.set(true)>{name.get()}</span>
                        <Badge variant=BadgeVariant::Secondary class="tabular-nums">{move || count.get()}</Badge>
                    }
                }
            >
                <form class="flex-1" on:submit=rename>
                    <Input class="h-8 bg-background" bind_value=name autofocus=true />
                </form>
            </Show>
            <div class="flex opacity-0 transition-opacity group-hover/col:opacity-100">
                <Button variant=ButtonVariant::Ghost size=ButtonSize::IconXs attr:title="Move left" on:click=move |_| shift("left")>
                    <ChevronLeft />
                </Button>
                <Button variant=ButtonVariant::Ghost size=ButtonSize::IconXs attr:title="Move right" on:click=move |_| shift("right")>
                    <ChevronRight />
                </Button>
                <Button variant=ButtonVariant::Ghost size=ButtonSize::IconXs attr:title="Rename" on:click=move |_| editing.update(|e| *e = !*e)>
                    <Pencil />
                </Button>
                <Button variant=ButtonVariant::Ghost size=ButtonSize::IconXs attr:title="Delete column" on:click=remove>
                    <Trash2 />
                </Button>
            </div>
        </div>
    }
}

#[component]
fn BoardColumn(column: Column, ctx: BoardCtx, filters: Filters) -> impl IntoView {
    let id = column.id;
    let title = RwSignal::new(String::new());
    let over = move || ctx.over_column.get() == Some(id);
    let at_end = move || over() && ctx.over_task.get().is_none() && ctx.dragging.get().is_some();
    let mine = move || filters.apply(ctx.tasks.get().into_iter().filter(|t| t.column_id == id).collect());
    let count = Signal::derive(move || mine().len());
    let limit = RwSignal::new(PAGE);
    let hidden = move || count.get().saturating_sub(limit.get());
    let show_more = move || limit.update(|l| *l += PAGE);
    let list = NodeRef::<html::Div>::new();
    let on_scroll = move |_| {
        let Some(el) = list.get() else { return };
        if el.scroll_top() + el.client_height() >= el.scroll_height() - 120 && hidden() > 0 {
            show_more();
        }
    };
    let done = Signal::derive(move || ctx.last_column() == Some(id));
    let add = move |ev: SubmitEvent| {
        ev.prevent_default();
        let body = json!({ "title": title.get_untracked(), "column_id": id });
        title.set(String::new());
        let project = ctx.project;
        ctx.call(async move { api::post::<Value>(&format!("/api/projects/{project}/tasks"), &body).await }, false);
    };

    view! {
        <div
            class="flex flex-col gap-2 p-2 w-72 rounded-xl border transition-all duration-200 shrink-0 bg-muted/40 max-h-[calc(100dvh-10rem)]"
            class=("ring-2", over)
            class=("ring-primary/30", over)
            class=("bg-muted", over)
            on:dragover=move |ev: DragEvent| {
                ev.prevent_default();
                ctx.over_column.set(Some(id));
                if ev.target() == ev.current_target() {
                    ctx.over_task.set(None);
                }
            }
            on:drop=move |ev: DragEvent| {
                ev.prevent_default();
                ctx.drop_at(id, None);
            }
        >
            <ColumnHeader column ctx count />
            <div
                node_ref=list
                class="flex overflow-y-auto flex-col flex-1 gap-2 px-1 py-0.5 -mx-1 min-h-0"
                on:scroll=on_scroll
                on:dragover=move |ev: DragEvent| {
                    if ev.target() == ev.current_target() {
                        ctx.over_task.set(None);
                    }
                }
            >
                {move || mine().into_iter().take(limit.get()).map(|t| view! { <TaskCard task=t done ctx /> }).collect_view()}
                <Show when=move || hidden() != 0>
                    <Button variant=ButtonVariant::Ghost size=ButtonSize::Sm class="shrink-0 text-muted-foreground" on:click=move |_| show_more()>
                        {move || format!("Show {} more", hidden().min(PAGE))}
                    </Button>
                </Show>
                <div class="h-1 rounded-full transition-all shrink-0 bg-primary" class=("opacity-0", move || !at_end()) />
            </div>
            <form on:submit=add on:dragover=move |_| ctx.over_task.set(None)>
                <Input
                    class="bg-transparent border-transparent shadow-none hover:bg-background focus:bg-background dark:bg-transparent"
                    placeholder="+ Add task"
                    bind_value=title
                />
            </form>
        </div>
    }
}

fn priority_class(p: i64) -> &'static str {
    match p {
        3 => "bg-destructive/10 text-destructive",
        2 => "bg-warning/20 text-foreground",
        _ => "bg-muted text-muted-foreground",
    }
}

fn priority_label(p: i64) -> &'static str {
    PRIORITIES.iter().find(|(k, _)| *k == p).map_or("", |(_, l)| *l)
}

#[component]
fn DueChip(due: String, done: bool) -> impl IntoView {
    let days = days_until(&due);
    let class = match days {
        _ if done => "bg-muted text-muted-foreground",
        d if d < 0 => "bg-destructive/10 text-destructive",
        0..=2 => "bg-warning/20 text-foreground",
        _ => "bg-muted text-muted-foreground",
    };
    view! {
        <span class=format!("inline-flex gap-1 items-center py-0.5 px-1.5 rounded-md {class}") title=due.clone()>
            <Calendar class="size-3" />
            {if done { due.clone() } else { due_label(days) }}
        </span>
    }
}

#[component]
fn TaskCard(task: Task, done: Signal<bool>, ctx: BoardCtx) -> impl IntoView {
    let id = task.id;
    let edit_task = task.clone();
    let cover = md::first_image(&task.description);
    let description = md::plain(&task.description);
    let has_description = !description.is_empty();
    let subtasks = (task.subtasks > 0).then(|| format!("{}/{}", task.subtasks_done, task.subtasks));
    let created = fmt_date(task.created);
    let priority = task.priority;
    let column = task.column_id;
    let before = move || ctx.over_task.get() == Some(id) && ctx.dragging.get().is_some_and(|d| d != id);

    view! {
        <div
            class="flex flex-col gap-1"
            on:dragover=move |ev: DragEvent| {
                ev.prevent_default();
                ctx.over_task.set(Some(id));
            }
            on:drop=move |ev: DragEvent| {
                ev.prevent_default();
                ev.stop_propagation();
                ctx.drop_at(column, Some(id));
            }
        >
            <div class="h-1 rounded-full transition-all shrink-0 bg-primary" class=("hidden", move || !before()) />
            <div
                draggable="true"
                class="flex flex-col gap-2 p-3 text-sm rounded-lg border shadow-xs shrink-0 cursor-grab active:cursor-grabbing bg-card lift animate-in fade-in-0 zoom-in-95"
                class=("opacity-40", move || ctx.dragging.get() == Some(id))
                on:dragstart=move |ev: DragEvent| {
                    if let Some(dt) = ev.data_transfer() {
                        let _ = dt.set_data("text/plain", &id.to_string());
                    }
                    ctx.dragging.set(Some(id));
                }
                on:dragend=move |_| ctx.end_drag()
                on:click=move |_| ctx.form.edit(edit_task.clone())
            >
                {cover.map(|src| view! { <img src=src alt="" loading="lazy" draggable="false" class="object-cover w-full h-32 rounded-md bg-muted" /> })}
                <span class="font-medium leading-snug" class=("line-through", done) class=("text-muted-foreground", done)>
                    {task.title}
                </span>
                <Show when=move || has_description>
                    <p class="text-xs text-muted-foreground line-clamp-3">{description.clone()}</p>
                </Show>
                <div class="flex flex-wrap gap-2 items-center text-xs text-muted-foreground">
                    <Show when=move || priority != 0>
                        <span class=format!("inline-flex gap-1 items-center py-0.5 px-1.5 rounded-md {}", priority_class(priority))>
                            <Flag class="size-3" />
                            {priority_label(priority)}
                        </span>
                    </Show>
                    {task.due.map(|d| view! { {move || view! { <DueChip due=d.clone() done=done.get() /> }} })}
                    {subtasks.map(|s| view! {
                        <span class="inline-flex gap-1 items-center py-0.5 px-1.5 rounded-md bg-muted">
                            <ListChecks class="size-3" />
                            {s}
                        </span>
                    })}
                    <span class="ml-auto">{created}</span>
                </div>
            </div>
        </div>
    }
}

#[component]
fn TaskModal(ctx: BoardCtx) -> impl IntoView {
    let form = ctx.form;
    let upload_project = RwSignal::new(Some(ctx.project));
    let cover = move || md::first_image(&form.description.get());
    let remove = move |_| {
        ctx.ui.confirm_delete("This task will be deleted.", move || {
            let id = form.id.get_untracked();
            form.open.set(false);
            ctx.call(async move { api::del(&format!("/api/tasks/{id}")).await }, false);
        });
    };
    let complete = move |_| {
        let Some(last) = ctx.columns.get_untracked().unwrap_or_default().last().map(|c| c.id) else { return };
        form.column.set(last);
        form.open.set(false);
        ctx.save_task(form.task());
    };
    let save = move |_| {
        form.open.set(false);
        ctx.save_task(form.task());
    };

    view! {
        <Modal open=form.open title="Edit task" class="max-w-[min(72rem,calc(100%-2rem))]">
            <div class="grid gap-6 lg:grid-cols-[minmax(0,1fr)_17rem]">
                <div class="flex flex-col gap-4 min-w-0">
                    <RichEditor title=form.title body=form.description on_change=Callback::new(|_| ()) upload_project allow_upload=true compact=true min_height="min-h-72" />
                    <Subtasks ctx />
                </div>
                <aside class="flex flex-col gap-4">
                    {move || cover().map(|src| view! { <img src=src alt="" class="object-cover w-full h-40 rounded-lg border bg-muted" /> })}
                    <div class="flex flex-col gap-2">
                        <Label>"Due date"</Label>
                        <DatePicker bind_value=form.due />
                    </div>
                    <div class="flex flex-col gap-2">
                        <Label>"Priority"</Label>
                        <Choice
                            class="w-full"
                            options={PRIORITIES.iter().map(|(key, label)| (key.to_string(), (*label).to_owned())).collect::<Vec<_>>()}
                            value=Signal::derive(move || form.priority.get().to_string())
                            on_change=Callback::new(move |picked: String| form.priority.set(picked.parse().unwrap_or_default()))
                        />
                    </div>
                    <div class="flex flex-col gap-2">
                        <Label>"Column"</Label>
                        <Choice
                            class="w-full"
                            options=Signal::derive(move || {
                                ctx.columns.get().unwrap_or_default().into_iter().map(|c| (c.id.to_string(), c.name)).collect::<Vec<_>>()
                            })
                            value=Signal::derive(move || form.column.get().to_string())
                            on_change=Callback::new(move |picked: String| form.column.set(picked.parse().unwrap_or_default()))
                        />
                    </div>
                    <div class="flex gap-2 items-center pt-2 lg:mt-auto">
                        <Button
                            variant=ButtonVariant::Destructive
                            size=ButtonSize::Icon
                            attr:title="Delete task"
                            attr:aria-label="Delete task"
                            on:click=remove
                        >
                            <Trash2 />
                        </Button>
                        <Button class="flex-1" variant=ButtonVariant::Outline on:click=complete>"Mark done"</Button>
                        <Button class="flex-1" on:click=save>"Save"</Button>
                    </div>
                </aside>
            </div>
        </Modal>
    }
}

fn sync_subtasks(ctx: BoardCtx, list: LocalResource<Vec<Subtask>>, fut: impl Future<Output = api::ApiResult<Value>> + 'static) {
    spawn_local(async move {
        ctx.ui.run(fut).await;
        list.refetch();
        ctx.source.refetch();
    });
}

#[component]
fn Subtasks(ctx: BoardCtx) -> impl IntoView {
    let form = ctx.form;
    let list = LocalResource::new(move || {
        let (id, open) = (form.id.get(), form.open.get());
        async move {
            if !open {
                return Vec::new();
            }
            ctx.ui.run(api::get::<Vec<Subtask>>(&format!("/api/tasks/{id}/subtasks"))).await.unwrap_or_default()
        }
    });
    let items = move || list.get().unwrap_or_default();
    let title = RwSignal::new(String::new());
    let add = move |ev: SubmitEvent| {
        ev.prevent_default();
        let body = json!({ "title": title.get_untracked() });
        title.set(String::new());
        let id = form.id.get_untracked();
        sync_subtasks(ctx, list, async move { api::post::<Value>(&format!("/api/tasks/{id}/subtasks"), &body).await });
    };

    view! {
        <div class="flex flex-col gap-2">
            <div class="flex justify-between items-center">
                <Label>"Subtasks"</Label>
                <span class="text-xs tabular-nums text-muted-foreground">
                    {move || {
                        let all = items();
                        format!("{}/{}", all.iter().filter(|s| s.done == 1).count(), all.len())
                    }}
                </span>
            </div>
            <For each=items key=|s| s.id let(subtask)>
                <SubtaskRow subtask ctx list />
            </For>
            <form on:submit=add>
                <Input placeholder="+ Add subtask" bind_value=title />
            </form>
        </div>
    }
}

#[component]
fn SubtaskRow(subtask: Subtask, ctx: BoardCtx, list: LocalResource<Vec<Subtask>>) -> impl IntoView {
    let id = subtask.id;
    let path = StoredValue::new(format!("/api/subtasks/{id}"));
    let title = RwSignal::new(subtask.title);
    let done = RwSignal::new(subtask.done == 1);
    let description = RwSignal::new(subtask.description);
    let due = RwSignal::new(subtask.due.unwrap_or_default());
    let priority = RwSignal::new(subtask.priority);
    let open = RwSignal::new(false);
    let upload_project = RwSignal::new(Some(ctx.project));
    let saver = AutoSave::new();
    let payload = move || {
        let due = due.get_untracked();
        json!({
            "title": title.get_untracked(), "done": done.get_untracked(), "description": description.get_untracked(),
            "due": (!due.is_empty()).then_some(due), "priority": priority.get_untracked(),
        })
    };
    let save = move || saver.send(path.get_value(), payload());
    Effect::new(move |first: Option<()>| {
        due.track();
        priority.track();
        if first.is_some() {
            save();
        }
    });
    let toggle = Callback::new(move |checked: bool| {
        done.set(checked);
        let body = payload();
        sync_subtasks(ctx, list, async move { api::put::<Value>(&path.get_value(), &body).await });
    });
    let remove = move |_| {
        ctx.ui.confirm_delete("This subtask will be deleted.", move || {
            sync_subtasks(ctx, list, async move { api::del(&path.get_value()).await });
        });
    };

    view! {
        <div class="rounded-lg border bg-card">
            <div class="flex gap-1.5 items-center py-1 pr-1 pl-1.5 group/sub">
                <Button
                    variant=ButtonVariant::Ghost
                    size=ButtonSize::IconXs
                    attr:title=move || if open.get() { "Hide details" } else { "Show details" }
                    attr:aria-expanded=move || open.get().to_string()
                    on:click=move |_| open.update(|o| *o = !*o)
                >
                    <span class="flex transition-transform" class=("rotate-90", move || open.get())>
                        <ChevronRight />
                    </span>
                </Button>
                <Checkbox aria_label="Done" checked=done on_checked_change=toggle />
                {move || {
                    let text = if done.get() { "line-through text-muted-foreground" } else { "" };
                    view! {
                        <Input
                            class=format!("h-8 bg-transparent border-transparent shadow-none hover:border-input focus:bg-background dark:bg-transparent {text}")
                            bind_value=title
                            on:change=move |_| save()
                        />
                    }
                }}
                {move || (priority.get() != 0).then(|| view! {
                    <span class=format!("inline-flex gap-1 items-center py-0.5 px-1.5 text-xs rounded-md shrink-0 {}", priority_class(priority.get()))>
                        <Flag class="size-3" />
                        {priority_label(priority.get())}
                    </span>
                })}
                {move || (!due.get().is_empty()).then(|| view! { <span class="text-xs shrink-0"><DueChip due=due.get() done=done.get() /></span> })}
                <Button
                    variant=ButtonVariant::Ghost
                    size=ButtonSize::IconXs
                    class="opacity-0 group-hover/sub:opacity-100 focus-visible:opacity-100"
                    attr:title="Delete subtask"
                    on:click=remove
                >
                    <Trash2 />
                </Button>
            </div>
            <Show when=move || open.get()>
                <div class="flex flex-col gap-3 px-3 pt-1 pb-3 animate-in fade-in-0">
                    <RichEditor
                        body=description
                        on_change=Callback::new(move |_| save())
                        upload_project
                        allow_upload=true
                        compact=true
                        min_height="min-h-28"
                        placeholder="Add details..."
                    />
                    <div class="grid gap-3 sm:grid-cols-2">
                        <div class="flex flex-col gap-2">
                            <Label>"Due date"</Label>
                            <DatePicker bind_value=due />
                        </div>
                        <div class="flex flex-col gap-2">
                            <Label>"Priority"</Label>
                            <Choice
                                class="w-full"
                                options={PRIORITIES.iter().map(|(key, label)| (key.to_string(), (*label).to_owned())).collect::<Vec<_>>()}
                                value=Signal::derive(move || priority.get().to_string())
                                on_change=Callback::new(move |picked: String| priority.set(picked.parse().unwrap_or_default()))
                            />
                        </div>
                    </div>
                </div>
            </Show>
        </div>
    }
}
