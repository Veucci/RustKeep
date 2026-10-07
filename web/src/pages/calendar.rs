use icons::{
    Bell, CalendarCheck, CalendarDays, ChevronLeft, ChevronRight, ExternalLink, MapPin, Plus, RefreshCw, Send, SquareKanban,
    Trash2, Unlink,
};
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::NavigateOptions;
use leptos_router::hooks::use_navigate;
use serde::Deserialize;
use serde_json::{Value, json};
use time::macros::format_description;
use time::{Date, Duration, Month, OffsetDateTime, UtcOffset};

use crate::api;
use crate::components::ui::badge::{Badge, BadgeVariant};
use crate::components::ui::button::{Button, ButtonSize, ButtonVariant};
use crate::components::ui::card::{Card, CardContent, CardDescription, CardHeader, CardTitle};
use crate::components::ui::checkbox::Checkbox;
use crate::components::ui::date_picker::{DatePicker, DateTimePicker, format_date, parse_date, shift_month, today};
use crate::components::ui::empty::{Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant, EmptyTitle};
use crate::components::ui::input::Input;
use crate::components::ui::label::Label;
use crate::id::Id;
use crate::md;
use crate::pages::editor::{RichEditor, parse_html};
use crate::components::ui::toggle_group::{ToggleGroup, ToggleGroupItem};
use crate::widgets::{ListSkeleton, Modal, Options, PageHeader, SearchBox, Segmented, Toolbar, Ui, fmt_time, has, parse_local, query_state, use_ui};

const VIEWS: Options = &[("month", "Month"), ("agenda", "Agenda")];
const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const COLORS: [(&str, &str, &str); 6] = [
    ("blue", "bg-blue-500", "bg-blue-500/15 text-blue-700 dark:text-blue-300"),
    ("green", "bg-emerald-500", "bg-emerald-500/15 text-emerald-700 dark:text-emerald-300"),
    ("amber", "bg-amber-500", "bg-amber-500/15 text-amber-700 dark:text-amber-300"),
    ("red", "bg-rose-500", "bg-rose-500/15 text-rose-700 dark:text-rose-300"),
    ("violet", "bg-violet-500", "bg-violet-500/15 text-violet-700 dark:text-violet-300"),
    ("slate", "bg-slate-500", "bg-slate-500/15 text-slate-700 dark:text-slate-300"),
];

#[derive(Clone, PartialEq, Deserialize)]
struct Event {
    id: Id,
    title: String,
    description: String,
    location: String,
    start_at: i64,
    end_at: i64,
    all_day: i64,
    color: String,
    google_link: Option<String>,
    in_google: i64,
}

#[derive(Clone, Deserialize)]
struct Item {
    kind: String,
    id: Id,
    title: String,
    day: Option<String>,
    at: Option<i64>,
    project_id: Option<Id>,
    task_id: Option<Id>,
    note_id: Option<Id>,
    context: String,
    done: i64,
}

#[derive(Clone, PartialEq, Deserialize)]
struct GoogleStatus {
    email: String,
    synced: Option<i64>,
}

#[derive(Clone, Deserialize)]
struct CalendarData {
    configured: bool,
    google: Option<GoogleStatus>,
    events: Vec<Event>,
    items: Vec<Item>,
}

#[derive(Clone, Copy, PartialEq)]
enum Source {
    Event,
    Task,
    Reminder,
}

#[derive(Clone, PartialEq)]
struct Entry {
    key: String,
    title: String,
    first: Date,
    last: Date,
    when: String,
    timed: bool,
    sort: i64,
    source: Source,
    label: &'static str,
    chip: &'static str,
    dot: &'static str,
    href: String,
    context: String,
    location: String,
    done: bool,
    event: Option<Event>,
}

fn local(ts: i64) -> OffsetDateTime {
    let utc = OffsetDateTime::from_unix_timestamp(ts).unwrap_or(OffsetDateTime::UNIX_EPOCH);
    utc.to_offset(UtcOffset::local_offset_at(utc).unwrap_or(UtcOffset::UTC))
}

fn utc_day(ts: i64) -> Date {
    OffsetDateTime::from_unix_timestamp(ts).unwrap_or(OffsetDateTime::UNIX_EPOCH).date()
}

fn utc_ts(day: Date) -> i64 {
    day.midnight().assume_utc().unix_timestamp()
}

fn clock(ts: i64) -> String {
    let t = local(ts);
    format!("{:02}:{:02}", t.hour(), t.minute())
}

fn local_text(ts: i64) -> String {
    format!("{}T{}", format_date(local(ts).date()), clock(ts))
}

