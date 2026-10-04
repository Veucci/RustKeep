use std::time::Duration;

use leptos::ev::SubmitEvent;
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::Deserialize;
use serde_json::{Value, json};
use wasm_bindgen::JsValue;

use crate::api::{self, ApiError};
use crate::components::ui::button::{Button, ButtonVariant};
use crate::components::ui::card::{Card, CardContent, CardDescription, CardHeader, CardTitle};
use crate::components::ui::input::{Input, InputType};
use crate::components::ui::label::Label;
use crate::components::ui::skeleton::Skeleton;

#[derive(Clone, Deserialize)]
pub struct Me {
    pub email: String,
    pub name: String,
    pub has_pin: i64,
    pub pin_ok: bool,
}

#[derive(Clone, Copy)]
pub struct Ui {
    pub me: RwSignal<Option<Me>>,
    pub toast: RwSignal<Option<String>>,
}

impl Ui {
    pub fn notify(&self, msg: impl Into<String>) {
        let toast = self.toast;
        toast.set(Some(msg.into()));
        set_timeout(move || toast.set(None), Duration::from_secs(4));
    }

    pub fn fail(&self, e: ApiError) {
        if e.status == 401 {
            let _ = window().location().set_href(&api::url("/login"));
            return;
        }
        if e.status == 403 {
            self.me.update(|m| m.iter_mut().for_each(|m| m.pin_ok = false));
        }
        self.notify(e.msg);
    }

    pub async fn refresh_me(self) {
        match api::get::<Me>("/api/me").await {
            Ok(m) => self.me.set(Some(m)),
            Err(e) => self.fail(e),
        }
    }

    pub async fn run<T>(self, fut: impl Future<Output = api::ApiResult<T>>) -> Option<T> {
        fut.await.map_err(|e| self.fail(e)).ok()
    }
}

pub fn use_ui() -> Ui {
    expect_context()
}

#[component]
pub fn Toast() -> impl IntoView {
    let ui = use_ui();
    view! {
        {move || {
            ui.toast
                .get()
                .map(|msg| {
                    view! {
                        <div class="fixed right-4 bottom-4 z-[200] rounded-lg border bg-popover px-4 py-3 text-sm shadow-lg animate-in fade-in-0 slide-in-from-bottom-4 duration-300">
                            {msg}
                        </div>
                    }
                })
        }}
    }
}

#[component]
pub fn Modal(open: RwSignal<bool>, #[prop(into)] title: String, children: ChildrenFn) -> impl IntoView {
    let title = StoredValue::new(title);
    view! {
        <Show when=move || open.get()>
            <div
                class="fixed inset-0 z-50 backdrop-blur-[2px] bg-black/50 animate-in fade-in-0 duration-200"
                on:click=move |_| open.set(false)
            />
            <div class="flex fixed inset-0 z-50 justify-center items-center p-4 pointer-events-none">
                <div class="flex flex-col gap-4 p-6 w-full max-w-md rounded-xl border shadow-lg pointer-events-auto bg-background pop-in">
                    <h3 class="text-lg font-semibold tracking-tight leading-none">{title.get_value()}</h3>
                    {children()}
                </div>
            </div>
        </Show>
    }
}

pub fn parse_local(v: &str) -> Option<i64> {
    let t = js_sys::Date::new(&JsValue::from_str(v)).get_time();
    (!v.is_empty() && !t.is_nan()).then(|| (t / 1000.0) as i64)
}

pub fn fmt_time(ts: i64) -> String {
    js_sys::Date::new(&JsValue::from_f64(ts as f64 * 1000.0)).to_locale_string("tr-TR", &JsValue::UNDEFINED).into()
}

pub fn fmt_size(b: i64) -> String {
    match b {
        b if b >= 1 << 30 => format!("{:.1} GB", b as f64 / (1u64 << 30) as f64),
        b if b >= 1 << 20 => format!("{:.1} MB", b as f64 / (1u64 << 20) as f64),
        b if b >= 1 << 10 => format!("{:.1} KB", b as f64 / 1024.0),
        b => format!("{b} B"),
    }
}

pub fn copy(ui: Ui, text: &str) {
    let _ = window().navigator().clipboard().write_text(text);
    ui.notify("Copied to clipboard");
}

#[component]
pub fn PinGate(children: ChildrenFn) -> impl IntoView {
    let ui = use_ui();
    let pin = RwSignal::new(String::new());
    let password = RwSignal::new(String::new());
    let has_pin = move || ui.me.with(|m| m.as_ref().is_some_and(|m| m.has_pin == 1));
    let unlocked = move || ui.me.with(|m| m.as_ref().is_some_and(|m| m.pin_ok));

    let submit = move |ev: SubmitEvent| {
        ev.prevent_default();
        let first = !has_pin();
        spawn_local(async move {
            let body = json!({ "pin": pin.get_untracked(), "password": password.get_untracked() });
            if first && ui.run(api::post::<Value>("/api/pin", &body)).await.is_none() {
                return;
            }
            if ui.run(api::post::<Value>("/api/pin/unlock", &body)).await.is_some() {
                pin.set(String::new());
                ui.refresh_me().await;
            }
        });
    };

    view! {
        <Show
            when=unlocked
            fallback=move || {
                view! {
                    <form class="px-4 mx-auto mt-16 max-w-sm page-enter" on:submit=submit>
                        <Card>
                            <CardHeader>
                                <CardTitle>"Secret area"</CardTitle>
                                <CardDescription>
                                    {move || if has_pin() { "Enter your PIN to continue." } else { "First use: confirm your password and choose a 4-8 digit PIN." }}
                                </CardDescription>
                            </CardHeader>
                            <CardContent class="flex flex-col gap-3">
                                <Show when=move || !has_pin()>
                                    <Label>"Account password"</Label>
                                    <Input r#type=InputType::Password bind_value=password />
                                </Show>
                                <Label>"PIN"</Label>
                                <Input r#type=InputType::Password bind_value=pin autocomplete="off" />
                                <Button class="w-full">"Unlock"</Button>
                            </CardContent>
                        </Card>
                    </form>
                }
            }
        >
            {children()}
        </Show>
    }
}

