use icons::{
    Archive, ArchiveRestore, ArrowLeft, Bell, Bold, Code, ExternalLink, Heading1, Heading2, ImagePlus, Italic, Link, List,
    ListChecks, ListOrdered, Pin, PinOff, Quote, Share2, Strikethrough, Trash2, X,
};
use leptos::ev::{ClipboardEvent, DragEvent, MouseEvent, SubmitEvent};
use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::A;
use leptos_router::hooks::{use_navigate, use_params_map};
use serde::Deserialize;
use serde_json::{Value, json};
use wasm_bindgen::JsCast;
use web_sys::{File, HtmlDocument, HtmlElement, HtmlInputElement, HtmlTemplateElement, Range};

use crate::id::Id;
use crate::api::{self, AutoSave, SaveState, client_id, next_rev};
use crate::components::ui::button::{Button, ButtonSize, ButtonVariant};
use crate::components::ui::input::Input;
use crate::md;
use crate::pages::projects::ProjectItem;
use crate::widgets::{Choice, Modal, PinGate, ReminderForm, ShareDialog, use_ui};

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
        let navigate = navigate.clone();
        let run = move || {
            let navigate = navigate.clone();
            spawn_local(async move {
                let path = format!("/api/notes/{}", id.get_value());
                let res = if secret { api::del(&path).await } else { api::post::<Value>(&format!("{path}/action/trash"), &()).await };
                if ui.run(std::future::ready(res)).await.is_some() {
                    navigate(if secret { "/secret" } else { "/notes" }, Default::default());
                }
            });
        };
        if secret {
            ui.confirm_delete("Secret notes are deleted permanently.", run);
        } else {
            run();
        }
    };
    let pick_project = Callback::new(move |picked: String| {
        project.set(picked.parse::<Id>().ok());
        save.run(());
    });
    let project_options = Signal::derive(move || {
        let projects = projects.get().unwrap_or_default().into_iter().map(|p| (p.id.to_string(), p.name));
        std::iter::once((String::new(), "No project".to_owned())).chain(projects).collect::<Vec<_>>()
    });

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
                    <Choice
                        options=project_options
                        value=Signal::derive(move || project.get().map(|p| p.to_string()).unwrap_or_default())
                        on_change=pick_project
                    />
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
            <RichEditor title body on_change=save upload_project=project allow_upload=!secret />
        </div>
        <ShareDialog open=share_open api_path=format!("/api/notes/{}/share", id.get_value()) link_prefix="/s/" token=share_token />
        <Modal open=remind_open title="Add reminder">
            <ReminderForm note_id=Some(id.get_value()) title=if secret { String::new() } else { title.get_untracked() } on_done=Callback::new(move |_| remind_open.set(false)) />
        </Modal>
    }
}

fn is_image(name: &str) -> bool {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    ["png", "jpg", "jpeg", "gif", "webp", "svg", "avif"].contains(&ext.as_str())
}

