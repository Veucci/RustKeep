use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::Path;
use libsql::params;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::auth::User;
use crate::util::{Res, all, bad, esc, exec};
use crate::{App, St};

#[derive(Serialize, Deserialize)]
pub struct Reminder {
    id: i64,
    title: String,
    remind_at: i64,
    note_id: Option<String>,
    sent: i64,
}

pub async fn list(app: St, user: User) -> Res<Json<Vec<Reminder>>> {
    let rows = all(
        &app.db,
        "SELECT id, title, remind_at, note_id, sent FROM reminders WHERE user_id = ?1 ORDER BY sent, remind_at",
        params![user.id],
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
            "INSERT INTO reminders (user_id, title, remind_at, note_id) \
             SELECT ?1, ?2, ?3, ?4 WHERE ?4 IS NULL OR EXISTS (SELECT 1 FROM notes WHERE id = ?4 AND user_id = ?1)",
            params![user.id, r.title.trim(), r.remind_at, r.note_id],
        )
        .await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn remove(app: St, user: User, Path(id): Path<i64>) -> Res<Json<Value>> {
    exec(&app.db, "DELETE FROM reminders WHERE id = ?1 AND user_id = ?2", params![id, user.id]).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct Due {
    id: i64,
    title: String,
    note_id: Option<String>,
    email: String,
}

async fn tick(app: &App) -> Res<()> {
    let due: Vec<Due> = all(
        &app.db,
        "SELECT r.id, r.title, r.note_id, u.email FROM reminders r JOIN users u ON u.id = r.user_id \
         WHERE r.sent = 0 AND r.remind_at <= unixepoch()",
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
        if let Err(e) = app.db.execute(sql, params![d.id]).await {
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
