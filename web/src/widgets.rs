use std::rc::Rc;
use std::time::Duration;

use icons::{ArrowUpDown, Search};
use leptos::ev::SubmitEvent;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::NavigateOptions;
use leptos_router::hooks::{use_location, use_navigate, use_query_map};
use wasm_bindgen::JsCast;
use serde::Deserialize;
use serde_json::{Value, json};
use wasm_bindgen::JsValue;

use crate::api::{self, ApiError};
use crate::components::ui::alert_dialog::{
    AlertDialogBody, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle,
    ControlledAlertDialog,
};
use crate::components::ui::button::{Button, ButtonVariant};
use crate::components::ui::card::{Card, CardContent, CardDescription, CardHeader, CardTitle};
use crate::components::ui::date_picker::DateTimePicker;
use crate::components::ui::dialog::{ControlledDialog, DialogBody, DialogTitle};
use crate::components::ui::input::{Input, InputType};
use crate::components::ui::label::Label;
use crate::components::ui::select::{Select, SelectContent, SelectGroup, SelectOption, SelectTrigger};
use crate::components::ui::skeleton::Skeleton;

#[derive(Clone, Deserialize)]
pub struct Me {
    pub email: String,
    pub name: String,
    pub has_pin: i64,
    pub pin_ok: bool,
}

#[derive(Clone)]
pub struct ConfirmRequest {
    message: String,
    action: Rc<dyn Fn()>,
}

#[derive(Clone, Copy)]
pub struct Ui {
    pub me: RwSignal<Option<Me>>,
    pub toast: RwSignal<Option<String>>,
    pub crumb: RwSignal<Option<(String, String)>>,
    pub confirm: RwSignal<Option<ConfirmRequest>, LocalStorage>,
}

impl Ui {
    pub fn confirm_delete(&self, message: impl Into<String>, action: impl Fn() + 'static) {
        self.confirm.set(Some(ConfirmRequest { message: message.into(), action: Rc::new(action) }));
    }

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

    pub fn set_crumb(self, text: String) {
        let path = window().location().pathname().unwrap_or_default();
        self.crumb.set(Some((path, text)));
    }

    pub fn act(self, path: String, done: impl Fn() + 'static) {
        spawn_local(async move {
            if self.run(api::post::<Value>(&path, &())).await.is_some() {
                done();
            }
        });
    }
}

pub fn create_note(ui: Ui, navigate: impl Fn(&str, NavigateOptions) + 'static, body: Value) {
    spawn_local(async move {
        if let Some(v) = ui.run(api::post::<Value>("/api/notes", &body)).await {
            navigate(&format!("/notes/{}", v["id"].as_str().unwrap_or_default()), Default::default());
        }
    });
}

pub type Options = &'static [(&'static str, &'static str)];

pub fn owned(options: Options) -> Vec<(String, String)> {
    options.iter().map(|(key, label)| ((*key).to_owned(), (*label).to_owned())).collect()
}

pub fn query_state(key: &'static str, default: &'static str) -> (Signal<String>, Callback<String>) {
    let query = use_query_map();
    let location = use_location();
    let navigate = use_navigate();
    let value = Memo::new(move |_| query.with(|q| q.get(key)).unwrap_or_else(|| default.to_owned()));
    let update = Callback::new(move |v: String| {
        let mut q = query.get_untracked();
        q.remove(key);
        if v != default {
            q.insert(key, v);
        }
        let path = location.pathname.get_untracked();
        let path = path.strip_prefix(api::base()).unwrap_or(&path);
        let opts = NavigateOptions { replace: true, scroll: false, ..Default::default() };
        navigate(&format!("{path}{}", q.to_query_string()), opts);
    });
    (value.into(), update)
}

pub fn focus_if_new() {
    if use_query_map().with_untracked(|q| q.get("new").is_none()) {
        return;
    }
    request_animation_frame(|| {
        if let Some(el) = document().get_element_by_id("new-item") {
            let _ = el.unchecked_into::<web_sys::HtmlElement>().focus();
        }
    });
}

pub fn has(text: &str, needle: &str) -> bool {
    text.to_lowercase().contains(&needle.trim().to_lowercase())
}