fn short_date(day: Date) -> String {
    day.format(format_description!("[month repr:short] [day padding:none]")).unwrap_or_default()
}

fn long_date(day: Date) -> String {
    day.format(format_description!("[weekday], [month repr:long] [day padding:none]")).unwrap_or_default()
}

fn color(key: &str) -> (&'static str, &'static str) {
    let (_, dot, chip) = COLORS.iter().find(|c| c.0 == key).unwrap_or(&COLORS[0]);
    (dot, chip)
}

fn event_when(e: &Event, first: Date, last: Date) -> String {
    match (e.all_day == 1, first == last) {
        (true, true) => "All day".into(),
        (true, false) => format!("All day, {} - {}", short_date(first), short_date(last)),
        (false, true) => format!("{} - {}", clock(e.start_at), clock(e.end_at)),
        (false, false) => format!("{} {} - {} {}", short_date(first), clock(e.start_at), short_date(last), clock(e.end_at)),
    }
}

fn event_entry(e: Event) -> Entry {
    let timed = e.all_day == 0;
    let (first, last) = if timed {
        (local(e.start_at).date(), local((e.end_at - 1).max(e.start_at)).date())
    } else {
        (utc_day(e.start_at), utc_day(e.end_at))
    };
    let (dot, chip) = color(&e.color);
    Entry {
        key: format!("event-{}", e.id),
        title: e.title.clone(),
        first,
        last,
        when: event_when(&e, first, last),
        timed,
        sort: e.start_at,
        source: Source::Event,
        label: if e.in_google == 1 { "Google event" } else { "Event" },
        chip,
        dot,
        href: String::new(),
        context: String::new(),
        location: e.location.clone(),
        done: false,
        event: Some(e),
    }
}

fn item_href(i: &Item) -> String {
    match (i.project_id, i.task_id, i.note_id) {
        (Some(project), Some(task), _) => format!("/projects/{project}?task={task}"),
        (_, _, Some(note)) => format!("/notes/{note}"),
        _ => "/reminders".into(),
    }
}

fn item_entry(i: Item) -> Option<Entry> {
    let (first, when, timed, sort) = match i.at {
        Some(at) => (local(at).date(), clock(at), true, at),
        None => {
            let day = parse_date(i.day.as_deref()?)?;
            (day, "Due".to_owned(), false, utc_ts(day))
        }
    };
    let (source, label, chip, dot) = match i.kind.as_str() {
        "task" => (Source::Task, "Task", "bg-secondary text-secondary-foreground", "bg-muted-foreground"),
        "subtask" => (Source::Task, "Subtask", "bg-secondary text-secondary-foreground", "bg-muted-foreground"),
        _ => (Source::Reminder, "Reminder", "bg-primary/10 text-foreground", "bg-primary"),
    };
    Some(Entry {
        key: format!("{}-{}", i.kind, i.id),
        href: item_href(&i),
        title: i.title,
        first,
        last: first,
        when,
        timed,
        sort,
        source,
        label,
        chip,
        dot,
        context: i.context,
        location: String::new(),
        done: i.done == 1,
        event: None,
    })
}

fn build(data: Option<CalendarData>) -> Vec<Entry> {
    let Some(data) = data else { return Vec::new() };
    let events = data.events.into_iter().map(event_entry);
    let mut list: Vec<Entry> = events.chain(data.items.into_iter().filter_map(item_entry)).collect();
    list.sort_by_key(|e| (e.first, e.timed, e.sort));
    list
}

fn grid_days(first: Date) -> Vec<Date> {
    let start = first - Duration::days(first.weekday().number_days_from_monday().into());
    (0..42).map(|n| start + Duration::days(n)).collect()
}

fn month_days(first: Date) -> Vec<Date> {
    (1..=first.month().length(first.year())).filter_map(|d| first.replace_day(d).ok()).collect()
}

fn month_start(day: Date) -> Date {
    day.replace_day(1).unwrap_or(day)
}

#[derive(Clone, Copy)]
struct Filters {
    q: RwSignal<String>,
    events: RwSignal<bool>,
    tasks: RwSignal<bool>,
    reminders: RwSignal<bool>,
    hide_done: RwSignal<bool>,
}

impl Filters {
    fn new() -> Self {
        Self {
            q: RwSignal::new(String::new()),
            events: RwSignal::new(true),
            tasks: RwSignal::new(true),
            reminders: RwSignal::new(true),
            hide_done: RwSignal::new(false),
        }
    }

    fn keep(&self, e: &Entry) -> bool {
        let shown = match e.source {
            Source::Event => self.events.get(),
            Source::Task => self.tasks.get(),
            Source::Reminder => self.reminders.get(),
        };
        shown && !(e.done && self.hide_done.get()) && has(&e.title, &self.q.get())
    }
}

