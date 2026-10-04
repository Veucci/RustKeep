use icons::{Calendar, ChevronLeft, ChevronRight, Download, FolderKanban, Pencil, Plus, Trash2};
use leptos::ev::SubmitEvent;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::A;
use leptos_router::hooks::{use_navigate, use_params_map};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use web_sys::DragEvent;

use crate::api;
use crate::components::ui::badge::{Badge, BadgeVariant};
use crate::components::ui::button::{Button, ButtonSize, ButtonVariant};
use crate::components::ui::card::{Card, CardContent, CardDescription, CardHeader, CardTitle};
use crate::components::ui::empty::{Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant, EmptyTitle};
use crate::components::ui::input::{Input, InputType};
use crate::components::ui::label::Label;
use crate::components::ui::progress::Progress;
use crate::components::ui::skeleton::Skeleton;
use crate::components::ui::textarea::Textarea;
use crate::pages::files::FileManager;
use crate::pages::notes::{NotesList, View};
use crate::widgets::{Modal, PageHeader, Ui, fmt_date, use_ui};

const STATUSES: [(&str, &str); 3] = [("active", "Active"), ("paused", "Paused"), ("completed", "Completed")];
const SELECT_CLASS: &str =
    "px-2 h-9 text-sm rounded-md border shadow-xs transition-colors bg-background border-input hover:bg-accent dark:bg-input/30";

#[derive(Clone, Deserialize)]
pub struct ProjectItem {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub status: String,
    pub tasks: i64,
    pub done: i64,
}

#[derive(Clone, Deserialize)]
struct Column {
    id: i64,
    name: String,
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct Task {
    id: i64,
    column_id: i64,
    title: String,
    description: String,
    due: Option<String>,
    #[serde(default, skip_serializing)]
    created: i64,
}

fn status_label(s: &str) -> &'static str {
    STATUSES.iter().find(|(k, _)| *k == s).map_or("Active", |(_, l)| *l)
}

fn percent(p: &ProjectItem) -> f64 {
    if p.tasks == 0 { 0.0 } else { p.done as f64 * 100.0 / p.tasks as f64 }
}

#[component]
pub fn Projects() -> impl IntoView {
    let ui = use_ui();
    let list = LocalResource::new(move || async move { ui.run(api::get::<Vec<ProjectItem>>("/api/projects")).await.unwrap_or_default() });
    let name = RwSignal::new(String::new());
    let description = RwSignal::new(String::new());
    let submit = move |ev: SubmitEvent| {
        ev.prevent_default();
        spawn_local(async move {
            let body = json!({ "name": name.get_untracked(), "description": description.get_untracked() });
            if ui.run(api::post::<Value>("/api/projects", &body)).await.is_some() {
                name.set(String::new());
                description.set(String::new());
                list.refetch();
            }
        });
    };

    view! {
        <div class="flex flex-col gap-6 mx-auto max-w-6xl page-enter">
            <PageHeader title="Projects" description="Boards, notes and files grouped by project." />
            <Card>
                <CardContent>
                    <form class="flex flex-col gap-2 sm:flex-row" on:submit=submit>
                        <Input bind_value=name placeholder="Project name" required=true />
                        <Input bind_value=description placeholder="Short description" />
                        <Button>
                            <Plus />
                            "Create project"
                        </Button>
                    </form>
                </CardContent>
            </Card>
            <div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
                {move || match list.get() {
                    None => (0..3).map(|_| view! { <Skeleton class="h-40 rounded-xl" /> }).collect_view().into_any(),
                    Some(items) if items.is_empty() => view! {
                        <Empty class="sm:col-span-2 lg:col-span-3">
                            <EmptyHeader>
                                <EmptyMedia variant=EmptyMediaVariant::Icon><FolderKanban /></EmptyMedia>
                                <EmptyTitle>"No projects yet"</EmptyTitle>
                                <EmptyDescription>"Create a project to get a board with To do, In progress and Done columns."</EmptyDescription>
                            </EmptyHeader>
                        </Empty>
                    }
                    .into_any(),
                    Some(items) => items.into_iter().map(|p| view! { <ProjectCard project=p /> }).collect_view().into_any(),
                }}
            </div>
        </div>
    }
}

