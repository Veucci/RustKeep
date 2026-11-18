use std::collections::HashMap;
use std::io::{Cursor, Read};

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Multipart, OriginalUri, Path, Query, Request};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri, header};
use axum::response::{Html, IntoResponse, Redirect, Response};
use hmac::{Hmac, Mac};
use libsql::params;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::Sha256;

use crate::auth::{User, login_allowed, unix_now};
use crate::convert;
use crate::util::{PREVIEW_SECS, Res, all, bad, cookie, encode_path, err, exec, hash, not_found, one, token, uuid, verify};
use crate::{App, St};

const MAX_PATH: usize = 512;
const ZIP_ENTRIES: usize = 10_000;
const ZIP_BYTES: u64 = 1 << 30;
const UNLOCK_SECS: i64 = 7 * 86400;
const SANDBOX: &str = "sandbox allow-scripts allow-forms allow-popups allow-popups-to-escape-sandbox allow-modals allow-downloads \
     allow-presentation allow-pointer-lock allow-orientation-lock allow-top-navigation-by-user-activation";

#[derive(Serialize, Deserialize)]
pub struct Site {
    id: String,
    name: String,
    enabled: i64,
    locked: i64,
    expires: Option<i64>,
    views: i64,
    files: i64,
    bytes: i64,
    created: i64,
    updated: i64,
}

#[derive(Serialize, Deserialize)]
pub struct SiteFile {
    path: String,
    size: i64,
    mime: String,
    created: i64,
    updated: i64,
}

#[derive(Deserialize)]
pub struct SiteReq {
    name: String,
    #[serde(default = "enabled")]
    enabled: bool,
    password: Option<String>,
    expires: Option<i64>,
}

fn enabled() -> bool {
    true
}

fn valid_name(name: &str) -> bool {
    (1..=63).contains(&name.len())
        && !name.starts_with('-')
        && !name.ends_with('-')
        && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

impl SiteReq {
    async fn name(&self, app: &App, id: &str) -> Res<String> {
        let name = self.name.trim().to_lowercase();
        if !valid_name(&name) {
            return Err(bad("Use 1 to 63 lowercase letters, digits and hyphens"));
        }
        let taken = app.db.query("SELECT 1 FROM sites WHERE name = ?1 AND id <> ?2", params![name.as_str(), id]).await?.next().await?.is_some();
        if taken {
            return Err(err(StatusCode::CONFLICT, "That site name is already taken"));
        }
        Ok(name)
    }

    fn password_hash(&self) -> Option<String> {
        self.password.as_deref().map(|p| if p.is_empty() { String::new() } else { hash(p) })
    }
}

#[derive(Deserialize)]
struct Key {
    key: String,
}

#[derive(Deserialize)]
pub(crate) struct Stored {
    pub(crate) key: String,
    pub(crate) mime: String,
}

#[derive(Deserialize)]
struct Existing {
    path: String,
    key: String,
}

struct Staged {
    path: String,
    key: String,
    size: i64,
}

#[derive(Deserialize)]
pub struct UploadQ {
    #[serde(default)]
    replace: bool,
    #[serde(default)]
    dir: String,
}

#[derive(Deserialize)]
pub struct FileQ {
    path: String,
}

#[derive(Deserialize)]
pub struct Move {
    from: String,
    to: String,
}

#[derive(Deserialize)]
pub struct MoveReq {
    moves: Vec<Move>,
}

#[derive(Deserialize)]
pub struct DeleteReq {
    paths: Vec<String>,
}

fn site_path(raw: &str) -> Res<Option<String>> {
    let normalized = raw.replace('\\', "/");
    let parts: Vec<&str> = normalized.split('/').filter(|s| !s.is_empty()).collect();
    let invalid = raw.len() > MAX_PATH || raw.contains('\0') || parts.iter().any(|p| matches!(*p, "." | ".."));
    let Some(last) = parts.last().filter(|_| !invalid) else { return Err(bad("Invalid file path")) };
    let junk = parts.contains(&"__MACOSX") || matches!(*last, ".DS_Store" | "Thumbs.db");
    Ok((!junk).then(|| parts.join("/")))
}

pub(crate) fn mime_of(path: &str) -> &'static str {
    let ext = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    convert::mime(&ext)
}

