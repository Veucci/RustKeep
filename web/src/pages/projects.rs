use std::cmp::Reverse;
use std::collections::HashSet;
use std::time::Duration;

use icons::{
    Archive, ArchiveRestore, ArrowLeft, Bell, Calendar, ChevronRight, CircleCheck, Download, Flag, FolderKanban, GripVertical, ListChecks,
    Pencil, Plus, Share2, SquareKanban, Trash2,
};
use leptos::ev::SubmitEvent;
use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::A;
use leptos_router::hooks::{use_location, use_navigate, use_params_map};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use wasm_bindgen::JsCast;
use web_sys::{DragEvent, Element, KeyboardEvent, MouseEvent, PointerEvent};

use crate::api::{self, AutoSave, SaveState};
use crate::id::Id;
use crate::components::ui::badge::{Badge, BadgeVariant};
use crate::components::ui::button::{Button, ButtonLink, ButtonSize, ButtonVariant};
use crate::components::ui::checkbox::Checkbox;
use crate::components::ui::card::{Card, CardContent, CardDescription, CardHeader, CardTitle};
use crate::components::ui::date_picker::DatePicker;
use crate::components::ui::empty::{Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant, EmptyTitle};
use crate::components::ui::input::Input;
use crate::components::ui::label::Label;
use crate::components::ui::multi_select::{
    MultiSelect, MultiSelectAlign, MultiSelectContent, MultiSelectTrigger, MultiSelectValue,
};
use crate::components::ui::progress::Progress;
use crate::components::ui::skeleton::Skeleton;
use crate::components::ui::textarea::Textarea;
use crate::md;
use crate::pages::editor::{RichEditor, SaveBadge};
use crate::pages::files::FileManager;
use crate::pages::notes::{NotesList, View};
use crate::pages::reminders::{ReminderForm, TaskReminders};
use crate::widgets::{
    Choice, Modal, Options, PageHeader, SearchBox, Segmented, ShareDialog, SortSelect, Toolbar, Ui, days_until, due_label,
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
const PROJECT_FILTER: Options = &[("only", "Show selected"), ("hide", "Hide selected")];
const TASK_STATE: Options = &[("active", "Active"), ("archived", "Archived")];
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
    #[serde(default)]
    pub share_token: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Hash, Deserialize)]
struct Column {
    id: Id,
    #[serde(default)]
    project_id: Id,
    name: String,
    done: i64,
}

#[derive(Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct Task {
    id: Id,
    column_id: Id,
    #[serde(default, skip_serializing)]
    project_id: Id,
    #[serde(default, skip_serializing)]
    project: String,
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
    #[serde(default, skip_serializing)]
    archived: i64,
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
            <PageHeader title="Projects" description="Boards, notes and files grouped by project.">
                <ButtonLink size=ButtonSize::Lg class="text-base" href=api::url("/projects/overview")>
                    <SquareKanban class="size-5" />
                    "Overview"
                </ButtonLink>
            </PageHeader>
            <NewProject />
            <ProjectList />
        </div>
    }
}

