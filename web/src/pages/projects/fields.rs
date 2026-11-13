use std::collections::{BTreeSet, HashSet};

use icons::{Plus, Trash2, X};
use leptos::ev::SubmitEvent;
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{BoardCtx, Filters, SetOption};
use crate::api;
use crate::components::ui::badge::{Badge, BadgeVariant};
use crate::components::ui::button::{Button, ButtonSize, ButtonVariant};
use crate::components::ui::input::Input;
use crate::components::ui::label::Label;
use crate::components::ui::multi_select::{MultiSelect, MultiSelectAlign, MultiSelectContent, MultiSelectTrigger};
use crate::id::Id;
use crate::widgets::{Modal, Ui, has, use_ui};

#[derive(Clone, PartialEq, Eq, Hash, Deserialize)]
pub struct Field {
    pub id: Id,
    pub name: String,
}

#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FieldValue {
    pub field_id: Id,
    pub value: String,
}

#[component]
pub fn FieldChips(fields: Vec<Field>, values: Vec<FieldValue>) -> impl IntoView {
    let chip = move |f: Field| {
        let picked: Vec<&str> = values.iter().filter(|v| v.field_id == f.id).map(|v| v.value.as_str()).collect();
        (!picked.is_empty()).then(|| format!("{}: {}", f.name, picked.join(", ")))
    };
    fields
        .into_iter()
        .filter_map(chip)
        .map(|text| view! { <span class="py-0.5 px-1.5 max-w-full rounded-md bg-muted truncate" title=text.clone()>{text.clone()}</span> })
        .collect_view()
}

fn refresh_fields(ui: Ui, fields: LocalResource<Vec<Field>>, fut: impl Future<Output = api::ApiResult<Value>> + 'static) {
    spawn_local(async move {
        ui.run(fut).await;
        fields.refetch();
    });
}

#[component]
pub(super) fn CustomFields(project: Id, open: RwSignal<bool>, fields: LocalResource<Vec<Field>>) -> impl IntoView {
    let ui = use_ui();
    let name = RwSignal::new(String::new());
    let add = move |ev: SubmitEvent| {
        ev.prevent_default();
        let body = json!({ "name": name.get_untracked() });
        name.set(String::new());
        refresh_fields(ui, fields, async move { api::post::<Value>(&format!("/api/projects/{project}/fields"), &body).await });
    };
    view! {
        <Modal open title="Custom fields">
            <p class="text-sm text-muted-foreground">"Free text fields for every task on this board. Fill them in from a task, then filter the board by them."</p>
            <div class="flex flex-col gap-2">
                <For each=move || fields.get().unwrap_or_default() key=|f| f.id let(field)>
                    <FieldRow field fields />
                </For>
            </div>
            <form class="flex gap-2" on:submit=add>
                <Input placeholder="Field name, e.g. Type" bind_value=name required=true />
                <Button>
                    <Plus />
                    "Add"
                </Button>
            </form>
        </Modal>
    }
}

#[component]
fn FieldRow(field: Field, fields: LocalResource<Vec<Field>>) -> impl IntoView {
    let ui = use_ui();
    let id = field.id;
    let name = RwSignal::new(field.name);
    let rename = move |_| {
        let body = json!({ "name": name.get_untracked() });
        refresh_fields(ui, fields, async move { api::put::<Value>(&format!("/api/fields/{id}"), &body).await });
    };
    let remove = move |_| {
        ui.confirm_delete("This field and its values on every task will be deleted.", move || {
            refresh_fields(ui, fields, async move { api::del(&format!("/api/fields/{id}")).await });
        });
    };
    view! {
        <div class="flex gap-2 items-center">
            <Input bind_value=name on:change=rename />
            <Button variant=ButtonVariant::Ghost size=ButtonSize::Icon attr:title="Delete field" on:click=remove>
                <Trash2 />
            </Button>
        </div>
    }
}

