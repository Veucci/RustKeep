use icons::{CircleCheck, Lock, Upload, X};
use leptos::ev::SubmitEvent;
use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_params_map;
use serde::Deserialize;
use serde_json::{Value, json};
use web_sys::{DragEvent, File, FormData};

use crate::api;
use crate::components::ui::button::Button;
use crate::components::ui::card::Card;
use crate::components::ui::input::{Input, InputType};
use crate::components::ui::label::Label;
use crate::components::ui::progress::Progress;
use crate::components::ui::textarea::Textarea;
use crate::components::ui::theme_toggle::ThemeToggle;
use crate::widgets::{ListSkeleton, LogoTile, fmt_size, use_ui};

#[derive(Clone, Deserialize)]
struct RequestInfo {
    title: String,
    description: String,
    owner: String,
    locked: i64,
}

fn file_progress(sizes: &[f64], loaded: f64) -> Vec<f64> {
    let mut before = 0.0;
    sizes
        .iter()
        .map(|&size| {
            let pct = if size > 0.0 { (loaded - before) / size * 100.0 } else if loaded >= before { 100.0 } else { 0.0 };
            before += size;
            pct.clamp(0.0, 100.0)
        })
        .collect()
}

#[component]
pub fn UploadPage() -> impl IntoView {
    let token = use_params_map().read_untracked().get("token").unwrap_or_default();
    let path = format!("/api/public/uploads/{token}");
    let info = LocalResource::new({
        let path = path.clone();
        move || {
            let path = path.clone();
            async move { api::get::<RequestInfo>(&path).await.ok() }
        }
    });
    let path = StoredValue::new(path);
    view! {
        <div class="flex flex-col gap-4 p-4 mx-auto max-w-2xl md:p-8">
            <div class="flex gap-2 items-center">
                <LogoTile class="size-8" />
                <span class="font-semibold tracking-tight">"RustKeep"</span>
                <span class="flex-1 text-sm text-muted-foreground">"File request"</span>
                <ThemeToggle />
            </div>
            {move || match info.get() {
                None => view! { <Card class="py-0"><ListSkeleton rows=3 /></Card> }.into_any(),
                Some(None) => view! { <p class="text-muted-foreground">"This link is invalid, paused or has expired."</p> }.into_any(),
                Some(Some(i)) => view! { <Request info=i path=path.get_value() /> }.into_any(),
            }}
        </div>
    }
}

#[component]
fn Request(info: RequestInfo, path: String) -> impl IntoView {
    let password = RwSignal::new(String::new());
    let unlocked = RwSignal::new(info.locked == 0);
    let path = StoredValue::new(path);
    view! {
        <Card class="flex flex-col gap-1 p-6">
            <h1 class="text-xl font-semibold tracking-tight">{info.title.clone()}</h1>
            <p class="text-sm text-muted-foreground">{format!("{} is asking you to upload files.", info.owner)}</p>
            <Show when={
                let any = !info.description.is_empty();
                move || any
            }>
                <p class="pt-3 text-sm whitespace-pre-line">{info.description.clone()}</p>
            </Show>
        </Card>
        <Show
            when=move || unlocked.get()
            fallback=move || view! { <Unlock path=path.get_value() password unlocked /> }
        >
            <Sender path=path.get_value() password />
        </Show>
    }
}

#[component]
fn Unlock(path: String, password: RwSignal<String>, unlocked: RwSignal<bool>) -> impl IntoView {
    let ui = use_ui();
    let checking = RwSignal::new(false);
    let path = StoredValue::new(format!("{path}/unlock"));
    let submit = move |ev: SubmitEvent| {
        ev.prevent_default();
        checking.set(true);
        spawn_local(async move {
            match api::post::<Value>(&path.get_value(), &json!({ "password": password.get_untracked() })).await {
                Ok(_) => unlocked.set(true),
                Err(e) => ui.notify(e.msg),
            }
            checking.set(false);
        });
    };
    view! {
        <Card class="p-6">
            <form class="flex flex-col gap-3" on:submit=submit>
                <Label class="flex gap-2 items-center"><Lock class="size-4" />"This request is password protected"</Label>
                <Input r#type=InputType::Password bind_value=password required=true autofocus=true placeholder="Password" autocomplete="off" />
                <Button class="self-end" attr:disabled=move || checking.get()>"Continue"</Button>
            </form>
        </Card>
    }
}

