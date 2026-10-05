use icons::{Calendar as CalendarIcon, ChevronLeft, ChevronRight, Clock};
use leptos::prelude::*;
use leptos_ui::clx;
use time::format_description::BorrowedFormatItem;
use time::macros::format_description;
use time::{Date, Month, OffsetDateTime};
use tw_merge::*;

use crate::components::ui::button::{Button, ButtonSize, ButtonVariant};
use crate::components::ui::popover::{Popover, PopoverAlign, PopoverContent, PopoverTrigger};
use crate::components::ui::select::{Select, SelectContent, SelectGroup, SelectOption, SelectTrigger, SelectValue};

mod components {
    use super::*;
    clx! {DatePickerRoot, div, "flex flex-col gap-4 p-3 rounded-lg border bg-card text-card-foreground shadow-sm w-fit"}
    clx! {DatePickerNavButton, button, "inline-flex items-center justify-center p-0 text-sm font-medium transition-colors bg-transparent border rounded-md opacity-50 whitespace-nowrap ring-offset-background focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:pointer-events-none disabled:opacity-50 disabled:cursor-not-allowed border-input hover:bg-accent hover:text-accent-foreground size-7 hover:opacity-100  [&_svg:not([class*='size-'])]:size-4"}
    clx! {DatePickerTitle, span, "text-sm font-medium text-center"}
    clx! {DatePickerHeader, header, "grid grid-cols-[auto_1fr_auto] items-center pt-1"}
    clx! {DatePickerWeekDay, th, "text-muted-foreground rounded-md w-9 font-normal text-[0.8rem]"}
    clx! {DatePickerWeekNumberHeader, th, "text-muted-foreground rounded-md w-6 font-normal text-[0.8rem] select-none"}
    clx! {DatePickerWeekNumberCell, td, "w-6 text-center text-[0.8rem] text-muted-foreground select-none"}
    clx! {DatePickerRow, tr, "flex w-full mt-2"}
    clx! {DatePickerMonth, div, "flex flex-col items-center justify-start gap-2 size-full"}
    clx! {DatePickerTable, table, "w-full space-y-1 border-collapse"}
}

pub use components::*;

#[component]
pub fn DatePickerCell(
    day: u8,
    year: i32,
    month: time::Month,
    disabled: bool,
    start_date: RwSignal<Date>,
    end_date: RwSignal<Date>,
    on_click: impl Fn(u8) + 'static,
    #[prop(into, optional)] class: String,
) -> impl IntoView {
    let current_date = if day > 0 && !disabled { Date::from_calendar_date(year, month, day).ok() } else { None };

    let is_current = move || {
        if let Some(date) = current_date { date == start_date.get() || date == end_date.get() } else { false }
    };

    let is_selected = move || {
        if let Some(date) = current_date { date > start_date.get() && date < end_date.get() } else { false }
    };

    let merged_class = tw_merge!(
        "inline-flex items-center justify-center text-sm size-9 rounded-md select-none",
        "hover:cursor-pointer hover:bg-accent",
        "aria-disabled:pointer-events-none aria-disabled:opacity-50 aria-disabled:cursor-not-allowed",
        "aria-current:bg-primary aria-current:hover:bg-primary aria-current:text-primary-foreground",
        class
    );

    let cell_class = move || {
        let base = merged_class.clone();
        if is_selected() { format!("{} bg-accent rounded-none", base) } else { base }
    };

    let handle_click = move |_| {
        if !disabled {
            on_click(day);
        }
    };

    view! {
        <td
            data-name="DatePickerCell"
            class=cell_class
            aria-current=move || is_current().to_string()
            aria-disabled=move || disabled.to_string()
            on:click=handle_click
        >
            {day}
        </td>
    }
}