pub fn today() -> String {
    let d = js_sys::Date::new_0();
    format!("{:04}-{:02}-{:02}", d.get_full_year(), d.get_month() + 1, d.get_date())
}

pub fn days_until(due: &str) -> i64 {
    let at = |s: &str| js_sys::Date::new(&JsValue::from_str(&format!("{s}T00:00:00"))).get_time();
    ((at(due) - at(&today())) / 86_400_000.0).round() as i64
}

pub fn due_label(days: i64) -> String {
    match days {
        0 => "Today".into(),
        1 => "Tomorrow".into(),
        -1 => "Yesterday".into(),
        d if d < 0 => format!("{} days overdue", -d),
        d => format!("In {d} days"),
    }
}

#[component]
pub fn Segmented(options: Options, value: Signal<String>, on_change: Callback<String>) -> impl IntoView {
    view! {
        <div class="inline-flex overflow-x-auto gap-1 items-center p-1 max-w-full rounded-lg w-fit bg-muted [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
            {options
                .iter()
                .map(|(key, label)| {
                    let class = move || {
                        let active = if value.get() == *key {
                            "bg-background text-foreground shadow-sm"
                        } else {
                            "text-muted-foreground hover:text-foreground"
                        };
                        format!("px-3 h-7 text-sm font-medium whitespace-nowrap rounded-md transition-all shrink-0 {active}")
                    };
                    view! { <button type="button" class=class on:click=move |_| on_change.run((*key).to_owned())>{*label}</button> }
                })
                .collect_view()}
        </div>
    }
}

#[component]
pub fn Choice(
    #[prop(into)] options: Signal<Vec<(String, String)>>,
    #[prop(into)] value: Signal<String>,
    on_change: Callback<String>,
    #[prop(into, optional)] class: String,
) -> impl IntoView {
    let label = move || {
        let current = value.get();
        options.with(|options| options.iter().find(|(key, _)| *key == current).map(|(_, label)| label.clone()))
    };
    view! {
        <Select
            class=class
            value=Signal::derive(move || Some(value.get()))
            on_change=Callback::new(move |picked: Option<String>| on_change.run(picked.unwrap_or_default()))
        >
            <SelectTrigger>
                <span class="truncate">{label}</span>
            </SelectTrigger>
            <SelectContent class="w-auto max-w-80 whitespace-nowrap">
                <SelectGroup>
                    <For each=move || options.get() key=|(key, _)| key.clone() let((key, label))>
                        <SelectOption value=key>{label}</SelectOption>
                    </For>
                </SelectGroup>
            </SelectContent>
        </Select>
    }
}

#[component]
pub fn SortSelect(options: Options, value: Signal<String>, on_change: Callback<String>) -> impl IntoView {
    view! {
        <div class="flex gap-1.5 items-center" title="Sort">
            <ArrowUpDown class="size-4 text-muted-foreground" />
            <Choice options=owned(options) value on_change />
        </div>
    }
}

#[component]
pub fn SearchBox(value: RwSignal<String>, #[prop(into)] placeholder: String) -> impl IntoView {
    view! {
        <div class="relative w-full sm:w-56">
            <Search class="absolute left-2.5 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
            <input
                data-search="true"
                class="pr-2 pl-8 w-full h-9 text-sm rounded-md border shadow-xs outline-none bg-background border-input dark:bg-input/30 focus-visible:border-ring focus-visible:ring-ring/50 focus-visible:ring-[3px]"
                placeholder=placeholder
                prop:value=move || value.get()
                on:input=move |ev| value.set(event_target_value(&ev))
            />
        </div>
    }
}

