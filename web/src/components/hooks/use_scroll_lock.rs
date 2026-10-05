use std::cell::RefCell;

use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::prelude::*;

const EXCLUDED_DATA_NAMES: &[&str] =
    &["ScrollArea", "CommandList", "SelectContent", "MultiSelectContent", "DropdownMenuContent", "ContextMenuContent"];

const FIXED_EXCLUDED: &[&str] = &["DropdownMenuContent", "MultiSelectContent", "ContextMenuContent"];

const SCROLLABLE_SELECTOR: &str = r#"[style*="overflow"],[class*="overflow"],[class*="scroll"],main,aside,section,div"#;

const FIXED_SELECTOR: &str =
    r#"[style*="fixed"],[class*="fixed"],header,nav,aside,[role="dialog"],[role="alertdialog"]"#;

struct BodyStyles {
    position: String,
    top: String,
    width: String,
    overflow: String,
    padding_right: String,
}

struct ScrollableEntry {
    element: web_sys::HtmlElement,
    scroll_top: i32,
    overflow: String,
    overflow_y: String,
    padding_right: String,
}

struct FixedEntry {
    element: web_sys::HtmlElement,
    padding_right: String,
}

struct State {
    depth: u32,
    locked: bool,
    window_scroll_y: f64,
    body_styles: Option<BodyStyles>,
    scrollable: Vec<ScrollableEntry>,
    fixed: Vec<FixedEntry>,
}

impl State {
    const fn new() -> Self {
        Self { depth: 0, locked: false, window_scroll_y: 0.0, body_styles: None, scrollable: Vec::new(), fixed: Vec::new() }
    }

    fn clear(&mut self) {
        self.locked = false;
        self.window_scroll_y = 0.0;
        self.body_styles = None;
        self.scrollable.clear();
        self.fixed.clear();
    }
}

thread_local! {
    static STATE: RefCell<State> = const { RefCell::new(State::new()) };
}