#[derive(Clone, Copy)]
struct EventForm {
    open: RwSignal<bool>,
    id: RwSignal<Option<Id>>,
    title: RwSignal<String>,
    description: RwSignal<String>,
    location: RwSignal<String>,
    all_day: RwSignal<bool>,
    start: RwSignal<String>,
    end: RwSignal<String>,
    color: RwSignal<String>,
    in_google: RwSignal<bool>,
    link: RwSignal<Option<String>>,
}

impl EventForm {
    fn new() -> Self {
        Self {
            open: RwSignal::new(false),
            id: RwSignal::new(None),
            title: RwSignal::new(String::new()),
            description: RwSignal::new(String::new()),
            location: RwSignal::new(String::new()),
            all_day: RwSignal::new(false),
            start: RwSignal::new(String::new()),
            end: RwSignal::new(String::new()),
            color: RwSignal::new(COLORS[0].0.to_owned()),
            in_google: RwSignal::new(false),
            link: RwSignal::new(None),
        }
    }

    fn create(&self, day: Date) {
        let date = format_date(day);
        self.id.set(None);
        self.title.set(String::new());
        self.description.set(String::new());
        self.location.set(String::new());
        self.all_day.set(false);
        self.start.set(format!("{date}T09:00"));
        self.end.set(format!("{date}T10:00"));
        self.color.set(COLORS[0].0.to_owned());
        self.in_google.set(false);
        self.link.set(None);
        self.open.set(true);
    }

    fn edit(&self, e: &Event) {
        let all_day = e.all_day == 1;
        let text = |ts: i64| if all_day { format_date(utc_day(ts)) } else { local_text(ts) };
        self.id.set(Some(e.id));
        self.title.set(e.title.clone());
        self.description.set(markdown_of(&e.description));
        self.location.set(e.location.clone());
        self.all_day.set(all_day);
        self.start.set(text(e.start_at));
        self.end.set(text(e.end_at));
        self.color.set(e.color.clone());
        self.in_google.set(e.in_google == 1);
        self.link.set(e.google_link.clone());
        self.open.set(true);
    }

    fn set_all_day(&self, on: bool) {
        self.all_day.set(on);
        let convert = |value: RwSignal<String>, time: &str| {
            value.update(|v| {
                let date = v.get(..10).unwrap_or_default().to_owned();
                *v = if on || date.is_empty() { date } else { format!("{date}T{time}") };
            });
        };
        convert(self.start, "09:00");
        convert(self.end, "10:00");
    }

    fn stamps(&self) -> Option<(i64, i64)> {
        let (start, end) = (self.start.get_untracked(), self.end.get_untracked());
        if self.all_day.get_untracked() {
            let first = parse_date(&start)?;
            return Some((utc_ts(first), utc_ts(parse_date(&end).unwrap_or(first))));
        }
        let first = parse_local(&start)?;
        Some((first, parse_local(&end).unwrap_or(first)))
    }

    fn payload(&self) -> Option<Value> {
        let (start_at, end_at) = self.stamps()?;
        Some(json!({
            "title": self.title.get_untracked(),
            "description": self.description.get_untracked(),
            "location": self.location.get_untracked(),
            "start_at": start_at,
            "end_at": end_at,
            "all_day": self.all_day.get_untracked(),
            "color": self.color.get_untracked(),
        }))
    }
}

fn markdown_of(description: &str) -> String {
    let html = description.contains('<') && description.contains('>');
    if html { md::to_markdown(&parse_html(description)) } else { description.to_owned() }
}

#[derive(Clone, Copy)]
struct Cal {
    ui: Ui,
    data: LocalResource<Option<CalendarData>>,
    entries: Memo<Vec<Entry>>,
    google: Memo<Option<GoogleStatus>>,
    cursor: RwSignal<Date>,
    selected: RwSignal<Date>,
    syncing: RwSignal<bool>,
    sending: RwSignal<bool>,
    form: EventForm,
}

impl Cal {
    fn on(self, day: Date) -> Vec<Entry> {
        self.entries.with(|list| list.iter().filter(|e| e.first <= day && day <= e.last).cloned().collect())
    }

    fn connected(self) -> bool {
        self.google.with(Option::is_some)
    }

    fn open(self, entry: &Entry, navigate: &impl Fn(&str, NavigateOptions)) {
        match &entry.event {
            Some(e) => self.form.edit(e),
            None => navigate(&entry.href, NavigateOptions::default()),
        }
    }