#[component]
fn ProjectCard(project: ProjectItem) -> impl IntoView {
    let pct = percent(&project);
    view! {
        <A href=api::url(&format!("/projects/{}", project.id)) attr:class="block">
            <Card class="h-full lift">
                <CardHeader>
                    <div class="flex justify-between items-start">
                        <div class="flex justify-center items-center rounded-lg size-9 bg-muted">
                            <FolderKanban class="size-4 text-muted-foreground" />
                        </div>
                        <Badge variant=BadgeVariant::Outline>{status_label(&project.status)}</Badge>
                    </div>
                    <CardTitle class="mt-2">{project.name}</CardTitle>
                    <CardDescription class="line-clamp-2">{project.description}</CardDescription>
                </CardHeader>
                <CardContent class="flex flex-col gap-2 mt-auto">
                    <div class="flex justify-between text-xs text-muted-foreground">
                        <span>"Progress"</span>
                        <span class="tabular-nums">{format!("{}/{} tasks", project.done, project.tasks)}</span>
                    </div>
                    <Progress value=pct />
                </CardContent>
            </Card>
        </A>
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Board,
    Notes,
    Files,
}

#[component]
pub fn ProjectDetail() -> impl IntoView {
    let ui = use_ui();
    let params = use_params_map();
    let id = move || params.read().get("id").and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
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
    let tab = RwSignal::new(Tab::Board);
    let save = move || {
        spawn_local(async move {
            let body = json!({ "name": name.get_untracked(), "description": description.get_untracked(), "status": status.get_untracked() });
            if ui.run(api::put::<Value>(&format!("/api/projects/{id}"), &body)).await.is_some() {
                ui.notify("Project saved");
            }
        });
    };
    let remove = move |_| {
        if !window().confirm_with_message("Delete this project and its board? Notes and files are kept.").unwrap_or(false) {
            return;
        }
        let navigate = navigate.clone();
        spawn_local(async move {
            if ui.run(api::del(&format!("/api/projects/{id}"))).await.is_some() {
                navigate("/projects", Default::default());
            }
        });
    };
    let tab_btn = move |t: Tab, label: &'static str| {
        let class = move || {
            let active = if tab.get() == t { "bg-background text-foreground shadow-sm" } else { "text-muted-foreground hover:text-foreground" };
            format!("px-3 h-7 text-sm font-medium rounded-md transition-all {active}")
        };
        view! { <button class=class on:click=move |_| tab.set(t)>{label}</button> }
    };

    view! {
        <div class="flex flex-col gap-6 mx-auto max-w-screen-2xl page-enter">
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
                <select
                    class=SELECT_CLASS
                    on:change=move |ev| {
                        status.set(event_target_value(&ev));
                        save();
                    }
                >
                    {STATUSES
                        .iter()
                        .map(|(k, l)| view! { <option value=*k selected=move || status.get() == *k>{*l}</option> })
                        .collect_view()}
                </select>
                <Button variant=ButtonVariant::Outline href=api::url(&format!("/api/projects/{id}/export"))>
                    <Download />
                    "Export Excel"
                </Button>
                <Button variant=ButtonVariant::Ghost size=ButtonSize::Icon attr:title="Delete project" on:click=remove>
                    <Trash2 />
                </Button>
            </div>
            <div class="inline-flex gap-1 items-center p-1 rounded-lg w-fit bg-muted">
                {tab_btn(Tab::Board, "Board")}
                {tab_btn(Tab::Notes, "Notes")}
                {tab_btn(Tab::Files, "Files")}
            </div>
            {move || match tab.get() {
                Tab::Board => view! { <Board project=id /> }.into_any(),
                Tab::Notes => view! { <div class="page-enter"><NotesList view=View::Project(id) /></div> }.into_any(),
                Tab::Files => view! { <div class="flex flex-col gap-4 page-enter"><FileManager project=id /></div> }.into_any(),
            }}
        </div>
    }
}

#[derive(Clone, Copy)]
struct BoardCtx {
    project: i64,
    columns: LocalResource<Vec<Column>>,
    tasks: LocalResource<Vec<Task>>,
    dragging: RwSignal<Option<Task>>,
    form: TaskForm,
    ui: Ui,
}

impl BoardCtx {
    fn call(self, fut: impl Future<Output = api::ApiResult<Value>> + 'static, columns: bool) {
        spawn_local(async move {
            if self.ui.run(fut).await.is_some() {
                self.tasks.refetch();
                if columns {
                    self.columns.refetch();
                }
            }
        });
    }

