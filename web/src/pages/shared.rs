use leptos::prelude::*;
use leptos_router::hooks::use_params_map;
use serde::Deserialize;
use serde_json::json;

use crate::api::{self, AutoSave, client_id, next_rev};
use crate::components::ui::theme_toggle::ThemeToggle;
use crate::pages::editor::{RichEditor, SaveBadge};
use crate::widgets::LogoTile;

#[derive(Clone, Deserialize)]
struct PublicNote {
    title: String,
    body: String,
}

#[component]
pub fn SharedNote() -> impl IntoView {
    let params = use_params_map();
    let token = params.read_untracked().get("token").unwrap_or_default();
    let path = format!("/api/public/notes/{token}");
    let note = LocalResource::new({
        let path = path.clone();
        move || {
            let path = path.clone();
            async move { api::get::<PublicNote>(&path).await.ok() }
        }
    });
    let path = StoredValue::new(path);

    view! {
        <div class="flex flex-col gap-4 p-4 mx-auto max-w-6xl md:p-8">
            <div class="flex gap-2 items-center">
                <LogoTile class="size-8" />
                <span class="font-semibold tracking-tight">"RustKeep"</span>
                <span class="flex-1 text-sm text-muted-foreground">"Shared note"</span>
                <ThemeToggle />
            </div>
            {move || {
                note.get()
                    .map(|n| match n {
                        Some(n) => {
                            let title = RwSignal::new(n.title);
                            let body = RwSignal::new(n.body);
                            let saver = AutoSave::new();
                            let save = Callback::new(move |_| {
                                let payload = json!({
                                    "title": title.get_untracked(), "body": body.get_untracked(),
                                    "rev": next_rev(), "client": client_id(),
                                });
                                saver.send(path.get_value(), payload);
                            });
                            view! {
                                <SaveBadge state=saver.state />
                                <RichEditor title body on_change=save />
                            }
                                .into_any()
                        }
                        None => view! { <p class="text-muted-foreground">"This link is invalid or has expired."</p> }.into_any(),
                    })
            }}
        </div>
    }
}