    fn go_to(self, day: Date) {
        self.cursor.set(month_start(day));
        self.selected.set(day);
    }

    fn sync(self, remote: bool) {
        if self.syncing.get_untracked() {
            return;
        }
        self.syncing.set(true);
        spawn_local(async move {
            if remote && let Some(v) = self.ui.run(api::post::<Value>("/api/calendar/sync", &())).await {
                self.ui.notify(format!("Synced {} events from Google Calendar", v["count"]));
            }
            self.data.refetch();
            self.syncing.set(false);
        });
    }

    async fn send(self, id: Id) {
        if self.ui.run(api::post::<Value>(&format!("/api/events/{id}/google"), &())).await.is_some() {
            self.ui.notify("Sent to Google Calendar");
        }
        self.sending.set(false);
    }

    fn claim_send(self) -> bool {
        let free = !self.sending.get_untracked();
        self.sending.set(true);
        free
    }

    fn push(self, id: Id) {
        if !self.claim_send() {
            return;
        }
        spawn_local(async move {
            self.send(id).await;
            self.data.refetch();
        });
    }

    fn save(self, push: bool) {
        if push && !self.claim_send() {
            return;
        }
        let form = self.form;
        let Some(body) = form.payload() else {
            self.sending.set(false);
            self.ui.notify("Pick a start date");
            return;
        };
        spawn_local(async move {
            let saved = match form.id.get_untracked() {
                Some(id) => self.ui.run(api::put::<Value>(&format!("/api/events/{id}"), &body)).await.map(|_| id),
                None => self.ui.run(api::post::<Value>("/api/events", &body)).await.and_then(|v| v["id"].as_str()?.parse().ok()),
            };
            let Some(id) = saved else {
                self.sending.set(false);
                return;
            };
            form.open.set(false);
            if push {
                self.send(id).await;
            } else {
                self.ui.notify("Event saved");
            }
            self.data.refetch();
        });
    }

    fn remove(self) {
        let form = self.form;
        let Some(id) = form.id.get_untracked() else { return };
        let message = if form.in_google.get_untracked() {
            "This event will be deleted here and from Google Calendar."
        } else {
            "This event will be deleted."
        };
        self.ui.confirm_delete(message, move || {
            form.open.set(false);
            spawn_local(async move {
                if self.ui.run(api::del(&format!("/api/events/{id}"))).await.is_some() {
                    self.data.refetch();
                }
            });
        });
    }

    fn disconnect(self) {
        let message = "Events already synced stay in RustKeep, but nothing will sync with Google Calendar until you connect again.";
        self.ui.confirm(message, "Disconnect", move || {
            spawn_local(async move {
                if self.ui.run(api::del("/api/google")).await.is_some() {
                    self.ui.notify("Google Calendar disconnected");
                    self.data.refetch();
                }
            });
        });
    }
}

fn handle_google_return(cal: Cal) {
    let (outcome, clear) = query_state("google", "");
    Effect::new(move |_| {
        match outcome.get_untracked().as_str() {
            "connected" => {
                cal.ui.notify("Google Calendar connected");
                cal.sync(true);
            }
            "failed" => cal.ui.notify("Could not connect Google Calendar"),
            _ => return,
        }
        clear.run(String::new());
    });
}

#[component]
pub fn CalendarPage() -> impl IntoView {
    let ui = use_ui();
    let data = LocalResource::new(move || async move { ui.run(api::get::<CalendarData>("/api/calendar")).await });
    let filters = Filters::new();
    let all = Memo::new(move |_| build(data.get().flatten()));
    let entries = Memo::new(move |_| all.with(|list| list.iter().filter(|e| filters.keep(e)).cloned().collect::<Vec<_>>()));
    let google = Memo::new(move |_| data.get().flatten().and_then(|d| d.google));
    let configured = Memo::new(move |_| data.get().flatten().is_some_and(|d| d.configured));
    let cal = Cal {
        ui,
        data,
        entries,
        google,
        cursor: RwSignal::new(month_start(today())),
        selected: RwSignal::new(today()),
        syncing: RwSignal::new(false),
        sending: RwSignal::new(false),
        form: EventForm::new(),
    };
    let (view, set_view) = query_state("view", "month");
    handle_google_return(cal);

    view! {
        <div class="flex flex-col gap-6 mx-auto max-w-7xl page-enter">
            <PageHeader title="Calendar" description="Events, task due dates and reminders in one place.">
                <Button
                    variant=ButtonVariant::Outline
                    size=ButtonSize::Icon
                    attr:title="Refresh and sync"
                    attr:aria-label="Refresh and sync"
                    on:click=move |_| cal.sync(cal.connected())
                >
                    <span class="inline-flex" class=("animate-spin", move || cal.syncing.get())>
                        <RefreshCw />
                    </span>
                </Button>
                <GoogleButton cal configured />
                <Button on:click=move |_| cal.form.create(cal.selected.get_untracked())>
                    <Plus />
                    "New event"
                </Button>
            </PageHeader>
            <GoogleStatusLine cal />
            <Toolbar>
                <MonthNav cal />
                <Segmented options=VIEWS value=view on_change=set_view />
            </Toolbar>
            <FilterBar filters />
            {move || match view.get().as_str() {
                "agenda" => view! { <Agenda cal /> }.into_any(),
                _ => view! { <MonthView cal /> }.into_any(),
            }}
            <EventDialog cal />
        </div>
    }
}

