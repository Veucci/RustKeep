use icons::{
    Bell, CalendarDays, Camera, ExternalLink, Folder, FolderOpen, Github, Globe, HardDrive, KeyRound,
    Linkedin, Mail, Notebook, SquareCheck, SquareKanban, Trash2,
};
use leptos::ev::SubmitEvent;
use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::A;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::api;
use crate::components::ui::badge::{Badge, BadgeVariant};
use crate::components::ui::button::{Button, ButtonSize, ButtonVariant};
use crate::components::ui::card::{Card, CardContent, CardDescription, CardHeader, CardTitle};
use crate::components::ui::input::{Input, InputType};
use crate::components::ui::label::Label;
use crate::components::ui::skeleton::Skeleton;
use crate::widgets::{LogoTile, MeAvatar, PageHeader, fmt_date, fmt_size, use_ui};

const REPO: &str = "https://github.com/Veucci/RustKeep";
const AUTHOR: &str = "Efe Özkan";
const AUTHOR_SITE: &str = "https://efeozkan.com.tr";

#[derive(Clone, Deserialize)]
struct AccountStats {
    created: i64,
    notes: i64,
    projects: i64,
    tasks: i64,
    files: i64,
    folders: i64,
    vault: i64,
    reminders: i64,
    events: i64,
    file_bytes: i64,
    note_bytes: i64,
}

#[component]
pub fn AccountPage() -> impl IntoView {
    let ui = use_ui();
    let stats = LocalResource::new(move || async move {
        ui.run(api::get::<AccountStats>("/api/me/stats")).await
    });
    let data = move || stats.get().flatten();

    view! {
        <div class="flex flex-col gap-6 mx-auto max-w-5xl page-enter">
            <PageHeader title="Account" description="Your profile, security and workspace usage." />
            <ProfileSummary created=Signal::derive(move || data().map(|s| s.created)) />
            <div class="grid grid-cols-1 gap-4 lg:grid-cols-2">
                <ProfileForm />
                <PasswordForm />
            </div>
            {move || match data() {
                Some(s) => view! { <UsageCard stats=s /> }.into_any(),
                None => view! { <Skeleton class="h-64 rounded-xl" /> }.into_any(),
            }}
            <AboutCard />
        </div>
    }
}

#[component]
fn ProfileSummary(created: Signal<Option<i64>>) -> impl IntoView {
    let ui = use_ui();
    let name = move || {
        ui.me
            .with(|m| m.as_ref().map(|m| m.name.clone()).unwrap_or_default())
    };
    let email = move || {
        ui.me
            .with(|m| m.as_ref().map(|m| m.email.clone()).unwrap_or_default())
    };
    let has_avatar = move || ui.me.with(|m| m.as_ref().is_some_and(|m| m.avatar > 0));
    let input = NodeRef::<html::Input>::new();
    let pick = move |_| {
        if let Some(el) = input.get() {
            el.click();
        }
    };
    let upload = move |_| {
        let Some(el) = input.get() else { return };
        let Some(file) = el.files().and_then(|l| l.get(0)) else { return };
        el.set_value("");
        spawn_local(async move {
            if ui.run(api::put_file::<Value>("/api/me/avatar", &file)).await.is_some() {
                ui.refresh_me().await;
                ui.notify("Profile picture updated");
            }
        });
    };
    let remove = move |_| {
        spawn_local(async move {
            if ui.run(api::del("/api/me/avatar")).await.is_some() {
                ui.refresh_me().await;
                ui.notify("Profile picture removed");
            }
        });
    };
    let since = move || {
        created
            .get()
            .map(|c| format!("Member since {}", fmt_date(c)))
            .unwrap_or_default()
    };

    view! {
        <Card>
            <CardContent class="flex gap-4 items-center">
                <button
                    type="button"
                    class="relative rounded-xl cursor-pointer group shrink-0 focus-visible:ring-[3px] focus-visible:ring-ring/50 outline-none"
                    title="Change profile picture"
                    on:click=pick
                >
                    <MeAvatar class="rounded-xl size-16" fallback_class="text-xl rounded-xl" />
                    <span class="flex absolute inset-0 z-20 justify-center items-center text-white rounded-xl opacity-0 transition-opacity group-hover:opacity-100 bg-black/50">
                        <Camera class="size-5" />
                    </span>
                </button>
                <input type="file" accept="image/*" class="hidden" node_ref=input on:change=upload />
                <div class="flex flex-col flex-1 gap-0.5 min-w-0">
                    <span class="text-lg font-semibold truncate">{name}</span>
                    <span class="text-sm truncate text-muted-foreground">{email}</span>
                    <span class="text-xs text-muted-foreground">{since}</span>
                </div>
                <div class="flex flex-wrap gap-2 justify-end">
                    <Button variant=ButtonVariant::Outline size=ButtonSize::Sm on:click=pick>
                        <Camera />
                        <span class="hidden sm:inline">"Change photo"</span>
                    </Button>
                    <Show when=has_avatar>
                        <Button variant=ButtonVariant::Ghost size=ButtonSize::IconSm attr:title="Remove photo" on:click=remove>
                            <Trash2 />
                        </Button>
                    </Show>
                </div>
            </CardContent>
        </Card>
    }
}

