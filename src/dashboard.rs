use axum::Json;
use libsql::params;
use serde::{Deserialize, Serialize};

use crate::St;
use crate::auth::User;
use crate::files::FileItem;
use crate::notes::NoteItem;
use crate::projects::{PROJECT_SELECT, Project};
use crate::reminders::Reminder;
use crate::util::{Res, all, one};

#[derive(Serialize, Deserialize)]
pub struct Stats {
    notes: i64,
    archived: i64,
    trash: i64,
    files: i64,
    storage: i64,
    reminders: i64,
    vault: i64,
}

#[derive(Serialize)]
pub struct Dashboard {
    stats: Stats,
    notes: Vec<NoteItem>,
    reminders: Vec<Reminder>,
    files: Vec<FileItem>,
    projects: Vec<Project>,
}

const STATS: &str = "SELECT \
    (SELECT COUNT(*) FROM notes WHERE user_id = ?1 AND secret = 0 AND archived = 0 AND trashed_at IS NULL) AS notes, \
    (SELECT COUNT(*) FROM notes WHERE user_id = ?1 AND secret = 0 AND archived = 1 AND trashed_at IS NULL) AS archived, \
    (SELECT COUNT(*) FROM notes WHERE user_id = ?1 AND secret = 0 AND trashed_at IS NOT NULL) AS trash, \
    (SELECT COUNT(*) FROM files WHERE user_id = ?1) AS files, \
    (SELECT COALESCE(SUM(size), 0) FROM files WHERE user_id = ?1) AS storage, \
    (SELECT COUNT(*) FROM reminders WHERE user_id = ?1 AND sent = 0) AS reminders, \
    (SELECT COUNT(*) FROM vault WHERE user_id = ?1) AS vault";

pub async fn get(app: St, user: User) -> Res<Json<Dashboard>> {
    let db = &app.db;
    let u = user.id;
    Ok(Json(Dashboard {
        stats: one(db, STATS, params![u]).await?,
        notes: all(
            db,
            "SELECT id, title, project_id, archived, updated, share_token FROM notes \
             WHERE user_id = ?1 AND secret = 0 AND archived = 0 AND trashed_at IS NULL ORDER BY updated DESC LIMIT 6",
            params![u],
        )
        .await?,
        reminders: all(
            db,
            "SELECT id, title, remind_at, note_id, sent FROM reminders WHERE user_id = ?1 AND sent = 0 ORDER BY remind_at LIMIT 5",
            params![u],
        )
        .await?,
        files: all(
            db,
            "SELECT id, name, mime, size, project_id, created, share_token, share_expires FROM files \
             WHERE user_id = ?1 ORDER BY created DESC LIMIT 5",
            params![u],
        )
        .await?,
        projects: all(db, &format!("{PROJECT_SELECT} WHERE p.user_id = ?1 ORDER BY p.created DESC"), params![u]).await?,
    }))
}