#[component]
fn GoogleButton(cal: Cal, configured: Memo<bool>) -> impl IntoView {
    let connect = move |_| {
        let _ = window().location().set_href(&api::url("/api/google/connect"));
    };
    view! {
        <Show
            when=move || cal.connected()
            fallback=move || {
                view! {
                    <Button
                        attr:disabled=move || !configured.get()
                        attr:title=move || if configured.get() { "Connect Google Calendar" } else { "Google Calendar is not configured on this server" }
                        variant=ButtonVariant::Outline
                        on:click=connect
                    >
                        <CalendarCheck />
                        "Connect Google Calendar"
                    </Button>
                }
            }
        >
            <Button variant=ButtonVariant::Outline on:click=move |_| cal.disconnect()>
                <Unlink />
                "Disconnect Google Calendar"
            </Button>
        </Show>
    }
}

#[component]
fn GoogleStatusLine(cal: Cal) -> impl IntoView {
    view! {
        {move || {
            cal.google.get().map(|g| {
                let synced = g.synced.map_or_else(|| "Not synced yet".to_owned(), |at| format!("Last synced {}", fmt_time(at)));
                view! {
                    <div class="flex flex-wrap gap-x-3 gap-y-1 items-center -mt-3 text-sm text-muted-foreground">
                        <span class="flex gap-1.5 items-center">
                            <span class="rounded-full size-2 bg-emerald-500" />
                            {format!("Connected to Google Calendar as {}", g.email)}
                        </span>
                        <span>{synced}</span>
                    </div>
                }
            })
        }}
    }
}

#[component]
fn MonthNav(cal: Cal) -> impl IntoView {
    let shift = move |forward: bool| cal.cursor.update(|c| *c = shift_month(*c, forward));
    view! {
        <div class="flex gap-2 items-center">
            <Button variant=ButtonVariant::Outline size=ButtonSize::Sm on:click=move |_| cal.go_to(today())>"Today"</Button>
            <div class="flex items-center">
                <Button variant=ButtonVariant::Ghost size=ButtonSize::IconSm attr:aria-label="Previous month" on:click=move |_| shift(false)>
                    <ChevronLeft />
                </Button>
                <Button variant=ButtonVariant::Ghost size=ButtonSize::IconSm attr:aria-label="Next month" on:click=move |_| shift(true)>
                    <ChevronRight />
                </Button>
            </div>
            <h2 class="text-lg font-semibold tracking-tight">{move || format!("{} {}", cal.cursor.get().month(), cal.cursor.get().year())}</h2>
        </div>
    }
}

#[component]
fn Toggle(signal: RwSignal<bool>, label: &'static str) -> impl IntoView {
    view! {
        <label class="flex gap-2 items-center text-sm cursor-pointer select-none">
            <Checkbox aria_label=label checked=signal on_checked_change=Callback::new(move |v| signal.set(v)) />
            {label}
        </label>
    }
}

#[component]
fn FilterBar(filters: Filters) -> impl IntoView {
    view! {
        <div class="flex flex-wrap gap-x-5 gap-y-3 items-center -mt-2">
            <SearchBox value=filters.q placeholder="Search calendar  /" />
            <Toggle signal=filters.events label="Events" />
            <Toggle signal=filters.tasks label="Tasks" />
            <Toggle signal=filters.reminders label="Reminders" />
            <Toggle signal=filters.hide_done label="Hide completed" />
        </div>
    }
}