fn strip_shared_folder(staged: &mut [Staged]) {
    let Some(prefix) = staged.first().and_then(|s| s.path.split_once('/')).map(|(first, _)| format!("{first}/")) else { return };
    let nested_index = format!("{prefix}index.html");
    let root_index = staged.iter().any(|s| s.path == "index.html");
    if root_index || !staged.iter().any(|s| s.path == nested_index) || !staged.iter().all(|s| s.path.starts_with(&prefix)) {
        return;
    }
    for s in staged.iter_mut() {
        s.path = s.path[prefix.len()..].to_owned();
    }
}

fn upload_dir(raw: &str) -> Res<String> {
    if raw.replace('\\', "/").trim_matches('/').is_empty() {
        return Ok(String::new());
    }
    requested(raw)
}

fn arrange(staged: &mut [Staged], dir: &str, replace: bool) {
    if replace {
        strip_shared_folder(staged);
    }
    if dir.is_empty() {
        return;
    }
    for s in staged.iter_mut() {
        s.path = format!("{dir}/{}", s.path);
    }
}

async fn owned(app: &App, user: &User, id: &str) -> Res<()> {
    one::<Key>(&app.db, "SELECT id AS key FROM sites WHERE id = ?1 AND user_id = ?2", params![id, user.id.as_str()]).await.map(|_| ())
}

async fn file_list(app: &App, id: &str) -> Res<Json<Vec<SiteFile>>> {
    Ok(Json(all(&app.db, "SELECT path, size, mime, created, updated FROM site_files WHERE site_id = ?1 ORDER BY path", params![id]).await?))
}

pub async fn list(app: St, user: User) -> Res<Json<Vec<Site>>> {
    let rows = all(
        &app.db,
        "SELECT s.id, s.name, s.enabled, s.password IS NOT NULL AS locked, s.expires, s.views, \
         COUNT(f.id) AS files, COALESCE(SUM(f.size), 0) AS bytes, s.created, s.updated \
         FROM sites s LEFT JOIN site_files f ON f.site_id = s.id WHERE s.user_id = ?1 GROUP BY s.id ORDER BY s.created DESC",
        params![user.id.as_str()],
    )
    .await?;
    Ok(Json(rows))
}

pub async fn create(app: St, user: User, Json(r): Json<SiteReq>) -> Res<Json<Value>> {
    let id = uuid();
    let name = r.name(&app, &id).await?;
    let password = r.password_hash().filter(|p| !p.is_empty());
    app.db
        .execute(
            "INSERT INTO sites (id, user_id, name, enabled, password, expires) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id.as_str(), user.id.as_str(), name.as_str(), r.enabled as i64, password, r.expires],
        )
        .await?;
    Ok(Json(json!({ "id": id, "name": name })))
}

