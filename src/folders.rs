use axum::Json;
use axum::extract::{Multipart, Path, Query, Request};
use axum::http::StatusCode;
use axum::response::Response;
use libsql::params;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::auth::User;
use crate::files::{Edit, ShareReq, Stored, clear_share, serve, set_share};
use crate::requests::{Origin, store};
use crate::util::{Res, all, bad, err, exec, not_found, one, uuid};
use crate::St;

const SUBTREE: &str = "WITH RECURSIVE sub(id) AS (SELECT id FROM folders WHERE id = ?1 AND user_id = ?2 \
     UNION ALL SELECT f.id FROM folders f JOIN sub ON f.parent_id = sub.id)";

const SHARED: &str = "WITH RECURSIVE sub(id) AS (SELECT id FROM folders WHERE share_token = ?1 \
     AND (share_expires IS NULL OR share_expires > unixepoch()) \
     UNION ALL SELECT f.id FROM folders f JOIN sub ON f.parent_id = sub.id)";

#[derive(Serialize, Deserialize)]
pub struct Folder {
    id: String,
    name: String,
    parent_id: Option<String>,
    created: i64,
    starred: i64,
    share_token: Option<String>,
    share_expires: Option<i64>,
    share_uploads: i64,
}

pub async fn list(app: St, user: User) -> Res<Json<Vec<Folder>>> {
    let rows = all(
        &app.db,
        "SELECT id, name, parent_id, created, starred, share_token, share_expires, share_uploads FROM folders WHERE user_id = ?1 ORDER BY name",
        params![user.id.as_str()],
    )
    .await?;
    Ok(Json(rows))
}

pub async fn create(app: St, user: User, Json(r): Json<Edit>) -> Res<Json<Value>> {
    let id = uuid();
    app.db
        .execute(
            "INSERT INTO folders (id, user_id, name, starred, parent_id) \
             VALUES (?1, ?2, ?3, ?4, (SELECT id FROM folders WHERE id = ?5 AND user_id = ?2))",
            params![id.as_str(), user.id.as_str(), r.clean_name()?, r.starred as i64, r.parent.as_deref()],
        )
        .await?;
    Ok(Json(json!({ "id": id })))
}

async fn inside(app: &St, id: &str, user: &User, target: &str) -> Res<bool> {
    let sql = format!("{SUBTREE} SELECT 1 FROM sub WHERE id = ?3");
    Ok(app.db.query(&sql, params![id, user.id.as_str(), target]).await?.next().await?.is_some())
}

