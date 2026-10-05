use axum::Json;
use libsql::params;
use serde::{Deserialize, Serialize};

use crate::St;
use crate::auth::User;
use crate::files::{FILE_COLUMNS, FileItem};
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
    overdue: i64,
}

#[derive(Serialize, Deserialize)]
pub struct DueTask {
    id: String,
    title: String,
    due: String,
    priority: i64,
    project_id: String,
    project: String,
}

#[derive(Serialize)]
pub struct Dashboard {
    stats: Stats,
    notes: Vec<NoteItem>,
    reminders: Vec<Reminder>,
    files: Vec<FileItem>,
    projects: Vec<Project>,
    tasks: Vec<DueTask>,
}

const OPEN_TASK: &str = "FROM tasks t JOIN projects p ON p.id = t.project_id \
    WHERE p.user_id = ?1 AND p.archived = 0 AND t.due IS NOT NULL AND t.column_id != \
    (SELECT c.id FROM board_columns c WHERE c.project_id = p.id ORDER BY c.position DESC LIMIT 1)";

const STATS: &str = "SELECT \
    (SELECT COUNT(*) FROM notes WHERE user_id = ?1 AND secret = 0 AND archived = 0 AND trashed_at IS NULL) AS notes, \
    (SELECT COUNT(*) FROM notes WHERE user_id = ?1 AND secret = 0 AND archived = 1 AND trashed_at IS NULL) AS archived, \
    (SELECT COUNT(*) FROM notes WHERE user_id = ?1 AND secret = 0 AND trashed_at IS NOT NULL) AS trash, \
    (SELECT COUNT(*) FROM files WHERE user_id = ?1 AND archived = 0) AS files, \
    (SELECT COALESCE(SUM(size), 0) FROM files WHERE user_id = ?1) AS storage, \
    (SELECT COUNT(*) FROM reminders WHERE user_id = ?1 AND sent = 0 AND archived = 0) AS reminders, \
    (SELECT COUNT(*) FROM vault WHERE user_id = ?1) AS vault";

pub async fn get(app: St, user: User) -> Res<Json<Dashboard>> {
    let db = &app.db;
    let u = user.id.as_str();
    let stats = format!("{STATS}, (SELECT COUNT(*) {OPEN_TASK} AND t.due < date('now', 'localtime')) AS overdue");
    Ok(Json(Dashboard {
        stats: one(db, &stats, params![u]).await?,
        tasks: all(
            db,
            &format!(
                "SELECT t.id, t.title, t.due, t.priority, p.id AS project_id, p.name AS project {OPEN_TASK} \
                 AND t.due <= date('now', 'localtime', '+14 days') ORDER BY t.due, t.priority DESC LIMIT 8"
            ),
            params![u],
        )
        .await?,
        notes: all(
            db,
            "SELECT id, title, project_id, archived, updated, share_token, pinned FROM notes \
             WHERE user_id = ?1 AND secret = 0 AND archived = 0 AND trashed_at IS NULL \
             ORDER BY pinned DESC, updated DESC LIMIT 6",
            params![u],
        )
        .await?,
        reminders: all(
            db,
            "SELECT id, title, remind_at, note_id, sent, archived FROM reminders \
             WHERE user_id = ?1 AND sent = 0 AND archived = 0 ORDER BY remind_at LIMIT 5",
            params![u],
        )
        .await?,
        files: all(
            db,
            &format!("SELECT {FILE_COLUMNS} FROM files WHERE user_id = ?1 AND archived = 0 ORDER BY created DESC LIMIT 5"),
            params![u],
        )
        .await?,
        projects: all(
            db,
            &format!("{PROJECT_SELECT} WHERE p.user_id = ?1 AND p.archived = 0 ORDER BY p.created DESC"),
            params![u],
        )
        .await?,
    }))
}