#[component]
fn ProfileForm() -> impl IntoView {
    let ui = use_ui();
    let name = RwSignal::new(String::new());
    let email = RwSignal::new(String::new());
    let password = RwSignal::new(String::new());
    Effect::new(move |_| {
        if let Some(m) = ui.me.get() {
            name.set(m.name);
            email.set(m.email);
        }
    });
    let submit = move |ev: SubmitEvent| {
        ev.prevent_default();
        spawn_local(async move {
            let body = json!({ "name": name.get_untracked(), "email": email.get_untracked(), "password": password.get_untracked() });
            if ui.run(api::put::<Value>("/api/me", &body)).await.is_some() {
                password.set(String::new());
                ui.refresh_me().await;
                ui.notify("Profile updated");
            }
        });
    };

    view! {
        <Card>
            <CardHeader>
                <CardTitle>"Profile"</CardTitle>
                <CardDescription>"Change your display name and sign-in email."</CardDescription>
            </CardHeader>
            <CardContent>
                <form class="flex flex-col gap-3" on:submit=submit>
                    <Label>"Name"</Label>
                    <Input bind_value=name required=true autocomplete="name" />
                    <Label>"Email"</Label>
                    <Input r#type=InputType::Email bind_value=email required=true autocomplete="email" />
                    <Label>"Current password"</Label>
                    <Input r#type=InputType::Password bind_value=password required=true autocomplete="current-password" />
                    <Button class="self-end mt-2">"Save profile"</Button>
                </form>
            </CardContent>
        </Card>
    }
}

#[component]
fn PasswordForm() -> impl IntoView {
    let ui = use_ui();
    let current = RwSignal::new(String::new());
    let next = RwSignal::new(String::new());
    let repeat = RwSignal::new(String::new());
    let submit = move |ev: SubmitEvent| {
        ev.prevent_default();
        if next.get_untracked() != repeat.get_untracked() {
            ui.notify("New passwords do not match");
            return;
        }
        spawn_local(async move {
            let body = json!({ "password": current.get_untracked(), "new_password": next.get_untracked() });
            if ui
                .run(api::put::<Value>("/api/me/password", &body))
                .await
                .is_some()
            {
                [current, next, repeat]
                    .iter()
                    .for_each(|s| s.set(String::new()));
                ui.notify("Password changed, other sessions were signed out");
            }
        });
    };
    let forgot = move |_| {
        let email = ui.me.with_untracked(|m| m.as_ref().map(|m| m.email.clone()).unwrap_or_default());
        spawn_local(async move {
            if ui.run(api::post::<Value>("/api/auth/forgot", &json!({ "email": email }))).await.is_some() {
                ui.notify(format!("We sent a reset link to {email}. Use it to set a new password."));
            }
        });
    };

    view! {
        <Card>
            <CardHeader>
                <CardTitle>"Password"</CardTitle>
                <CardDescription>"Use at least 8 characters. Other devices will be signed out."</CardDescription>
            </CardHeader>
            <CardContent>
                <form class="flex flex-col gap-3" on:submit=submit>
                    <Label>"Current password"</Label>
                    <Input r#type=InputType::Password bind_value=current required=true autocomplete="current-password" />
                    <Label>"New password"</Label>
                    <Input r#type=InputType::Password bind_value=next required=true autocomplete="new-password" />
                    <Label>"Repeat new password"</Label>
                    <Input r#type=InputType::Password bind_value=repeat required=true autocomplete="new-password" />
                    <div class="flex flex-wrap gap-2 justify-between items-center mt-2">
                        <Button attr:r#type="button" variant=ButtonVariant::Link class="px-0" on:click=forgot>"Forgot password?"</Button>
                        <Button>"Change password"</Button>
                    </div>
                </form>
            </CardContent>
        </Card>
    }
}

#[component]
fn Metric(label: &'static str, value: i64, href: &'static str, children: Children) -> impl IntoView {
    view! {
        <A href=api::url(href) attr:class="flex gap-3 items-center p-3 rounded-lg border transition-colors hover:bg-accent">
            <div class="flex justify-center items-center rounded-md size-8 bg-muted shrink-0 text-muted-foreground [&_svg]:size-4">
                {children()}
            </div>
            <div class="flex flex-col min-w-0">
                <span class="text-lg font-semibold tabular-nums leading-tight">{value}</span>
                <span class="text-xs text-muted-foreground">{label}</span>
            </div>
        </A>
    }
}