const ISO_DATE: &[BorrowedFormatItem<'_>] = format_description!("[year]-[month]-[day]");
const DISPLAY_DATE: &[BorrowedFormatItem<'_>] = format_description!("[month repr:short] [day padding:none], [year]");
const WEEKDAYS: [&str; 7] = ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"];

pub fn today() -> Date {
    OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc()).date()
}

pub fn parse_date(value: &str) -> Option<Date> {
    Date::parse(value.get(..10)?, ISO_DATE).ok()
}

pub fn format_date(date: Date) -> String {
    date.format(ISO_DATE).unwrap_or_default()
}

fn display_date(date: Date) -> String {
    date.format(DISPLAY_DATE).unwrap_or_default()
}

fn shift_month(date: Date, forward: bool) -> Date {
    let (year, month) = match (forward, date.month()) {
        (true, Month::December) => (date.year() + 1, Month::January),
        (false, Month::January) => (date.year() - 1, Month::December),
        (true, month) => (date.year(), month.next()),
        (false, month) => (date.year(), month.previous()),
    };
    Date::from_calendar_date(year, month, 1).unwrap_or(date)
}

fn month_weeks(first: Date) -> Vec<Vec<Option<Date>>> {
    let leading = usize::from(first.weekday().number_days_from_monday());
    let days = (1..=first.month().length(first.year())).map(|day| first.replace_day(day).ok());
    let mut cells: Vec<Option<Date>> = std::iter::repeat_n(None, leading).chain(days).collect();
    cells.resize(cells.len().div_ceil(7) * 7, None);
    cells.chunks(7).map(<[_]>::to_vec).collect()
}

#[component]
fn CalendarDay(day: Option<Date>, selected: Signal<Option<Date>>, on_select: Callback<Date>, close_popover: bool) -> impl IntoView {
    let Some(date) = day else { return view! { <td class="size-9" /> }.into_any() };
    view! {
        <td>
            <button
                type="button"
                data-name="CalendarDay"
                data-today=(date == today()).to_string()
                data-popover-close=close_popover.then_some("")
                aria-pressed=move || (selected.get() == Some(date)).to_string()
                class="inline-flex items-center justify-center text-sm rounded-md select-none size-9 hover:bg-accent hover:text-accent-foreground data-[today=true]:font-semibold data-[today=true]:aria-[pressed=false]:bg-accent data-[today=true]:aria-[pressed=false]:text-accent-foreground aria-pressed:bg-primary aria-pressed:text-primary-foreground aria-pressed:hover:bg-primary"
                on:click=move |_| on_select.run(date)
            >
                {date.day()}
            </button>
        </td>
    }
    .into_any()
}

#[component]
pub fn Calendar(
    #[prop(into)] selected: Signal<Option<Date>>,
    on_select: Callback<Date>,
    #[prop(optional)] close_popover: bool,
    #[prop(into, optional)] class: String,
) -> impl IntoView {
    let shown = RwSignal::new(selected.get_untracked().unwrap_or_else(today).replace_day(1).unwrap_or_else(|_| today()));
    let shift = move |forward: bool| shown.update(|date| *date = shift_month(*date, forward));

    view! {
        <DatePickerRoot class=class>
            <DatePickerHeader>
                <DatePickerNavButton class="justify-self-start" attr:r#type="button" attr:aria-label="Go to previous month" on:click=move |_| shift(false)>
                    <ChevronLeft />
                </DatePickerNavButton>
                <DatePickerTitle>{move || format!("{} {}", shown.get().month(), shown.get().year())}</DatePickerTitle>
                <DatePickerNavButton class="justify-self-end" attr:r#type="button" attr:aria-label="Go to next month" on:click=move |_| shift(true)>
                    <ChevronRight />
                </DatePickerNavButton>
            </DatePickerHeader>
            <DatePickerTable attr:role="grid">
                <thead>
                    <tr class="flex">
                        {WEEKDAYS.map(|day| view! { <DatePickerWeekDay>{day}</DatePickerWeekDay> }).collect_view()}
                    </tr>
                </thead>
                <tbody>
                    {move || {
                        month_weeks(shown.get())
                            .into_iter()
                            .map(|week| {
                                view! {
                                    <DatePickerRow>
                                        {week
                                            .into_iter()
                                            .map(|day| view! { <CalendarDay day selected on_select close_popover /> })
                                            .collect_view()}
                                    </DatePickerRow>
                                }
                            })
                            .collect_view()
                    }}
                </tbody>
            </DatePickerTable>
        </DatePickerRoot>
    }
}

#[component]
fn PickerTrigger(label: Signal<Option<String>>, placeholder: String, class: String, children: Children) -> impl IntoView {
    let class = tw_merge!("justify-start w-full font-normal", class);
    view! {
        <PopoverTrigger class=class>
            {children()}
            <span class=move || if label.get().is_some() { "truncate" } else { "truncate text-muted-foreground" }>
                {move || label.get().unwrap_or_else(|| placeholder.clone())}
            </span>
        </PopoverTrigger>
    }
}

#[component]
fn ClearButton(bind_value: RwSignal<String>) -> impl IntoView {
    view! {
        <Show when=move || bind_value.with(|value| !value.is_empty())>
            <Button
                variant=ButtonVariant::Ghost
                size=ButtonSize::Sm
                class="w-full"
                attr:data-popover-close=""
                on:click=move |_| bind_value.set(String::new())
            >
                "Clear"
            </Button>
        </Show>
    }
}

#[component]
pub fn DatePicker(
    bind_value: RwSignal<String>,
    #[prop(into, default = "Pick a date".into())] placeholder: String,
    #[prop(into, optional)] class: String,
) -> impl IntoView {
    let selected = Signal::derive(move || bind_value.with(|value| parse_date(value)));
    let label = Signal::derive(move || selected.get().map(display_date));
    let on_select = Callback::new(move |date: Date| bind_value.set(format_date(date)));

    view! {
        <Popover align=PopoverAlign::Start>
            <PickerTrigger label placeholder class>
                <CalendarIcon />
            </PickerTrigger>
            <PopoverContent class="p-3 w-auto min-h-0">
                <div class="flex flex-col gap-2">
                    <Calendar selected on_select close_popover=true class="p-0 border-0 shadow-none" />
                    <ClearButton bind_value />
                </div>
            </PopoverContent>
        </Popover>
    }
}

#[component]
fn TimeSelect(
    value: Signal<Option<String>>,
    on_change: Callback<Option<String>>,
    count: u8,
    label: &'static str,
) -> impl IntoView {
    view! {
        <Select value on_change class="flex-1">
            <SelectTrigger attr:aria-label=label>
                <SelectValue placeholder="00" />
            </SelectTrigger>
            <SelectContent class="w-full max-h-[200px]">
                <SelectGroup aria_label=label>
                    {(0..count)
                        .map(|n| {
                            let option = format!("{n:02}");
                            view! { <SelectOption value=option.clone()>{option}</SelectOption> }
                        })
                        .collect_view()}
                </SelectGroup>
            </SelectContent>
        </Select>
    }
}

#[component]
pub fn DateTimePicker(
    bind_value: RwSignal<String>,
    #[prop(into, default = "Pick a date and time".into())] placeholder: String,
    #[prop(into, optional)] class: String,
) -> impl IntoView {
    let selected = Signal::derive(move || bind_value.with(|value| parse_date(value)));
    let hour = Signal::derive(move || bind_value.with(|value| value.get(11..13).map(str::to_owned)));
    let minute = Signal::derive(move || bind_value.with(|value| value.get(14..16).map(str::to_owned)));
    let label = Signal::derive(move || {
        selected.get().map(|date| {
            format!("{} {}:{}", display_date(date), hour.get().unwrap_or_default(), minute.get().unwrap_or_default())
        })
    });

    let write = move |date: Option<Date>, hour: Option<String>, minute: Option<String>| {
        let date = format_date(date.unwrap_or_else(today));
        let hour = hour.unwrap_or_else(|| "00".into());
        let minute = minute.unwrap_or_else(|| "00".into());
        bind_value.set(format!("{date}T{hour}:{minute}"));
    };
    let on_select = Callback::new(move |date: Date| write(Some(date), hour.get_untracked(), minute.get_untracked()));
    let on_hour = Callback::new(move |value| write(selected.get_untracked(), value, minute.get_untracked()));
    let on_minute = Callback::new(move |value| write(selected.get_untracked(), hour.get_untracked(), value));

    view! {
        <Popover align=PopoverAlign::Start>
            <PickerTrigger label placeholder class>
                <CalendarIcon />
            </PickerTrigger>
            <PopoverContent class="p-3 w-auto min-h-0">
                <div class="flex flex-col gap-2">
                    <Calendar selected on_select class="p-0 border-0 shadow-none" />
                    <div class="flex gap-2 items-center pt-3 border-t">
                        <Clock class="size-4 text-muted-foreground" />
                        <TimeSelect value=hour on_change=on_hour count=24 label="Hour" />
                        <span class="text-muted-foreground">":"</span>
                        <TimeSelect value=minute on_change=on_minute count=60 label="Minute" />
                    </div>
                    <ClearButton bind_value />
                </div>
            </PopoverContent>
        </Popover>
    }
}
