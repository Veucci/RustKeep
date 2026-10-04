use leptos::prelude::*;

use crate::pages::files::FileManager;
use crate::pages::notes::{NotesList, View};
use crate::pages::projects::ProjectList;
use crate::pages::reminders::ReminderList;
use crate::widgets::{Options, PageHeader, Segmented, query_state};

const TABS: Options = &[("notes", "Notes"), ("projects", "Projects"), ("files", "Files"), ("reminders", "Reminders")];

#[component]
pub fn ArchivePage() -> impl IntoView {
    let (tab, set_tab) = query_state("tab", "notes");
    view! {
        <div class="flex flex-col gap-6 mx-auto max-w-6xl page-enter">
            <PageHeader title="Archive" description="Everything you put away. Restore any item to bring it back." />
            <Segmented options=TABS value=tab on_change=set_tab />
            {move || match tab.get().as_str() {
                "projects" => view! { <ProjectList archive_only=true /> }.into_any(),
                "files" => view! { <FileManager archive_only=true /> }.into_any(),
                "reminders" => view! { <ReminderList archive_only=true /> }.into_any(),
                _ => view! { <NotesList view=View::Archived embedded=true /> }.into_any(),
            }}
        </div>
    }
}
