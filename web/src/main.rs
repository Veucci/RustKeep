#![forbid(unsafe_code)]

mod api;
#[allow(dead_code)]
mod components;
mod md;
mod pages;
mod widgets;

use icons::{
    Archive, Bell, Folder, KeyRound, LayoutDashboard, Lock, LogOut, Menu, Notebook, PanelLeft, SquareKanban,
    Trash2,
};
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::{Outlet, ParentRoute, Route, Router, Routes};
use leptos_router::hooks::use_location;
use leptos_router::path;
use serde_json::Value;

use crate::components::hooks::use_theme_mode::ThemeMode;
use crate::components::ui::avatar::{Avatar, AvatarFallback};
use crate::components::ui::button::{Button, ButtonSize, ButtonVariant};
use crate::components::ui::separator::{Separator, SeparatorOrientation};
use crate::components::ui::sidenav::{
    Sidenav, SidenavContent, SidenavFooter, SidenavGroup, SidenavGroupContent, SidenavGroupLabel, SidenavHeader,
    SidenavLink, SidenavMenu, SidenavMenuItem, SidenavTrigger, SidenavWrapper,
};
use crate::components::ui::theme_toggle::ThemeToggle;
use crate::pages::notes::View;
use crate::widgets::{LogoTile, Toast, Ui, use_ui};

fn main() {
    std::panic::set_hook(Box::new(|info| web_sys::console::error_1(&info.to_string().into())));
    leptos::mount::mount_to_body(App);
}

#[component]
fn App() -> impl IntoView {
    let theme = ThemeMode::init();
    Effect::new(move |_| {
        let dark = theme.is_dark();
        if let Some(el) = document().document_element() {
            let _ = el.class_list().toggle_with_force("dark", dark);
        }
    });
    provide_context(Ui { me: RwSignal::new(None), toast: RwSignal::new(None) });

    view! {
        <Router base=api::base()>
            <Routes fallback=|| view! { <p class="p-8">"Page not found"</p> }>
                <Route path=path!("/login") view=pages::auth::Login />
                <Route path=path!("/register") view=pages::auth::Register />
                <Route path=path!("/s/:token") view=pages::shared::SharedNote />
                <ParentRoute path=path!("") view=Layout>
                    <Route path=path!("") view=pages::dashboard::Dashboard />
                    <Route path=path!("notes") view=|| view! { <pages::notes::NotesList view=View::Active /> } />
                    <Route path=path!("archive") view=|| view! { <pages::notes::NotesList view=View::Archived /> } />
                    <Route path=path!("trash") view=|| view! { <pages::notes::NotesList view=View::Trash /> } />
                    <Route path=path!("secret") view=|| view! { <pages::notes::NotesList view=View::Secret /> } />
                    <Route path=path!("notes/:id") view=pages::editor::NoteEditor />
                    <Route path=path!("projects") view=pages::projects::Projects />
                    <Route path=path!("projects/:id") view=pages::projects::ProjectDetail />
                    <Route path=path!("files") view=pages::files::FilesPage />
                    <Route path=path!("vault") view=pages::vault::Vault />
                    <Route path=path!("reminders") view=pages::reminders::Reminders />
                </ParentRoute>
            </Routes>
            <Toast />
        </Router>
    }
}

fn section(path: &str) -> &'static str {
    let rest = path.strip_prefix(api::base()).unwrap_or(path).trim_start_matches('/');
    match rest.split('/').next().unwrap_or("") {
        "notes" | "secret" => "Notes",
        "projects" => "Projects",
        "files" => "Files",
        "reminders" => "Reminders",
        "vault" => "Vault",
        "archive" => "Archive",
        "trash" => "Trash",
        _ => "Dashboard",
    }
}

#[component]
fn NavItem(path: &'static str, label: &'static str, children: Children) -> impl IntoView {
    view! {
        <SidenavMenuItem>
            <SidenavLink href=api::url(path)>
                {children()}
                <span>{label}</span>
            </SidenavLink>
        </SidenavMenuItem>
    }
}

#[component]
fn NavGroup(label: &'static str, children: Children) -> impl IntoView {
    view! {
        <SidenavGroup>
            <SidenavGroupLabel>{label}</SidenavGroupLabel>
            <SidenavGroupContent>
                <SidenavMenu>{children()}</SidenavMenu>
            </SidenavGroupContent>
        </SidenavGroup>
    }
}

#[component]
fn Brand() -> impl IntoView {
    view! {
        <div class="flex gap-2 items-center p-2">
            <LogoTile class="size-8" />
            <div class="flex flex-col leading-tight">
                <span class="text-sm font-semibold">"RustKeep"</span>
                <span class="text-xs text-muted-foreground">"Personal workspace"</span>
            </div>
        </div>
    }
}

