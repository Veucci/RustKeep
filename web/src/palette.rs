use icons::{CornerDownLeft, Search};
use leptos::html;
use leptos::prelude::*;
use leptos_router::hooks::use_navigate;
use serde_json::json;
use web_sys::KeyboardEvent;

use crate::api;
use crate::pages::notes::NoteItem;
use crate::pages::projects::ProjectItem;
use crate::widgets::{create_note, has};

#[derive(Clone, PartialEq)]
struct Item {
    label: String,
    kind: &'static str,
    href: String,
}

const NEW_NOTE: &str = "new-note";

const PAGES: [(&str, &str); 12] = [
    ("Dashboard", "/"),
    ("Notes", "/notes"),
    ("Projects", "/projects"),
    ("Files", "/files"),
    ("Reminders", "/reminders"),
    ("Vault", "/vault"),
    ("Secret notes", "/secret"),
    ("Archive", "/archive"),
    ("Trash", "/trash"),
    ("New note", NEW_NOTE),
    ("New project", "/projects?new=1"),
    ("New reminder", "/reminders?new=1"),
];

fn item(label: impl Into<String>, kind: &'static str, href: String) -> Item {
    Item { label: label.into(), kind, href }
}

fn untitled(title: String) -> String {
    if title.is_empty() { "Untitled".into() } else { title }
}

#[component]
pub fn Palette(open: RwSignal<bool>) -> impl IntoView {
    let ui = crate::widgets::use_ui();
    let navigate = use_navigate();
    let q = RwSignal::new(String::new());
    let active = RwSignal::new(0usize);
    let input = NodeRef::<html::Input>::new();
    let projects = LocalResource::new(move || {
        let load = open.get();
        async move { if load { api::get::<Vec<ProjectItem>>("/api/projects").await.unwrap_or_default() } else { Vec::new() } }
    });
    let notes = LocalResource::new(move || {
        let load = open.get();
        let path = format!("/api/notes?q={}", js_sys::encode_uri_component(&q.get()));
        async move { if load { api::get::<Vec<NoteItem>>(&path).await.unwrap_or_default() } else { Vec::new() } }
    });
    let items = Memo::new(move |_| {
        let needle = q.get();
        let mut out: Vec<Item> =
            PAGES.iter().filter(|(l, _)| has(l, &needle)).map(|(l, h)| item(*l, "Go to", (*h).to_owned())).collect();
        let projects = projects.get().unwrap_or_default();
        let matching = projects.into_iter().filter(|p| p.archived == 0 && has(&p.name, &needle)).take(5);
        out.extend(matching.map(|p| item(p.name, "Project", format!("/projects/{}", p.id))));
        let notes = notes.get().unwrap_or_default().into_iter().take(8);
        out.extend(notes.map(|n| item(untitled(n.title), "Note", format!("/notes/{}", n.id))));
        out
    });
    Effect::new(move |_| {
        q.track();
        active.set(0);
    });
    Effect::new(move |_| {
        if let Some(el) = input.get() {
            let _ = el.focus();
        }
    });

    let go = move |it: Item| {
        open.set(false);
        q.set(String::new());
        if it.href == NEW_NOTE {
            create_note(ui, navigate.clone(), json!({}));
        } else {
            navigate(&it.href, Default::default());
        }
    };
    let go_key = go.clone();
    let on_key = move |e: KeyboardEvent| {
        let len = items.with_untracked(Vec::len).max(1);
        match e.key().as_str() {
            "ArrowDown" => active.update(|a| *a = (*a + 1) % len),
            "ArrowUp" => active.update(|a| *a = (*a + len - 1) % len),
            "Escape" => open.set(false),
            "Enter" => {
                if let Some(it) = items.with_untracked(|v| v.get(active.get_untracked()).cloned()) {
                    go_key(it);
                }
            }
            _ => return,
        }
        e.prevent_default();
    };

    view! {
        <Show when=move || open.get()>
            <div class="fixed inset-0 z-50 backdrop-blur-[2px] bg-black/50 animate-in fade-in-0" on:click=move |_| open.set(false) />
            <div class="fixed inset-x-0 top-[12vh] z-50 px-4 mx-auto w-full max-w-lg">
                <div class="overflow-hidden rounded-xl border shadow-2xl bg-popover pop-in">
                    <div class="flex gap-2 items-center px-3 border-b">
                        <Search class="size-4 text-muted-foreground" />
                        <input
                            node_ref=input
                            class="flex-1 h-12 text-sm bg-transparent outline-none"
                            placeholder="Search notes, projects and pages..."
                            prop:value=move || q.get()
                            on:input=move |ev| q.set(event_target_value(&ev))
                            on:keydown=on_key.clone()
                        />
                        <kbd class="px-1.5 font-mono rounded border text-[10px] bg-muted text-muted-foreground">"Esc"</kbd>
                    </div>
                    <div class="overflow-y-auto p-1 max-h-80">
                        <Show when=move || items.with(Vec::is_empty)>
                            <p class="py-6 text-sm text-center text-muted-foreground">"No results"</p>
                        </Show>
                        {
                            let go = go.clone();
                            move || {
                                items
                                    .get()
                                    .into_iter()
                                    .enumerate()
                                    .map(|(i, it)| {
                                        let go = go.clone();
                                        let kind = it.kind;
                                        let label = it.label.clone();
                                        view! {
                                            <button
                                                type="button"
                                                class="flex gap-3 items-center py-2 px-3 w-full text-sm text-left rounded-md"
                                                class=("bg-accent", move || active.get() == i)
                                                on:mouseenter=move |_| active.set(i)
                                                on:click=move |_| go(it.clone())
                                            >
                                                <span class="flex-1 truncate">{label}</span>
                                                <span class="text-xs text-muted-foreground">{kind}</span>
                                                <Show when=move || active.get() == i>
                                                    <CornerDownLeft class="size-3.5 text-muted-foreground" />
                                                </Show>
                                            </button>
                                        }
                                    })
                                    .collect_view()
                            }
                        }
                    </div>
                </div>
            </div>
        </Show>
    }
}