#[component]
fn MonthView(cal: Cal) -> impl IntoView {
    view! {
        <div class="grid grid-cols-1 gap-4 xl:grid-cols-[minmax(0,1fr)_20rem]">
            <Card class="overflow-hidden gap-0 py-0">
                <div class="grid grid-cols-7 border-b bg-muted/40">
                    {WEEKDAYS.map(|d| view! { <div class="py-2 text-xs font-medium text-center text-muted-foreground">{d}</div> }).collect_view()}
                </div>
                <div class="grid grid-cols-7 -mt-px -ml-px">
                    {move || {
                        let first = cal.cursor.get();
                        grid_days(first).into_iter().map(|day| view! { <DayCell day month=first.month() cal /> }).collect_view()
                    }}
                </div>
            </Card>
            <DayPanel cal />
        </div>
    }
}

#[component]
fn DayCell(day: Date, month: Month, cal: Cal) -> impl IntoView {
    let list = Memo::new(move |_| cal.on(day));
    let outside = day.month() != month;
    let number = if day == today() {
        "flex justify-center items-center text-xs font-semibold rounded-full size-6 bg-primary text-primary-foreground"
    } else {
        "flex justify-center items-center text-xs rounded-full size-6"
    };
    view! {
        <div
            role="button"
            tabindex="0"
            title="Double-click to add an event"
            class="flex overflow-hidden flex-col gap-1 p-1 min-w-0 border-t border-l transition-colors cursor-pointer outline-none sm:p-1.5 min-h-16 sm:min-h-28 hover:bg-muted/40 focus-visible:bg-muted/60"
            class=("bg-muted/30", outside)
            class=("text-muted-foreground", outside)
            class=("bg-accent", move || cal.selected.get() == day)
            on:click=move |_| cal.selected.set(day)
            on:dblclick=move |_| cal.form.create(day)
        >
            <span class=number>{day.day()}</span>
            <div class="hidden flex-col gap-0.5 min-w-0 sm:flex">
                {move || list.get().into_iter().take(3).map(|entry| view! { <Chip entry cal /> }).collect_view()}
                {move || {
                    let more = list.with(Vec::len).saturating_sub(3);
                    (more > 0).then(|| view! { <span class="px-1.5 text-xs text-muted-foreground">{format!("+{more} more")}</span> })
                }}
            </div>
            <div class="flex flex-wrap gap-0.5 px-1 sm:hidden">
                {move || list.get().into_iter().take(4).map(|e| view! { <span class=format!("rounded-full size-1.5 {}", e.dot) /> }).collect_view()}
            </div>
        </div>
    }
}

#[component]
fn Chip(entry: Entry, cal: Cal) -> impl IntoView {
    let navigate = use_navigate();
    let done = if entry.done { "line-through opacity-60" } else { "" };
    let class = format!("justify-start gap-1 px-1.5 w-full h-5 text-xs font-normal rounded has-[>svg]:px-1.5 hover:opacity-80 {} {done}", entry.chip);
    let time = entry.timed.then(|| clock_of(&entry));
    let icon = match entry.source {
        Source::Task => Some(view! { <SquareKanban class="size-3 shrink-0" /> }.into_any()),
        Source::Reminder => Some(view! { <Bell class="size-3 shrink-0" /> }.into_any()),
        Source::Event => None,
    };
    let title = entry.title.clone();
    view! {
        <Button
            variant=ButtonVariant::Ghost
            size=ButtonSize::Sm
            class=class
            attr:r#type="button"
            attr:title=format!("{} - {}", entry.title, entry.when)
            on:click=move |ev: leptos::ev::MouseEvent| {
                ev.stop_propagation();
                cal.open(&entry, &navigate);
            }
        >
            {icon}
            {time.map(|t| view! { <span class="hidden font-medium tabular-nums xl:inline shrink-0">{t}</span> })}
            <span class="truncate">{title}</span>
        </Button>
    }
}

fn clock_of(entry: &Entry) -> String {
    entry.event.as_ref().map_or_else(|| entry.when.clone(), |e| clock(e.start_at))
}

