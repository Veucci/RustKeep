use icons::{Bell, Trash2};
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::A;
use serde::Deserialize;

use crate::api;
use crate::components::ui::badge::{Badge, BadgeVariant};
use crate::components::ui::button::{Button, ButtonSize, ButtonVariant};
use crate::components::ui::card::{Card, CardContent};
use crate::widgets::{PageHeader, ReminderForm, fmt_time, use_ui};

#[derive(Clone, Deserialize)]
pub struct Reminder {
    pub id: i64,
    pub title: String,
    pub remind_at: i64,
    pub note_id: Option<String>,
    pub sent: i64,
}

#[component]
pub fn Reminders() -> impl IntoView {
    let ui = use_ui();
    let items = LocalResource::new(move || async move { ui.run(api::get::<Vec<Reminder>>("/api/reminders")).await.unwrap_or_default() });
    let reload = Callback::new(move |_| items.refetch());
    let remove = move |id: i64| {
        spawn_local(async move {
            if ui.run(api::del(&format!("/api/reminders/{id}"))).await.is_some() {
                items.refetch();
            }
        });
    };

    view! {
        <div class="flex flex-col gap-6 mx-auto max-w-4xl page-enter">
            <PageHeader title="Reminders" description="Delivered to your email address at the chosen time." />
            <Card>
                <CardContent>
                    <ReminderForm note_id=None title="" on_done=reload />
                </CardContent>
            </Card>
            <Card class="overflow-hidden gap-0 py-0 divide-y">
                {move || {
                    items
                        .get()
                        .unwrap_or_default()
                        .into_iter()
                        .map(|r| {
                            let id = r.id;
                            view! {
                                <div class="flex gap-3 items-center py-3 px-4 transition-colors hover:bg-muted/50">
                                    <div class="flex justify-center items-center rounded-lg size-9 bg-muted shrink-0">
                                        <Bell class="size-4 text-muted-foreground" />
                                    </div>
                                    <div class="flex flex-col flex-1 min-w-0">
                                        <span class="font-medium">{r.title}</span>
                                        <span class="text-xs text-muted-foreground">{fmt_time(r.remind_at)}</span>
                                    </div>
                                    {r.note_id.map(|n| view! { <A href=api::url(&format!("/notes/{n}")) attr:class="text-sm underline underline-offset-4">"Note"</A> })}
                                    {if r.sent == 1 {
                                        view! { <Badge variant=BadgeVariant::Secondary>"Sent"</Badge> }.into_any()
                                    } else {
                                        view! { <Badge variant=BadgeVariant::Outline>"Pending"</Badge> }.into_any()
                                    }}
                                    <Button variant=ButtonVariant::Ghost size=ButtonSize::IconSm on:click=move |_| remove(id)>
                                        <Trash2 />
                                    </Button>
                                </div>
                            }
                        })
                        .collect_view()
                }}
            </Card>
        </div>
    }
}