#[component]
pub(super) fn FieldFilter(field: Field, ctx: BoardCtx, filters: Filters) -> impl IntoView {
    let id = field.id;
    let name = StoredValue::new(field.name);
    let selected = RwSignal::new(HashSet::<String>::new());
    Effect::new(move |_| {
        let picked = selected.get();
        filters.fields.update(|m| {
            m.insert(id, picked);
        });
    });
    on_cleanup(move || {
        filters.fields.try_update(|m| m.remove(&id));
    });
    let count = move || selected.with(HashSet::len);
    let options = move || {
        let mut all: BTreeSet<String> = ctx.field_values(id).into_iter().collect();
        all.extend(selected.get());
        all.into_iter().collect::<Vec<_>>()
    };
    let label = move || match count() {
        0 => name.get_value(),
        n => format!("{}: {n}", name.get_value()),
    };
    view! {
        <MultiSelect values=selected align=MultiSelectAlign::End>
            <MultiSelectTrigger class="w-40">
                <span class="text-sm truncate" class=("text-muted-foreground", move || count() == 0)>{label}</span>
            </MultiSelectTrigger>
            <MultiSelectContent class="overflow-visible p-0 w-60 max-h-none">
                <div class="flex justify-between items-center p-2 border-b">
                    <span class="text-sm font-medium truncate">{name.get_value()}</span>
                    <Button
                        variant=ButtonVariant::Ghost
                        size=ButtonSize::Sm
                        attr:disabled=move || count() == 0
                        on:click=move |_| selected.update(HashSet::clear)
                    >
                        "Clear"
                    </Button>
                </div>
                <div class="flex overflow-y-auto flex-col p-1 max-h-72">
                    <For each=options key=String::clone let(value)>
                        <SetOption id=value.clone() name=value selected />
                    </For>
                    <Show when=move || options().is_empty()>
                        <p class="py-4 text-sm text-center text-muted-foreground">"No values yet"</p>
                    </Show>
                </div>
            </MultiSelectContent>
        </MultiSelect>
    }
}

#[component]
pub(super) fn FieldInput(field: Field, ctx: BoardCtx) -> impl IntoView {
    let form = ctx.form;
    let id = field.id;
    let draft = RwSignal::new(String::new());
    let mine = move || form.fields.with(|list| list.iter().filter(|v| v.field_id == id).map(|v| v.value.clone()).collect::<Vec<_>>());
    let add = move |value: String| {
        let value = value.trim().to_owned();
        draft.set(String::new());
        if !value.is_empty() && !mine().contains(&value) {
            form.fields.update(|list| list.push(FieldValue { field_id: id, value }));
        }
    };
    let remove = move |value: String| form.fields.update(|list| list.retain(|v| v.field_id != id || v.value != value));
    let suggestions = move || {
        let (taken, typed) = (mine(), draft.get());
        ctx.field_values(id).into_iter().filter(|v| !taken.contains(v) && has(v, &typed)).take(8).collect::<Vec<_>>()
    };
    view! {
        <div class="flex flex-col gap-2">
            <Label>{field.name}</Label>
            <div class="flex flex-wrap gap-1 empty:hidden">
                <For each=mine key=String::clone let(value)>
                    <Badge variant=BadgeVariant::Secondary class="gap-1 pr-0.5 max-w-full">
                        <span class="truncate">{value.clone()}</span>
                        <Button variant=ButtonVariant::Ghost size=ButtonSize::IconXs class="size-4" attr:title="Remove" on:click=move |_| remove(value.clone())>
                            <X class="size-3" />
                        </Button>
                    </Badge>
                </For>
            </div>
            <form on:submit=move |ev: SubmitEvent| {
                ev.prevent_default();
                add(draft.get_untracked());
            }>
                <Input placeholder="Type a value and press Enter" bind_value=draft />
            </form>
            <div class="flex flex-wrap gap-1 empty:hidden">
                <For each=suggestions key=String::clone let(value)>
                    {
                        let label = value.clone();
                        view! {
                            <Button variant=ButtonVariant::Outline size=ButtonSize::Badge class="max-w-full" on:click=move |_| add(value.clone())>
                                <span class="truncate">{label}</span>
                            </Button>
                        }
                    }
                </For>
            </div>
        </div>
    }
}
