#![forbid(unsafe_code)]

mod api;
mod id;
#[allow(dead_code)]
mod components;
mod md;
mod pages;
mod palette;
mod widgets;

use icons::{
    Archive, ArrowLeft, Bell, CalendarDays, Folder, KeyRound, LayoutDashboard, Lock, LogOut, Menu, Notebook, PanelLeft, Search,
    SquareKanban, Trash2,
};
use leptos::ev;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::{A, Outlet, ParentRoute, Route, Router, Routes};
use leptos_router::hooks::{use_location, use_navigate};
use leptos_router::path;
use serde_json::Value;
use wasm_bindgen::JsCast;

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
use crate::widgets::{ConfirmDialog, LogoTile, Toast, Ui, use_ui};

fn main() {
    std::panic::set_hook(Box::new(|info| web_sys::console::error_1(&info.to_string().into())));
    crate::components::hooks::use_scroll_lock::init();
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
    provide_context(Ui {
        me: RwSignal::new(None),
        toast: RwSignal::new(None),
        crumb: RwSignal::new(None),
        confirm: RwSignal::new_local(None),
    });

    view! {
        <Router base=api::base()>
            <Routes fallback=NotFound>
                <Route path=path!("/login") view=pages::auth::Login />
                <Route path=path!("/register") view=pages::auth::Register />
                <Route path=path!("/s/:token") view=pages::shared::SharedNote />
                <Route path=path!("/d/:token") view=pages::shared_folder::SharedFolder />
                <ParentRoute path=path!("") view=Layout>
                    <Route path=path!("") view=pages::dashboard::Dashboard />
                    <Route path=path!("notes") view=|| view! { <pages::notes::NotesList view=View::Active /> } />
                    <Route path=path!("archive") view=pages::archive::ArchivePage />
                    <Route path=path!("trash") view=|| view! { <pages::notes::NotesList view=View::Trash /> } />
                    <Route path=path!("secret") view=|| view! { <pages::notes::NotesList view=View::Secret /> } />
                    <Route path=path!("notes/:id") view=pages::editor::NoteEditor />
                    <Route path=path!("projects") view=pages::projects::Projects />
                    <Route path=path!("projects/overview") view=pages::projects::ProjectsOverview />
                    <Route path=path!("projects/:id") view=pages::projects::ProjectDetail />
                    <Route path=path!("files") view=pages::files::FilesPage />
                    <Route path=path!("vault") view=pages::vault::Vault />
                    <Route path=path!("reminders") view=pages::reminders::Reminders />
                    <Route path=path!("calendar") view=pages::calendar::CalendarPage />
                </ParentRoute>
            </Routes>
            <Toast />
            <ConfirmDialog />
        </Router>
    }
}

#[component]
fn NotFound() -> impl IntoView {
    view! {
        <div class="flex flex-col gap-3 items-center p-16 text-center">
            <p class="text-lg font-semibold">"Page not found"</p>
            <Button href=api::url("/")>"Go to dashboard"</Button>
        </div>
    }
}

fn section(path: &str) -> (&'static str, &'static str) {
    let rest = path.strip_prefix(api::base()).unwrap_or(path).trim_start_matches('/');
    match rest.split('/').next().unwrap_or("") {
        "notes" => ("Notes", "/notes"),
        "secret" => ("Secret notes", "/secret"),
        "projects" => ("Projects", "/projects"),
        "files" => ("Files", "/files"),
        "reminders" => ("Reminders", "/reminders"),
        "calendar" => ("Calendar", "/calendar"),
        "vault" => ("Vault", "/vault"),
        "archive" => ("Archive", "/archive"),
        "trash" => ("Trash", "/trash"),
        _ => ("Dashboard", "/"),
    }
}

fn parent(path: &str) -> &'static str {
    let rest = path.strip_prefix(api::base()).unwrap_or(path).trim_end_matches('/');
    let (_, root) = section(path);
    if rest == root.trim_end_matches('/') { "/" } else { root }
}