fn is_excluded(el: &web_sys::Element) -> bool {
    if let Some(name) = el.get_attribute("data-name")
        && EXCLUDED_DATA_NAMES.iter().any(|&n| n == name)
    {
        return true;
    }
    for &name in EXCLUDED_DATA_NAMES {
        let sel = format!(r#"[data-name="{name}"]"#);
        if el.closest(&sel).ok().flatten().is_some() {
            return true;
        }
    }
    false
}

fn is_fixed_excluded(el: &web_sys::Element) -> bool {
    for &name in FIXED_EXCLUDED {
        let sel = format!(r#"[data-name="{name}"]"#);
        if el.closest(&sel).ok().flatten().is_some() {
            return true;
        }
    }
    false
}

fn set_style(style: &web_sys::CssStyleDeclaration, prop: &str, val: &str) {
    if val.is_empty() {
        let _ = style.remove_property(prop);
    } else {
        let _ = style.set_property(prop, val);
    }
}

fn parse_px(s: &str) -> f64 {
    s.trim_end_matches("px").parse::<f64>().unwrap_or(0.0)
}

pub fn init() {
    let Some(window) = web_sys::window() else { return };
    let window_js: &JsValue = window.as_ref();

    if js_sys::Reflect::get(window_js, &"ScrollLock".into())
        .ok()
        .filter(|v| !v.is_undefined() && !v.is_null())
        .is_some()
    {
        return;
    }

    let obj = js_sys::Object::new();
    let obj_js: &JsValue = obj.as_ref();

    let f = Closure::wrap(Box::new(lock) as Box<dyn Fn()>);
    let _ = js_sys::Reflect::set(obj_js, &"lock".into(), f.as_ref());
    f.forget();

    let f = Closure::wrap(Box::new(|delay: JsValue| {
        unlock(delay.as_f64().unwrap_or(0.0) as u32);
    }) as Box<dyn Fn(JsValue)>);
    let _ = js_sys::Reflect::set(obj_js, &"unlock".into(), f.as_ref());
    f.forget();

    let f = Closure::wrap(Box::new(is_locked) as Box<dyn Fn() -> bool>);
    let _ = js_sys::Reflect::set(obj_js, &"isLocked".into(), f.as_ref());
    f.forget();

    let _ = js_sys::Reflect::set(window_js, &"ScrollLock".into(), obj_js);
}

pub fn lock() {
    let proceed = STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.depth += 1;
        if s.locked {
            return false;
        }
        s.locked = true;
        true
    });
    if !proceed {
        return;
    }

    let Some(window) = web_sys::window() else { return };
    let Some(document) = window.document() else { return };
    let Some(body) = document.body() else { return };

    let window_scroll_y = window.scroll_y().unwrap_or(0.0);
    let inner_width = window.inner_width().ok().and_then(|w| w.as_f64()).unwrap_or(0.0);
    let scrollbar_width = inner_width - body.client_width() as f64;

    let body_style = body.style();
    let original_body = BodyStyles {
        position: body_style.get_property_value("position").unwrap_or_default(),
        top: body_style.get_property_value("top").unwrap_or_default(),
        width: body_style.get_property_value("width").unwrap_or_default(),
        overflow: body_style.get_property_value("overflow").unwrap_or_default(),
        padding_right: body_style.get_property_value("padding-right").unwrap_or_default(),
    };

    let body_js: &JsValue = body.as_ref();
    let doc_element = document.document_element();

    struct SRead {
        el: web_sys::HtmlElement,
        scroll_top: i32,
        overflow: String,
        overflow_y: String,
        padding_right: String,
        computed_padding: f64,
        el_scrollbar: i32,
    }

    let mut s_reads: Vec<SRead> = Vec::new();

    if let Ok(nodes) = document.query_selector_all(SCROLLABLE_SELECTOR) {
        for i in 0..nodes.length() {
            let Some(node) = nodes.item(i) else { continue };
            let Ok(element) = node.dyn_into::<web_sys::Element>() else {
                continue;
            };

            let el_js: &JsValue = element.as_ref();
            if el_js == body_js {
                continue;
            }
            if let Some(ref de) = doc_element {
                let de_js: &JsValue = de.as_ref();
                if el_js == de_js {
                    continue;
                }
            }

            if is_excluded(&element) {
                continue;
            }

            let Some(cs) = window.get_computed_style(&element).ok().flatten() else {
                continue;
            };
            let ov = cs.get_property_value("overflow").unwrap_or_default();
            let ovy = cs.get_property_value("overflow-y").unwrap_or_default();
            let scrollable = matches!(ov.as_str(), "auto" | "scroll") || matches!(ovy.as_str(), "auto" | "scroll");

            if !scrollable || element.scroll_height() <= element.client_height() {
                continue;
            }

            let Ok(el) = element.dyn_into::<web_sys::HtmlElement>() else {
                continue;
            };

            let st = el.style();
            let cp = cs.get_property_value("padding-right").ok().map(|p| parse_px(&p)).unwrap_or(0.0);

            s_reads.push(SRead {
                scroll_top: el.scroll_top(),
                overflow: st.get_property_value("overflow").unwrap_or_default(),
                overflow_y: st.get_property_value("overflow-y").unwrap_or_default(),
                padding_right: st.get_property_value("padding-right").unwrap_or_default(),
                computed_padding: cp,
                el_scrollbar: el.offset_width() - el.client_width(),
                el,
            });
        }
    }

    struct FRead {
        el: web_sys::HtmlElement,
        original_pr: String,
        computed_padding: f64,
    }

    let mut f_reads: Vec<FRead> = Vec::new();

    if scrollbar_width > 0.0
        && let Ok(nodes) = document.query_selector_all(FIXED_SELECTOR)
    {
        for i in 0..nodes.length() {
            let Some(node) = nodes.item(i) else { continue };
            let Ok(element) = node.dyn_into::<web_sys::Element>() else {
                continue;
            };

            let Some(cs) = window.get_computed_style(&element).ok().flatten() else {
                continue;
            };
            if cs.get_property_value("position").unwrap_or_default() != "fixed" {
                continue;
            }
            if is_fixed_excluded(&element) {
                continue;
            }

            let Ok(el) = element.dyn_into::<web_sys::HtmlElement>() else {
                continue;
            };

            let cp = cs.get_property_value("padding-right").ok().map(|p| parse_px(&p)).unwrap_or(0.0);

            f_reads.push(FRead {
                original_pr: el.style().get_property_value("padding-right").unwrap_or_default(),
                computed_padding: cp,
                el,
            });
        }
    }

    let _ = body_style.set_property("position", "fixed");
    let _ = body_style.set_property("top", &format!("-{window_scroll_y}px"));
    let _ = body_style.set_property("width", "100%");
    let _ = body_style.set_property("overflow", "hidden");

    if scrollbar_width > 0.0 {
        let _ = body_style.set_property("padding-right", &format!("{scrollbar_width}px"));

        for fr in &f_reads {
            let np = fr.computed_padding + scrollbar_width;
            let _ = fr.el.style().set_property("padding-right", &format!("{np}px"));
        }
    }

    for sr in &s_reads {
        let _ = sr.el.style().set_property("overflow", "hidden");
        if sr.el_scrollbar > 0 {
            let np = sr.computed_padding + sr.el_scrollbar as f64;
            let _ = sr.el.style().set_property("padding-right", &format!("{np}px"));
        }
    }

    STATE.with(|state| {
        let mut s = state.borrow_mut();
        s.window_scroll_y = window_scroll_y;
        s.body_styles = Some(original_body);
        s.scrollable = s_reads
            .into_iter()
            .map(|r| ScrollableEntry {
                element: r.el,
                scroll_top: r.scroll_top,
                overflow: r.overflow,
                overflow_y: r.overflow_y,
                padding_right: r.padding_right,
            })
            .collect();
        s.fixed = f_reads.into_iter().map(|r| FixedEntry { element: r.el, padding_right: r.original_pr }).collect();
    });
}

