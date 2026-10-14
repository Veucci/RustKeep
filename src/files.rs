use axum::Json;
use axum::extract::{Multipart, Path, Query, Request};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::Response;
use libsql::params;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::auth::User;
use crate::convert;
use crate::util::{Res, all, archive_set, bad, err, exec, one, token, uuid};
use crate::{App, St};

#[derive(Deserialize)]
pub struct ListQ {
    project: Option<String>,
    folder: Option<String>,
    #[serde(default)]
    replace: bool,
}

#[derive(Deserialize)]
pub struct ConflictQ {
    folder: Option<String>,
    names: Vec<String>,
}

#[derive(Deserialize)]
struct Key {
    key: String,
}

#[derive(Deserialize)]
pub struct Edit {
    pub name: String,
    pub parent: Option<String>,
    pub starred: bool,
}

impl Edit {
    pub fn clean_name(&self) -> Res<&str> {
        let name = self.name.trim();
        if name.is_empty() || name.chars().count() > 255 || name.contains('/') {
            return Err(bad("Invalid name"));
        }
        Ok(name)
    }
}

#[derive(Serialize, Deserialize)]
pub struct FileItem {
    id: String,
    name: String,
    mime: String,
    size: i64,
    project_id: Option<String>,
    created: i64,
    share_token: Option<String>,
    share_expires: Option<i64>,
    archived: i64,
    folder_id: Option<String>,
    starred: i64,
}

pub const FILE_COLUMNS: &str =
    "id, name, mime, size, project_id, created, share_token, share_expires, archived, folder_id, starred";

pub async fn list(app: St, user: User, Query(q): Query<ListQ>) -> Res<Json<Vec<FileItem>>> {
    let sql = format!(
        "SELECT {FILE_COLUMNS} FROM files WHERE user_id = ?1 AND (?2 IS NULL OR project_id = ?2) ORDER BY created DESC"
    );
    let rows = all(&app.db, &sql, params![user.id.as_str(), q.project]).await?;
    Ok(Json(rows))
}

async fn taken(app: &App, user: &User, folder: Option<&str>, name: &str, except: &str) -> Res<Option<String>> {
    let mut rows = app
        .db
        .query(
            "SELECT id FROM files WHERE user_id = ?1 AND name = ?2 AND id <> ?4 \
             AND folder_id IS (SELECT id FROM folders WHERE id = ?3 AND user_id = ?1) LIMIT 1",
            params![user.id.as_str(), name, folder, except],
        )
        .await?;
    Ok(rows.next().await?.map(|r| r.get::<String>(0)).transpose()?)
}

fn numbered(name: &str, n: u32) -> String {
    match name.rfind('.') {
        Some(i) if i > 0 => format!("{} ({n}){}", &name[..i], &name[i..]),
        _ => format!("{name} ({n})"),
    }
}

async fn free_name(app: &App, user: &User, folder: Option<&str>, name: &str) -> Res<String> {
    let mut n = 1;
    loop {
        let candidate = numbered(name, n);
        if taken(app, user, folder, &candidate, "").await?.is_none() {
            return Ok(candidate);
        }
        n += 1;
    }
}

pub async fn conflicts(app: St, user: User, Json(r): Json<ConflictQ>) -> Res<Json<Vec<String>>> {
    let mut found = Vec::new();
    for name in r.names {
        if taken(&app, &user, r.folder.as_deref(), &name, "").await?.is_some() {
            found.push(name);
        }
    }
    Ok(Json(found))
}

async fn replace_content(app: &App, id: &str, key: &str, mime: &str, size: i64) -> Res<()> {
    let old: Key = one(&app.db, "SELECT key FROM files WHERE id = ?1", params![id]).await?;
    exec(
        &app.db,
        "UPDATE files SET key = ?1, mime = ?2, size = ?3, created = unixepoch(), archived = 0 WHERE id = ?4",
        params![key, mime, size, id],
    )
    .await?;
    app.storage.remove(&old.key).await;
    Ok(())
}

struct NewFile<'a> {
    project: Option<&'a str>,
    folder: Option<&'a str>,
    name: &'a str,
    mime: &'a str,
    size: i64,
    key: &'a str,
}

async fn insert(app: &App, user: &User, f: NewFile<'_>) -> Res<String> {
    let id = uuid();
    app.db
        .execute(
            "INSERT INTO files (id, user_id, project_id, folder_id, name, mime, size, key) \
             VALUES (?1, ?2, (SELECT id FROM projects WHERE id = ?3 AND user_id = ?2), \
             (SELECT id FROM folders WHERE id = ?4 AND user_id = ?2), ?5, ?6, ?7, ?8)",
            params![id.as_str(), user.id.as_str(), f.project, f.folder, f.name, f.mime, f.size, f.key],
        )
        .await?;
    Ok(id)
}

