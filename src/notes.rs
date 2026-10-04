use axum::Json;
use axum::extract::{Path, Query};
use libsql::params;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{App, St};
use crate::auth::User;
use crate::util::{Res, all, bad, exec, one, token, uuid};

const SHARED: &str = "share_token = ?1 AND secret = 0 AND trashed_at IS NULL \
                      AND (share_expires IS NULL OR share_expires > unixepoch())";

#[derive(Deserialize)]
pub struct ListQ {
    view: Option<String>,
    project: Option<i64>,
    q: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct NoteItem {
    id: String,
    title: String,
    project_id: Option<i64>,
    archived: i64,
    updated: i64,
    share_token: Option<String>,
}

pub async fn list(app: St, user: User, Query(q): Query<ListQ>) -> Res<Json<Vec<NoteItem>>> {
    let filter = match q.view.as_deref() {
        Some("archived") => "archived = 1 AND trashed_at IS NULL AND secret = 0",
        Some("trash") => "trashed_at IS NOT NULL AND secret = 0",
        Some("secret") => return Ok(Json(list_secret(&app, &user, q.q).await?)),
        Some("project") => "trashed_at IS NULL AND secret = 0",
        _ => "archived = 0 AND trashed_at IS NULL AND secret = 0",
    };
    let sql = format!(
        "SELECT id, title, project_id, archived, updated, share_token FROM notes WHERE user_id = ?1 AND {filter} \
         AND (?2 IS NULL OR project_id = ?2) AND (?3 IS NULL OR title LIKE ?3 OR body LIKE ?3) \
         ORDER BY updated DESC"
    );
    let like = q.q.filter(|s| !s.is_empty()).map(|s| format!("%{s}%"));
    Ok(Json(all(&app.db, &sql, params![user.id, q.project, like]).await?))
}

fn open_title(app: &App, title: String) -> String {
    app.crypto.open(&title).unwrap_or(title)
}

async fn list_secret(app: &App, user: &User, search: Option<String>) -> Res<Vec<NoteItem>> {
    user.need_pin()?;
    let rows: Vec<NoteItem> = all(
        &app.db,
        "SELECT id, title, project_id, archived, updated, share_token FROM notes \
         WHERE user_id = ?1 AND secret = 1 AND trashed_at IS NULL ORDER BY updated DESC",
        params![user.id],
    )
    .await?;
    let needle = search.unwrap_or_default().to_lowercase();
    Ok(rows
        .into_iter()
        .map(|n| NoteItem { title: open_title(app, n.title), ..n })
        .filter(|n| n.title.to_lowercase().contains(&needle))
        .collect())
}

#[derive(Serialize, Deserialize)]
pub struct Note {
    id: String,
    title: String,
    body: String,
    project_id: Option<i64>,
    secret: i64,
    archived: i64,
    trashed_at: Option<i64>,
    updated: i64,
    share_token: Option<String>,
    share_expires: Option<i64>,
}

pub async fn get(app: St, user: User, Path(id): Path<String>) -> Res<Json<Note>> {
    let mut n: Note = one(
        &app.db,
        "SELECT id, title, body, project_id, secret, archived, trashed_at, updated, share_token, share_expires \
         FROM notes WHERE id = ?1 AND user_id = ?2",
        params![id, user.id],
    )
    .await?;
    if n.secret == 1 {
        user.need_pin()?;
        n.title = open_title(&app, n.title);
        n.body = app.crypto.open(&n.body)?;
    }
    Ok(Json(n))
}

#[derive(Deserialize)]
pub struct CreateReq {
    #[serde(default)]
    title: String,
    project_id: Option<i64>,
    #[serde(default)]
    secret: bool,
}

pub async fn create(app: St, user: User, Json(r): Json<CreateReq>) -> Res<Json<Value>> {
    let (title, body) = if r.secret {
        user.need_pin()?;
        (app.crypto.seal(&r.title), app.crypto.seal(""))
    } else {
        (r.title, String::new())
    };
    let id = uuid();
    app.db
        .execute(
            "INSERT INTO notes (id, user_id, title, body, project_id, secret) \
             VALUES (?1, ?2, ?3, ?4, (SELECT id FROM projects WHERE id = ?5 AND user_id = ?2), ?6)",
            params![id.clone(), user.id, title, body, r.project_id, r.secret as i64],
        )
        .await?;
    Ok(Json(json!({ "id": id })))
}

#[derive(Deserialize)]
pub struct UpdateReq {
    title: String,
    body: String,
    project_id: Option<i64>,
    rev: i64,
    client: String,
}

#[derive(Deserialize)]
struct SecretFlag {
    secret: i64,
}

pub async fn update(app: St, user: User, Path(id): Path<String>, Json(r): Json<UpdateReq>) -> Res<Json<Value>> {
    let f: SecretFlag = one(&app.db, "SELECT secret FROM notes WHERE id = ?1 AND user_id = ?2", params![id.clone(), user.id]).await?;
    let (title, body) = if f.secret == 1 {
        user.need_pin()?;
        (app.crypto.seal(&r.title), app.crypto.seal(&r.body))
    } else {
        (r.title, r.body)
    };
    let n = app
        .db
        .execute(
            "UPDATE notes SET title = ?1, body = ?2, project_id = (SELECT id FROM projects WHERE id = ?3 AND user_id = ?6), \
             rev = ?4, rev_client = ?7, updated = unixepoch() \
             WHERE id = ?5 AND user_id = ?6 AND (rev_client IS NOT ?7 OR rev < ?4)",
            params![title, body, r.project_id, r.rev, id, user.id, r.client],
        )
        .await?;
    Ok(Json(json!({ "saved": n > 0 })))
}

pub async fn action(app: St, user: User, Path((id, action)): Path<(String, String)>) -> Res<Json<Value>> {
    let set = match action.as_str() {
        "archive" => "archived = 1",
        "unarchive" => "archived = 0",
        "trash" => "trashed_at = unixepoch()",
        "restore" => "trashed_at = NULL",
        _ => return Err(bad("Invalid action")),
    };
    let sql = format!("UPDATE notes SET {set}, updated = unixepoch() WHERE id = ?1 AND user_id = ?2 AND secret = 0");
    exec(&app.db, &sql, params![id, user.id]).await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn remove(app: St, user: User, Path(id): Path<String>) -> Res<Json<Value>> {
    exec(
        &app.db,
        "DELETE FROM notes WHERE id = ?1 AND user_id = ?2 AND (secret = 0 OR ?3 = 1)",
        params![id.clone(), user.id, user.pin_ok as i64],
    )
    .await?;
    app.db.execute("DELETE FROM reminders WHERE note_id = ?1 AND user_id = ?2", params![id, user.id]).await?;
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
        "UPDATE notes SET share_token = ?1, share_expires = ?2 WHERE id = ?3 AND user_id = ?4 AND secret = 0",
        params![t.clone(), r.expires, id, user.id],
    )
    .await?;
    Ok(Json(json!({ "token": t })))
}

pub async fn unshare(app: St, user: User, Path(id): Path<String>) -> Res<Json<Value>> {
    exec(
        &app.db,
        "UPDATE notes SET share_token = NULL, share_expires = NULL WHERE id = ?1 AND user_id = ?2",
        params![id, user.id],
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Serialize, Deserialize)]
pub struct PublicNote {
    title: String,
    body: String,
}

pub async fn public_get(app: St, Path(t): Path<String>) -> Res<Json<PublicNote>> {
    let sql = format!("SELECT title, body FROM notes WHERE {SHARED}");
    Ok(Json(one(&app.db, &sql, params![t]).await?))
}

#[derive(Deserialize)]
pub struct PublicUpdate {
    title: String,
    body: String,
    rev: i64,
    client: String,
}

pub async fn public_update(app: St, Path(t): Path<String>, Json(r): Json<PublicUpdate>) -> Res<Json<Value>> {
    let sql = format!(
        "UPDATE notes SET title = ?2, body = ?3, rev = ?4, rev_client = ?5, updated = unixepoch() \
         WHERE {SHARED} AND (rev_client IS NOT ?5 OR rev < ?4)"
    );
    let n = app.db.execute(&sql, params![t, r.title, r.body, r.rev, r.client]).await?;
    Ok(Json(json!({ "saved": n > 0 })))
}