pub async fn update(app: St, user: User, Path(id): Path<String>, Json(r): Json<SiteReq>) -> Res<Json<Value>> {
    let name = r.name(&app, &id).await?;
    exec(
        &app.db,
        "UPDATE sites SET name = ?1, enabled = ?2, expires = ?3, \
         password = CASE WHEN ?4 IS NULL THEN password WHEN ?4 = '' THEN NULL ELSE ?4 END \
         WHERE id = ?5 AND user_id = ?6",
        params![name, r.enabled as i64, r.expires, r.password_hash(), id.as_str(), user.id.as_str()],
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn remove(app: St, user: User, Path(id): Path<String>) -> Res<Json<Value>> {
    owned(&app, &user, &id).await?;
    let tx = app.db.transaction().await?;
    let keys: Vec<Key> = all(&tx, "SELECT key FROM site_files WHERE site_id = ?1", params![id.as_str()]).await?;
    tx.execute("DELETE FROM site_files WHERE site_id = ?1", params![id.as_str()]).await?;
    tx.execute("DELETE FROM sites WHERE id = ?1", params![id.as_str()]).await?;
    tx.commit().await?;
    for k in keys {
        app.storage.remove(&k.key).await;
    }
    Ok(Json(json!({ "ok": true })))
}

pub async fn files(app: St, user: User, Path(id): Path<String>) -> Res<Json<Vec<SiteFile>>> {
    owned(&app, &user, &id).await?;
    file_list(&app, &id).await
}

async fn receive(app: &App, mp: &mut Multipart, staged: &mut Vec<Staged>) -> Res<()> {
    let mut next_path = None;
    while let Some(mut field) = mp.next_field().await? {
        match field.name().unwrap_or_default() {
            "path" => next_path = Some(field.text().await?),
            "file" => {
                let raw = next_path.take().or_else(|| field.file_name().map(str::to_owned)).unwrap_or_default();
                let Some(path) = site_path(&raw)? else { continue };
                let key = token();
                let size = app.storage.save(&key, &mut field).await?;
                staged.push(Staged { path, key, size });
            }
            "zip" => receive_zip(app, field.bytes().await?, staged).await?,
            _ => {}
        }
    }
    Ok(())
}

type Archive = zip::ZipArchive<Cursor<Bytes>>;

fn read_entry(archive: &mut Archive, index: usize, budget: u64) -> Res<Option<(String, Vec<u8>)>> {
    let entry = archive.by_index(index).map_err(|_| bad("Invalid zip archive"))?;
    if entry.is_dir() {
        return Ok(None);
    }
    let name = entry.name().to_owned();
    let mut data = Vec::new();
    entry.take(budget + 1).read_to_end(&mut data).map_err(|_| bad("Invalid zip archive"))?;
    Ok(Some((name, data)))
}

async fn receive_zip(app: &App, bytes: Bytes, staged: &mut Vec<Staged>) -> Res<()> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| bad("Invalid zip archive"))?;
    if archive.len() > ZIP_ENTRIES {
        return Err(bad("The zip archive has more than 10000 entries"));
    }
    let mut total = 0u64;
    for index in 0..archive.len() {
        let budget = ZIP_BYTES - total;
        let (back, entry) = tokio::task::spawn_blocking(move || {
            let entry = read_entry(&mut archive, index, budget);
            (archive, entry)
        })
        .await?;
        archive = back;
        let Some((name, data)) = entry? else { continue };
        total += data.len() as u64;
        if total > ZIP_BYTES {
            return Err(bad("The zip archive is larger than 1 GiB uncompressed"));
        }
        let Some(path) = site_path(&name)? else { continue };
        let key = token();
        let size = app.storage.save_bytes(&key, &data).await?;
        staged.push(Staged { path, key, size });
    }
    Ok(())
}

async fn save_batch(app: &App, site: &str, staged: &[Staged], replace: bool) -> Res<Vec<String>> {
    let latest: HashMap<&str, &Staged> = staged.iter().map(|s| (s.path.as_str(), s)).collect();
    let tx = app.db.transaction().await?;
    let existing: Vec<Existing> = all(&tx, "SELECT path, key FROM site_files WHERE site_id = ?1", params![site]).await?;
    for s in latest.values() {
        tx.execute(
            "INSERT INTO site_files (id, site_id, path, key, size, mime) VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
             ON CONFLICT(site_id, path) DO UPDATE SET key = excluded.key, size = excluded.size, mime = excluded.mime, updated = unixepoch()",
            params![uuid(), site, s.path.as_str(), s.key.as_str(), s.size, mime_of(&s.path)],
        )
        .await?;
    }
    for e in existing.iter().filter(|e| replace && !latest.contains_key(e.path.as_str())) {
        tx.execute("DELETE FROM site_files WHERE site_id = ?1 AND path = ?2", params![site, e.path.as_str()]).await?;
    }
    tx.execute("UPDATE sites SET updated = unixepoch() WHERE id = ?1", params![site]).await?;
    tx.commit().await?;
    let replaced = existing.into_iter().filter(|e| replace || latest.contains_key(e.path.as_str())).map(|e| e.key);
    let duplicates = staged.iter().filter(|s| latest[s.path.as_str()].key != s.key).map(|s| s.key.clone());
    Ok(replaced.chain(duplicates).collect())
}

async fn commit(app: &App, site: &str, staged: &[Staged], replace: bool) -> Res<()> {
    let garbage = match save_batch(app, site, staged, replace).await {
        Ok(garbage) => garbage,
        Err(e) => {
            discard(app, staged).await;
            return Err(e);
        }
    };
    for key in garbage {
        app.storage.remove(&key).await;
    }
    Ok(())
}

async fn discard(app: &App, staged: &[Staged]) {
    for s in staged {
        app.storage.remove(&s.key).await;
    }
}