pub fn unlock(delay_ms: u32) {
    let release = STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.depth = s.depth.saturating_sub(1);
        s.depth == 0 && s.locked
    });
    if !release {
        return;
    }

    if delay_ms > 0 {
        leptos::prelude::set_timeout(perform_unlock, std::time::Duration::from_millis(u64::from(delay_ms)));
    } else {
        perform_unlock();
    }
}

pub fn is_locked() -> bool {
    STATE.with(|s| s.borrow().locked)
}

fn perform_unlock() {
    let Some(window) = web_sys::window() else { return };

    STATE.with(|state| {
        let mut s = state.borrow_mut();
        if s.depth > 0 {
            return;
        }

        if let Some(body) = window.document().and_then(|d| d.body())
            && let Some(ref orig) = s.body_styles
        {
            let st = body.style();
            set_style(&st, "position", &orig.position);
            set_style(&st, "top", &orig.top);
            set_style(&st, "width", &orig.width);
            set_style(&st, "overflow", &orig.overflow);
            set_style(&st, "padding-right", &orig.padding_right);
        }

        window.scroll_to_with_x_and_y(0.0, s.window_scroll_y);

        for entry in &s.scrollable {
            let st = entry.element.style();
            set_style(&st, "overflow", &entry.overflow);
            set_style(&st, "overflow-y", &entry.overflow_y);
            set_style(&st, "padding-right", &entry.padding_right);
            entry.element.set_scroll_top(entry.scroll_top);
        }

        for entry in &s.fixed {
            set_style(&entry.element.style(), "padding-right", &entry.padding_right);
        }

        s.clear();
    });
}