#[component]
fn EntryRow(entry: Entry, cal: Cal) -> impl IntoView {
    let navigate = use_navigate();
    let pushable = entry.event.as_ref().filter(|e| e.in_google == 0).map(|e| e.id);
    let in_google = entry.event.as_ref().is_some_and(|e| e.in_google == 1);
    let done = entry.done;
    let detail = format!("{} - {}", entry.when, entry.label);
    let (title, context, location, dot) = (entry.title.clone(), entry.context.clone(), entry.location.clone(), entry.dot);
    view! {
        <div class="flex gap-3 items-start py-2.5 px-3 -mx-3 rounded-lg transition-colors cursor-pointer hover:bg-accent" on:click=move |_| cal.open(&entry, &navigate)>
            <span class=format!("mt-1.5 rounded-full size-2.5 shrink-0 {dot}") />
            <div class="flex flex-col flex-1 gap-0.5 min-w-0">
                <span class="text-sm font-medium truncate" class=("line-through", done) class=("text-muted-foreground", done)>{title}</span>
                <span class="text-xs text-muted-foreground">{detail}</span>
                {(!context.is_empty()).then(|| view! { <span class="text-xs truncate text-muted-foreground">{context}</span> })}
                {(!location.is_empty()).then(|| view! {
                    <span class="flex gap-1 items-center text-xs text-muted-foreground">
                        <MapPin class="size-3 shrink-0" />
                        <span class="truncate">{location}</span>
                    </span>
                })}
            </div>
            {in_google.then(|| view! { <CalendarCheck class="mt-0.5 size-4 text-muted-foreground shrink-0" /> })}
            <Show when=move || pushable.is_some() && cal.connected()>
                <Button
                    variant=ButtonVariant::Ghost
                    size=ButtonSize::IconXs
                    attr:disabled=move || cal.sending.get()
                    attr:title="Send to Google Calendar"
                    attr:aria-label="Send to Google Calendar"
                    on:click=move |ev| {
                        ev.stop_propagation();
                        if let Some(id) = pushable {
                            cal.push(id);
                        }
                    }
                >
                    <Send />
                </Button>
            </Show>
        </div>
    }
}

fn nothing(title: &'static str, description: &'static str) -> AnyView {
    view! {
        <Empty class="border-0">
            <EmptyHeader>
                <EmptyMedia variant=EmptyMediaVariant::Icon><CalendarDays /></EmptyMedia>
                <EmptyTitle>{title}</EmptyTitle>
                <EmptyDescription>{description}</EmptyDescription>
            </EmptyHeader>
        </Empty>
    }
    .into_any()
}

#[component]
fn DayPanel(cal: Cal) -> impl IntoView {
    let list = Memo::new(move |_| cal.on(cal.selected.get()));
    view! {
        <Card class="gap-3 self-start">
            <CardHeader class="flex flex-row justify-between items-start sm:flex">
                <div class="flex flex-col gap-1.5">
                    <CardTitle>{move || long_date(cal.selected.get())}</CardTitle>
                    <CardDescription>
                        {move || match list.with(Vec::len) {
                            0 => "Nothing planned".to_owned(),
                            1 => "1 item".to_owned(),
                            n => format!("{n} items"),
                        }}
                    </CardDescription>
                </div>
                <Button variant=ButtonVariant::Outline size=ButtonSize::Sm on:click=move |_| cal.form.create(cal.selected.get_untracked())>
                    <Plus />
                    "Add"
                </Button>
            </CardHeader>
            <CardContent class="flex flex-col">
                {move || match (cal.data.get().is_some(), list.get()) {
                    (false, _) => view! { <ListSkeleton rows=3 /> }.into_any(),
                    (true, items) if items.is_empty() => nothing("Free day", "Double-click a day or press Add to plan something."),
                    (true, items) => items.into_iter().map(|entry| view! { <EntryRow entry cal /> }).collect_view().into_any(),
                }}
            </CardContent>
        </Card>
    }
}

#[component]
fn Agenda(cal: Cal) -> impl IntoView {
    let groups = move || {
        month_days(cal.cursor.get())
            .into_iter()
            .filter_map(|day| Some(cal.on(day)).filter(|list| !list.is_empty()).map(|list| (day, list)))
            .collect::<Vec<_>>()
    };
    view! {
        <Card class="overflow-hidden gap-0 py-0 divide-y">
            {move || {
                let groups = groups();
                if groups.is_empty() {
                    return nothing("Nothing this month", "Events, due dates and reminders will show up here.");
                }
                groups.into_iter().map(|(day, list)| view! { <AgendaDay day list cal /> }).collect_view().into_any()
            }}
        </Card>
    }
}

#[component]
fn AgendaDay(day: Date, list: Vec<Entry>, cal: Cal) -> impl IntoView {
    let weekday = day.format(format_description!("[weekday repr:short], [month repr:short]")).unwrap_or_default();
    view! {
        <div class="flex flex-col gap-1 p-4 sm:flex-row sm:gap-6">
            <Button
                variant=ButtonVariant::Ghost
                class="justify-start items-baseline gap-2 px-2 -mx-2 h-auto sm:flex-col sm:gap-0 sm:w-20 shrink-0"
                attr:title="Add an event on this day"
                on:click=move |_| cal.form.create(day)
            >
                <span class="text-2xl font-semibold tabular-nums" class=("text-primary", day == today())>{day.day()}</span>
                <span class="text-xs font-normal text-muted-foreground">{weekday}</span>
            </Button>
            <div class="flex flex-col flex-1 min-w-0 sm:px-3">
                {list.into_iter().map(|entry| view! { <EntryRow entry cal /> }).collect_view()}
            </div>
        </div>
    }
}

