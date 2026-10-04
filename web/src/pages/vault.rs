use icons::{Copy, Eye, KeyRound, Trash2};
use leptos::ev::SubmitEvent;
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::api;
use crate::components::ui::button::{Button, ButtonSize, ButtonVariant};
use crate::components::ui::card::{Card, CardContent, CardHeader, CardTitle};
use crate::components::ui::input::{Input, InputType};
use crate::components::ui::label::Label;
use crate::widgets::{PageHeader, PinGate, Ui, copy, use_ui};

#[derive(Clone, Deserialize)]
struct Item {
    id: i64,
    name: String,
    note: String,
}

#[component]
pub fn Vault() -> impl IntoView {
    view! {
        <PinGate>
            <VaultInner />
        </PinGate>
    }
}

#[component]
fn VaultInner() -> impl IntoView {
    let ui = use_ui();
    let items = LocalResource::new(move || async move { ui.run(api::get::<Vec<Item>>("/api/vault")).await.unwrap_or_default() });
    let name = RwSignal::new(String::new());
    let value = RwSignal::new(String::new());
    let note = RwSignal::new(String::new());
    let submit = move |ev: SubmitEvent| {
        ev.prevent_default();
        spawn_local(async move {
            let body = json!({ "name": name.get_untracked(), "value": value.get_untracked(), "note": note.get_untracked() });
            if ui.run(api::post::<Value>("/api/vault", &body)).await.is_some() {
                [name, value, note].iter().for_each(|s| s.set(String::new()));
                items.refetch();
            }
        });
    };
    let reload = Callback::new(move |_| items.refetch());

    view! {
        <div class="flex flex-col gap-6 mx-auto max-w-4xl page-enter">
            <PageHeader title="Vault" description="API keys and secrets, encrypted at rest." />
            <Card>
                <CardHeader>
                    <CardTitle>"Store a secret"</CardTitle>
                </CardHeader>
                <CardContent>
                    <form class="grid gap-3 sm:grid-cols-3" on:submit=submit>
                        <div class="flex flex-col gap-2">
                            <Label>"Name"</Label>
                            <Input bind_value=name placeholder="OpenAI API key" required=true />
                        </div>
                        <div class="flex flex-col gap-2">
                            <Label>"Value"</Label>
                            <Input r#type=InputType::Password bind_value=value autocomplete="off" required=true />
                        </div>
                        <div class="flex flex-col gap-2">
                            <Label>"Note"</Label>
                            <Input bind_value=note />
                        </div>
                        <Button class="sm:col-span-3 sm:justify-self-end">"Save"</Button>
                    </form>
                </CardContent>
            </Card>
            <Card class="overflow-hidden gap-0 py-0 divide-y">
                {move || items.get().unwrap_or_default().into_iter().map(|i| view! { <VaultRow item=i reload /> }).collect_view()}
            </Card>
        </div>
    }
}

async fn reveal(ui: Ui, id: i64) -> Option<String> {
    let v = ui.run(api::get::<Value>(&format!("/api/vault/{id}"))).await?;
    v["value"].as_str().map(str::to_owned)
}

#[component]
fn VaultRow(item: Item, reload: Callback<()>) -> impl IntoView {
    let ui = use_ui();
    let id = item.id;
    let shown = RwSignal::new(None::<String>);
    let toggle = move |_| {
        if shown.get_untracked().is_some() {
            shown.set(None);
            return;
        }
        spawn_local(async move { shown.set(reveal(ui, id).await) });
    };
    let copy_value = move |_| {
        spawn_local(async move {
            if let Some(v) = reveal(ui, id).await {
                copy(ui, &v);
            }
        })
    };
    let remove = move |_| {
        if !window().confirm_with_message("Delete this secret?").unwrap_or(false) {
            return;
        }
        spawn_local(async move {
            if ui.run(api::del(&format!("/api/vault/{id}"))).await.is_some() {
                reload.run(());
            }
        });
    };

    view! {
        <div class="flex gap-3 items-center py-3 px-4 transition-colors hover:bg-muted/50">
            <div class="flex justify-center items-center rounded-lg size-9 bg-muted shrink-0">
                <KeyRound class="size-4 text-muted-foreground" />
            </div>
            <div class="flex flex-col flex-1 min-w-0">
                <span class="font-medium">{item.name}</span>
                <span class="text-xs text-muted-foreground">{item.note}</span>
                <code class="font-mono text-sm break-all">{move || shown.get().unwrap_or_else(|| "************".into())}</code>
            </div>
            <Button variant=ButtonVariant::Ghost size=ButtonSize::IconSm attr:title="Show" on:click=toggle>
                <Eye />
            </Button>
            <Button variant=ButtonVariant::Ghost size=ButtonSize::IconSm attr:title="Copy" on:click=copy_value>
                <Copy />
            </Button>
            <Button variant=ButtonVariant::Ghost size=ButtonSize::IconSm attr:title="Delete" on:click=remove>
                <Trash2 />
            </Button>
        </div>
    }
}