#[component]
fn UsageCard(stats: AccountStats) -> impl IntoView {
    let s = stats;
    let total = s.file_bytes + s.note_bytes;

    view! {
        <Card>
            <CardHeader>
                <CardTitle>"Usage"</CardTitle>
                <CardDescription>"Everything stored in your workspace, including archived and trashed items."</CardDescription>
            </CardHeader>
            <CardContent class="flex flex-col gap-4">
                <div class="flex gap-4 items-center p-4 rounded-lg bg-muted/50">
                    <HardDrive class="size-6 text-muted-foreground shrink-0" />
                    <div class="flex flex-col flex-1">
                        <span class="text-2xl font-semibold tracking-tight tabular-nums">{fmt_size(total)}</span>
                        <span class="text-xs text-muted-foreground">
                            {format!("{} in files, {} in notes", fmt_size(s.file_bytes), fmt_size(s.note_bytes))}
                        </span>
                    </div>
                </div>
                <div class="grid grid-cols-2 gap-3 sm:grid-cols-4">
                    <Metric label="Notes" value=s.notes href="/notes"><Notebook /></Metric>
                    <Metric label="Projects" value=s.projects href="/projects"><SquareKanban /></Metric>
                    <Metric label="Tasks" value=s.tasks href="/projects/overview"><SquareCheck /></Metric>
                    <Metric label="Files" value=s.files href="/files"><Folder /></Metric>
                    <Metric label="Folders" value=s.folders href="/files"><FolderOpen /></Metric>
                    <Metric label="Reminders" value=s.reminders href="/reminders"><Bell /></Metric>
                    <Metric label="Events" value=s.events href="/calendar"><CalendarDays /></Metric>
                    <Metric label="Vault secrets" value=s.vault href="/vault"><KeyRound /></Metric>
                </div>
            </CardContent>
        </Card>
    }
}

#[component]
fn AuthorLink(href: &'static str, label: &'static str, children: Children) -> impl IntoView {
    view! {
        <Button variant=ButtonVariant::Outline href=href attr:target="_blank" attr:rel="noopener">
            {children()}
            {label}
        </Button>
    }
}

#[component]
fn AboutCard() -> impl IntoView {
    view! {
        <Card>
            <CardHeader>
                <div class="flex gap-3 items-center">
                    <LogoTile class="size-10" />
                    <div class="flex flex-col gap-1">
                        <CardTitle class="flex gap-2 items-center">
                            "About RustKeep"
                            <Badge variant=BadgeVariant::Muted>{concat!("v", env!("CARGO_PKG_VERSION"))}</Badge>
                        </CardTitle>
                        <CardDescription>"Free and open source, released under the MIT license."</CardDescription>
                    </div>
                </div>
            </CardHeader>
            <CardContent class="flex flex-col gap-4">
                <p class="text-sm text-muted-foreground">
                    "RustKeep is a self-hosted workspace for notes, files, secrets, reminders and project boards. "
                    "The source code is public: report issues, suggest features or contribute on GitHub."
                </p>
                <a
                    href=REPO
                    target="_blank"
                    rel="noopener"
                    class="flex gap-3 items-center p-4 rounded-lg border transition-colors hover:bg-accent group"
                >
                    <Github class="size-6 shrink-0" />
                    <div class="flex flex-col flex-1 min-w-0">
                        <span class="font-medium">"Veucci/RustKeep"</span>
                        <span class="text-xs truncate text-muted-foreground">{REPO}</span>
                    </div>
                    <ExternalLink class="size-4 text-muted-foreground group-hover:text-foreground" />
                </a>
                <div class="flex flex-wrap gap-3 justify-between items-center">
                    <span class="text-sm">
                        <span class="text-muted-foreground">"Built by "</span>
                        <a href=AUTHOR_SITE target="_blank" rel="noopener" class="font-medium underline-offset-4 hover:underline">{AUTHOR}</a>
                    </span>
                    <div class="flex flex-wrap gap-2">
                        <AuthorLink href=AUTHOR_SITE label="Website"><Globe /></AuthorLink>
                        <AuthorLink href="https://www.linkedin.com/in/efe-%C3%B6zkan" label="LinkedIn"><Linkedin /></AuthorLink>
                        <AuthorLink href="https://github.com/Veucci" label="GitHub"><Github /></AuthorLink>
                        <AuthorLink href="mailto:contact@efeozkan.com.tr" label="contact@efeozkan.com.tr"><Mail /></AuthorLink>
                    </div>
                </div>
            </CardContent>
        </Card>
    }
}