pub async fn upload(app: St, user: User, Path(id): Path<String>, Query(q): Query<UploadQ>, mut mp: Multipart) -> Res<Json<Vec<SiteFile>>> {
    owned(&app, &user, &id).await?;
    let dir = upload_dir(&q.dir)?;
    let mut staged = Vec::new();
    if let Err(e) = receive(&app, &mut mp, &mut staged).await {
        discard(&app, &staged).await;
        return Err(e);
    }
    if staged.is_empty() {
        return Err(bad("No file in request"));
    }
    arrange(&mut staged, &dir, q.replace);
    commit(&app, &id, &staged, q.replace).await?;
    file_list(&app, &id).await
}

fn requested(raw: &str) -> Res<String> {
    site_path(raw)?.ok_or_else(|| bad("Invalid file path"))
}

pub async fn raw(app: St, user: User, Path(id): Path<String>, Query(q): Query<FileQ>, req: Request) -> Res<Response> {
    let f: Stored = one(
        &app.db,
        "SELECT f.key, f.mime FROM site_files f JOIN sites s ON s.id = f.site_id WHERE s.id = ?1 AND s.user_id = ?2 AND f.path = ?3",
        params![id.as_str(), user.id.as_str(), requested(&q.path)?],
    )
    .await?;
    send(&app, &f, req, StatusCode::OK).await
}

pub async fn put_file(app: St, user: User, Path(id): Path<String>, Query(q): Query<FileQ>, body: Bytes) -> Res<Json<Value>> {
    owned(&app, &user, &id).await?;
    let path = requested(&q.path)?;
    let key = token();
    let size = app.storage.save_bytes(&key, &body).await?;
    commit(&app, &id, &[Staged { path, key, size }], false).await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn move_files(app: St, user: User, Path(id): Path<String>, Json(r): Json<MoveReq>) -> Res<Json<Vec<SiteFile>>> {
    owned(&app, &user, &id).await?;
    let tx = app.db.transaction().await?;
    let find = "SELECT key FROM site_files WHERE site_id = ?1 AND path = ?2";
    for m in &r.moves {
        let (from, to) = (requested(&m.from)?, requested(&m.to)?);
        one::<Key>(&tx, find, params![id.as_str(), from.as_str()]).await?;
        if from == to {
            continue;
        }
        if !all::<Key>(&tx, find, params![id.as_str(), to.as_str()]).await?.is_empty() {
            return Err(err(StatusCode::CONFLICT, &format!("A file named \"{to}\" already exists")));
        }
        tx.execute(
            "UPDATE site_files SET path = ?1, mime = ?2 WHERE site_id = ?3 AND path = ?4",
            params![to.as_str(), mime_of(&to), id.as_str(), from.as_str()],
        )
        .await?;
    }
    tx.execute("UPDATE sites SET updated = unixepoch() WHERE id = ?1", params![id.as_str()]).await?;
    tx.commit().await?;
    file_list(&app, &id).await
}

pub async fn delete_files(app: St, user: User, Path(id): Path<String>, Json(r): Json<DeleteReq>) -> Res<Json<Vec<SiteFile>>> {
    owned(&app, &user, &id).await?;
    let tx = app.db.transaction().await?;
    let mut keys = Vec::new();
    for raw in &r.paths {
        let path = requested(raw)?;
        let removed: Vec<Key> =
            all(&tx, "DELETE FROM site_files WHERE site_id = ?1 AND path = ?2 RETURNING key", params![id.as_str(), path.as_str()]).await?;
        keys.extend(removed);
    }
    tx.execute("UPDATE sites SET updated = unixepoch() WHERE id = ?1", params![id.as_str()]).await?;
    tx.commit().await?;
    for k in keys {
        app.storage.remove(&k.key).await;
    }
    file_list(&app, &id).await
}

#[derive(Deserialize)]
struct Live {
    id: String,
    name: String,
    password: Option<String>,
}

async fn live(app: &App, name: &str) -> Res<Option<Live>> {
    let rows = all(
        &app.db,
        "SELECT id, name, password FROM sites WHERE name = ?1 AND enabled = 1 AND (expires IS NULL OR expires > unixepoch())",
        params![name],
    )
    .await?;
    Ok(rows.into_iter().next())
}

fn cookie_name(site: &Live) -> String {
    format!("site_{}", site.id.replace('-', ""))
}

fn proof(app: &App, site: &Live, stored: &str) -> Hmac<Sha256> {
    app.crypto.mac(format!("site-unlock:{}:{stored}", site.id).as_bytes())
}

fn unlocked(app: &App, site: &Live, headers: &HeaderMap) -> bool {
    let Some(stored) = site.password.as_deref() else { return true };
    let given = cookie(headers, &cookie_name(site)).and_then(|v| hex::decode(v).ok()).unwrap_or_default();
    proof(app, site, stored).verify_slice(&given).is_ok()
}

fn spa(app: &App, status: StatusCode) -> Response {
    (status, Html(app.index.clone())).into_response()
}

fn add_slash(uri: &Uri) -> Response {
    let query = uri.query().map(|q| format!("?{q}")).unwrap_or_default();
    Redirect::temporary(&format!("{}/{query}", uri.path())).into_response()
}

pub(crate) async fn send(app: &App, f: &Stored, req: Request, status: StatusCode) -> Res<Response> {
    let mut res = app.storage.serve(&f.key, req).await?;
    if res.status() == StatusCode::OK {
        *res.status_mut() = status;
    }
    let charset = if f.mime.starts_with("text/") || f.mime.ends_with("json") { "; charset=utf-8" } else { "" };
    let h = res.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_str(&format!("{}{charset}", f.mime))?);
    h.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static(SANDBOX));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    h.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
    Ok(res)
}