#[component]
fn UserCard() -> impl IntoView {
    let ui = use_ui();
    let logout = move |_| {
        spawn_local(async move {
            let _ = api::post::<Value>("/api/auth/logout", &()).await;
            let _ = window().location().set_href(&api::url("/login"));
        });
    };
    let lock = move |_| {
        spawn_local(async move {
            let _ = api::post::<Value>("/api/pin/lock", &()).await;
            ui.refresh_me().await;
            ui.notify("Secret area locked");
        });
    };
    let pin_ok = move || ui.me.with(|m| m.as_ref().is_some_and(|m| m.pin_ok));
    let name = move || ui.me.with(|m| m.as_ref().map(|m| m.name.clone()).unwrap_or_default());
    let email = move || ui.me.with(|m| m.as_ref().map(|m| m.email.clone()).unwrap_or_default());
    let initials = move || name().chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();

    view! {
        <div class="flex gap-2 items-center p-2 rounded-lg hover:bg-sidenav-accent">
            <Avatar class="rounded-lg">
                <AvatarFallback class="rounded-lg">{initials}</AvatarFallback>
            </Avatar>
            <div class="flex flex-col flex-1 min-w-0 text-sm leading-tight">
                <span class="font-medium truncate">{name}</span>
                <span class="text-xs truncate text-muted-foreground">{email}</span>
            </div>
            <Show when=pin_ok>
                <Button variant=ButtonVariant::Ghost size=ButtonSize::IconSm attr:title="Lock secret area" on:click=lock>
                    <Lock />
                </Button>
            </Show>
            <Button variant=ButtonVariant::Ghost size=ButtonSize::IconSm attr:title="Sign out" on:click=logout>
                <LogOut />
            </Button>
        </div>
    }
}

#[component]
fn NavBody() -> impl IntoView {
    view! {
        <SidenavHeader>
            <Brand />
        </SidenavHeader>
        <SidenavContent>
            <NavGroup label="Overview">
                <NavItem path="/" label="Dashboard"><LayoutDashboard /></NavItem>
            </NavGroup>
            <NavGroup label="Workspace">
                <NavItem path="/notes" label="Notes"><Notebook /></NavItem>
                <NavItem path="/projects" label="Projects"><SquareKanban /></NavItem>
                <NavItem path="/files" label="Files"><Folder /></NavItem>
                <NavItem path="/reminders" label="Reminders"><Bell /></NavItem>
            </NavGroup>
            <NavGroup label="Security">
                <NavItem path="/vault" label="Vault"><KeyRound /></NavItem>
            </NavGroup>
            <NavGroup label="Library">
                <NavItem path="/archive" label="Archive"><Archive /></NavItem>
                <NavItem path="/trash" label="Trash"><Trash2 /></NavItem>
            </NavGroup>
        </SidenavContent>
        <SidenavFooter>
            <UserCard />
        </SidenavFooter>
    }
}

#[component]
fn Layout() -> impl IntoView {
    let ui = use_ui();
    spawn_local(ui.refresh_me());
    let location = use_location();
    let mobile = RwSignal::new(false);
    Effect::new(move |_| {
        location.pathname.track();
        mobile.set(false);
    });

    view! {
        <SidenavWrapper class="min-h-svh">
            <Sidenav>
                <NavBody />
            </Sidenav>
            <Show when=move || mobile.get()>
                <div class="fixed inset-0 z-40 bg-black/50 md:hidden animate-in fade-in-0" on:click=move |_| mobile.set(false) />
                <aside class="flex fixed inset-y-0 left-0 z-50 flex-col border-r shadow-xl md:hidden w-72 bg-sidenav text-sidenav-foreground animate-in slide-in-from-left duration-300">
                    <NavBody />
                </aside>
            </Show>
            <div class="flex flex-col flex-1 min-w-0 bg-background">
                <header class="flex sticky top-0 z-20 gap-2 items-center px-4 h-14 border-b backdrop-blur bg-background/80">
                    <div class="hidden md:flex">
                        <SidenavTrigger>
                            <PanelLeft />
                        </SidenavTrigger>
                    </div>
                    <Button class="md:hidden" variant=ButtonVariant::Ghost size=ButtonSize::IconSm on:click=move |_| mobile.set(true)>
                        <Menu />
                    </Button>
                    <Separator orientation=SeparatorOrientation::Vertical class="mr-1 h-4" />
                    <nav class="flex gap-1.5 items-center text-sm">
                        <span class="hidden sm:inline text-muted-foreground">"RustKeep"</span>
                        <span class="hidden sm:inline text-muted-foreground">"/"</span>
                        <span class="font-medium">{move || section(&location.pathname.get())}</span>
                    </nav>
                    <div class="flex-1" />
                    <ThemeToggle />
                </header>
                <main class="flex-1 p-4 md:p-6 lg:p-8">
                    <Outlet />
                </main>
            </div>
        </SidenavWrapper>
    }
}