#[component]
fn Sender(path: String, password: RwSignal<String>) -> impl IntoView {
    let ui = use_ui();
    let name = RwSignal::new(String::new());
    let note = RwSignal::new(String::new());
    let picked = RwSignal::new_local(Vec::<File>::new());
    let sent = RwSignal::new(Vec::<(String, i64)>::new());
    let loaded = RwSignal::new(None::<f64>);
    let over = RwSignal::new(false);
    let input = NodeRef::<html::Input>::new();
    let path = StoredValue::new(path);
    let add = move |list: Vec<File>| picked.update(|p| p.extend(list));
    let progress = Signal::derive(move || {
        let sizes: Vec<f64> = picked.with(|p| p.iter().map(|f| f.size()).collect());
        file_progress(&sizes, loaded.get().unwrap_or(0.0))
    });
    let submit = move |ev: SubmitEvent| {
        ev.prevent_default();
        let files = picked.get_untracked();
        if files.is_empty() {
            return ui.notify("Choose at least one file");
        }
        let form = FormData::new().expect("FormData");
        let _ = form.append_with_str("name", &name.get_untracked());
        let _ = form.append_with_str("note", &note.get_untracked());
        let _ = form.append_with_str("password", &password.get_untracked());
        for f in &files {
            let _ = form.append_with_blob_and_filename("file", f, &f.name());
        }
        loaded.set(Some(0.0));
        let done = move |res: api::ApiResult<()>| {
            loaded.set(None);
            if let Err(e) = res {
                return ui.notify(e.msg);
            }
            sent.update(|s| s.extend(files.iter().map(|f| (f.name(), f.size() as i64))));
            picked.set(Vec::new());
            note.set(String::new());
            ui.notify(format!("Sent {} file(s)", files.len()));
        };
        api::post_with_progress(&path.get_value(), &form, move |bytes| loaded.set(Some(bytes)), done);
    };
    let sending = move || loaded.with(Option::is_some);
    view! {
        <Show when=move || sent.with(|s| !s.is_empty())>
            <Card class="flex flex-col gap-2 p-4">
                <p class="flex gap-2 items-center text-sm font-medium"><CircleCheck class="text-green-600 size-4" />"Delivered"</p>
                {move || sent.get().into_iter().map(|(n, size)| view! {
                    <div class="flex gap-3 justify-between text-sm">
                        <span class="truncate">{n}</span>
                        <span class="tabular-nums text-muted-foreground shrink-0">{fmt_size(size)}</span>
                    </div>
                }).collect_view()}
            </Card>
        </Show>
        <Card class="p-6">
            <form class="flex flex-col gap-4" on:submit=submit>
                <div class="flex flex-col gap-2">
                    <Label>"Your name"</Label>
                    <Input bind_value=name required=true autocomplete="name" placeholder="Jane Doe" />
                </div>
                <div class="flex flex-col gap-2">
                    <Label>"Message (optional)"</Label>
                    <Textarea bind_value=note rows=3u32 placeholder="Anything the recipient should know" />
                </div>
                <button
                    type="button"
                    class="flex flex-col gap-2 justify-center items-center p-8 text-sm rounded-xl border-2 border-dashed transition-colors text-muted-foreground hover:bg-muted/50"
                    class=("border-primary", move || over.get())
                    class=("bg-muted/50", move || over.get())
                    disabled=sending
                    on:click=move |_| { if let Some(el) = input.get() { el.click(); } }
                    on:dragover=move |ev: DragEvent| { ev.prevent_default(); over.set(true); }
                    on:dragleave=move |_| over.set(false)
                    on:drop=move |ev: DragEvent| {
                        ev.prevent_default();
                        over.set(false);
                        add(ev.data_transfer().and_then(|dt| dt.files()).map(|l| api::files_of(&l)).unwrap_or_default());
                    }
                >
                    <Upload class="size-6" />
                    <span class="font-medium text-foreground">"Drop files here or click to choose"</span>
                </button>
                <input
                    type="file"
                    multiple
                    class="hidden"
                    node_ref=input
                    on:change=move |_| {
                        let Some(el) = input.get() else { return };
                        add(el.files().map(|l| api::files_of(&l)).unwrap_or_default());
                        el.set_value("");
                    }
                />
                {move || picked.get().into_iter().enumerate().map(|(i, f)| view! {
                    <div class="flex flex-col gap-1.5">
                        <div class="flex gap-3 items-center text-sm">
                            <span class="flex-1 truncate">{f.name()}</span>
                            <span class="tabular-nums text-muted-foreground shrink-0">{fmt_size(f.size() as i64)}</span>
                            <Show when=move || !sending()>
                                <button
                                    type="button"
                                    class="flex justify-center items-center rounded-md size-6 hover:bg-accent shrink-0"
                                    title="Remove"
                                    on:click=move |_| picked.update(|p| { p.remove(i); })
                                >
                                    <X class="size-4" />
                                </button>
                            </Show>
                        </div>
                        <Show when=sending>
                            <Progress value=Signal::derive(move || progress.with(|p| p.get(i).copied().unwrap_or(0.0))) />
                        </Show>
                    </div>
                }).collect_view()}
                <Button class="self-end" attr:disabled=sending>
                    {move || if sending() { "Uploading..." } else { "Send files" }}
                </Button>
            </form>
        </Card>
    }
}

#[cfg(test)]
mod tests {
    use super::file_progress;

    #[test]
    fn splits_sent_bytes_across_files_in_order() {
        assert_eq!(file_progress(&[100.0, 300.0], 0.0), [0.0, 0.0]);
        assert_eq!(file_progress(&[100.0, 300.0], 50.0), [50.0, 0.0]);
        assert_eq!(file_progress(&[100.0, 300.0], 250.0), [100.0, 50.0]);
        assert_eq!(file_progress(&[100.0, 0.0, 300.0], 500.0), [100.0, 100.0, 100.0]);
    }
}