    fn save_task(self, t: Task) {
        self.call(async move { api::put::<Value>(&format!("/api/tasks/{}", t.id), &t).await }, false);
    }

    fn drop_on(self, column_id: i64) {
        if let Some(mut t) = self.dragging.get_untracked().filter(|t| t.column_id != column_id) {
            t.column_id = column_id;
            self.save_task(t);
        }
        self.dragging.set(None);
    }
}

#[derive(Clone, Copy)]
struct TaskForm {
    open: RwSignal<bool>,
    id: RwSignal<i64>,
    title: RwSignal<String>,
    description: RwSignal<String>,
    column: RwSignal<i64>,
    due: RwSignal<String>,
}

impl TaskForm {
    fn new() -> Self {
        Self {
            open: RwSignal::new(false),
            id: RwSignal::new(0),
            title: RwSignal::new(String::new()),
            description: RwSignal::new(String::new()),
            column: RwSignal::new(0),
            due: RwSignal::new(String::new()),
        }
    }

    fn edit(&self, t: Task) {
        self.id.set(t.id);
        self.title.set(t.title);
        self.description.set(t.description);
        self.column.set(t.column_id);
        self.due.set(t.due.unwrap_or_default());
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
            created: 0,
        }
    }
}

#[component]
fn Board(project: i64) -> impl IntoView {
    let ui = use_ui();
    let columns = LocalResource::new(move || async move {
        ui.run(api::get::<Vec<Column>>(&format!("/api/projects/{project}/columns"))).await.unwrap_or_default()
    });
    let tasks = LocalResource::new(move || async move {
        ui.run(api::get::<Vec<Task>>(&format!("/api/projects/{project}/tasks"))).await.unwrap_or_default()
    });
    let ctx = BoardCtx { project, columns, tasks, dragging: RwSignal::new(None), form: TaskForm::new(), ui };
    let new_column = RwSignal::new(String::new());
    let add_column = move |ev: SubmitEvent| {
        ev.prevent_default();
        let body = json!({ "name": new_column.get_untracked() });
        new_column.set(String::new());
        ctx.call(async move { api::post::<Value>(&format!("/api/projects/{project}/columns"), &body).await }, true);
    };

    view! {
        <div class="flex overflow-x-auto gap-4 items-start px-1 pb-4 -mx-1 page-enter">
            {move || {
                let cols = columns.get().unwrap_or_default();
                let last = cols.last().map(|c| c.id);
                cols.into_iter().map(|c| view! { <BoardColumn is_last=Some(c.id) == last column=c ctx /> }).collect_view()
            }}
            <form
                class="flex flex-col gap-2 p-3 w-72 rounded-xl border border-dashed transition-colors shrink-0 hover:bg-muted/30"
                on:submit=add_column
            >
                <span class="text-sm font-medium text-muted-foreground">"Add column"</span>
                <Input class="bg-background" placeholder="Column name" bind_value=new_column required=true />
            </form>
        </div>
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
        if window().confirm_with_message("Delete this column and all of its tasks?").unwrap_or(false) {
            ctx.call(async move { api::del(&format!("/api/columns/{id}")).await }, true);
        }
    };

    view! {
        <div class="flex gap-1 items-center px-1 group/col min-h-8">
            <Show
                when=move || editing.get()
                fallback=move || {
                    view! {
                        <span class="flex-1 text-sm font-semibold truncate">{name.get()}</span>
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
fn BoardColumn(column: Column, is_last: bool, ctx: BoardCtx) -> impl IntoView {
    let id = column.id;
    let title = RwSignal::new(String::new());
    let over = RwSignal::new(false);
    let mine = move || ctx.tasks.get().unwrap_or_default().into_iter().filter(|t| t.column_id == id).collect::<Vec<_>>();
    let count = Signal::derive(move || mine().len());
    let add = move |ev: SubmitEvent| {
        ev.prevent_default();
        let body = json!({ "title": title.get_untracked(), "column_id": id });
        title.set(String::new());
        let project = ctx.project;
        ctx.call(async move { api::post::<Value>(&format!("/api/projects/{project}/tasks"), &body).await }, false);
    };

    view! {
        <div
            class="flex flex-col gap-2 p-2 w-72 rounded-xl border transition-all duration-200 shrink-0 bg-muted/40"
            class=("ring-2", move || over.get())
            class=("ring-primary/30", move || over.get())
            class=("bg-muted", move || over.get())
            on:dragover=move |ev: DragEvent| {
                ev.prevent_default();
                over.set(true);
            }
            on:dragleave=move |_| over.set(false)
            on:drop=move |ev: DragEvent| {
                ev.prevent_default();
                over.set(false);
                ctx.drop_on(id);
            }
        >
            <ColumnHeader column ctx count />
            {move || mine().into_iter().map(|t| view! { <TaskCard task=t done=is_last ctx /> }).collect_view()}
            <form on:submit=add>
                <Input
                    class="bg-transparent border-transparent shadow-none hover:bg-background focus:bg-background dark:bg-transparent"
                    placeholder="+ Add task"
                    bind_value=title
                />
            </form>
        </div>
    }
}

fn today() -> String {
    js_sys::Date::new_0().to_iso_string().as_string().unwrap_or_default().chars().take(10).collect()
}

#[component]
fn TaskCard(task: Task, done: bool, ctx: BoardCtx) -> impl IntoView {
    let drag_task = task.clone();
    let edit_task = task.clone();
    let overdue = !done && task.due.as_deref().is_some_and(|d| d < today().as_str());
    let due_class = if overdue { "bg-destructive/10 text-destructive" } else { "bg-muted text-muted-foreground" };
    let description = task.description.trim().to_owned();
    let has_description = !description.is_empty();
    let created = fmt_date(task.created);

    view! {
        <div
            draggable="true"
            class="flex flex-col gap-2 p-3 text-sm rounded-lg border shadow-xs cursor-grab active:cursor-grabbing bg-card lift animate-in fade-in-0 zoom-in-95"
            on:dragstart=move |ev: DragEvent| {
                if let Some(dt) = ev.data_transfer() {
                    let _ = dt.set_data("text/plain", &drag_task.id.to_string());
                }
                ctx.dragging.set(Some(drag_task.clone()));
            }
            on:click=move |_| ctx.form.edit(edit_task.clone())
        >
            <span class="font-medium leading-snug" class=("line-through", done) class=("text-muted-foreground", done)>
                {task.title}
            </span>
            <Show when=move || has_description>
                <p class="text-xs whitespace-pre-line text-muted-foreground line-clamp-4">{description.clone()}</p>
            </Show>
            <div class="flex flex-wrap gap-2 items-center text-xs text-muted-foreground">
                {task.due.map(|d| view! {
                    <span class=format!("inline-flex gap-1 items-center py-0.5 px-1.5 rounded-md {due_class}")>
                        <Calendar class="size-3" />
                        {d}
                    </span>
                })}
                <span class="ml-auto">{created}</span>
            </div>
        </div>
    }
}

#[component]
fn TaskModal(ctx: BoardCtx) -> impl IntoView {
    let form = ctx.form;
    let remove = move |_| {
        let id = form.id.get_untracked();
        form.open.set(false);
        ctx.call(async move { api::del(&format!("/api/tasks/{id}")).await }, false);
    };
    let submit = move |ev: SubmitEvent| {
        ev.prevent_default();
        form.open.set(false);
        ctx.save_task(form.task());
    };

    view! {
        <Modal open=form.open title="Edit task">
            <form class="flex flex-col gap-3" on:submit=submit>
                <Label>"Title"</Label>
                <Input bind_value=form.title required=true />
                <Label>"Description"</Label>
                <Textarea bind_value=form.description rows=4u32 />
                <div class="grid grid-cols-2 gap-3">
                    <div class="flex flex-col gap-2">
                        <Label>"Due date"</Label>
                        <Input r#type=InputType::Date bind_value=form.due />
                    </div>
                    <div class="flex flex-col gap-2">
                        <Label>"Column"</Label>
                        <select class=SELECT_CLASS on:change=move |ev| form.column.set(event_target_value(&ev).parse().unwrap_or_default())>
                            {move || {
                                ctx.columns
                                    .get()
                                    .unwrap_or_default()
                                    .into_iter()
                                    .map(|c| view! { <option value=c.id.to_string() selected=move || form.column.get() == c.id>{c.name}</option> })
                                    .collect_view()
                            }}
                        </select>
                    </div>
                </div>
                <div class="flex gap-2 justify-end">
                    <Button attr:r#type="button" variant=ButtonVariant::Destructive on:click=remove>"Delete"</Button>
                    <Button>"Save"</Button>
                </div>
            </form>
        </Modal>
    }
}