#[component]
pub fn ShareDialog(
    open: RwSignal<bool>,
    #[prop(into)] api_path: String,
    link_prefix: &'static str,
    token: RwSignal<Option<String>>,
) -> impl IntoView {
    let ui = use_ui();
    let api_path = StoredValue::new(api_path);
    let expires = RwSignal::new(String::new());

    let create = move |_| {
        spawn_local(async move {
            let body = json!({ "expires": parse_local(&expires.get_untracked()) });
            if let Some(v) = ui.run(api::post::<Value>(&api_path.get_value(), &body)).await {
                token.set(v["token"].as_str().map(str::to_owned));
            }
        });
    };
    let remove = move |_| {
        spawn_local(async move {
            if ui.run(api::del(&api_path.get_value())).await.is_some() {
                token.set(None);
            }
        });
    };

    view! {
        <Modal open title="Share">
            {move || match token.get() {
                Some(t) => {
                    let link = api::absolute(&format!("{link_prefix}{t}"));
                    let copy_link = link.clone();
                    view! {
                        <p class="text-sm text-muted-foreground">"Anyone with this link can access it."</p>
                        <Input readonly=true bind_value=RwSignal::new(link) />
                        <div class="flex gap-2 justify-end">
                            <Button variant=ButtonVariant::Destructive on:click=remove>"Stop sharing"</Button>
                            <Button on:click=move |_| copy(ui, &copy_link)>"Copy"</Button>
                        </div>
                    }
                        .into_any()
                }
                None => {
                    view! {
                        <Label>"Expiration date (optional)"</Label>
                        <Input r#type=InputType::DatetimeLocal bind_value=expires />
                        <div class="flex justify-end">
                            <Button on:click=create>"Create link"</Button>
                        </div>
                    }
                        .into_any()
                }
            }}
        </Modal>
    }
}

#[component]
pub fn ReminderForm(note_id: Option<String>, #[prop(into)] title: String, on_done: Callback<()>) -> impl IntoView {
    let ui = use_ui();
    let title = RwSignal::new(title);
    let at = RwSignal::new(String::new());
    let submit = move |ev: SubmitEvent| {
        ev.prevent_default();
        let Some(remind_at) = parse_local(&at.get_untracked()) else {
            ui.notify("Pick a date and time");
            return;
        };
        let note_id = note_id.clone();
        spawn_local(async move {
            let body = json!({ "title": title.get_untracked(), "remind_at": remind_at, "note_id": note_id });
            if ui.run(api::post::<Value>("/api/reminders", &body)).await.is_some() {
                at.set(String::new());
                ui.notify("Reminder added");
                on_done.run(());
            }
        });
    };
    view! {
        <form class="flex flex-col gap-3 sm:flex-row sm:items-end" on:submit=submit>
            <div class="flex flex-col flex-1 gap-2">
                <Label>"Title"</Label>
                <Input bind_value=title required=true />
            </div>
            <div class="flex flex-col gap-2">
                <Label>"When"</Label>
                <Input r#type=InputType::DatetimeLocal bind_value=at required=true />
            </div>
            <Button>"Remind me"</Button>
        </form>
    }
}

#[component]
pub fn PageHeader(
    #[prop(into)] title: TextProp,
    #[prop(into, optional)] description: String,
    #[prop(optional)] children: Option<Children>,
) -> impl IntoView {
    view! {
        <div class="flex flex-wrap gap-3 justify-between items-end">
            <div class="flex flex-col gap-1">
                <h1 class="text-2xl font-semibold tracking-tight">{move || title.get()}</h1>
                <p class="text-sm text-muted-foreground">{description}</p>
            </div>
            <div class="flex flex-wrap gap-2 items-center">{children.map(|c| c())}</div>
        </div>
    }
}

#[component]
pub fn ListSkeleton(#[prop(default = 4)] rows: usize) -> impl IntoView {
    (0..rows)
        .map(|_| {
            view! {
                <div class="flex gap-3 items-center py-3 px-4">
                    <Skeleton class="rounded-lg size-9" />
                    <div class="flex flex-col flex-1 gap-2">
                        <Skeleton class="w-1/3 h-4" />
                        <Skeleton class="w-1/5 h-3" />
                    </div>
                </div>
            }
        })
        .collect_view()
}