#[derive(Clone, Copy)]
enum Tool {
    Command(&'static str),
    Block(&'static str),
    Insert(&'static str),
}

const STATE_COMMANDS: [&str; 5] = ["bold", "italic", "strikeThrough", "insertUnorderedList", "insertOrderedList"];
const SKIPPED_TAGS: [&str; 6] = ["style", "script", "meta", "title", "head", "template"];
const CHECKLIST: &str = "<ul><li><input type=\"checkbox\">&nbsp;</li></ul>";

fn html_document() -> Option<HtmlDocument> {
    document().dyn_into::<HtmlDocument>().ok()
}

fn exec(command: &str, value: &str) {
    if let Some(d) = html_document() {
        let _ = d.exec_command_with_show_ui_and_value(command, false, value);
    }
}

impl Tool {
    fn run(self, block: &str) {
        match self {
            Tool::Command(command) => exec(command, ""),
            Tool::Block(tag) if block == tag => exec("formatBlock", "<p>"),
            Tool::Block(tag) => exec("formatBlock", &format!("<{tag}>")),
            Tool::Insert(html) => exec("insertHTML", html),
        }
    }

    fn active(self, states: &[&str], block: &str) -> bool {
        match self {
            Tool::Command(command) => states.contains(&command),
            Tool::Block(tag) => block == tag,
            Tool::Insert(_) => false,
        }
    }
}

fn read_dom(node: &web_sys::Node) -> Vec<md::Node> {
    let children = node.child_nodes();
    (0..children.length()).filter_map(|i| children.get(i)).filter_map(|n| convert(&n)).collect()
}

fn convert(node: &web_sys::Node) -> Option<md::Node> {
    if node.node_type() == web_sys::Node::TEXT_NODE {
        return Some(md::Node::Text(node.text_content().unwrap_or_default()));
    }
    let el = node.dyn_ref::<web_sys::Element>()?;
    let tag = el.tag_name().to_ascii_lowercase();
    if SKIPPED_TAGS.contains(&tag.as_str()) {
        return None;
    }
    let attrs = el
        .get_attribute_names()
        .iter()
        .filter_map(|name| name.as_string())
        .map(|name| {
            let value = el.get_attribute(&name).unwrap_or_default();
            (name, value)
        })
        .collect();
    let checked = el.dyn_ref::<HtmlInputElement>().is_some_and(HtmlInputElement::checked);
    Some(md::Node::Element(md::Element { tag, attrs, checked, children: read_dom(node) }))
}

fn parse_html(html: &str) -> Vec<md::Node> {
    let template = document().create_element("template").ok().and_then(|e| e.dyn_into::<HtmlTemplateElement>().ok());
    let Some(template) = template else { return Vec::new() };
    template.set_inner_html(html);
    read_dom(&template.content())
}

fn selection_in(el: &web_sys::Node) -> Option<Range> {
    let selection = window().get_selection().ok().flatten()?;
    let range = (selection.range_count() > 0).then(|| selection.get_range_at(0).ok()).flatten()?;
    el.contains(range.common_ancestor_container().ok().as_ref()).then_some(range)
}

fn place_caret(el: &HtmlElement, saved: Option<Range>) {
    let _ = el.focus();
    let range = saved.or_else(|| {
        let end = document().create_range().ok()?;
        end.select_node_contents(el).ok()?;
        end.collapse_with_to_start(false);
        Some(end)
    });
    if let (Some(selection), Some(range)) = (window().get_selection().ok().flatten(), range) {
        let _ = selection.remove_all_ranges();
        let _ = selection.add_range(&range);
    }
}

fn enable_checkboxes(el: &web_sys::Element) {
    let Ok(list) = el.query_selector_all("input[type=checkbox]") else { return };
    for input in (0..list.length()).filter_map(|i| list.get(i)?.dyn_into::<web_sys::Element>().ok()) {
        let _ = input.remove_attribute("disabled");
    }
}

fn follow_link(ev: &MouseEvent) {
    if !(ev.ctrl_key() || ev.meta_key()) {
        return;
    }
    let anchor = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()).and_then(|e| e.closest("a").ok().flatten());
    let href = anchor.and_then(|a| a.get_attribute("href")).unwrap_or_default();
    if ["http://", "https://", "mailto:", "/"].iter().any(|p| href.starts_with(p)) {
        let _ = window().open_with_url_and_target(&href, "_blank");
    }
}

fn file_html(f: &api::Uploaded) -> String {
    let url = md::escape_attr(&api::url(&format!("/api/files/{}/raw", f.id)));
    let name = md::escape_attr(&f.name);
    if is_image(&f.name) { format!("<img src=\"{url}\" alt=\"{name}\">") } else { format!("<a href=\"{url}\">{name}</a>&nbsp;") }
}

fn link_href(raw: &str) -> String {
    let url = raw.trim();
    if url.contains(':') || url.starts_with(['/', '#']) { url.to_owned() } else { format!("https://{url}") }
}

#[component]
pub fn RichEditor(
    #[prop(optional)] title: Option<RwSignal<String>>,
    body: RwSignal<String>,
    on_change: Callback<()>,
    #[prop(optional)] upload_project: Option<RwSignal<Option<Id>>>,
    #[prop(optional)] allow_upload: bool,
    #[prop(optional)] compact: bool,
    #[prop(default = "min-h-[65vh]")] min_height: &'static str,
    #[prop(default = "Start writing...")] placeholder: &'static str,
) -> impl IntoView {
    let ui = use_ui();
    let editor = NodeRef::<html::Div>::new();
    let file_input = NodeRef::<html::Input>::new();
    let shown = StoredValue::new(None::<String>);
    let saved = StoredValue::new_local(None::<Range>);
    let states = RwSignal::new(Vec::<&'static str>::new());
    let block = RwSignal::new(String::new());
    let link_open = RwSignal::new(false);
    let link_url = RwSignal::new(String::new());
    let link_input = NodeRef::<html::Input>::new();
    Effect::new(move |_| {
        if let Some(input) = link_input.get() {
            let _ = input.focus();
        }
    });
    let toolbar_top = if compact { "top-0" } else { "top-14" };

    Effect::new(move |_| {
        let markdown = body.get();
        let Some(el) = editor.get() else { return };
        if shown.with_value(|s| s.as_deref() == Some(markdown.as_str())) {
            return;
        }
        el.set_inner_html(&md::render(&markdown));
        enable_checkboxes(&el);
        shown.set_value(Some(markdown));
    });
    let refresh = move || {
        let Some(d) = html_document() else { return };
        states.set(STATE_COMMANDS.iter().copied().filter(|c| d.query_command_state(c).unwrap_or(false)).collect());
        block.set(d.query_command_value("formatBlock").unwrap_or_default().to_ascii_lowercase());
    };
    let sync = move || {
        let Some(el) = editor.get_untracked() else { return };
        let markdown = md::to_markdown(&read_dom(&el));
        shown.set_value(Some(markdown.clone()));
        body.set(markdown);
        on_change.run(());
        refresh();
    };
    let remember = move || saved.set_value(editor.get_untracked().and_then(|el| selection_in(&el)));
    let insert = move |html: String| {
        let Some(el) = editor.get_untracked() else { return };
        place_caret(&el, saved.get_value());
        exec("insertHTML", &html);
    };
    let upload_files = move |files: Vec<File>| {
        let project = upload_project.and_then(|p| p.get_untracked());
        spawn_local(async move {
            let Some(uploaded) = ui.run(api::upload(files, project, None)).await else { return };
            insert(uploaded.iter().map(file_html).collect());
        });
    };
    let pick_files = move |_| {
        let Some(input) = file_input.get() else { return };
        let files = input.files().map(|l| api::files_of(&l)).unwrap_or_default();
        input.set_value("");
        upload_files(files);
    };
    let on_paste = move |ev: ClipboardEvent| {
        let Some(data) = ev.clipboard_data() else { return };
        let files = data.files().map(|l| api::files_of(&l)).unwrap_or_default();
        let markdown = md::to_markdown(&parse_html(&data.get_data("text/html").unwrap_or_default()));
        let use_files = !files.is_empty() && md::plain(&markdown).is_empty();
        if markdown.is_empty() && !use_files {
            return;
        }
        ev.prevent_default();
        remember();
        if !use_files {
            insert(md::render(&markdown));
        } else if allow_upload {
            upload_files(files);
        }
    };
    let on_drop = move |ev: DragEvent| {
        let files = ev.data_transfer().and_then(|d| d.files()).map(|l| api::files_of(&l)).unwrap_or_default();
        if files.is_empty() {
            return;
        }
        ev.prevent_default();
        remember();
        if allow_upload {
            upload_files(files);
        }
    };
    let apply_link = move |ev: SubmitEvent| {
        ev.prevent_default();
        link_open.set(false);
        let Some(el) = editor.get_untracked().filter(|_| !link_url.get_untracked().trim().is_empty()) else { return };
        let href = link_href(&link_url.get_untracked());
        let collapsed = saved.with_value(|r| r.as_ref().is_none_or(Range::collapsed));
        place_caret(&el, saved.get_value());
        if collapsed {
            exec("insertHTML", &format!("<a href=\"{0}\">{0}</a>&nbsp;", md::escape_attr(&href)));
        } else {
            exec("createLink", &href);
        }
    };
    let keep_focus = |ev: MouseEvent| ev.prevent_default();
    let tool = move |t: Tool, label: &'static str, icon: AnyView| {
        let variant = Signal::derive(move || if t.active(&states.get(), &block.get()) { ButtonVariant::Secondary } else { ButtonVariant::Ghost });
        view! {
            <Button variant size=ButtonSize::IconSm attr:title=label attr:aria-label=label on:mousedown=keep_focus on:click=move |_| t.run(&block.get_untracked())>
                {icon}
            </Button>
        }
    };
    let divider = || view! { <span class="mx-1 w-px h-5 bg-border" /> };

    view! {
        {title.map(|title| view! {
            <Input
                class="px-0 h-auto text-2xl font-bold bg-transparent border-0 shadow-none md:text-3xl focus-visible:ring-0 dark:bg-transparent"
                placeholder="Title"
                bind_value=title
                on:input=move |ev| {
                    title.set(event_target_value(&ev));
                    on_change.run(());
                }
            />
        })}
        <div class="flex flex-col gap-2">
            <div class=format!("flex sticky {toolbar_top} z-10 flex-wrap gap-0.5 items-center p-1 rounded-lg border shadow-xs backdrop-blur bg-background/90")>
                {tool(Tool::Command("bold"), "Bold", view! { <Bold /> }.into_any())}
                {tool(Tool::Command("italic"), "Italic", view! { <Italic /> }.into_any())}
                {tool(Tool::Command("strikeThrough"), "Strikethrough", view! { <Strikethrough /> }.into_any())}
                {divider()}
                {tool(Tool::Block("h1"), "Heading 1", view! { <Heading1 /> }.into_any())}
                {tool(Tool::Block("h2"), "Heading 2", view! { <Heading2 /> }.into_any())}
                {divider()}
                {tool(Tool::Command("insertUnorderedList"), "Bulleted list", view! { <List /> }.into_any())}
                {tool(Tool::Command("insertOrderedList"), "Numbered list", view! { <ListOrdered /> }.into_any())}
                {tool(Tool::Insert(CHECKLIST), "Checklist", view! { <ListChecks /> }.into_any())}
                {divider()}
                {tool(Tool::Block("blockquote"), "Quote", view! { <Quote /> }.into_any())}
                {tool(Tool::Block("pre"), "Code block", view! { <Code /> }.into_any())}
                <Button
                    variant=ButtonVariant::Ghost
                    size=ButtonSize::IconSm
                    attr:title="Link"
                    attr:aria-label="Link"
                    on:mousedown=keep_focus
                    on:click=move |_| {
                        remember();
                        link_url.set(String::new());
                        link_open.update(|open| *open = !*open);
                    }
                >
                    <Link />
                </Button>
                <Show when=move || allow_upload>
                    <Button
                        variant=ButtonVariant::Ghost
                        size=ButtonSize::IconSm
                        attr:title="Insert image or file"
                        attr:aria-label="Insert image or file"
                        on:mousedown=keep_focus
                        on:click=move |_| {
                            remember();
                            if let Some(i) = file_input.get() {
                                i.click();
                            }
                        }
                    >
                        <ImagePlus />
                    </Button>
                </Show>
                <input type="file" multiple class="hidden" node_ref=file_input on:change=pick_files />
                <Show when=move || link_open.get()>
                    <form class="flex gap-1 items-center ml-auto" on:submit=apply_link>
                        <Input class="w-56 h-8" placeholder="Paste a link" bind_value=link_url node_ref=link_input />
                        <Button size=ButtonSize::Sm>"Add"</Button>
                        <Button attr:r#type="button" variant=ButtonVariant::Ghost size=ButtonSize::IconSm attr:aria-label="Cancel" on:click=move |_| link_open.set(false)>
                            <X />
                        </Button>
                    </form>
                </Show>
            </div>
            <div class="relative">
                <Show when=move || body.with(|b| b.trim().is_empty())>
                    <span class="absolute top-4 left-4 text-sm pointer-events-none text-muted-foreground">{placeholder}</span>
                </Show>
                <div
                    node_ref=editor
                    contenteditable="true"
                    role="textbox"
                    aria-multiline="true"
                    class=format!("p-4 rounded-xl border outline-none md bg-card {min_height} focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50")
                    on:input=move |_| sync()
                    on:change=move |_| sync()
                    on:keyup=move |_| refresh()
                    on:mouseup=move |_| refresh()
                    on:focus=move |_| exec("defaultParagraphSeparator", "p")
                    on:click=move |ev: MouseEvent| follow_link(&ev)
                    on:paste=on_paste
                    on:drop=on_drop
                />
            </div>
        </div>
    }
}
