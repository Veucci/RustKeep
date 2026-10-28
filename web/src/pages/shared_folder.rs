use icons::{ChevronRight, Download, Folder as FolderIcon, Upload};
use leptos::html;
use leptos::prelude::*;
use leptos_router::hooks::use_params_map;
use serde::Deserialize;
use web_sys::{DragEvent, File, FormData};

use crate::api;
use crate::components::ui::button::Button;
use crate::components::ui::card::Card;
use crate::components::ui::progress::Progress;
use crate::components::ui::theme_toggle::ThemeToggle;
use crate::pages::drive::{FileItem, Folder, Kind, KindIcon, Preview, trail};
use crate::widgets::{ListSkeleton, LogoTile, fmt_size, use_ui};

#[derive(Clone, Deserialize)]
struct Tree {
    root: String,
    name: String,
    #[serde(default)]
    uploads: bool,
    folders: Vec<Folder>,
    files: Vec<FileItem>,
}

const ROW: &str = "flex gap-3 items-center px-4 py-2.5 w-full text-left border-b last:border-b-0 hover:bg-muted/50";

#[component]
pub fn SharedFolder() -> impl IntoView {
    let token = use_params_map().read_untracked().get("token").unwrap_or_default();
    let base = format!("/api/public/folders/{token}");
    let tree = LocalResource::new({
        let base = base.clone();
        move || {
            let base = base.clone();
            async move { api::get::<Tree>(&base).await.ok() }
        }
    });
    let base = StoredValue::new(base);
    let at = RwSignal::new(String::new());
    view! {
        <div class="flex flex-col gap-4 p-4 mx-auto max-w-5xl md:p-8">
            <div class="flex gap-2 items-center">
                <LogoTile class="size-8" />
                <span class="font-semibold tracking-tight">"RustKeep"</span>
                <span class="flex-1 text-sm text-muted-foreground">"Shared folder"</span>
                <ThemeToggle />
            </div>
            {move || match tree.get() {
                None => view! { <Card class="py-0"><ListSkeleton rows=3 /></Card> }.into_any(),
                Some(None) => view! { <p class="text-muted-foreground">"This link is invalid or has expired."</p> }.into_any(),
                Some(Some(t)) => view! { <Browser tree=t base=base.get_value() at reload=Callback::new(move |()| tree.refetch()) /> }.into_any(),
            }}
        </div>
    }
}

#[component]
fn Browser(tree: Tree, base: String, at: RwSignal<String>, reload: Callback<()>) -> impl IntoView {
    if at.with_untracked(String::is_empty) {
        at.set(tree.root.clone());
    }
    let uploads = tree.uploads;
    let tree = StoredValue::new(tree);
    let preview = RwSignal::new(None::<String>);
    let upload_path = StoredValue::new(format!("{base}/upload"));
    let raw = move |id: &str| api::url(&format!("{base}/files/{id}"));
    let raw = StoredValue::new(raw);
    let dirs = move || tree.with_value(|t| t.folders.iter().filter(|f| f.parent_id.as_ref() == Some(&at.get())).cloned().collect::<Vec<_>>());
    let docs = Signal::derive(move || tree.with_value(|t| t.files.iter().filter(|f| f.folder_id.as_ref() == Some(&at.get())).cloned().collect::<Vec<_>>()));
    let path = move || tree.with_value(|t| trail(&t.folders, Some(&at.get())));
    view! {
        <div class="flex flex-wrap gap-2 justify-between items-center">
            <nav class="flex flex-wrap gap-1 items-center text-lg font-medium">
                {move || {
                    let crumbs = path();
                    let last = crumbs.len().saturating_sub(1);
                    crumbs.into_iter().enumerate().map(|(i, f)| view! {
                        <Show when=move || i != 0><ChevronRight class="size-4 text-muted-foreground" /></Show>
                        <button type="button" class="py-1 px-2 rounded-md hover:bg-muted" class=("text-muted-foreground", i != last) on:click=move |_| at.set(f.id.clone())>
                            {f.name.clone()}
                        </button>
                    }).collect_view()
                }}
            </nav>
            <Show when=move || uploads>
                <Uploader path=upload_path.get_value() at reload />
            </Show>
        </div>
        <Card class="overflow-hidden gap-0 py-0">
            {move || dirs().into_iter().map(|f| view! {
                <button type="button" class=ROW on:click=move |_| at.set(f.id.clone())>
                    <FolderIcon class="text-amber-500 fill-current size-5 shrink-0" />
                    <span class="flex-1 text-sm font-medium truncate">{f.name.clone()}</span>
                </button>
            }).collect_view()}
            {move || docs.get().into_iter().map(|f| {
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
            <Show when=move || dirs().is_empty() && docs.with(Vec::is_empty)>
                <p class="p-8 text-sm text-center text-muted-foreground">
                    {if uploads { "This folder is empty. Use Upload to add files." } else { "This folder is empty." }}
                </p>
            </Show>
        </Card>
        <p class="text-xs text-muted-foreground">
            {tree.with_value(|t| format!("Shared folder: {}", t.name))}
            {uploads.then_some(". You can upload files, but not change or delete anything.")}
        </p>
        <Preview files=docs current=preview src=Callback::new(move |id: String| raw.with_value(|r| r(&id))) />
    }
}

#[component]
fn Uploader(path: String, at: RwSignal<String>, reload: Callback<()>) -> impl IntoView {
    let ui = use_ui();
    let input = NodeRef::<html::Input>::new();
    let progress = RwSignal::new(None::<f64>);
    let path = StoredValue::new(path);
    let send = move |files: Vec<File>| {
        if files.is_empty() || progress.with_untracked(Option::is_some) {
            return;
        }
        let total: f64 = files.iter().map(|f| f.size()).sum();
        let form = FormData::new().expect("FormData");
        for f in &files {
            let _ = form.append_with_blob_and_filename("file", f, &f.name());
        }
        progress.set(Some(0.0));
        let url = format!("{}?folder={}", path.get_value(), at.get_untracked());
        let on_progress = move |bytes: f64| progress.set(Some(bytes / total.max(1.0) * 100.0));
        api::post_with_progress(&url, &form, on_progress, move |res| {
            progress.set(None);
            match res {
                Ok(()) => {
                    ui.notify(format!("Uploaded {} file(s)", files.len()));
                    reload.run(());
                }
                Err(e) => ui.notify(e.msg),
            }
        });
    };
    let drop_files = window_event_listener(leptos::ev::drop, move |ev: DragEvent| {
        ev.prevent_default();
        send(ev.data_transfer().and_then(|dt| dt.files()).map(|l| api::files_of(&l)).unwrap_or_default());
    });
    let allow_drop = window_event_listener(leptos::ev::dragover, |ev: DragEvent| ev.prevent_default());
    on_cleanup(move || {
        drop_files.remove();
        allow_drop.remove();
    });
    view! {
        <div class="flex gap-3 items-center">
            <Show when=move || progress.with(Option::is_some)>
                <Progress class="w-32" value=Signal::derive(move || progress.get().unwrap_or(0.0)) />
            </Show>
            <Button on:click=move |_| { if let Some(el) = input.get() { el.click(); } } attr:disabled=move || progress.with(Option::is_some)>
                <Upload />
                {move || if progress.with(Option::is_some) { "Uploading..." } else { "Upload" }}
            </Button>
            <input
                type="file"
                multiple
                class="hidden"
                node_ref=input
                on:change=move |_| {
                    let Some(el) = input.get() else { return };
                    send(el.files().map(|l| api::files_of(&l)).unwrap_or_default());
                    el.set_value("");
                }
            />
        </div>
    }
}
