use icons::{
    Archive, ArchiveRestore, ArrowLeft, Bell, Bold, Code, ExternalLink, Heading, Italic, Link, List, ListChecks, Paperclip, Pin,
    PinOff, Quote, Share2, Trash2,
};
use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::A;
use leptos_router::hooks::{use_navigate, use_params_map};
use serde::Deserialize;
use serde_json::{Value, json};
use web_sys::HtmlTextAreaElement;

use crate::id::Id;
use crate::api::{self, AutoSave, SaveState, client_id, next_rev};
use crate::components::ui::button::{Button, ButtonSize, ButtonVariant};
use crate::components::ui::input::Input;
use crate::components::ui::textarea::Textarea;
use crate::md;
use crate::pages::projects::ProjectItem;
use crate::widgets::{Modal, PinGate, ReminderForm, ShareDialog, use_ui};

#[derive(Clone, Deserialize)]
pub struct Note {
    id: String,
    title: String,
    body: String,
    project_id: Option<Id>,
    secret: i64,
    archived: i64,
    share_token: Option<String>,
    #[serde(default)]
    pinned: i64,
}

fn back_target(secret: bool, archived: bool, project: Option<Id>) -> (String, &'static str) {
    match (secret, archived, project) {
        (true, _, _) => ("/secret".into(), "Secret notes"),
        (_, true, _) => ("/archive?tab=notes".into(), "Archive"),
        (_, _, Some(p)) => (format!("/projects/{p}?tab=notes"), "Project notes"),
        _ => ("/notes".into(), "All notes"),
    }
}

#[component]
pub fn NoteEditor() -> impl IntoView {
    let ui = use_ui();
    let params = use_params_map();
    let id = move || params.read().get("id").unwrap_or_default();
    let pin_ok = Memo::new(move |_| ui.me.with(|m| m.as_ref().is_some_and(|m| m.pin_ok)));
    let note = LocalResource::new(move || {
        let path = format!("/api/notes/{}", id());
        pin_ok.track();
        async move { api::get::<Note>(&path).await }
    });

    view! {
        {move || {
            note.get()
                .map(|res| match res {
                    Ok(n) => view! { <EditorBody note=n /> }.into_any(),
                    Err(e) if e.status == 403 => view! { <PinGate><span /></PinGate> }.into_any(),
                    Err(e) => view! { <p class="text-muted-foreground">{e.msg}</p> }.into_any(),
                })
        }}
    }
}

#[component]
pub fn SaveBadge(state: RwSignal<SaveState>) -> impl IntoView {
    let text = move || match state.get() {
        SaveState::Idle => "",
        SaveState::Saving => "Saving...",
        SaveState::Saved => "Saved",
        SaveState::Failed => "Save failed",
    };
    let dot = move || match state.get() {
        SaveState::Saving => "bg-warning animate-pulse",
        SaveState::Failed => "bg-destructive",
        _ => "bg-success",
    };
    view! {
        <span class="flex gap-1.5 items-center text-xs text-muted-foreground">
            <span class=move || format!("size-1.5 rounded-full transition-colors {}", dot()) />
            {text}
        </span>
    }
}

