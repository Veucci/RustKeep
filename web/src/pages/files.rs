use std::cmp::Reverse;

use icons::{Archive, ArchiveRestore, Copy, FileText, Share2, Trash2, Upload};
use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::Deserialize;
use web_sys::{DragEvent, File};

use crate::api;
use crate::id::Id;
use crate::components::ui::badge::{Badge, BadgeVariant};
use crate::components::ui::button::{Button, ButtonSize, ButtonVariant};
use crate::components::ui::card::Card;
use crate::components::ui::empty::{Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant, EmptyTitle};
use crate::components::ui::table::{Table, TableBody, TableCell, TableHead, TableHeader, TableRow};
use crate::widgets::{
    ListSkeleton, Options, PageHeader, SearchBox, Segmented, ShareDialog, SortSelect, Toolbar, copy, fmt_size, fmt_time, has,
    query_state, use_ui,
};

const SHOW: Options = &[("active", "Active"), ("archived", "Archived"), ("all", "All")];
const SORTS: Options =
    &[("newest", "Newest"), ("oldest", "Oldest"), ("name", "Name A-Z"), ("size", "Largest"), ("small", "Smallest")];

#[derive(Clone, Deserialize)]
pub struct FileItem {
    pub id: String,
    pub name: String,
    pub size: i64,
    pub created: i64,
    pub share_token: Option<String>,
    #[serde(default)]
    pub archived: i64,
}

fn arrange(mut list: Vec<FileItem>, q: &str, show: &str, sort: &str) -> Vec<FileItem> {
    list.retain(|f| has(&f.name, q) && (show == "all" || (f.archived == 1) == (show == "archived")));
    match sort {
        "oldest" => list.sort_by_key(|f| f.created),
        "name" => list.sort_by_key(|f| f.name.to_lowercase()),
        "size" => list.sort_by_key(|f| Reverse(f.size)),
        "small" => list.sort_by_key(|f| f.size),
        _ => list.sort_by_key(|f| Reverse(f.created)),
    }
    list
}

#[component]
pub fn FilesPage() -> impl IntoView {
    view! {
        <div class="flex flex-col gap-6 mx-auto max-w-5xl page-enter">
            <PageHeader title="Files" description="Upload anything. Private links need a session, shared links can expire." />
            <FileManager />
        </div>
    }
}

#[component]
pub fn FileManager(#[prop(optional)] project: Option<Id>, #[prop(optional)] archive_only: bool) -> impl IntoView {
    let ui = use_ui();
    let q = RwSignal::new(String::new());
    let (show, set_show) = query_state("show", "active");
    let show = if archive_only { Signal::stored("archived".to_owned()) } else { show };
    let (sort, set_sort) = query_state("sort", "newest");
    let path = project.map_or("/api/files".to_owned(), |p| format!("/api/files?project={p}"));
    let files = LocalResource::new(move || {
        let path = path.clone();
        async move { ui.run(api::get::<Vec<FileItem>>(&path)).await.unwrap_or_default() }
    });
    let input = NodeRef::<html::Input>::new();
    let busy = RwSignal::new(false);
    let over = RwSignal::new(false);
    let send = move |list: Vec<File>| {
        if list.is_empty() {
            return;
        }
        busy.set(true);
        spawn_local(async move {
            if let Some(saved) = ui.run(api::upload(list, project)).await {
                ui.notify(format!("Uploaded {} file(s)", saved.len()));
                files.refetch();
            }
            busy.set(false);
        });
    };
    let on_drop = move |ev: DragEvent| {
        ev.prevent_default();
        over.set(false);
        if let Some(list) = ev.data_transfer().and_then(|d| d.files()) {
            send(api::files_of(&list));
        }
    };
    let on_pick = move |_| {
        let Some(el) = input.get() else { return };
        let picked = el.files().map(|l| api::files_of(&l)).unwrap_or_default();
        el.set_value("");
        send(picked);
    };
    let reload = Callback::new(move |_| files.refetch());

    let rows = move || files.get().map(|list| arrange(list, &q.get(), &show.get(), &sort.get()));

    view! {
        <Show when=move || !archive_only>
        <label
            class="flex flex-col gap-3 justify-center items-center p-10 text-sm rounded-xl border-2 border-dashed transition-all duration-200 cursor-pointer text-muted-foreground hover:border-primary/40 hover:bg-muted/40"
            class=("border-primary", move || over.get())
            class=("bg-muted/60", move || over.get())
            class=("scale-[1.01]", move || over.get())
            on:dragover=move |ev: DragEvent| {
                ev.prevent_default();
                over.set(true);
            }
            on:dragleave=move |_| over.set(false)
            on:drop=on_drop
        >
            <div class="flex justify-center items-center rounded-full size-12 bg-muted" class=("animate-bounce", move || busy.get())>
                <Upload class="size-5" />
            </div>
            <div class="flex flex-col gap-1 items-center">
                <span class="font-medium text-foreground">
                    {move || if busy.get() { "Uploading..." } else { "Drop files here or click to browse" }}
                </span>
                <span class="text-xs">"Any file type is accepted"</span>
            </div>
            <input type="file" multiple class="hidden" node_ref=input on:change=on_pick />
        </label>
        </Show>
        <Toolbar>
            <div class="flex flex-wrap flex-1 gap-2 items-center min-w-0 max-w-full">
                <SearchBox value=q placeholder="Search files  /" />
                <Show when=move || !archive_only>
                    <Segmented options=SHOW value=show on_change=set_show />
                </Show>
            </div>
            <SortSelect options=SORTS value=sort on_change=set_sort />
        </Toolbar>
        <Card class="overflow-hidden gap-0 py-0">
            {move || match rows() {
                None => view! { <ListSkeleton rows=3 /> }.into_any(),
                Some(list) if list.is_empty() => view! {
                    <Empty class="m-4 border-0">
                        <EmptyHeader>
                            <EmptyMedia variant=EmptyMediaVariant::Icon><FileText /></EmptyMedia>
                            <EmptyTitle>"No files here"</EmptyTitle>
                            <EmptyDescription>"Nothing matches this view. Upload a file or change the filter."</EmptyDescription>
                        </EmptyHeader>
                    </Empty>
                }
                .into_any(),
                Some(list) => view! {
                    <Table class="max-w-none">
                        <TableHeader class="bg-muted/50">
                            <TableRow>
                                <TableHead class="px-4">"Name"</TableHead>
                                <TableHead>"Size"</TableHead>
                                <TableHead class="hidden sm:table-cell">"Uploaded"</TableHead>
                                <TableHead>""</TableHead>
                            </TableRow>
                        </TableHeader>
                        <TableBody>
                            {list.into_iter().map(|f| view! { <FileRow file=f reload /> }).collect_view()}
                        </TableBody>
                    </Table>
                }
                .into_any(),
            }}
        </Card>
    }
}