#[component]
fn ColorPicker(value: RwSignal<String>) -> impl IntoView {
    view! {
        <ToggleGroup>
            {COLORS
                .iter()
                .map(|(key, dot, _)| {
                    view! {
                        <ToggleGroupItem
                            title=*key
                            class="flex-none p-0 rounded-full size-8"
                            pressed=Signal::derive(move || value.get() == *key)
                            on:click=move |_| value.set((*key).to_owned())
                        >
                            <span class=format!("rounded-full size-5 {dot}") />
                        </ToggleGroupItem>
                    }
                })
                .collect_view()}
        </ToggleGroup>
    }
}

#[component]
fn EventDialog(cal: Cal) -> impl IntoView {
    let form = cal.form;
    let upload_project = RwSignal::new(None::<Id>);
    Effect::new(move |_| {
        let start = form.start.get();
        if form.end.get_untracked() < start {
            form.end.set(start);
        }
    });
    let editing = move || form.id.get().is_some();
    let pushable = move || cal.connected() && !form.in_google.get();

    view! {
        <Modal open=form.open title="Event" class="max-w-[min(72rem,calc(100%-2rem))]">
            <div class="grid grid-cols-1 gap-6 lg:grid-cols-[minmax(0,1fr)_17rem]">
                <div class="flex flex-col gap-4 min-w-0">
                    <RichEditor
                        title=form.title
                        body=form.description
                        on_change=Callback::new(|_| ())
                        upload_project
                        allow_upload=true
                        compact=true
                        min_height="min-h-72"
                        placeholder="Add details..."
                    />
                </div>
                <aside class="flex flex-col gap-4">
                    <label class="flex gap-2 items-center text-sm cursor-pointer select-none w-fit">
                        <Checkbox aria_label="All day" checked=form.all_day on_checked_change=Callback::new(move |on| form.set_all_day(on)) />
                        "All day"
                    </label>
                    <div class="flex flex-col gap-2">
                        <Label>"Starts"</Label>
                        {move || if form.all_day.get() {
                            view! { <DatePicker bind_value=form.start /> }.into_any()
                        } else {
                            view! { <DateTimePicker bind_value=form.start /> }.into_any()
                        }}
                    </div>
                    <div class="flex flex-col gap-2">
                        <Label>"Ends"</Label>
                        {move || if form.all_day.get() {
                            view! { <DatePicker bind_value=form.end /> }.into_any()
                        } else {
                            view! { <DateTimePicker bind_value=form.end /> }.into_any()
                        }}
                    </div>
                    <div class="flex flex-col gap-2">
                        <Label>"Location"</Label>
                        <Input bind_value=form.location placeholder="Add a location" />
                    </div>
                    <div class="flex flex-col gap-2">
                        <Label>"Color"</Label>
                        <ColorPicker value=form.color />
                    </div>
                    <Show when=move || form.in_google.get()>
                        <div class="flex flex-col gap-1.5 text-sm text-muted-foreground">
                            <Badge variant=BadgeVariant::Secondary>
                                <CalendarCheck class="mr-1 size-3" />
                                "In Google Calendar"
                            </Badge>
                            <span class="text-xs">"Changes are saved to Google Calendar too."</span>
                            {move || form.link.get().map(|href| view! {
                                <a href=href target="_blank" rel="noopener" class="flex gap-1 items-center text-xs underline underline-offset-4 hover:text-foreground">
                                    <ExternalLink class="size-3" />
                                    "Open in Google"
                                </a>
                            })}
                        </div>
                    </Show>
                    <div class="flex flex-col gap-2 pt-2 lg:mt-auto">
                        <Show when=pushable>
                            <Button
                                class="w-full"
                                variant=ButtonVariant::Outline
                                attr:disabled=move || cal.sending.get()
                                on:click=move |_| cal.save(true)
                            >
                                <Send />
                                "Send to Google Calendar"
                            </Button>
                        </Show>
                        <div class="flex gap-2 items-center">
                            <Show when=editing>
                                <Button
                                    variant=ButtonVariant::Destructive
                                    size=ButtonSize::Icon
                                    attr:title="Delete event"
                                    attr:aria-label="Delete event"
                                    on:click=move |_| cal.remove()
                                >
                                    <Trash2 />
                                </Button>
                            </Show>
                            <Button class="flex-1" on:click=move |_| cal.save(false)>"Save"</Button>
                        </div>
                    </div>
                </aside>
            </div>
        </Modal>
    }
}
