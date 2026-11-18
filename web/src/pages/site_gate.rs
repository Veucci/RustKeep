use icons::Lock;
use leptos::ev::SubmitEvent;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_params_map;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::api;
use crate::components::ui::button::{Button, ButtonLink, ButtonVariant};
use crate::components::ui::card::Card;
use crate::components::ui::input::{Input, InputType};
use crate::components::ui::label::Label;
use crate::components::ui::theme_toggle::ThemeToggle;
use crate::widgets::{ListSkeleton, LogoTile, use_ui};

#[derive(Clone, Deserialize)]
struct PublicSite {
    name: String,
    locked: bool,
    unlocked: bool,
}

#[component]
pub fn SiteGate() -> impl IntoView {
    let rest = use_params_map().read_untracked().get("rest").unwrap_or_default();
    let name = rest.split('/').next().unwrap_or_default().to_owned();
    let info = LocalResource::new(move || {
        let path = format!("/api/public/sites/{name}");
        async move { api::get::<PublicSite>(&path).await.ok() }
    });
    view! {
        <div class="flex flex-col gap-4 p-4 mx-auto max-w-2xl md:p-8">
            <div class="flex gap-2 items-center">
                <LogoTile class="size-8" />
                <span class="font-semibold tracking-tight">"RustKeep"</span>
                <span class="flex-1 text-sm text-muted-foreground">"Site"</span>
                <ThemeToggle />
            </div>
            {move || match info.get() {
                None => view! { <Card class="py-0"><ListSkeleton rows=3 /></Card> }.into_any(),
                Some(None) => view! { <p class="text-muted-foreground">"Site not found. The link is invalid, disabled or has expired."</p> }.into_any(),
                Some(Some(site)) if site.locked && !site.unlocked => view! { <Unlock name=site.name /> }.into_any(),
                Some(Some(site)) => view! {
                    <Card class="flex flex-col gap-3 items-start p-6">
                        <h1 class="text-xl font-semibold tracking-tight">"Page not found"</h1>
                        <p class="text-sm text-muted-foreground">{format!("This page does not exist on {}.", site.name)}</p>
                        <ButtonLink variant=ButtonVariant::Outline href=api::url(&format!("/site/{}/", site.name)) attr:rel="external">
                            "Go to the site home"
                        </ButtonLink>
                    </Card>
                }
                .into_any(),
            }}
        </div>
    }
}

#[component]
fn Unlock(name: String) -> impl IntoView {
    let ui = use_ui();
    let password = RwSignal::new(String::new());
    let checking = RwSignal::new(false);
    let path = StoredValue::new(format!("/api/public/sites/{name}/unlock"));
    let submit = move |ev: SubmitEvent| {
        ev.prevent_default();
        checking.set(true);
        spawn_local(async move {
            match api::post::<Value>(&path.get_value(), &json!({ "password": password.get_untracked() })).await {
                Ok(_) => {
                    let _ = window().location().reload();
                }
                Err(e) if e.status == 401 => ui.notify("Wrong password"),
                Err(e) => ui.notify(e.msg),
            }
            checking.set(false);
        });
    };
    view! {
        <Card class="p-6">
            <form class="flex flex-col gap-3" on:submit=submit>
                <Label class="flex gap-2 items-center"><Lock class="size-4" />"This site is password protected"</Label>
                <Input r#type=InputType::Password bind_value=password required=true autofocus=true placeholder="Password" autocomplete="off" />
                <Button class="self-end" attr:disabled=move || checking.get()>"Continue"</Button>
            </form>
        </Card>
    }
}
