use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::Path;
use libsql::params;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::auth::User;
use crate::util::{Res, all, archive_set, bad, esc, exec, uuid};
use crate::{App, St};

#[derive(Serialize, Deserialize)]
pub struct Reminder {
    id: String,
    title: String,
    remind_at: i64,
    note_id: Option<String>,
    sent: i64,
    archived: i64,
}

pub async fn list(app: St, user: User) -> Res<Json<Vec<Reminder>>> {
    let rows = all(
        &app.db,
        "SELECT id, title, remind_at, note_id, sent, archived FROM reminders WHERE user_id = ?1 ORDER BY sent, remind_at",
        params![user.id.as_str()],
    )
    .await?;
    Ok(Json(rows))
}

#[derive(Deserialize)]
pub struct CreateReq {
    title: String,
    remind_at: i64,
    note_id: Option<String>,
}

pub async fn create(app: St, user: User, Json(r): Json<CreateReq>) -> Res<Json<Value>> {
    if r.title.trim().is_empty() {
        return Err(bad("Title is required"));
    }
    app.db
        .execute(
            "INSERT INTO reminders (id, user_id, title, remind_at, note_id) \
             SELECT ?5, ?1, ?2, ?3, ?4 WHERE ?4 IS NULL OR EXISTS (SELECT 1 FROM notes WHERE id = ?4 AND user_id = ?1)",
            params![user.id.as_str(), r.title.trim(), r.remind_at, r.note_id, uuid()],
        )
        .await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn action(app: St, user: User, Path((id, action)): Path<(String, String)>) -> Res<Json<Value>> {
    let set = match action.as_str() {
        "done" => "sent = 1",
        "snooze" => "sent = 0, remind_at = MAX(remind_at, unixepoch()) + 86400",
        other => archive_set(other)?,
    };
    exec(&app.db, &format!("UPDATE reminders SET {set} WHERE id = ?1 AND user_id = ?2"), params![id.as_str(), user.id.as_str()]).await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn remove(app: St, user: User, Path(id): Path<String>) -> Res<Json<Value>> {
    exec(&app.db, "DELETE FROM reminders WHERE id = ?1 AND user_id = ?2", params![id.as_str(), user.id.as_str()]).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct Due {
    id: String,
    title: String,
    note_id: Option<String>,
    email: String,
}

async fn tick(app: &App) -> Res<()> {
    let due: Vec<Due> = all(
        &app.db,
        "SELECT r.id, r.title, r.note_id, u.email FROM reminders r JOIN users u ON u.id = r.user_id \
         WHERE r.sent = 0 AND r.archived = 0 AND r.remind_at <= unixepoch()",
        (),
    )
    .await?;
    for d in due {
        let link = app.url(&d.note_id.map(|id| format!("/notes/{id}")).unwrap_or_default());
        let html = format!("<h2>{}</h2><p><a href=\"{link}\">Open in RustKeep</a></p>", esc(&d.title));
        let sql = if app.mail(&d.email, &format!("Reminder: {}", d.title), html).await {
            "UPDATE reminders SET sent = 1 WHERE id = ?1"
        } else {
            "UPDATE reminders SET remind_at = unixepoch() + 600 WHERE id = ?1"
        };
        if let Err(e) = app.db.execute(sql, params![d.id.as_str()]).await {
            eprintln!("reminder {} update failed: {e}", d.id);
        }
    }
    Ok(())
}

pub async fn run(app: Arc<App>) {
    let mut every = tokio::time::interval(Duration::from_secs(30));
    loop {
        every.tick().await;
        if let Err(e) = tick(&app).await {
            eprintln!("reminder tick failed: {}", e.1);
        }
    }
}