pub async fn upload(app: St, user: User, Query(q): Query<ListQ>, mut mp: Multipart) -> Res<Json<Vec<Value>>> {
    let folder = q.folder.as_deref();
    let mut saved = Vec::new();
    while let Some(mut field) = mp.next_field().await? {
        let Some(name) = field.file_name().map(str::to_owned) else { continue };
        let mime = field.content_type().unwrap_or("application/octet-stream").to_owned();
        let key = token();
        let size = app.storage.save(&key, &mut field).await?;
        let existing = taken(&app, &user, folder, &name, "").await?;
        if let Some(id) = existing.as_deref().filter(|_| q.replace) {
            replace_content(&app, id, &key, &mime, size).await?;
            saved.push(json!({ "id": id, "name": name }));
            continue;
        }
        let name = if existing.is_some() { free_name(&app, &user, folder, &name).await? } else { name };
        let id = insert(&app, &user, NewFile { project: q.project.as_deref(), folder, name: &name, mime: &mime, size, key: &key }).await?;
        saved.push(json!({ "id": id, "name": name }));
    }
    if saved.is_empty() {
        return Err(bad("No file in request"));
    }
    Ok(Json(saved))
}

const CONVERT_LIMIT: i64 = 50 * 1024 * 1024;

#[derive(Deserialize)]
pub struct ConvertReq {
    to: String,
}

#[derive(Deserialize)]
struct Original {
    name: String,
    key: String,
    size: i64,
    project_id: Option<String>,
    folder_id: Option<String>,
}

fn split_ext(name: &str) -> (&str, &str) {
    name.rsplit_once('.').filter(|(stem, _)| !stem.is_empty()).unwrap_or((name, ""))
}

pub async fn formats(app: St, user: User, Path(id): Path<String>) -> Res<Json<Vec<&'static str>>> {
    let f: Original = one(&app.db, "SELECT name, key, size, project_id, folder_id FROM files WHERE id = ?1 AND user_id = ?2", params![id.as_str(), user.id.as_str()]).await?;
    Ok(Json(convert::targets(split_ext(&f.name).1)))
}

pub async fn convert(app: St, user: User, Path(id): Path<String>, Json(r): Json<ConvertReq>) -> Res<Json<Value>> {
    let f: Original = one(&app.db, "SELECT name, key, size, project_id, folder_id FROM files WHERE id = ?1 AND user_id = ?2", params![id.as_str(), user.id.as_str()]).await?;
    let (stem, from) = split_ext(&f.name);
    if !convert::targets(from).contains(&r.to.as_str()) {
        return Err(bad("This conversion is not supported"));
    }
    if f.size > CONVERT_LIMIT {
        return Err(bad("Files larger than 50 MB cannot be converted"));
    }
    let bytes = app.storage.read(&f.key).await?;
    let (title, source, target) = (stem.to_owned(), from.to_owned(), r.to.clone());
    let out = tokio::task::spawn_blocking(move || convert::run(bytes, &title, &source, &target))
        .await?
        .map_err(|_| err(StatusCode::UNPROCESSABLE_ENTITY, "This file could not be converted"))?;
    let folder = f.folder_id.as_deref();
    let name = format!("{stem}.{}", r.to);
    let name = if taken(&app, &user, folder, &name, "").await?.is_some() { free_name(&app, &user, folder, &name).await? } else { name };
    let key = token();
    let size = app.storage.save_bytes(&key, &out).await?;
    let new = NewFile { project: f.project_id.as_deref(), folder, name: &name, mime: convert::mime(&r.to), size, key: &key };
    let id = insert(&app, &user, new).await?;
    Ok(Json(json!({ "id": id, "name": name })))
}

#[derive(Deserialize)]
pub struct Stored {
    name: String,
    mime: String,
    key: String,
}

fn disposition(name: &str) -> String {
    format!("inline; {}", filename(name))
}

pub fn attachment(name: &str) -> String {
    format!("attachment; {}", filename(name))
}