#[component]
pub fn Toolbar(children: Children) -> impl IntoView {
    view! { <div class="flex flex-wrap gap-2 justify-between items-center">{children()}</div> }
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
pub fn Modal(
    open: RwSignal<bool>,
    #[prop(into)] title: String,
    children: ChildrenFn,
    #[prop(optional, into)] class: String,
) -> impl IntoView {
    let title = StoredValue::new(title);
    let children = StoredValue::new(children);
    view! {
        <ControlledDialog open on_close=Callback::new(move |()| open.set(false)) class=format!("max-w-[min(28rem,calc(100%-2rem))] {class}")>
            <DialogBody>
                <DialogTitle class="pr-6">{title.get_value()}</DialogTitle>
                {children.with_value(|children| children())}
            </DialogBody>
        </ControlledDialog>
    }
}

#[component]
pub fn ConfirmDialog() -> impl IntoView {
    let ui = use_ui();
    let open = Signal::derive(move || ui.confirm.with(Option::is_some));
    let close = Callback::new(move |()| ui.confirm.set(None));
    let message = move || ui.confirm.with(|request| request.as_ref().map(|r| r.message.clone()).unwrap_or_default());
    let accept = move |_| {
        if let Some(request) = ui.confirm.get_untracked() {
            ui.confirm.set(None);
            (request.action)();
        }
    };
    view! {
        <ControlledAlertDialog open on_close=close class="max-w-[min(28rem,calc(100%-2rem))]">
            <AlertDialogBody>
                <AlertDialogHeader>
                    <AlertDialogTitle>"Are you sure?"</AlertDialogTitle>
                    <AlertDialogDescription>{message}</AlertDialogDescription>
                </AlertDialogHeader>
                <AlertDialogFooter>
                    <Button variant=ButtonVariant::Outline on:click=move |_| close.run(())>"Cancel"</Button>
                    <Button variant=ButtonVariant::Destructive on:click=accept>"Delete"</Button>
                </AlertDialogFooter>
            </AlertDialogBody>
        </ControlledAlertDialog>
    }
}

pub fn parse_local(v: &str) -> Option<i64> {
    let t = js_sys::Date::new(&JsValue::from_str(v)).get_time();
    (!v.is_empty() && !t.is_nan()).then(|| (t / 1000.0) as i64)
}

fn fmt_with(ts: i64, time: bool) -> String {
    let opts = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&opts, &"dateStyle".into(), &"medium".into());
    if time {
        let _ = js_sys::Reflect::set(&opts, &"timeStyle".into(), &"short".into());
    }
    let locale = window().navigator().language().unwrap_or_else(|| "en-US".into());
    js_sys::Date::new(&JsValue::from_f64(ts as f64 * 1000.0)).to_locale_string(&locale, &opts).into()
}

pub fn fmt_time(ts: i64) -> String {
    fmt_with(ts, true)
}

pub fn fmt_date(ts: i64) -> String {
    fmt_with(ts, false)
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

fn qr_svg(link: &str) -> String {
    let Ok(code) = qrcode::QrCode::new(link) else { return String::new() };
    code.render::<qrcode::render::svg::Color>().quiet_zone(false).build()
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
                        <div class="self-center p-3 bg-white rounded-lg border size-48 [&_svg]:size-full" inner_html=qr_svg(&link) />
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
                        <DateTimePicker bind_value=expires />
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
                <Input id="new-item" bind_value=title required=true />
            </div>
            <div class="flex flex-col gap-2">
                <Label>"When"</Label>
                <DateTimePicker bind_value=at class="sm:w-56" />
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

#[component]
pub fn Logo(#[prop(into, optional)] class: String) -> impl IntoView {
    view! {
        <svg
            class=class
            viewBox="0 0 64 64"
            fill="none"
            stroke="currentColor"
            stroke-linejoin="round"
            role="img"
            aria-label="RustKeep"
        >
            <path d="M24 55H15a4 4 0 0 1-4-4V13a4 4 0 0 1 4-4h21l12 12v5a11 11 0 0 1-11 11h-4l17.5 18.5" stroke-width="7" />
            <path d="M36 9v8a4 4 0 0 0 4 4h8z" fill="currentColor" stroke-width="2" />
            <path d="M20 19h8M20 26h11M20 33h5" stroke-width="5" stroke-linecap="round" />
        </svg>
    }
}

#[component]
pub fn LogoTile(#[prop(into, optional)] class: String) -> impl IntoView {
    let class = format!("flex justify-center items-center rounded-lg bg-primary text-primary-foreground shrink-0 {class}");
    view! {
        <div class=class>
            <Logo class="size-[70%]" />
        </div>
    }
}
