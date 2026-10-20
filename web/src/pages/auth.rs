use leptos::ev::SubmitEvent;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::A;
use leptos_router::hooks::use_params_map;
use serde_json::{Value, json};

use crate::api;
use crate::components::ui::button::Button;
use crate::components::ui::card::{Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle};
use crate::components::ui::input::{Input, InputType};
use crate::components::ui::label::Label;
use crate::widgets::{LogoTile, use_ui};

#[component]
fn AuthCard(#[prop(into)] title: String, #[prop(into)] subtitle: String, children: Children) -> impl IntoView {
    view! {
        <div class="flex justify-center items-center p-4 min-h-screen">
            <Card class="w-full max-w-sm page-enter">
                <CardHeader>
                    <div class="flex gap-2 items-center mb-3">
                        <LogoTile class="size-9" />
                        <span class="text-lg font-semibold tracking-tight">"RustKeep"</span>
                    </div>
                    <CardTitle class="text-xl">{title}</CardTitle>
                    <CardDescription>{subtitle}</CardDescription>
                </CardHeader>
                {children()}
            </Card>
        </div>
    }
}

#[component]
pub fn Login() -> impl IntoView {
    let ui = use_ui();
    let email = RwSignal::new(String::new());
    let password = RwSignal::new(String::new());
    let submit = move |ev: SubmitEvent| {
        ev.prevent_default();
        spawn_local(async move {
            let body = json!({ "email": email.get_untracked(), "password": password.get_untracked() });
            match api::post::<Value>("/api/auth/login", &body).await {
                Ok(_) => {
                    let _ = window().location().set_href(&api::url("/"));
                }
                Err(e) => ui.notify(e.msg),
            }
        });
    };
    view! {
        <AuthCard title="Welcome back" subtitle="Sign in to your account">
            <form on:submit=submit>
                <CardContent class="flex flex-col gap-3">
                    <Label>"Email"</Label>
                    <Input r#type=InputType::Email bind_value=email required=true autocomplete="email" />
                    <div class="flex justify-between items-center">
                        <Label>"Password"</Label>
                        <A href=api::url("/forgot") attr:class="text-sm underline-offset-4 text-muted-foreground hover:text-foreground hover:underline">
                            "Forgot password?"
                        </A>
                    </div>
                    <Input r#type=InputType::Password bind_value=password required=true autocomplete="current-password" />
                    <Button class="mt-2 w-full">"Sign in"</Button>
                </CardContent>
            </form>
            <CardFooter class="text-sm text-muted-foreground">
                "No account yet?"
                <A href=api::url("/register") attr:class="underline underline-offset-4">"Register"</A>
            </CardFooter>
        </AuthCard>
    }
}

#[component]
pub fn Register() -> impl IntoView {
    let ui = use_ui();
    let name = RwSignal::new(String::new());
    let email = RwSignal::new(String::new());
    let password = RwSignal::new(String::new());
    let done = RwSignal::new(None::<String>);
    let submit = move |ev: SubmitEvent| {
        ev.prevent_default();
        spawn_local(async move {
            let body = json!({ "name": name.get_untracked(), "email": email.get_untracked(), "password": password.get_untracked() });
            match api::post::<Value>("/api/auth/register", &body).await {
                Ok(v) => done.set(v["message"].as_str().map(str::to_owned)),
                Err(e) => ui.notify(e.msg),
            }
        });
    };
    view! {
        <AuthCard title="Register" subtitle="Your account becomes active after admin approval">
            <Show
                when=move || done.get().is_none()
                fallback=move || view! { <CardContent class="text-sm">{done.get()}</CardContent> }
            >
                <form on:submit=submit>
                    <CardContent class="flex flex-col gap-3">
                        <Label>"Name"</Label>
                        <Input bind_value=name required=true />
                        <Label>"Email"</Label>
                        <Input r#type=InputType::Email bind_value=email required=true />
                        <Label>"Password (at least 8 characters)"</Label>
                        <Input r#type=InputType::Password bind_value=password required=true minlength=8 />
                        <Button class="mt-2 w-full">"Register"</Button>
                    </CardContent>
                </form>
            </Show>
            <BackToSignIn />
        </AuthCard>
    }
}

#[component]
fn BackToSignIn() -> impl IntoView {
    view! {
        <CardFooter class="text-sm text-muted-foreground">
            <A href=api::url("/login") attr:class="underline underline-offset-4">"Back to sign in"</A>
        </CardFooter>
    }
}

#[component]
pub fn ForgotPassword() -> impl IntoView {
    let ui = use_ui();
    let email = RwSignal::new(String::new());
    let done = RwSignal::new(None::<String>);
    let submit = move |ev: SubmitEvent| {
        ev.prevent_default();
        spawn_local(async move {
            let body = json!({ "email": email.get_untracked() });
            match api::post::<Value>("/api/auth/forgot", &body).await {
                Ok(v) => done.set(v["message"].as_str().map(str::to_owned)),
                Err(e) => ui.notify(e.msg),
            }
        });
    };
    view! {
        <AuthCard title="Forgot password?" subtitle="Enter your email and we will send you a reset link">
            <Show
                when=move || done.get().is_none()
                fallback=move || view! {
                    <CardContent class="flex flex-col gap-2 text-sm">
                        <p class="font-medium">"Check your inbox"</p>
                        <p class="text-muted-foreground">{done.get()}" Open the link in the email to choose a new password."</p>
                    </CardContent>
                }
            >
                <form on:submit=submit>
                    <CardContent class="flex flex-col gap-3">
                        <Label>"Email"</Label>
                        <Input r#type=InputType::Email bind_value=email required=true autocomplete="email" />
                        <Button class="mt-2 w-full">"Send reset link"</Button>
                    </CardContent>
                </form>
            </Show>
            <BackToSignIn />
        </AuthCard>
    }
}

#[component]
pub fn ResetPassword() -> impl IntoView {
    let ui = use_ui();
    let params = use_params_map();
    let password = RwSignal::new(String::new());
    let repeat = RwSignal::new(String::new());
    let done = RwSignal::new(false);
    let submit = move |ev: SubmitEvent| {
        ev.prevent_default();
        if password.get_untracked() != repeat.get_untracked() {
            ui.notify("Passwords do not match");
            return;
        }
        let token = params.with_untracked(|p| p.get("token")).unwrap_or_default();
        spawn_local(async move {
            let body = json!({ "token": token, "password": password.get_untracked() });
            match api::post::<Value>("/api/auth/reset", &body).await {
                Ok(_) => done.set(true),
                Err(e) => ui.notify(e.msg),
            }
        });
    };
    view! {
        <AuthCard title="Choose a new password" subtitle="Use at least 8 characters">
            <Show
                when=move || !done.get()
                fallback=|| view! {
                    <CardContent class="flex flex-col gap-3 text-sm">
                        <p class="text-muted-foreground">"Your password has been changed. All devices were signed out."</p>
                        <Button class="w-full" href=api::url("/login")>"Sign in"</Button>
                    </CardContent>
                }
            >
                <form on:submit=submit>
                    <CardContent class="flex flex-col gap-3">
                        <Label>"New password"</Label>
                        <Input r#type=InputType::Password bind_value=password required=true minlength=8 autocomplete="new-password" />
                        <Label>"Repeat new password"</Label>
                        <Input r#type=InputType::Password bind_value=repeat required=true minlength=8 autocomplete="new-password" />
                        <Button class="mt-2 w-full">"Reset password"</Button>
                    </CardContent>
                </form>
            </Show>
            <BackToSignIn />
        </AuthCard>
    }
}
