use std::cell::Cell;
use std::sync::OnceLock;

use gloo_net::http::{Request, RequestBuilder, Response};
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use crate::id::Id;
use web_sys::{AbortController, File, FileList, FormData};

#[derive(Clone, Debug)]
pub struct ApiError {
    pub status: u16,
    pub msg: String,
}

pub type ApiResult<T> = Result<T, ApiError>;

pub fn base() -> &'static str {
    static BASE: OnceLock<String> = OnceLock::new();
    BASE.get_or_init(|| {
        let origin = window().location().origin().unwrap_or_default();
        let uri = document().base_uri().ok().flatten().unwrap_or_default();
        uri.strip_prefix(&origin).unwrap_or("").trim_end_matches('/').to_owned()
    })
}

pub fn url(path: &str) -> String {
    format!("{}{path}", base())
}

pub fn absolute(path: &str) -> String {
    format!("{}{}", window().location().origin().unwrap_or_default(), url(path))
}

async fn read<T: DeserializeOwned>(res: Result<Response, gloo_net::Error>) -> ApiResult<T> {
    let res = res.map_err(|e| ApiError { status: 0, msg: e.to_string() })?;
    if res.ok() {
        return res.json().await.map_err(|e| ApiError { status: 0, msg: e.to_string() });
    }
    let status = res.status();
    let msg = res
        .json::<Value>()
        .await
        .ok()
        .and_then(|v| v["error"].as_str().map(str::to_owned))
        .unwrap_or_else(|| format!("Request failed ({status})"));
    Err(ApiError { status, msg })
}

pub async fn get<T: DeserializeOwned>(path: &str) -> ApiResult<T> {
    read(Request::get(&url(path)).send().await).await
}

async fn with_body<T: DeserializeOwned>(req: RequestBuilder, body: &impl Serialize) -> ApiResult<T> {
    let req = req.json(body).map_err(|e| ApiError { status: 0, msg: e.to_string() })?;
    read(req.send().await).await
}

pub async fn post<T: DeserializeOwned>(path: &str, body: &impl Serialize) -> ApiResult<T> {
    with_body(Request::post(&url(path)), body).await
}

pub async fn put<T: DeserializeOwned>(path: &str, body: &impl Serialize) -> ApiResult<T> {
    with_body(Request::put(&url(path)), body).await
}

pub async fn del(path: &str) -> ApiResult<Value> {
    read(Request::delete(&url(path)).send().await).await
}

#[derive(Clone, Deserialize)]
pub struct Uploaded {
    pub id: String,
    pub name: String,
}

pub fn files_of(list: &FileList) -> Vec<File> {
    (0..list.length()).filter_map(|i| list.get(i)).collect()
}

pub async fn upload(files: Vec<File>, project: Option<Id>, folder: Option<String>) -> ApiResult<Vec<Uploaded>> {
    let form = FormData::new().expect("FormData");
    for f in &files {
        let _ = form.append_with_blob_and_filename("file", f, &f.name());
    }
    let query: Vec<String> = [project.map(|p| format!("project={p}")), folder.map(|f| format!("folder={f}"))].into_iter().flatten().collect();
    let path = format!("/api/files?{}", query.join("&"));
    let req = Request::post(&url(&path)).body(form).map_err(|e| ApiError { status: 0, msg: e.to_string() })?;
    read(req.send().await).await
}

thread_local! {
    static REV: Cell<i64> = const { Cell::new(0) };
    static CLIENT: String = format!("{:x}", (js_sys::Math::random() * 1e15) as u64);
}

pub fn next_rev() -> i64 {
    REV.with(|r| {
        let v = (js_sys::Date::now() as i64).max(r.get() + 1);
        r.set(v);
        v
    })
}

pub fn client_id() -> String {
    CLIENT.with(Clone::clone)
}

#[derive(Clone, Copy, PartialEq)]
pub enum SaveState {
    Idle,
    Saving,
    Saved,
    Failed,
}

#[derive(Clone, Copy)]
pub struct AutoSave {
    inflight: StoredValue<Option<AbortController>, LocalStorage>,
    pub state: RwSignal<SaveState>,
}

impl AutoSave {
    pub fn new() -> Self {
        Self { inflight: StoredValue::new_local(None), state: RwSignal::new(SaveState::Idle) }
    }

    pub fn send(self, path: String, body: Value) {
        if let Some(prev) = self.inflight.get_value() {
            prev.abort();
        }
        let ctl = AbortController::new().expect("AbortController");
        let signal = ctl.signal();
        self.inflight.set_value(Some(ctl));
        self.state.set(SaveState::Saving);
        spawn_local(async move {
            let req = Request::put(&url(&path)).abort_signal(Some(&signal)).json(&body);
            let res = match req {
                Ok(r) => read::<Value>(r.send().await).await,
                Err(e) => Err(ApiError { status: 0, msg: e.to_string() }),
            };
            if signal.aborted() {
                return;
            }
            self.state.set(if res.is_ok() { SaveState::Saved } else { SaveState::Failed });
        });
    }
}