#[component]
fn FileRow(file: FileItem, reload: Callback<()>) -> impl IntoView {
    let ui = use_ui();
    let id = StoredValue::new(file.id.clone());
    let token = RwSignal::new(file.share_token);
    let open = RwSignal::new(false);
    let private_url = api::url(&format!("/api/files/{}/raw", file.id));
    let archived = file.archived == 1;
    let toggle_archive = move |_| {
        let action = if archived { "unarchive" } else { "archive" };
        ui.act(format!("/api/files/{}/action/{action}", id.get_value()), move || reload.run(()));
    };
    let remove = move |_| {
        if !window().confirm_with_message("Delete this file?").unwrap_or(false) {
            return;
        }
        spawn_local(async move {
            if ui.run(api::del(&format!("/api/files/{}", id.get_value()))).await.is_some() {
                reload.run(());
            }
        });
    };

    view! {
        <TableRow class="group">
            <TableCell class="py-3 px-4 max-w-72">
                <div class="flex gap-3 items-center">
                    <div class="flex justify-center items-center rounded-lg size-9 bg-muted shrink-0">
                        <FileText class="size-4 text-muted-foreground" />
                    </div>
                    <a href=private_url target="_blank" class="font-medium break-all hover:underline">{file.name}</a>
                    <Show when=move || token.get().is_some()>
                        <Badge variant=BadgeVariant::Secondary>"Shared"</Badge>
                    </Show>
                    <Show when=move || archived>
                        <Badge variant=BadgeVariant::Outline>"Archived"</Badge>
                    </Show>
                </div>
            </TableCell>
            <TableCell class="py-3 tabular-nums text-muted-foreground">{fmt_size(file.size)}</TableCell>
            <TableCell class="hidden py-3 sm:table-cell text-muted-foreground">{fmt_time(file.created)}</TableCell>
            <TableCell class="py-3">
                <div class="flex gap-1 justify-end opacity-70 transition-opacity group-hover:opacity-100">
                    <Button
                        variant=ButtonVariant::Ghost
                        size=ButtonSize::IconSm
                        attr:title="Copy private link"
                        on:click=move |_| copy(ui, &api::absolute(&format!("/api/files/{}/raw", id.get_value())))
                    >
                        <Copy />
                    </Button>
                    <Button variant=ButtonVariant::Ghost size=ButtonSize::IconSm attr:title="Share" on:click=move |_| open.set(true)>
                        <Share2 />
                    </Button>
                    <Button
                        variant=ButtonVariant::Ghost
                        size=ButtonSize::IconSm
                        attr:title=if archived { "Unarchive" } else { "Archive" }
                        on:click=toggle_archive
                    >
                        {if archived { view! { <ArchiveRestore /> }.into_any() } else { view! { <Archive /> }.into_any() }}
                    </Button>
                    <Button variant=ButtonVariant::Ghost size=ButtonSize::IconSm attr:title="Delete" on:click=remove>
                        <Trash2 />
                    </Button>
                </div>
                <ShareDialog open api_path=format!("/api/files/{}/share", file.id) link_prefix="/api/public/files/" token />
            </TableCell>
        </TableRow>
    }
}
