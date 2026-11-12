use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::Path;
use libsql::params;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::auth::User;
use crate::templates::reminder_email;
use crate::util::{Res, all, archive_set, bad, exec, uuid};
use crate::{App, St};

#[derive(Serialize, Deserialize)]
pub struct Reminder {
    id: String,
    title: String,
    description: String,
    remind_at: i64,
    note_id: Option<String>,
    task_id: Option<String>,
    project_id: Option<String>,
    sent: i64,
    archived: i64,
}

pub const REMINDER_COLUMNS: &str = "r.id, r.title, r.description, r.remind_at, r.note_id, r.task_id, \
     (SELECT t.project_id FROM tasks t WHERE t.id = r.task_id) AS project_id, r.sent, r.archived";

pub async fn list(app: St, user: User) -> Res<Json<Vec<Reminder>>> {
    let rows = all(
        &app.db,
        &format!("SELECT {REMINDER_COLUMNS} FROM reminders r WHERE r.user_id = ?1 ORDER BY r.sent, r.remind_at"),
        params![user.id.as_str()],
    )
    .await?;
    Ok(Json(rows))
}

#[derive(Deserialize)]
pub struct ReminderReq {
    title: String,
    #[serde(default)]
    description: String,
    remind_at: i64,
    #[serde(default)]
    utc_offset: i64,
    note_id: Option<String>,
    task_id: Option<String>,
}

impl ReminderReq {
    fn validated_title(&self) -> Res<&str> {
        if self.utc_offset.abs() > 18 * 60 {
            return Err(bad("Invalid time zone"));
        }
        let title = self.title.trim();
        if title.is_empty() { Err(bad("Title is required")) } else { Ok(title) }
    }
}

pub async fn create(app: St, user: User, Json(r): Json<ReminderReq>) -> Res<Json<Value>> {
    app.db
        .execute(
            "INSERT INTO reminders (id, user_id, title, remind_at, note_id, description, utc_offset, task_id) \
             SELECT ?5, ?1, ?2, ?3, ?4, ?6, ?7, ?8 \
             WHERE (?4 IS NULL OR EXISTS (SELECT 1 FROM notes WHERE id = ?4 AND user_id = ?1)) \
             AND (?8 IS NULL OR EXISTS (SELECT 1 FROM tasks t JOIN projects p ON p.id = t.project_id WHERE t.id = ?8 AND p.user_id = ?1))",
            params![
                user.id.as_str(),
                r.validated_title()?,
                r.remind_at,
                r.note_id.clone(),
                uuid(),
                r.description.trim(),
                r.utc_offset,
                r.task_id.clone()
            ],
        )
        .await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn update(app: St, user: User, Path(id): Path<String>, Json(r): Json<ReminderReq>) -> Res<Json<Value>> {
    exec(
        &app.db,
        "UPDATE reminders SET title = ?3, description = ?4, utc_offset = ?5, remind_at = ?6, retry_at = NULL, \
         sent = CASE WHEN remind_at != ?6 AND ?6 > unixepoch() THEN 0 ELSE sent END \
         WHERE id = ?1 AND user_id = ?2",
        params![id.as_str(), user.id.as_str(), r.validated_title()?, r.description.trim(), r.utc_offset, r.remind_at],
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn action(app: St, user: User, Path((id, action)): Path<(String, String)>) -> Res<Json<Value>> {
    let set = match action.as_str() {
        "done" => "sent = 1",
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
pub struct Due {
    id: String,
    pub title: String,
    pub description: String,
    remind_at: i64,
    pub when: String,
    pub utc_offset: i64,
    pub note_id: Option<String>,
    pub note_title: Option<String>,
    pub task_id: Option<String>,
    project_id: Option<String>,
    pub task_title: Option<String>,
    pub project_name: Option<String>,
    email: String,
}

impl Due {
    pub fn path(&self) -> String {
        match (&self.project_id, &self.task_id, &self.note_id) {
            (Some(project), Some(task), _) => format!("/projects/{project}?task={task}"),
            (_, _, Some(note)) => format!("/notes/{note}"),
            _ => "/reminders".to_owned(),
        }
    }
}

async fn tick(app: &App) -> Res<()> {
    let due: Vec<Due> = all(
        &app.db,
        "SELECT r.id, r.title, r.description, r.remind_at, r.utc_offset, n.id AS note_id, u.email, \
         IFNULL(strftime('%Y-%m-%d %H:%M', r.remind_at + r.utc_offset * 60, 'unixepoch'), '') AS \"when\", \
         CASE WHEN n.secret = 0 THEN n.title END AS note_title, \
         t.id AS task_id, t.title AS task_title, p.id AS project_id, p.name AS project_name \
         FROM reminders r JOIN users u ON u.id = r.user_id \
         LEFT JOIN notes n ON n.id = r.note_id AND n.trashed_at IS NULL \
         LEFT JOIN tasks t ON t.id = r.task_id LEFT JOIN projects p ON p.id = t.project_id \
         WHERE r.sent = 0 AND r.archived = 0 AND r.remind_at <= unixepoch() AND IFNULL(r.retry_at, 0) <= unixepoch()",
        (),
    )
    .await?;
    for d in due {
        let html = reminder_email(&d, &app.url(&d.path()), &app.url("/icon-192.png"), &app.cfg.public_url);
        let sql = if app.mail(&d.email, &format!("Reminder: {}", d.title), html).await {
            "UPDATE reminders SET sent = 1, retry_at = NULL WHERE id = ?1 AND remind_at = ?2"
        } else {
            "UPDATE reminders SET retry_at = unixepoch() + 600 WHERE id = ?1 AND remind_at = ?2"
        };
        if let Err(e) = app.db.execute(sql, params![d.id.as_str(), d.remind_at]).await {
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