fn typing() -> bool {
    document().active_element().is_some_and(|el| {
        let tag = el.tag_name().to_ascii_lowercase();
        matches!(tag.as_str(), "input" | "textarea" | "select") || el.has_attribute("contenteditable")
    })
}

fn shortcuts(palette: RwSignal<bool>) {
    let listener = window_event_listener(ev::keydown, move |e| {
        if (e.ctrl_key() || e.meta_key()) && e.key().eq_ignore_ascii_case("k") {
            e.prevent_default();
            palette.update(|o| *o = !*o);
            return;
        }
        if e.key() != "/" || typing() || palette.get_untracked() {
            return;
        }
        if let Ok(Some(el)) = document().query_selector("input[data-search]") {
            e.prevent_default();
            let _ = el.unchecked_into::<web_sys::HtmlElement>().focus();
        }
    });
    on_cleanup(move || listener.remove());
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
        <A href=api::url("/") attr:class="flex gap-2 items-center p-2 rounded-lg transition-colors hover:bg-sidenav-accent" attr:title="Go to dashboard">
            <LogoTile class="size-8" />
            <div class="flex flex-col leading-tight">
                <span class="text-sm font-semibold">"RustKeep"</span>
                <span class="text-xs text-muted-foreground">"Personal workspace"</span>
            </div>
        </A>
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
                <NavItem path="/calendar" label="Calendar"><CalendarDays /></NavItem>
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
fn Breadcrumb() -> impl IntoView {
    let ui = use_ui();
    let location = use_location();
    let navigate = use_navigate();
    let current = move || section(&location.pathname.get());
    let crumb = move || {
        let path = location.pathname.get();
        ui.crumb.get().filter(|(p, _)| *p == path).map(|(_, text)| text)
    };
    let at_home = move || current().1 == "/";
    let up = move |_| navigate(parent(&location.pathname.get_untracked()), Default::default());

    view! {
        <Show when=move || !at_home()>
            <Button variant=ButtonVariant::Ghost size=ButtonSize::IconSm attr:title="Go back" on:click=up.clone()>
                <ArrowLeft />
            </Button>
        </Show>
        <nav class="flex gap-1.5 items-center min-w-0 text-sm">
            <A href=api::url("/") attr:class="hidden sm:inline text-muted-foreground hover:text-foreground">"RustKeep"</A>
            <Show when=move || !at_home()>
                <span class="hidden sm:inline text-muted-foreground">"/"</span>
                <A
                    href=move || api::url(current().1)
                    attr:class=move || if crumb().is_some() { "text-muted-foreground hover:text-foreground" } else { "font-medium" }
                >
                    {move || current().0}
                </A>
            </Show>
            <Show when=at_home>
                <span class="hidden sm:inline text-muted-foreground">"/"</span>
                <span class="font-medium">"Dashboard"</span>
            </Show>
            {move || crumb().map(|c| view! {
                <span class="text-muted-foreground">"/"</span>
                <span class="font-medium truncate max-w-48 sm:max-w-80">{if c.is_empty() { "Untitled".to_owned() } else { c }}</span>
            })}
        </nav>
    }
}

#[component]
fn Layout() -> impl IntoView {
    let ui = use_ui();
    spawn_local(ui.refresh_me());
    let location = use_location();
    let mobile = RwSignal::new(false);
    let palette = RwSignal::new(false);
    shortcuts(palette);
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
            <div class="flex overflow-x-clip flex-col flex-1 min-w-0 bg-background">
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
                    <Breadcrumb />
                    <div class="flex-1" />
                    <Button variant=ButtonVariant::Outline size=ButtonSize::Sm class="text-muted-foreground" attr:title="Search and jump (Ctrl+K)" on:click=move |_| palette.set(true)>
                        <Search />
                        <span class="hidden sm:inline">"Search..."</span>
                        <kbd class="hidden px-1.5 font-mono rounded border md:inline text-[10px] bg-muted">"Ctrl K"</kbd>
                    </Button>
                    <ThemeToggle />
                </header>
                <main class="flex-1 p-4 md:p-6 lg:p-8">
                    <Outlet />
                </main>
            </div>
        </SidenavWrapper>
        <palette::Palette open=palette />
    }
}