pub async fn update(app: St, user: User, Path(id): Path<String>, Json(r): Json<Edit>) -> Res<Json<Value>> {
    if let Some(p) = r.parent.as_deref()
        && inside(&app, &id, &user, p).await?
    {
        return Err(bad("A folder cannot be moved into itself"));
    }
    exec(
        &app.db,
        "UPDATE folders SET name = ?1, starred = ?2, parent_id = (SELECT id FROM folders WHERE id = ?3 AND user_id = ?5) \
         WHERE id = ?4 AND user_id = ?5",
        params![r.clean_name()?, r.starred as i64, r.parent.as_deref(), id.as_str(), user.id.as_str()],
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct Key {
    key: String,
}

pub async fn remove(app: St, user: User, Path(id): Path<String>) -> Res<Json<Value>> {
    let p = || params![id.as_str(), user.id.as_str()];
    let in_tree = "folder_id IN (SELECT id FROM sub) AND user_id = ?2";
    let keys: Vec<Key> = all(&app.db, &format!("{SUBTREE} SELECT key FROM files WHERE {in_tree}"), p()).await?;
    let tx = app.db.transaction().await?;
    let links = "SELECT id FROM upload_links WHERE folder_id IN (SELECT id FROM sub) AND user_id = ?2";
    tx.execute(&format!("{SUBTREE} UPDATE files SET upload_link_id = NULL WHERE upload_link_id IN ({links})"), p()).await?;
    tx.execute(&format!("{SUBTREE} DELETE FROM upload_links WHERE id IN ({links})"), p()).await?;
    tx.execute(&format!("{SUBTREE} DELETE FROM files WHERE {in_tree}"), p()).await?;
    exec(&tx, &format!("{SUBTREE} DELETE FROM folders WHERE id IN (SELECT id FROM sub)"), p()).await?;
    tx.commit().await?;
    for k in keys {
        app.storage.remove(&k.key).await;
    }
    Ok(Json(json!({ "ok": true })))
}

pub async fn share(app: St, user: User, Path(id): Path<String>, Json(r): Json<ShareReq>) -> Res<Json<Value>> {
    let created = set_share(&app, "folders", &id, &user, r.expires).await?;
    let sql = "UPDATE folders SET share_uploads = ?1 WHERE id = ?2 AND user_id = ?3";
    exec(&app.db, sql, params![r.uploads as i64, id.as_str(), user.id.as_str()]).await?;
    Ok(created)
}

pub async fn unshare(app: St, user: User, Path(id): Path<String>) -> Res<Json<Value>> {
    clear_share(&app, "folders", &id, &user).await
}

#[derive(Serialize, Deserialize)]
struct PublicFolder {
    id: String,
    name: String,
    parent_id: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct PublicFile {
    id: String,
    name: String,
    mime: String,
    size: i64,
    created: i64,
    folder_id: Option<String>,
}

#[derive(Deserialize)]
struct SharedRoot {
    id: String,
    user_id: String,
    name: String,
    share_uploads: i64,
}

async fn shared_root(app: &St, t: &str) -> Res<SharedRoot> {
    let sql = "SELECT id, user_id, name, share_uploads FROM folders WHERE share_token = ?1 \
         AND (share_expires IS NULL OR share_expires > unixepoch())";
    one(&app.db, sql, params![t]).await
}

pub async fn public_list(app: St, Path(t): Path<String>) -> Res<Json<Value>> {
    let root = shared_root(&app, &t).await?;
    let folders: Vec<PublicFolder> =
        all(&app.db, &format!("{SHARED} SELECT id, name, parent_id FROM folders WHERE id IN (SELECT id FROM sub)"), params![t.as_str()])
            .await?;
    let files: Vec<PublicFile> = all(
        &app.db,
        &format!(
            "{SHARED} SELECT id, name, mime, size, created, folder_id FROM files \
             WHERE archived = 0 AND folder_id IN (SELECT id FROM sub) ORDER BY name"
        ),
        params![t.as_str()],
    )
    .await?;
    let uploads = root.share_uploads == 1;
    Ok(Json(json!({ "root": root.id, "name": root.name, "uploads": uploads, "folders": folders, "files": files })))
}

pub async fn public_raw(app: St, Path((t, id)): Path<(String, String)>, req: Request) -> Res<Response> {
    let sql = format!("{SHARED} SELECT name, mime, key FROM files WHERE id = ?2 AND archived = 0 AND folder_id IN (SELECT id FROM sub)");
    let f: Stored = one(&app.db, &sql, params![t.as_str(), id.as_str()]).await?;
    serve(&app, f, req).await
}

#[derive(Deserialize)]
pub struct UploadQ {
    folder: Option<String>,
}

pub async fn public_upload(app: St, Path(t): Path<String>, Query(q): Query<UploadQ>, mut mp: Multipart) -> Res<Json<Value>> {
    let root = shared_root(&app, &t).await?;
    if root.share_uploads == 0 {
        return Err(err(StatusCode::FORBIDDEN, "Uploads are not allowed on this link"));
    }
    let target = q.folder.unwrap_or(root.id);
    let sql = format!("{SHARED} SELECT 1 FROM sub WHERE id = ?2");
    if app.db.query(&sql, params![t.as_str(), target.as_str()]).await?.next().await?.is_none() {
        return Err(not_found());
    }
    let mut saved = Vec::new();
    while let Some(mut field) = mp.next_field().await? {
        if field.file_name().is_some() {
            saved.push(store(&app, &root.user_id, Some(&target), Origin::default(), &mut field).await?);
        }
    }
    if saved.is_empty() {
        return Err(bad("No file in request"));
    }
    Ok(Json(json!({ "saved": saved })))
}