async fn lookup(app: &App, site: &str, path: &str) -> Res<Option<Stored>> {
    let rows = all(&app.db, "SELECT key, mime FROM site_files WHERE site_id = ?1 AND path = ?2", params![site, path]).await?;
    Ok(rows.into_iter().next())
}

enum Hit {
    File(Stored, bool),
    Folder,
    Missing(Option<Stored>),
}

async fn resolve(app: &App, site: &str, raw: &str) -> Res<Hit> {
    let wanted = if raw.is_empty() || raw.ends_with('/') { format!("{raw}index.html") } else { raw.to_owned() };
    let Ok(Some(path)) = site_path(&wanted) else { return Ok(Hit::Missing(lookup(app, site, "404.html").await?)) };
    if let Some(f) = lookup(app, site, &path).await? {
        return Ok(Hit::File(f, path == "index.html"));
    }
    if lookup(app, site, &format!("{path}/index.html")).await?.is_some() {
        return Ok(Hit::Folder);
    }
    Ok(Hit::Missing(lookup(app, site, "404.html").await?))
}

#[derive(Deserialize)]
pub struct Target {
    name: String,
    #[serde(default)]
    path: String,
}

#[derive(Deserialize)]
pub struct PreviewTarget {
    pub(crate) token: String,
    #[serde(default)]
    pub(crate) path: String,
}

pub async fn preview(app: St, user: User, Path(id): Path<String>, Query(q): Query<FileQ>) -> Res<Json<Value>> {
    owned(&app, &user, &id).await?;
    let token = app.crypto.sign_preview("s", &id, unix_now() + PREVIEW_SECS);
    Ok(Json(json!({ "url": format!("/preview/s/{token}/{}", encode_path(&requested(&q.path)?)) })))
}

pub async fn preview_serve(app: St, Path(t): Path<PreviewTarget>, req: Request) -> Res<Response> {
    let site = app.crypto.check_preview("s", &t.token, unix_now()).ok_or_else(not_found)?;
    let Hit::File(f, _) = resolve(&app, &site, &t.path).await? else { return Err(not_found()) };
    send(&app, &f, req, StatusCode::OK).await
}

pub async fn redirect(OriginalUri(uri): OriginalUri) -> Response {
    add_slash(&uri)
}

pub async fn serve(app: St, Path(t): Path<Target>, OriginalUri(uri): OriginalUri, req: Request) -> Res<Response> {
    let Some(site) = live(&app, &t.name).await? else { return Ok(spa(&app, StatusCode::NOT_FOUND)) };
    if !unlocked(&app, &site, req.headers()) {
        return Ok(spa(&app, StatusCode::UNAUTHORIZED));
    }
    match resolve(&app, &site.id, &t.path).await? {
        Hit::File(f, root) => {
            if root && req.method() == Method::GET {
                app.db.execute("UPDATE sites SET views = views + 1 WHERE id = ?1", params![site.id.as_str()]).await?;
            }
            send(&app, &f, req, StatusCode::OK).await
        }
        Hit::Folder => Ok(add_slash(&uri)),
        Hit::Missing(Some(f)) => send(&app, &f, req, StatusCode::NOT_FOUND).await,
        Hit::Missing(None) => Ok(spa(&app, StatusCode::NOT_FOUND)),
    }
}

