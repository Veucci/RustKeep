use leptos::prelude::*;
use web_sys::Storage;

#[derive(Debug, Clone, Copy)]
pub struct ThemeMode {
    state: RwSignal<bool>,
}

const LOCALSTORAGE_KEY: &str = "darkmode";

pub fn use_theme_mode() -> ThemeMode {
    expect_context::<ThemeMode>()
}


impl ThemeMode {
    #[must_use]
    pub fn init() -> Self {
        let theme_mode = Self { state: RwSignal::new(false) };

        provide_context(theme_mode);

        Effect::new(move |_| {
            let initial = Self::get_storage_state().unwrap_or(Self::prefers_dark_mode());
            theme_mode.state.set(initial);
        });

        theme_mode
    }

    pub fn toggle(&self) {
        self.state.update(|state| {
            *state = !*state;
            Self::set_storage_state(*state);
        });
    }

    pub fn set_dark(&self) {
        self.set(true);
    }

    pub fn set_light(&self) {
        self.set(false);
    }

    pub fn set(&self, dark: bool) {
        self.state.set(dark);
        Self::set_storage_state(dark);
    }

    #[must_use]
    pub fn get(&self) -> bool {
        self.state.get()
    }

    #[must_use]
    pub fn is_dark(&self) -> bool {
        self.state.get()
    }

    #[must_use]
    pub fn is_light(&self) -> bool {
        !self.state.get()
    }


    fn get_storage() -> Option<Storage> {
        window().local_storage().ok().flatten()
    }

    fn get_storage_state() -> Option<bool> {
        Self::get_storage()
            .and_then(|storage| storage.get(LOCALSTORAGE_KEY).ok())
            .flatten()
            .and_then(|entry| entry.parse::<bool>().ok())
    }

    fn prefers_dark_mode() -> bool {
        window()
            .match_media("(prefers-color-scheme: dark)")
            .ok()
            .flatten()
            .map(|media| media.matches())
            .unwrap_or_default()
    }

    fn set_storage_state(state: bool) {
        if let Some(storage) = Self::get_storage() {
            storage.set(LOCALSTORAGE_KEY, state.to_string().as_str()).ok();
        }
    }
}
