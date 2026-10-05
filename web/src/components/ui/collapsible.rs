use leptos::context::Provider;
use leptos::prelude::*;
use tw_merge::*;

#[derive(Clone, Copy)]
struct CollapsibleContext {
    open: RwSignal<bool>,
}

#[component]
pub fn Collapsible(

    #[prop(optional)]
    open: Option<RwSignal<bool>>,

    #[prop(default = false)]
    default_open: bool,
    children: Children,
    #[prop(optional, into)] class: String,
) -> impl IntoView {
    let open_signal = open.unwrap_or_else(|| RwSignal::new(default_open));
    let ctx = CollapsibleContext { open: open_signal };

    let class = tw_merge!("", class);

    view! {
        <Provider value=ctx>
            <div
                data-name="Collapsible"
                data-state=move || if open_signal.get() { "open" } else { "closed" }
                class=class
            >
                {children()}
            </div>
        </Provider>
    }
}

#[component]
pub fn CollapsibleTrigger(children: Children, #[prop(optional, into)] class: String) -> impl IntoView {
    let ctx = expect_context::<CollapsibleContext>();

    view! {
        <button
            type="button"
            data-name="CollapsibleTrigger"
            data-state=move || if ctx.open.get() { "open" } else { "closed" }
            class=class
            on:click=move |_| ctx.open.update(|v| *v = !*v)
        >
            {children()}
        </button>
    }
}

#[component]
pub fn CollapsibleContent(
    children: Children,
    #[prop(optional, into)] class: String,
    #[prop(optional, into)] outer_class: String,
) -> impl IntoView {
    let ctx = expect_context::<CollapsibleContext>();
    let outer = tw_merge!(
        "grid overflow-hidden transition-all duration-300 data-[state=closed]:grid-rows-[0fr] data-[state=open]:grid-rows-[1fr]",
        outer_class
    );

    view! {
        <div
            data-name="CollapsibleContent"
            data-state=move || if ctx.open.get() { "open" } else { "closed" }
            class=outer
        >
            <div class=tw_merge!("min-h-0", class)>{children()}</div>
        </div>
    }
}