pub async fn public_get(app: St, Path(name): Path<String>, headers: HeaderMap) -> Res<Json<Value>> {
    let site = live(&app, &name).await?.ok_or_else(not_found)?;
    let unlocked = unlocked(&app, &site, &headers);
    Ok(Json(json!({ "name": site.name, "locked": site.password.is_some(), "unlocked": unlocked })))
}

#[derive(Deserialize)]
pub struct UnlockReq {
    password: String,
}

pub async fn unlock(app: St, Path(name): Path<String>, Json(r): Json<UnlockReq>) -> Res<impl IntoResponse> {
    let site = live(&app, &name).await?.ok_or_else(not_found)?;
    let mut headers = HeaderMap::new();
    let Some(stored) = site.password.as_deref() else { return Ok((headers, Json(json!({ "ok": true })))) };
    let attempts = format!("site:{}", site.id);
    if !login_allowed(&app, &attempts) {
        return Err(err(StatusCode::TOO_MANY_REQUESTS, "Too many attempts, try again later"));
    }
    if !verify(&r.password, stored) {
        return Err(err(StatusCode::UNAUTHORIZED, "Wrong password"));
    }
    app.login_attempts.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(&attempts);
    let value = hex::encode(proof(&app, &site, stored).finalize().into_bytes());
    let path = if app.cfg.base.is_empty() { "/" } else { &app.cfg.base };
    let c = format!("{}={value}; Path={path}; HttpOnly; SameSite=None; Secure; Max-Age={UNLOCK_SECS}", cookie_name(&site));
    headers.insert(header::SET_COOKIE, HeaderValue::from_str(&c)?);
    Ok((headers, Json(json!({ "ok": true }))))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn staged(paths: &[&str]) -> Vec<Staged> {
        paths.iter().map(|p| Staged { path: (*p).to_owned(), key: token(), size: 0 }).collect()
    }

    #[test]
    fn validates_names() {
        assert!(valid_name("my-site-1"));
        assert!(!valid_name("-a") && !valid_name("a-") && !valid_name(""));
        assert!(!valid_name("a_b") && !valid_name(&"a".repeat(64)) && !valid_name("A"));
    }

    #[test]
    fn normalizes_paths() {
        assert_eq!(site_path("\\css//app.css").ok().flatten().as_deref(), Some("css/app.css"));
        assert!(site_path("a/../b").is_err() && site_path("./a").is_err() && site_path("").is_err());
        assert!(site_path("a\0b").is_err() && site_path(&"a".repeat(513)).is_err());
        assert!(site_path("__MACOSX/x").is_ok_and(|p| p.is_none()));
        assert!(site_path("img/.DS_Store").is_ok_and(|p| p.is_none()));
    }

    #[test]
    fn strips_single_top_folder_with_index() {
        let mut s = staged(&["site/index.html", "site/css/a.css"]);
        strip_shared_folder(&mut s);
        assert_eq!(s[0].path, "index.html");
        assert_eq!(s[1].path, "css/a.css");
        let mut s = staged(&["css/a.css", "css/b.css"]);
        strip_shared_folder(&mut s);
        assert_eq!(s[0].path, "css/a.css");
    }

    #[test]
    fn strips_only_on_replace_and_prefixes_dir() {
        let mut s = staged(&["site/index.html", "site/a.css"]);
        arrange(&mut s, "", false);
        assert_eq!(s[0].path, "site/index.html");
        arrange(&mut s, "", true);
        assert_eq!(s[0].path, "index.html");
        let mut s = staged(&["site/index.html", "site/a.css"]);
        arrange(&mut s, "docs/v1", true);
        assert_eq!(s[0].path, "docs/v1/index.html");
        assert_eq!(s[1].path, "docs/v1/a.css");
    }

    #[test]
    fn normalizes_upload_dir() {
        assert_eq!(upload_dir("").ok().as_deref(), Some(""));
        assert_eq!(upload_dir("/\\/").ok().as_deref(), Some(""));
        assert_eq!(upload_dir("\\docs//v1/").ok().as_deref(), Some("docs/v1"));
        assert!(upload_dir("a/../b").is_err() && upload_dir("__MACOSX").is_err());
    }
}