fn filename(name: &str) -> String {
    let enc: String = name
        .bytes()
        .map(|b| match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect();
    format!("filename*=UTF-8''{enc}")
}

fn inert(mime: &str) -> bool {
    let media = ["video/", "audio/", "image/"].iter().any(|p| mime.starts_with(p));
    (media && !mime.contains("svg")) || mime == "application/pdf"
}

pub async fn serve(app: &App, f: Stored, req: Request) -> Res<Response> {
    let mut res = app.storage.serve(&f.key, req).await?;
    let h = res.headers_mut();
    let mime = HeaderValue::from_str(&f.mime).unwrap_or(HeaderValue::from_static("application/octet-stream"));
    h.insert(header::CONTENT_TYPE, mime);
    if let Ok(v) = HeaderValue::from_str(&disposition(&f.name)) {
        h.insert(header::CONTENT_DISPOSITION, v);
    }
    if !inert(&f.mime) {
        h.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static("sandbox"));
    }
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    Ok(res)
}

pub async fn raw(app: St, user: User, Path(id): Path<String>, req: Request) -> Res<Response> {
    let f = one(&app.db, "SELECT name, mime, key FROM files WHERE id = ?1 AND user_id = ?2", params![id.as_str(), user.id.as_str()]).await?;
    serve(&app, f, req).await
}

pub async fn public_raw(app: St, Path(t): Path<String>, req: Request) -> Res<Response> {
    let f = one(
        &app.db,
        "SELECT name, mime, key FROM files WHERE share_token = ?1 AND (share_expires IS NULL OR share_expires > unixepoch())",
        params![t],
    )
    .await?;
    serve(&app, f, req).await
}

pub async fn update(app: St, user: User, Path(id): Path<String>, Json(r): Json<Edit>) -> Res<Json<Value>> {
    if taken(&app, &user, r.parent.as_deref(), r.clean_name()?, &id).await?.is_some() {
        return Err(err(StatusCode::CONFLICT, "A file with this name already exists in this folder"));
    }
    exec(
        &app.db,
        "UPDATE files SET name = ?1, starred = ?2, folder_id = (SELECT id FROM folders WHERE id = ?3 AND user_id = ?5) \
         WHERE id = ?4 AND user_id = ?5",
        params![r.clean_name()?, r.starred as i64, r.parent.as_deref(), id.as_str(), user.id.as_str()],
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn action(app: St, user: User, Path((id, action)): Path<(String, String)>) -> Res<Json<Value>> {
    let sql = format!("UPDATE files SET {} WHERE id = ?1 AND user_id = ?2", archive_set(&action)?);
    exec(&app.db, &sql, params![id.as_str(), user.id.as_str()]).await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn remove(app: St, user: User, Path(id): Path<String>) -> Res<Json<Value>> {
    let f: Stored = one(&app.db, "SELECT name, mime, key FROM files WHERE id = ?1 AND user_id = ?2", params![id.as_str(), user.id.as_str()]).await?;
    exec(&app.db, "DELETE FROM files WHERE id = ?1", params![id.as_str()]).await?;
    app.storage.remove(&f.key).await;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct ShareReq {
    pub expires: Option<i64>,
}

pub async fn set_share(app: &App, table: &str, id: &str, user: &User, expires: Option<i64>) -> Res<Json<Value>> {
    let t = token();
    let sql = format!("UPDATE {table} SET share_token = ?1, share_expires = ?2 WHERE id = ?3 AND user_id = ?4");
    exec(&app.db, &sql, params![t.clone(), expires, id, user.id.as_str()]).await?;
    Ok(Json(json!({ "token": t })))
}

pub async fn clear_share(app: &App, table: &str, id: &str, user: &User) -> Res<Json<Value>> {
    let sql = format!("UPDATE {table} SET share_token = NULL, share_expires = NULL WHERE id = ?1 AND user_id = ?2");
    exec(&app.db, &sql, params![id, user.id.as_str()]).await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn share(app: St, user: User, Path(id): Path<String>, Json(r): Json<ShareReq>) -> Res<Json<Value>> {
    set_share(&app, "files", &id, &user, r.expires).await
}

pub async fn unshare(app: St, user: User, Path(id): Path<String>) -> Res<Json<Value>> {
    clear_share(&app, "files", &id, &user).await
}

#[cfg(test)]
mod tests {
    use super::numbered;

    #[test]
    fn numbers_duplicate_names() {
        assert_eq!(numbered("report.pdf", 1), "report (1).pdf");
        assert_eq!(numbered("archive.tar.gz", 2), "archive.tar (2).gz");
        assert_eq!(numbered(".env", 1), ".env (1)");
        assert_eq!(numbered("README", 3), "README (3)");
    }
}
