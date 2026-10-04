use std::path::PathBuf;

use axum::Json;
use axum::body::Body;
use axum::extract::multipart::Field;
use axum::extract::{Multipart, Path, Query, Request};
use axum::http::{HeaderValue, header};
use axum::response::Response;
use libsql::params;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;
use tower::ServiceExt;
use tower_http::services::ServeFile;

use crate::auth::User;
use crate::util::{Res, all, archive_set, bad, exec, one, token, uuid};
use crate::{App, St};

#[derive(Deserialize)]
pub struct ListQ {
    project: Option<String>,
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
}

pub async fn list(app: St, user: User, Query(q): Query<ListQ>) -> Res<Json<Vec<FileItem>>> {
    let rows = all(
        &app.db,
        "SELECT id, name, mime, size, project_id, created, share_token, share_expires, archived FROM files \
         WHERE user_id = ?1 AND (?2 IS NULL OR project_id = ?2) ORDER BY created DESC",
        params![user.id.as_str(), q.project],
    )
    .await?;
    Ok(Json(rows))
}

fn path_of(app: &App, key: &str) -> PathBuf {
    app.cfg.data_dir.join("files").join(key)
}

async fn write_field(field: &mut Field<'_>, path: &PathBuf) -> Res<i64> {
    let mut f = tokio::fs::File::create(path).await?;
    let mut size = 0;
    while let Some(chunk) = field.chunk().await? {
        size += chunk.len() as i64;
        f.write_all(&chunk).await?;
    }
    f.flush().await?;
    Ok(size)
}

pub async fn upload(app: St, user: User, Query(q): Query<ListQ>, mut mp: Multipart) -> Res<Json<Vec<Value>>> {
    let mut saved = Vec::new();
    while let Some(mut field) = mp.next_field().await? {
        let Some(name) = field.file_name().map(str::to_owned) else { continue };
        let mime = field.content_type().unwrap_or("application/octet-stream").to_owned();
        let key = token();
        let path = path_of(&app, &key);
        let size = match write_field(&mut field, &path).await {
            Ok(s) => s,
            Err(e) => {
                let _ = tokio::fs::remove_file(&path).await;
                return Err(e);
            }
        };
        let id = uuid();
        app.db
            .execute(
                "INSERT INTO files (id, user_id, project_id, name, mime, size, key) \
                 VALUES (?1, ?2, (SELECT id FROM projects WHERE id = ?3 AND user_id = ?2), ?4, ?5, ?6, ?7)",
                params![id.as_str(), user.id.as_str(), q.project.as_deref(), name.clone(), mime, size, key],
            )
            .await?;
        saved.push(json!({ "id": id, "name": name }));
    }
    if saved.is_empty() {
        return Err(bad("No file in request"));
    }
    Ok(Json(saved))
}

#[derive(Deserialize)]
struct Stored {
    name: String,
    mime: String,
    key: String,
}

fn disposition(name: &str) -> String {
    let enc: String = name
        .bytes()
        .map(|b| match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect();
    format!("inline; filename*=UTF-8''{enc}")
}

async fn serve(app: &App, f: Stored, req: Request) -> Res<Response> {
    let mut res = ServeFile::new(path_of(app, &f.key)).oneshot(req).await?;
    let h = res.headers_mut();
    let mime = HeaderValue::from_str(&f.mime).unwrap_or(HeaderValue::from_static("application/octet-stream"));
    h.insert(header::CONTENT_TYPE, mime);
    if let Ok(v) = HeaderValue::from_str(&disposition(&f.name)) {
        h.insert(header::CONTENT_DISPOSITION, v);
    }
    h.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static("sandbox"));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    Ok(res.map(Body::new))
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

pub async fn action(app: St, user: User, Path((id, action)): Path<(String, String)>) -> Res<Json<Value>> {
    let sql = format!("UPDATE files SET {} WHERE id = ?1 AND user_id = ?2", archive_set(&action)?);
    exec(&app.db, &sql, params![id.as_str(), user.id.as_str()]).await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn remove(app: St, user: User, Path(id): Path<String>) -> Res<Json<Value>> {
    let f: Stored = one(&app.db, "SELECT name, mime, key FROM files WHERE id = ?1 AND user_id = ?2", params![id.as_str(), user.id.as_str()]).await?;
    exec(&app.db, "DELETE FROM files WHERE id = ?1", params![id.as_str()]).await?;
    let _ = tokio::fs::remove_file(path_of(&app, &f.key)).await;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct ShareReq {
    expires: Option<i64>,
}

pub async fn share(app: St, user: User, Path(id): Path<String>, Json(r): Json<ShareReq>) -> Res<Json<Value>> {
    let t = token();
    exec(
        &app.db,
        "UPDATE files SET share_token = ?1, share_expires = ?2 WHERE id = ?3 AND user_id = ?4",
        params![t.clone(), r.expires, id, user.id.as_str()],
    )
    .await?;
    Ok(Json(json!({ "token": t })))
}

pub async fn unshare(app: St, user: User, Path(id): Path<String>) -> Res<Json<Value>> {
    exec(
        &app.db,
        "UPDATE files SET share_token = NULL, share_expires = NULL WHERE id = ?1 AND user_id = ?2",
        params![id.as_str(), user.id.as_str()],
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}
