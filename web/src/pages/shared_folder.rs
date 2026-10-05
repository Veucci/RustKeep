use icons::{ChevronRight, Download, Folder as FolderIcon};
use leptos::prelude::*;
use leptos_router::hooks::use_params_map;
use serde::Deserialize;

use crate::api;
use crate::components::ui::card::Card;
use crate::components::ui::theme_toggle::ThemeToggle;
use crate::pages::drive::{FileItem, Folder, Kind, KindIcon, Preview, trail};
use crate::widgets::{ListSkeleton, LogoTile, fmt_size};

#[derive(Clone, Deserialize)]
struct Tree {
    root: String,
    name: String,
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
                Some(Some(t)) => view! { <Browser tree=t base=base.get_value() /> }.into_any(),
            }}
        </div>
    }
}

#[component]
fn Browser(tree: Tree, base: String) -> impl IntoView {
    let tree = StoredValue::new(tree);
    let at = RwSignal::new(tree.with_value(|t| t.root.clone()));
    let preview = RwSignal::new(None::<String>);
    let raw = move |id: &str| api::url(&format!("{base}/files/{id}"));
    let raw = StoredValue::new(raw);
    let dirs = move || tree.with_value(|t| t.folders.iter().filter(|f| f.parent_id.as_ref() == Some(&at.get())).cloned().collect::<Vec<_>>());
    let docs = Signal::derive(move || tree.with_value(|t| t.files.iter().filter(|f| f.folder_id.as_ref() == Some(&at.get())).cloned().collect::<Vec<_>>()));
    let path = move || tree.with_value(|t| trail(&t.folders, Some(&at.get())));
    view! {
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
                <p class="p-8 text-sm text-center text-muted-foreground">"This folder is empty."</p>
            </Show>
        </Card>
        <p class="text-xs text-muted-foreground">{tree.with_value(|t| format!("Shared folder: {}", t.name))}</p>
        <Preview files=docs current=preview src=Callback::new(move |id: String| raw.with_value(|r| r(&id))) />
    }
}