#[component]
pub fn ProjectsOverview() -> impl IntoView {
    let ui = use_ui();
    ui.crumb.set(Some((use_location().pathname.get_untracked(), "Overview".into())));
    view! {
        <div class="flex flex-col gap-6 mx-auto max-w-screen-2xl page-enter">
            <PageHeader title="Overview" description="Tasks from every active project on one board." />
            <Board />
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
            <div class="flex flex-wrap flex-1 gap-2 items-center min-w-0 max-w-full basis-full sm:basis-0">
                <SearchBox value=q placeholder="Search projects  /" />
                <Show when=move || !archive_only>
                    <Segmented options=SHOW value=show on_change=set_show />
                </Show>
            </div>
            <SortSelect options=SORTS value=sort on_change=set_sort />
        </Toolbar>
        <div class="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
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
                            class="opacity-0 transition-opacity group-hover:opacity-100 pointer-coarse:opacity-100"
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
    let id = Memo::new(move |_| params.read().get("id").unwrap_or_default());
    let project = LocalResource::new(move || {
        let path = format!("/api/projects/{}", id.get());
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
    let share_token = RwSignal::new(project.share_token);
    let share_open = RwSignal::new(false);
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
                <ButtonLink variant=ButtonVariant::Ghost size=ButtonSize::Sm href=api::url("/projects")>
                    <ArrowLeft />
                    "All projects"
                </ButtonLink>
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
                <Button variant=ButtonVariant::Outline attr:title="Share" on:click=move |_| share_open.set(true)>
                    <Share2 />
                    <span class="hidden sm:inline">"Share"</span>
                </Button>
                <ButtonLink variant=ButtonVariant::Outline attr:title="Export Excel" attr:download="" href=api::url(&format!("/api/projects/{id}/export"))>
                    <Download />
                    <span class="hidden sm:inline">"Export Excel"</span>
                </ButtonLink>
                <Button variant=ButtonVariant::Outline attr:title=move || if archived.get() { "Unarchive" } else { "Archive" } on:click=toggle_archive>
                    {move || if archived.get() { view! { <ArchiveRestore /> }.into_any() } else { view! { <Archive /> }.into_any() }}
                    <span class="hidden sm:inline">{move || if archived.get() { "Unarchive" } else { "Archive" }}</span>
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
        <ShareDialog open=share_open api_path=format!("/api/projects/{id}/share") link_prefix="/b/" token=share_token />
    }
}

#[derive(Clone, Copy)]
struct BoardCtx {
    project: Option<Id>,
    columns: LocalResource<Vec<Column>>,
    tasks: RwSignal<Vec<Task>>,
    source: LocalResource<Option<Vec<Task>>>,
    dragging: RwSignal<Option<Id>>,
    over_task: RwSignal<Option<Id>>,
    over_column: RwSignal<Option<Id>>,
    dragging_column: RwSignal<Option<Id>>,
    pointer: RwSignal<Option<(f64, f64)>>,
    drag_label: RwSignal<String>,
    scroll_timer: StoredValue<Option<IntervalHandle>>,
    board: NodeRef<html::Div>,
    form: TaskForm,
    ui: Ui,
}

const EDGE_SCROLL_ZONE: f64 = 48.0;
const EDGE_SCROLL_STEP: f64 = 6.0;
const LANE_GAP: f64 = 16.0;

fn task_under(x: f64, y: f64) -> Option<Id> {
    let found = document().element_from_point(x as f32, y as f32)?.closest("[data-task]").ok()??;
    found.get_attribute("data-task")?.parse().ok()
}

fn lane_under(x: f64) -> Option<Id> {
    let lanes = document().query_selector_all("[data-lane]").ok()?;
    let spans_pointer = |lane: &Element| {
        let bounds = lane.get_bounding_client_rect();
        (bounds.left() - LANE_GAP / 2.0..=bounds.right() + LANE_GAP / 2.0).contains(&x)
    };
    let found = (0..lanes.length()).filter_map(|i| lanes.item(i)?.dyn_into::<Element>().ok()).find(spans_pointer)?;
    found.get_attribute("data-lane")?.parse().ok()
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

    fn end_drag(self) {
        self.dragging.set(None);
        self.dragging_column.set(None);
        self.over_task.set(None);
        self.over_column.set(None);
        self.pointer.set(None);
        if let Some(timer) = self.scroll_timer.get_value() {
            timer.clear();
        }
    }

    fn begin_pointer_drag(self, ev: &PointerEvent, label: String) {
        if let Some(grip) = ev.current_target().and_then(|t| t.dyn_into::<Element>().ok()) {
            let _ = grip.set_pointer_capture(ev.pointer_id());
        }
        self.drag_label.set(label);
        self.pointer.set(Some((0.0, 0.0)));
        self.track_pointer(ev);
        let timer = set_interval_with_handle(move || self.edge_scroll(), Duration::from_millis(16));
        self.scroll_timer.set_value(timer.ok());
    }

    fn track_pointer(self, ev: &PointerEvent) {
        if self.pointer.get_untracked().is_some() {
            let at = (f64::from(ev.client_x()), f64::from(ev.client_y()));
            self.pointer.set(Some(at));
            self.hit_test(at);
        }
    }

    fn hit_test(self, (x, y): (f64, f64)) {
        self.over_column.set(lane_under(x));
        self.over_task.set(task_under(x, y));
    }

    fn edge_scroll(self) {
        let (Some(board), Some(at)) = (self.board.get_untracked(), self.pointer.get_untracked()) else { return };
        let bounds = board.get_bounding_client_rect();
        if at.0 < bounds.left() + EDGE_SCROLL_ZONE {
            board.scroll_by_with_x_and_y(-EDGE_SCROLL_STEP, 0.0);
        } else if at.0 > bounds.right() - EDGE_SCROLL_ZONE {
            board.scroll_by_with_x_and_y(EDGE_SCROLL_STEP, 0.0);
        }
        self.hit_test(at);
    }

    fn drop_task_at_pointer(self) {
        let before = self.over_task.get_untracked();
        let target = self.over_column.get_untracked();
        match self.lanes().into_iter().find(|l| Some(l.id) == target) {
            Some(lane) => self.drop_at(&lane, before),
            None => self.end_drag(),
        }
    }

    fn drop_column_at_pointer(self) {
        let dragged = self.dragging_column.get_untracked();
        let target = self.over_column.get_untracked();
        self.end_drag();
        let Some(id) = dragged.filter(|id| target != Some(*id)) else { return };
        if let Some(index) = self.lanes().iter().position(|l| Some(l.id) == target) {
            self.move_column(id, index);
        }
    }

    fn move_column(self, id: Id, index: usize) {
        let body = json!({ "index": index });
        self.call(async move { api::post::<Value>(&format!("/api/columns/{id}/move"), &body).await }, true);
    }

    fn drop_at(self, lane: &Column, before: Option<Id>) {
        let dragged = self.dragging.get_untracked();
        self.end_drag();
        let Some(id) = dragged.filter(|id| before != Some(*id)) else { return };
        let Some(task) = self.tasks.get_untracked().into_iter().find(|t| t.id == id) else { return };
        let Some(column_id) = self.lane_columns(lane).into_iter().find(|c| c.project_id == task.project_id).map(|c| c.id) else {
            self.ui.notify(format!("{} has no \"{}\" column", task.project, lane.name));
            return;
        };
        self.tasks.update(|list| reorder(list, id, column_id, before));
        let body = json!({ "column_id": column_id, "before": before });
        self.call(async move { api::post::<Value>(&format!("/api/tasks/{id}/move"), &body).await }, false);
    }

    fn lanes(self) -> Vec<Column> {
        let mut list = self.columns.get().unwrap_or_default();
        if self.project.is_none() {
            let mut seen = HashSet::new();
            list.retain(|c| seen.insert(c.name.trim().to_lowercase()));
        }
        list
    }

    fn lane_columns(self, lane: &Column) -> Vec<Column> {
        let merged = |c: &Column| self.project.is_none() && c.name.trim().eq_ignore_ascii_case(lane.name.trim());
        self.columns.get().unwrap_or_default().into_iter().filter(|c| c.id == lane.id || merged(c)).collect()
    }

    fn project_columns(self, project: Id) -> Vec<Column> {
        self.columns.get().unwrap_or_default().into_iter().filter(|c| c.project_id == project).collect()
    }

    fn is_done(self, column: Id) -> bool {
        self.columns.get().unwrap_or_default().iter().any(|c| c.id == column && c.done == 1)
    }
}

#[derive(Clone, Copy)]
struct TaskForm {
    open: RwSignal<bool>,
    id: RwSignal<Id>,
    project: RwSignal<Id>,
    title: RwSignal<String>,
    description: RwSignal<String>,
    column: RwSignal<Id>,
    due: RwSignal<String>,
    priority: RwSignal<i64>,
    archived: RwSignal<bool>,
    saved: StoredValue<Value>,
}

impl TaskForm {
    fn new() -> Self {
        Self {
            open: RwSignal::new(false),
            id: RwSignal::new(Id::default()),
            project: RwSignal::new(Id::default()),
            title: RwSignal::new(String::new()),
            description: RwSignal::new(String::new()),
            column: RwSignal::new(Id::default()),
            due: RwSignal::new(String::new()),
            priority: RwSignal::new(0),
            archived: RwSignal::new(false),
            saved: StoredValue::new(Value::Null),
        }
    }

    fn edit(&self, t: Task) {
        self.id.set(t.id);
        self.project.set(t.project_id);
        self.title.set(t.title);
        self.description.set(t.description);
        self.column.set(t.column_id);
        self.due.set(t.due.unwrap_or_default());
        self.priority.set(t.priority);
        self.archived.set(t.archived == 1);
        self.saved.set_value(self.payload());
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

    fn payload(&self) -> Value {
        serde_json::to_value(self.task()).unwrap_or_default()
    }
}

#[derive(Clone, Copy)]
struct Filters {
    q: RwSignal<String>,
    projects: RwSignal<HashSet<String>>,
    project_mode: RwSignal<String>,
    due: Signal<String>,
    order: Signal<String>,
    state: Signal<String>,
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
        let only = self.project_mode.get() == "only";
        let project_ok = self.projects.with(|p| p.is_empty() || p.contains(t.project_id.as_str()) == only);
        let state_ok = (t.archived == 1) == (self.state.get() == "archived");
        state_ok && due_ok && project_ok && (has(&t.title, &self.q.get()) || has(&t.description, &self.q.get()))
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
fn Board(#[prop(optional)] project: Option<Id>) -> impl IntoView {
    let ui = use_ui();
    let path = move |what: &str| project.map_or(format!("/api/board/{what}"), |p| format!("/api/projects/{p}/{what}"));
    let columns = LocalResource::new(move || async move { ui.run(api::get::<Vec<Column>>(&path("columns"))).await.unwrap_or_default() });
    let source = LocalResource::new(move || async move { ui.run(api::get::<Vec<Task>>(&path("tasks"))).await });
    let tasks = RwSignal::new(Vec::new());
    let ctx = BoardCtx {
        project,
        columns,
        tasks,
        source,
        dragging: RwSignal::new(None),
        over_task: RwSignal::new(None),
        over_column: RwSignal::new(None),
        dragging_column: RwSignal::new(None),
        pointer: RwSignal::new(None),
        drag_label: RwSignal::new(String::new()),
        scroll_timer: StoredValue::new(None),
        board: NodeRef::new(),
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
    let (state, set_state) = query_state("tasks", "active");
    let filters = Filters {
        q: RwSignal::new(String::new()),
        projects: RwSignal::new(HashSet::new()),
        project_mode: RwSignal::new("only".into()),
        due,
        order,
        state,
    };

    view! {
        <Toolbar>
            <div class="flex flex-wrap flex-1 gap-2 items-center min-w-0 max-w-full basis-full sm:basis-0">
                <SearchBox value=filters.q placeholder="Search tasks  /" />
                <Segmented options=TASK_STATE value=state on_change=set_state />
                <Segmented options=DUE value=due on_change=set_due />
                <Show when=move || project.is_none()>
                    <ProjectFilter filters />
                </Show>
            </div>
            <SortSelect options=ORDER value=order on_change=set_order />
        </Toolbar>
        <div
            node_ref=ctx.board
            class="flex overflow-x-auto gap-4 items-start px-1 pb-4 -mx-1 page-enter sm:snap-none"
            class=("snap-x", move || ctx.pointer.get().is_none())
            class=("snap-mandatory", move || ctx.pointer.get().is_none())
        >
            <For each=move || ctx.lanes() key=Column::clone let(column)>
                <BoardColumn column ctx filters />
            </For>
            {project.map(|project| view! { <AddColumn project ctx /> })}
        </div>
        <DragGhost ctx />
        <TaskModal ctx />
    }
}

#[component]
fn AddColumn(project: Id, ctx: BoardCtx) -> impl IntoView {
    let name = RwSignal::new(String::new());
    let add = move |ev: SubmitEvent| {
        ev.prevent_default();
        let body = json!({ "name": name.get_untracked() });
        name.set(String::new());
        ctx.call(async move { api::post::<Value>(&format!("/api/projects/{project}/columns"), &body).await }, true);
    };
    view! {
        <form class="flex flex-col gap-2 p-3 w-72 rounded-xl border border-dashed transition-colors shrink-0 hover:bg-muted/30" on:submit=add>
            <span class="text-sm font-medium text-muted-foreground">"Add column"</span>
            <Input class="bg-background" placeholder="Column name" bind_value=name required=true />
        </form>
    }
}

#[component]
fn ProjectFilter(filters: Filters) -> impl IntoView {
    let ui = use_ui();
    let projects = LocalResource::new(move || async move {
        let list = ui.run(api::get::<Vec<ProjectItem>>("/api/projects")).await.unwrap_or_default();
        let mut list: Vec<_> = list.into_iter().filter(|p| p.archived == 0).map(|p| (p.id.to_string(), p.name)).collect();
        list.sort_by_key(|(_, name)| name.to_lowercase());
        list
    });
    let q = RwSignal::new(String::new());
    let visible = move || projects.get().unwrap_or_default().into_iter().filter(|(_, name)| has(name, &q.get())).collect::<Vec<_>>();
    let all_visible_selected = Signal::derive(move || {
        let shown = visible();
        !shown.is_empty() && filters.projects.with(|p| shown.iter().all(|(id, _)| p.contains(id)))
    });
    let toggle_visible = move |_| {
        let select = !all_visible_selected.get_untracked();
        filters.projects.update(|p| {
            for (id, _) in visible() {
                if select { p.insert(id) } else { p.remove(&id) };
            }
        });
    };

    view! {
        <MultiSelect values=filters.projects align=MultiSelectAlign::Start>
            <MultiSelectTrigger class="w-48">
                <MultiSelectValue placeholder="All projects" />
            </MultiSelectTrigger>
            <MultiSelectContent class="overflow-visible p-0 w-72 max-h-none">
                <div class="flex flex-col gap-1 p-2 border-b">
                    <Input class="h-8" placeholder="Search projects" bind_value=q />
                    <div class="flex justify-between items-center">
                        <div class="flex flex-1 gap-2 items-center py-1.5 px-2 text-sm rounded-sm cursor-pointer hover:bg-accent" on:click=toggle_visible>
                            <Checkbox class="pointer-events-none" checked=all_visible_selected aria_label="Select all" />
                            "Select all"
                        </div>
                        <Button
                            variant=ButtonVariant::Ghost
                            size=ButtonSize::Sm
                            attr:disabled=move || filters.projects.with(HashSet::is_empty)
                            on:click=move |_| filters.projects.update(HashSet::clear)
                        >
                            "Clear"
                        </Button>
                    </div>
                </div>
                <div class="flex overflow-y-auto flex-col p-1 max-h-72">
                    <For each=visible key=|(id, _)| id.clone() let((id, name))>
                        <ProjectOption id name selected=filters.projects />
                    </For>
                    <Show when=move || visible().is_empty()>
                        <p class="py-4 text-sm text-center text-muted-foreground">"No projects found"</p>
                    </Show>
                </div>
            </MultiSelectContent>
        </MultiSelect>
        <Show when=move || filters.projects.with(|p| !p.is_empty())>
            <Segmented
                options=PROJECT_FILTER
                value=filters.project_mode.into()
                on_change=Callback::new(move |mode| filters.project_mode.set(mode))
            />
        </Show>
    }
}

#[component]
fn ProjectOption(id: String, name: String, selected: RwSignal<HashSet<String>>) -> impl IntoView {
    let id = StoredValue::new(id);
    let checked = Signal::derive(move || selected.with(|s| id.with_value(|id| s.contains(id))));
    let toggle = move |_| {
        let id = id.get_value();
        selected.update(|s| {
            if !s.remove(&id) {
                s.insert(id);
            }
        });
    };
    view! {
        <div
            role="option"
            aria-selected=move || checked.get().to_string()
            class="flex gap-2 items-center py-1.5 px-2 text-sm rounded-sm cursor-pointer hover:bg-accent hover:text-accent-foreground"
            on:click=toggle
        >
            <Checkbox class="pointer-events-none" checked aria_label=name.clone() />
            <span class="truncate">{name}</span>
        </div>
    }
}

#[component]
fn DragGhost(ctx: BoardCtx) -> impl IntoView {
    let at = move || ctx.pointer.get().unwrap_or_default();
    view! {
        <Show when=move || ctx.pointer.get().is_some()>
            <div
                class="fixed z-50 p-3 w-56 text-sm font-medium rounded-lg border shadow-lg -translate-x-1/2 -translate-y-full pointer-events-none truncate bg-card"
                style:left=move || format!("{}px", at().0)
                style:top=move || format!("{}px", at().1)
            >
                {move || ctx.drag_label.get()}
            </div>
        </Show>
    }
}

#[component]
fn DragGrip(
    ctx: BoardCtx,
    label: String,
    title: &'static str,
    #[prop(into)] on_start: Callback<()>,
    #[prop(into)] on_finish: Callback<()>,
    #[prop(optional, into)] class: String,
) -> impl IntoView {
    view! {
        <Button
            variant=ButtonVariant::Ghost
            size=ButtonSize::IconXs
            class=format!("cursor-grab touch-none active:cursor-grabbing {class}")
            attr:title=title
            on:pointerdown=move |ev: PointerEvent| {
                if ev.button() == 0 {
                    on_start.run(());
                    ctx.begin_pointer_drag(&ev, label.clone());
                }
            }
            on:pointermove=move |ev: PointerEvent| ctx.track_pointer(&ev)
            on:pointerup=move |_| on_finish.run(())
            on:pointercancel=move |_| ctx.end_drag()
            on:click=move |ev: MouseEvent| ev.stop_propagation()
        >
            <GripVertical />
        </Button>
    }
}

#[component]
fn ColumnHeader(column: Column, count: Signal<usize>, ctx: BoardCtx) -> impl IntoView {
    let id = column.id;
    let done = column.done == 1;
    let editing = RwSignal::new(false);
    let name = RwSignal::new(column.name.clone());
    let rename = move |ev: SubmitEvent| {
        ev.prevent_default();
        editing.set(false);
        let body = json!({ "name": name.get_untracked() });
        ctx.call(async move { api::put::<Value>(&format!("/api/columns/{id}"), &body).await }, true);
    };
    let step = move |ev: KeyboardEvent| {
        let delta = match ev.key().as_str() {
            "ArrowLeft" => -1,
            "ArrowRight" => 1,
            _ => return,
        };
        ev.prevent_default();
        let at = ctx.lanes().iter().position(|c| c.id == id);
        if let Some(index) = at.and_then(|a| a.checked_add_signed(delta)) {
            ctx.move_column(id, index);
        }
    };
    let toggle_done = move |_| {
        let body = json!({ "done": !done });
        ctx.call(async move { api::put::<Value>(&format!("/api/columns/{id}/done"), &body).await }, true);
    };
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
                        {done.then(|| view! { <CircleCheck class="size-3.5 shrink-0 text-primary" /> })}
                        <span class="flex-1 text-sm font-semibold truncate" on:dblclick=move |_| editing.set(ctx.project.is_some())>{name.get()}</span>
                        <Badge variant=BadgeVariant::Secondary class="tabular-nums">{move || count.get()}</Badge>
                    }
                }
            >
                <form class="flex-1" on:submit=rename>
                    <Input class="h-8 bg-background" bind_value=name autofocus=true />
                </form>
            </Show>
            <div class="flex opacity-0 transition-opacity group-hover/col:opacity-100 pointer-coarse:opacity-100" class=("hidden", ctx.project.is_none())>
                <DragGrip
                    ctx
                    label=column.name.clone()
                    title="Drag to reorder, or use the arrow keys"
                    on_start=move |_| ctx.dragging_column.set(Some(id))
                    on_finish=move |_| ctx.drop_column_at_pointer()
                    on:keydown=step
                />
                <Button
                    variant=ButtonVariant::Ghost
                    size=ButtonSize::IconXs
                    attr:title=if done { "Stop counting tasks here as done" } else { "Count tasks here as done" }
                    attr:aria-pressed=done.to_string()
                    on:click=toggle_done
                >
                    <CircleCheck />
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
    let lane = StoredValue::new(column.clone());
    let title = RwSignal::new(String::new());
    let over = move || ctx.over_column.get() == Some(id);
    let at_end = move || over() && ctx.over_task.get().is_none() && ctx.dragging.get().is_some();
    let mine = move || {
        let members: HashSet<Id> = lane.with_value(|l| ctx.lane_columns(l)).into_iter().map(|c| c.id).collect();
        filters.apply(ctx.tasks.get().into_iter().filter(|t| members.contains(&t.column_id)).collect())
    };
    let count = Signal::derive(move || mine().len());
    let limit = RwSignal::new(PAGE);
    let visible = move || mine().into_iter().take(limit.get()).collect::<Vec<_>>();
    let hidden = move || count.get().saturating_sub(limit.get());
    let show_more = move || limit.update(|l| *l += PAGE);
    let list = NodeRef::<html::Div>::new();
    let on_scroll = move |_| {
        let Some(el) = list.get() else { return };
        if el.scroll_top() + el.client_height() >= el.scroll_height() - 120 && hidden() > 0 {
            show_more();
        }
    };
    let add = move |ev: SubmitEvent| {
        ev.prevent_default();
        let Some(project) = ctx.project else { return };
        let body = json!({ "title": title.get_untracked(), "column_id": id });
        title.set(String::new());
        ctx.call(async move { api::post::<Value>(&format!("/api/projects/{project}/tasks"), &body).await }, false);
    };

    view! {
        <div
            data-lane=id.to_string()
            class="flex flex-col gap-2 p-2 w-72 rounded-xl border transition-all duration-200 shrink-0 snap-start scroll-ml-1 bg-muted/40 max-h-[calc(100dvh-10rem)]"
            class=("opacity-40", move || ctx.dragging_column.get() == Some(id))
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
                lane.with_value(|l| ctx.drop_at(l, None));
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
                <For each=visible key=Task::clone let(task)>
                    <TaskCard task lane ctx />
                </For>
                <Show when=move || hidden() != 0>
                    <Button variant=ButtonVariant::Ghost size=ButtonSize::Sm class="shrink-0 text-muted-foreground" on:click=move |_| show_more()>
                        {move || format!("Show {} more", hidden().min(PAGE))}
                    </Button>
                </Show>
                <div class="h-1 rounded-full transition-all shrink-0 bg-primary" class=("opacity-0", move || !at_end()) />
            </div>
            <form on:submit=add on:dragover=move |_| ctx.over_task.set(None) class=("hidden", move || ctx.project.is_none() || filters.state.get() == "archived")>
                <Input
                    class="bg-transparent border-transparent shadow-none hover:bg-background focus:bg-background dark:bg-transparent"
                    placeholder="+ Add task"
                    bind_value=title
                />
            </form>
        </div>
    }
}

pub fn priority_class(p: i64) -> &'static str {
    match p {
        3 => "bg-destructive/10 text-destructive",
        2 => "bg-warning/20 text-foreground",
        _ => "bg-muted text-muted-foreground",
    }
}

pub fn priority_label(p: i64) -> &'static str {
    PRIORITIES.iter().find(|(k, _)| *k == p).map_or("", |(_, l)| *l)
}

#[component]
pub fn DueChip(due: String, done: bool) -> impl IntoView {
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
fn TaskCard(task: Task, lane: StoredValue<Column>, ctx: BoardCtx) -> impl IntoView {
    let id = task.id;
    let edit_task = task.clone();
    let cover = md::first_image(&task.description);
    let description = md::plain(&task.description);
    let has_description = !description.is_empty();
    let subtasks = (task.subtasks > 0).then(|| format!("{}/{}", task.subtasks_done, task.subtasks));
    let created = fmt_date(task.created);
    let priority = task.priority;
    let column = task.column_id;
    let done = Signal::derive(move || ctx.is_done(column));
    let project_name = ctx.project.is_none().then_some(task.project);
    let before = move || ctx.over_task.get() == Some(id) && ctx.dragging.get().is_some_and(|d| d != id);
    let label = task.title.clone();

    view! {
        <div
            data-task=id.to_string()
            class="flex flex-col gap-1"
            on:dragover=move |ev: DragEvent| {
                ev.prevent_default();
                ctx.over_task.set(Some(id));
            }
            on:drop=move |ev: DragEvent| {
                ev.prevent_default();
                ev.stop_propagation();
                lane.with_value(|l| ctx.drop_at(l, Some(id)));
            }
        >
            <div class="h-1 rounded-full transition-all shrink-0 bg-primary" class=("hidden", move || !before()) />
            <div
                draggable="true"
                class="flex relative flex-col gap-2 p-3 text-sm rounded-lg border shadow-xs shrink-0 cursor-grab active:cursor-grabbing bg-card lift animate-in fade-in-0 zoom-in-95"
                class=("opacity-40", move || ctx.dragging.get() == Some(id))
                on:dragstart=move |ev: DragEvent| {
                    if ctx.pointer.get_untracked().is_some() {
                        ev.prevent_default();
                        return;
                    }
                    if let Some(dt) = ev.data_transfer() {
                        let _ = dt.set_data("text/plain", &id.to_string());
                    }
                    ctx.dragging.set(Some(id));
                }
                on:dragend=move |_| ctx.end_drag()
                on:click=move |_| ctx.form.edit(edit_task.clone())
            >
                {cover.map(|src| view! { <img src=src alt="" loading="lazy" draggable="false" class="object-cover w-full h-32 rounded-md bg-muted" /> })}
                <DragGrip
                    ctx
                    label
                    title="Drag to another column"
                    class="hidden absolute top-1 right-1 pointer-coarse:inline-flex size-8 bg-card/80"
                    on_start=move |_| ctx.dragging.set(Some(id))
                    on_finish=move |_| ctx.drop_task_at_pointer()
                />
                <span class="font-medium leading-snug pointer-coarse:pr-7" class=("line-through", done) class=("text-muted-foreground", done)>
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
                {project_name.map(|name| view! {
                    <div class="flex gap-1 items-center pt-2 text-xs border-t text-muted-foreground">
                        <FolderKanban class="size-3 shrink-0" />
                        <span class="truncate">{name}</span>
                    </div>
                })}
            </div>
        </div>
    }
}

#[component]
fn TaskModal(ctx: BoardCtx) -> impl IntoView {
    let form = ctx.form;
    let upload_project = RwSignal::new(None);
    Effect::new(move |_| upload_project.set(Some(form.project.get())));
    let cover = move || md::first_image(&form.description.get());
    let remove = move |_| {
        ctx.ui.confirm_delete("This task will be deleted.", move || {
            let id = form.id.get_untracked();
            form.open.set(false);
            ctx.call(async move { api::del(&format!("/api/tasks/{id}")).await }, false);
        });
    };
    let archive = move |_| {
        let id = form.id.get_untracked();
        let action = if form.archived.get_untracked() { "unarchive" } else { "archive" };
        form.open.set(false);
        ctx.call(async move { api::post::<Value>(&format!("/api/tasks/{id}/action/{action}"), &()).await }, false);
    };
    let remind_open = RwSignal::new(false);
    let reminders_reload = RwSignal::new(0u32);
    let saver = AutoSave::new();
    Effect::new(move |_| {
        form.title.track();
        form.description.track();
        form.due.track();
        form.priority.track();
        form.column.track();
        let body = form.payload();
        if !form.open.get_untracked() || form.saved.with_value(|s| *s == body) {
            return;
        }
        form.saved.set_value(body.clone());
        saver.send(format!("/api/tasks/{}", form.id.get_untracked()), body);
    });
    Effect::new(move |_| {
        if !form.open.get() && saver.state.get() == SaveState::Saved {
            saver.state.set(SaveState::Idle);
            ctx.source.refetch();
        }
    });

    view! {
        <Modal open=form.open title="Edit task" class="max-w-[min(72rem,calc(100%-2rem))]">
            <div class="grid grid-cols-1 gap-6 lg:grid-cols-[minmax(0,1fr)_17rem]">
                <div class="flex flex-col gap-4 min-w-0">
                    <RichEditor title=form.title body=form.description on_change=Callback::new(|_| ()) upload_project allow_upload=true compact=true height="h-72" />
                    <Subtasks ctx />
                </div>
                <aside class="flex flex-col gap-4">
                    <SaveBadge state=saver.state />
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
                                ctx.project_columns(form.project.get()).into_iter().map(|c| (c.id.to_string(), c.name)).collect::<Vec<_>>()
                            })
                            value=Signal::derive(move || form.column.get().to_string())
                            on_change=Callback::new(move |picked: String| form.column.set(picked.parse().unwrap_or_default()))
                        />
                    </div>
                    <Button class="w-full" variant=ButtonVariant::Outline on:click=move |_| remind_open.set(true)>
                        <Bell />
                        "Add reminder"
                    </Button>
                    <TaskReminders task_id=Signal::derive(move || form.id.get().to_string()) reload=reminders_reload />
                    <div class="grid grid-cols-2 gap-2 pt-2 lg:mt-auto">
                        <Button class="w-full" variant=ButtonVariant::Outline on:click=archive>
                            {move || if form.archived.get() { view! { <ArchiveRestore /> }.into_any() } else { view! { <Archive /> }.into_any() }}
                            {move || if form.archived.get() { "Unarchive" } else { "Archive" }}
                        </Button>
                        <Button class="w-full" variant=ButtonVariant::Destructive on:click=remove>
                            <Trash2 />
                            "Delete"
                        </Button>
                    </div>
                </aside>
            </div>
        </Modal>
        <div class="relative z-110">
            <Modal open=remind_open title="Add reminder" class="max-w-[min(40rem,calc(100%-2rem))]">
                {move || remind_open.get().then(|| view! {
                    <ReminderForm
                        task_id=form.id.get_untracked().to_string()
                        title=form.title.get_untracked()
                        on_done=Callback::new(move |_| {
                            remind_open.set(false);
                            reminders_reload.update(|n| *n += 1);
                        })
                    />
                })}
            </Modal>
        </div>
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
    let paste = move |ev: leptos::ev::ClipboardEvent| {
        let text = ev.clipboard_data().and_then(|d| d.get_data("text").ok()).unwrap_or_default();
        let lines: Vec<String> = text.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_owned).collect();
        if lines.len() < 2 {
            return;
        }
        ev.prevent_default();
        let id = form.id.get_untracked();
        let path = format!("/api/tasks/{id}/subtasks");
        spawn_local(async move {
            for line in lines {
                if ctx.ui.run(api::post::<Value>(&path, &json!({ "title": line }))).await.is_none() {
                    break;
                }
            }
            list.refetch();
            ctx.source.refetch();
        });
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
                <Input placeholder="+ Add subtask" bind_value=title on:paste=paste />
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
    let upload_project = RwSignal::new(Some(ctx.form.project.get_untracked()));
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
                    class="opacity-0 group-hover/sub:opacity-100 focus-visible:opacity-100 pointer-coarse:opacity-100"
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
                        height="h-28"
                        placeholder="Add details..."
                    />
                    <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
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