#[component]
fn EditorBody(note: Note) -> impl IntoView {
    let ui = use_ui();
    let navigate = use_navigate();
    let id = StoredValue::new(note.id.clone());
    let secret = note.secret == 1;
    let title = RwSignal::new(note.title);
    let body = RwSignal::new(note.body);
    let project = RwSignal::new(note.project_id);
    let archived = RwSignal::new(note.archived == 1);
    let share_token = RwSignal::new(note.share_token);
    let pinned = RwSignal::new(note.pinned == 1);
    let back = move || back_target(secret, archived.get(), project.get());
    Effect::new(move |_| ui.set_crumb(title.get()));
    let share_open = RwSignal::new(false);
    let remind_open = RwSignal::new(false);
    let saver = AutoSave::new();
    let save = Callback::new(move |_| {
        let payload = json!({
            "title": title.get_untracked(), "body": body.get_untracked(), "project_id": project.get_untracked(),
            "rev": next_rev(), "client": client_id(),
        });
        saver.send(format!("/api/notes/{}", id.get_value()), payload);
    });
    let projects = LocalResource::new(move || async move {
        ui.run(api::get::<Vec<ProjectItem>>("/api/projects")).await.unwrap_or_default()
    });

    let toggle_archive = move |_| {
        let action = if archived.get_untracked() { "unarchive" } else { "archive" };
        spawn_local(async move {
            if ui.run(api::post::<Value>(&format!("/api/notes/{}/action/{action}", id.get_value()), &())).await.is_some() {
                archived.update(|a| *a = !*a);
            }
        });
    };
    let toggle_pin = move |_| {
        let action = if pinned.get_untracked() { "unpin" } else { "pin" };
        ui.act(format!("/api/notes/{}/action/{action}", id.get_value()), move || pinned.update(|p| *p = !*p));
    };
    let trash = move |_| {
        if secret && !window().confirm_with_message("Secret notes are deleted permanently. Continue?").unwrap_or(false) {
            return;
        }
        let navigate = navigate.clone();
        spawn_local(async move {
            let path = format!("/api/notes/{}", id.get_value());
            let res = if secret { api::del(&path).await } else { api::post::<Value>(&format!("{path}/action/trash"), &()).await };
            if ui.run(std::future::ready(res)).await.is_some() {
                navigate(if secret { "/secret" } else { "/notes" }, Default::default());
            }
        });
    };
    let pick_project = move |ev| {
        project.set(event_target_value(&ev).parse::<Id>().ok());
        save.run(());
    };

    view! {
        <div class="flex flex-col gap-4 mx-auto max-w-6xl page-enter">
            <div class="flex flex-wrap gap-2 items-center">
                <A
                    href=move || api::url(&back().0)
                    attr:class="inline-flex gap-1.5 items-center px-2.5 h-8 text-sm font-medium rounded-md transition-colors hover:bg-accent [&_svg]:size-4"
                >
                    <ArrowLeft />
                    {move || back().1}
                </A>
                <SaveBadge state=saver.state />
                <div class="flex-1" />
                <Show when=move || !secret>
                    <select
                        class="px-2 h-8 text-sm rounded-md border shadow-xs transition-colors bg-background border-input hover:bg-accent dark:bg-input/30"
                        on:change=pick_project
                    >
                        <option value="" selected=move || project.get().is_none()>"No project"</option>
                        {move || {
                            projects
                                .get()
                                .unwrap_or_default()
                                .into_iter()
                                .map(|p| {
                                    let pid = p.id;
                                    view! { <option value=pid.to_string() selected=move || project.get() == Some(pid)>{p.name}</option> }
                                })
                                .collect_view()
                        }}
                    </select>
                    {move || project.get().map(|p| view! {
                        <Button variant=ButtonVariant::Ghost size=ButtonSize::IconSm attr:title="Open project" href=api::url(&format!("/projects/{p}"))>
                            <ExternalLink />
                        </Button>
                    })}
                    <Button variant=ButtonVariant::Outline size=ButtonSize::Sm attr:title=move || if pinned.get() { "Unpin" } else { "Pin to top" } on:click=toggle_pin>
                        {move || if pinned.get() { view! { <PinOff /> }.into_any() } else { view! { <Pin /> }.into_any() }}
                    </Button>
                    <Button variant=ButtonVariant::Outline size=ButtonSize::Sm on:click=move |_| share_open.set(true)>
                        <Share2 />
                        "Share"
                    </Button>
                    <Button variant=ButtonVariant::Outline size=ButtonSize::Sm on:click=toggle_archive>
                        {move || if archived.get() { view! { <ArchiveRestore /> "Unarchive" }.into_any() } else { view! { <Archive /> "Archive" }.into_any() }}
                    </Button>
                </Show>
                <Button variant=ButtonVariant::Outline size=ButtonSize::Sm on:click=move |_| remind_open.set(true)>
                    <Bell />
                    "Remind"
                </Button>
                <Button variant=ButtonVariant::Outline size=ButtonSize::Sm on:click=trash>
                    <Trash2 />
                    {if secret { "Delete" } else { "Trash" }}
                </Button>
            </div>
            <MdEditor title body on_change=save upload_project=project allow_upload=!secret />
        </div>
        <ShareDialog open=share_open api_path=format!("/api/notes/{}/share", id.get_value()) link_prefix="/s/" token=share_token />
        <Modal open=remind_open title="Add reminder">
            <ReminderForm note_id=Some(id.get_value()) title=if secret { String::new() } else { title.get_untracked() } on_done=Callback::new(move |_| remind_open.set(false)) />
        </Modal>
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Write,
    Split,
    Preview,
}

fn apply(ta: &HtmlTextAreaElement, body: RwSignal<String>, before: &str, after: &str, line: bool) {
    let mut v: Vec<u16> = body.get_untracked().encode_utf16().collect();
    let start = (ta.selection_start().ok().flatten().unwrap_or(0) as usize).min(v.len());
    let end = (ta.selection_end().ok().flatten().unwrap_or(0) as usize).clamp(start, v.len());
    let at = if line { v[..start].iter().rposition(|&c| c == u16::from(b'\n')).map_or(0, |i| i + 1) } else { start };
    let b: Vec<u16> = before.encode_utf16().collect();
    v.splice(end..end, after.encode_utf16());
    v.splice(at..at, b.iter().copied());
    let text = String::from_utf16_lossy(&v);
    ta.set_value(&text);
    body.set(text);
    let shift = b.len() as u32;
    let _ = ta.set_selection_range(start as u32 + shift, end as u32 + shift);
    let _ = ta.focus();
}

fn is_image(name: &str) -> bool {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    ["png", "jpg", "jpeg", "gif", "webp", "svg", "avif"].contains(&ext.as_str())
}

#[component]
pub fn MdEditor(
    title: RwSignal<String>,
    body: RwSignal<String>,
    on_change: Callback<()>,
    #[prop(optional)] upload_project: Option<RwSignal<Option<Id>>>,
    #[prop(optional)] allow_upload: bool,
) -> impl IntoView {
    let ui = use_ui();
    let ta = NodeRef::<html::Textarea>::new();
    let file_input = NodeRef::<html::Input>::new();
    let mode = RwSignal::new(Mode::Split);
    let format = move |before: &str, after: &str, line: bool| {
        if let Some(el) = ta.get() {
            apply(&el, body, before, after, line);
            on_change.run(());
        }
    };
    let upload = move |_| {
        let Some(input) = file_input.get() else { return };
        let files = input.files().map(|l| api::files_of(&l)).unwrap_or_default();
        input.set_value("");
        let project = upload_project.and_then(|p| p.get_untracked());
        spawn_local(async move {
            let Some(saved) = ui.run(api::upload(files, project)).await else { return };
            let links: String = saved
                .iter()
                .map(|f| {
                    let bang = if is_image(&f.name) { "!" } else { "" };
                    format!("{bang}[{}]({})\n", f.name, api::url(&format!("/api/files/{}/raw", f.id)))
                })
                .collect();
            format(&links, "", false);
        });
    };
    let mode_btn = move |m: Mode, label: &'static str| {
        let variant = Signal::derive(move || if mode.get() == m { ButtonVariant::Secondary } else { ButtonVariant::Ghost });
        view! { <Button variant size=ButtonSize::Sm on:click=move |_| mode.set(m)>{label}</Button> }
    };
    let tool = |icon: AnyView, before: &'static str, after: &'static str, line: bool| {
        view! { <Button variant=ButtonVariant::Ghost size=ButtonSize::IconSm on:click=move |_| format(before, after, line)>{icon}</Button> }
    };
    let grid = move || if mode.get() == Mode::Split { "grid gap-4 md:grid-cols-2" } else { "grid gap-4" };

    view! {
        <Input
            class="px-0 h-auto text-2xl font-bold bg-transparent border-0 shadow-none md:text-3xl focus-visible:ring-0 dark:bg-transparent"
            placeholder="Title"
            bind_value=title
            on:input=move |ev| {
                title.set(event_target_value(&ev));
                on_change.run(());
            }
        />
        <div class="flex sticky top-14 z-10 flex-wrap gap-1 items-center p-1 rounded-lg border shadow-xs backdrop-blur bg-background/90">
            {tool(view! { <Bold /> }.into_any(), "**", "**", false)}
            {tool(view! { <Italic /> }.into_any(), "*", "*", false)}
            {tool(view! { <Heading /> }.into_any(), "## ", "", true)}
            {tool(view! { <List /> }.into_any(), "- ", "", true)}
            {tool(view! { <ListChecks /> }.into_any(), "- [ ] ", "", true)}
            {tool(view! { <Quote /> }.into_any(), "> ", "", true)}
            {tool(view! { <Code /> }.into_any(), "```\n", "\n```", false)}
            {tool(view! { <Link /> }.into_any(), "[", "](https://)", false)}
            <Show when=move || allow_upload>
                <Button variant=ButtonVariant::Ghost size=ButtonSize::IconSm on:click=move |_| { if let Some(i) = file_input.get() { i.click() } }>
                    <Paperclip />
                </Button>
            </Show>
            <input type="file" multiple class="hidden" node_ref=file_input on:change=upload />
            <div class="flex-1" />
            {mode_btn(Mode::Write, "Write")}
            {mode_btn(Mode::Split, "Split")}
            {mode_btn(Mode::Preview, "Preview")}
        </div>
        <div class=grid>
            <div class:hidden=move || mode.get() == Mode::Preview>
                <Textarea
                    class="p-4 font-mono text-sm leading-6 rounded-xl min-h-[65vh] resize-y bg-card"
                    placeholder="Write in Markdown..."
                    bind_value=body
                    node_ref=ta
                    on:input=move |ev| {
                        body.set(event_target_value(&ev));
                        on_change.run(());
                    }
                />
            </div>
            <Show when=move || mode.get() != Mode::Write>
                <div class="overflow-auto p-4 rounded-xl border md min-w-0 min-h-[65vh] bg-card animate-in fade-in-0" inner_html=move || md::render(&body.get()) />
            </Show>
        </div>
    }
}
