use leptos::prelude::*;

pub fn use_can_scroll_vertical() -> (impl Fn(web_sys::Event) + Clone, RwSignal<bool>, RwSignal<bool>) {
    let can_scroll_up_signal = RwSignal::new(false);
    let can_scroll_down_signal = RwSignal::new(false);

    let on_scroll = move |ev: web_sys::Event| {
        let target = event_target::<web_sys::HtmlElement>(&ev);
        let scroll_top = target.scroll_top();
        let scroll_height = target.scroll_height();
        let client_height = target.client_height();

        can_scroll_up_signal.set(scroll_top > 0);
        can_scroll_down_signal.set(scroll_top < scroll_height - client_height - 1);
    };

    (on_scroll, can_scroll_up_signal, can_scroll_down_signal)
}
