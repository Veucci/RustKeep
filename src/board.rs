use axum::Json;
use axum::extract::{Path, Request};
use axum::response::Response;
use libsql::params;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::St;
use crate::auth::User;
use crate::files::{Stored, serve};
use crate::fields::{FIELD_SELECT, Field};
use crate::folders::PublicFile;
use crate::projects::find;
use crate::util::{Res, all, archive_set, bad, exec, one, uuid};

pub const OWNED: &str = "project_id IN (SELECT id FROM projects WHERE user_id = ?2)";
const OWNED_TASKS: &str = "(SELECT t.id FROM tasks t JOIN projects p ON p.id = t.project_id WHERE p.user_id = ?2)";
const COLUMN_IN_PROJECT: &str = "EXISTS (SELECT 1 FROM board_columns c JOIN projects p ON p.id = c.project_id \
     WHERE c.id = ?3 AND p.id = ?1 AND p.user_id = ?2)";

#[derive(Serialize, Deserialize)]
pub struct Column {
    id: String,
    project_id: String,
    name: String,
    position: i64,
    done: i64,
}

const COLUMN_SELECT: &str =
    "SELECT c.id, c.project_id, c.name, c.position, c.done FROM board_columns c JOIN projects p ON p.id = c.project_id WHERE p.user_id = ?1";

pub async fn columns(app: St, user: User, Path(id): Path<String>) -> Res<Json<Vec<Column>>> {
    let sql = format!("{COLUMN_SELECT} AND p.id = ?2 ORDER BY c.position");
    Ok(Json(all(&app.db, &sql, params![user.id.as_str(), id.as_str()]).await?))
}

pub async fn overview_columns(app: St, user: User) -> Res<Json<Vec<Column>>> {
    let sql = format!("{COLUMN_SELECT} AND p.archived = 0 ORDER BY c.position, p.created");
    Ok(Json(all(&app.db, &sql, params![user.id.as_str()]).await?))
}

#[derive(Deserialize)]
pub struct NameReq {
    name: String,
}

impl NameReq {
    pub fn required(&self, missing: &str) -> Res<&str> {
        let name = self.name.trim();
        if name.is_empty() { Err(bad(missing)) } else { Ok(name) }
    }
}

pub async fn create_column(app: St, user: User, Path(id): Path<String>, Json(r): Json<NameReq>) -> Res<Json<Value>> {
    find(&app, &user, &id).await?;
    app.db
        .execute(
            "INSERT INTO board_columns (id, project_id, name, position) \
             SELECT ?3, ?1, ?2, COALESCE(MAX(position) + 1, 0) FROM board_columns WHERE project_id = ?1",
            params![id.as_str(), r.required("Column name is required")?, uuid()],
        )
        .await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn rename_column(app: St, user: User, Path(id): Path<String>, Json(r): Json<NameReq>) -> Res<Json<Value>> {
    let sql = format!("UPDATE board_columns SET name = ?3 WHERE id = ?1 AND {OWNED}");
    exec(&app.db, &sql, params![id.as_str(), user.id.as_str(), r.required("Column name is required")?]).await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn remove_column(app: St, user: User, Path(id): Path<String>) -> Res<Json<Value>> {
    exec(&app.db, &format!("DELETE FROM board_columns WHERE id = ?1 AND {OWNED}"), params![id.as_str(), user.id.as_str()]).await?;
    for sql in [
        "DELETE FROM reminders WHERE task_id IN (SELECT id FROM tasks WHERE column_id = ?1)",
        "DELETE FROM task_field_values WHERE task_id IN (SELECT id FROM tasks WHERE column_id = ?1)",
        "DELETE FROM subtasks WHERE task_id IN (SELECT id FROM tasks WHERE column_id = ?1)",
        "DELETE FROM tasks WHERE column_id = ?1",
    ] {
        app.db.execute(sql, params![id.as_str()]).await?;
    }
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct ColumnDoneReq {
    done: bool,
}

pub async fn set_column_done(app: St, user: User, Path(id): Path<String>, Json(r): Json<ColumnDoneReq>) -> Res<Json<Value>> {
    let sql = format!("UPDATE board_columns SET done = ?3 WHERE id = ?1 AND {OWNED}");
    exec(&app.db, &sql, params![id.as_str(), user.id.as_str(), r.done as i64]).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct Located {
    project_id: String,
}

#[derive(Deserialize)]
pub struct ColumnMoveReq {
    index: usize,
}

async fn renumber(db: &libsql::Connection, sql: &str, order: &[String]) -> Res<()> {
    let tx = db.transaction().await?;
    for (pos, id) in order.iter().enumerate() {
        tx.execute(sql, params![pos as i64, id.as_str()]).await?;
    }
    tx.commit().await?;
    Ok(())
}

async fn ids(db: &libsql::Connection, sql: &str, parent: &str, moved: &str) -> Res<Vec<String>> {
    Ok(all::<Row>(db, sql, params![parent, moved]).await?.into_iter().map(|r| r.id).collect())
}

pub async fn move_column(app: St, user: User, Path(id): Path<String>, Json(r): Json<ColumnMoveReq>) -> Res<Json<Value>> {
    let sql = format!("SELECT project_id FROM board_columns WHERE id = ?1 AND {OWNED}");
    let column: Located = one(&app.db, &sql, params![id.as_str(), user.id.as_str()]).await?;
    let others = "SELECT id FROM board_columns WHERE project_id = ?1 AND id != ?2 ORDER BY position, rowid";
    let mut order = ids(&app.db, others, &column.project_id, &id).await?;
    order.insert(r.index.min(order.len()), id);
    renumber(&app.db, "UPDATE board_columns SET position = ?1 WHERE id = ?2", &order).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Serialize, Deserialize)]
pub struct Task {
    id: String,
    column_id: String,
    project_id: String,
    project: String,
    title: String,
    description: String,
    due: Option<String>,
    created: i64,
    priority: i64,
    archived: i64,
    subtasks: i64,
    subtasks_done: i64,
    #[serde(deserialize_with = "crate::fields::json_text")]
    fields: Value,
}

const TASK_SELECT: &str = "SELECT t.id, t.column_id, t.project_id, p.name AS project, t.title, t.description, t.due, \
     t.created, t.priority, t.archived, \
     (SELECT COUNT(*) FROM subtasks s WHERE s.task_id = t.id) AS subtasks, \
     (SELECT COUNT(*) FROM subtasks s WHERE s.task_id = t.id AND s.done = 1) AS subtasks_done, \
     (SELECT json_group_array(json_object('field_id', v.field_id, 'value', v.value)) FROM task_field_values v WHERE v.task_id = t.id) AS fields \
     FROM tasks t JOIN projects p ON p.id = t.project_id WHERE p.user_id = ?1";

pub async fn tasks(app: St, user: User, Path(id): Path<String>) -> Res<Json<Vec<Task>>> {
    let sql = format!("{TASK_SELECT} AND p.id = ?2 ORDER BY t.position, t.created");
    Ok(Json(all(&app.db, &sql, params![user.id.as_str(), id.as_str()]).await?))
}

pub async fn overview_tasks(app: St, user: User) -> Res<Json<Vec<Task>>> {
    let sql = format!("{TASK_SELECT} AND p.archived = 0 ORDER BY t.position, t.created");
    Ok(Json(all(&app.db, &sql, params![user.id.as_str()]).await?))
}

#[derive(Deserialize)]
pub struct TaskReq {
    title: String,
    #[serde(default)]
    description: String,
    column_id: String,
    due: Option<String>,
    #[serde(default)]
    priority: i64,
    fields: Option<Vec<crate::fields::FieldValue>>,
}

const NEXT_POSITION: &str = "(SELECT COALESCE(MAX(position) + 1, 0) FROM tasks WHERE column_id = ?3)";

pub async fn create_task(app: St, user: User, Path(id): Path<String>, Json(r): Json<TaskReq>) -> Res<Json<Value>> {
    if r.title.trim().is_empty() {
        return Err(bad("Task title is required"));
    }
    let sql = format!(
        "INSERT INTO tasks (id, project_id, column_id, title, description, due, priority, position) \
         SELECT ?8, ?1, ?3, ?4, ?5, ?6, ?7, {NEXT_POSITION} WHERE {COLUMN_IN_PROJECT}"
    );
    let p = params![id.as_str(), user.id.as_str(), r.column_id, r.title.trim(), r.description, r.due, r.priority.clamp(0, 3), uuid()];
    exec(&app.db, &sql, p).await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn update_task(app: St, user: User, Path(id): Path<String>, Json(r): Json<TaskReq>) -> Res<Json<Value>> {
    if r.title.trim().is_empty() {
        return Err(bad("Task title is required"));
    }
    let sql = format!(
        "UPDATE tasks SET position = CASE WHEN column_id = ?3 THEN position ELSE {NEXT_POSITION} END, \
         column_id = ?3, title = ?4, description = ?5, due = ?6, priority = ?7 WHERE id = ?1 AND {OWNED} \
         AND ?3 IN (SELECT c.id FROM board_columns c WHERE c.project_id = tasks.project_id)"
    );
    let p = params![id.as_str(), user.id.as_str(), r.column_id, r.title.trim(), r.description, r.due, r.priority.clamp(0, 3)];
    exec(&app.db, &sql, p).await?;
    if let Some(values) = &r.fields {
        crate::fields::replace_values(&app.db, &id, values).await?;
    }
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct MoveReq {
    column_id: String,
    before: Option<String>,
}

#[derive(Deserialize)]
struct Row {
    id: String,
}

pub async fn move_task(app: St, user: User, Path(id): Path<String>, Json(r): Json<MoveReq>) -> Res<Json<Value>> {
    let sql = format!(
        "UPDATE tasks SET column_id = ?3 WHERE id = ?1 AND {OWNED} \
         AND ?3 IN (SELECT c.id FROM board_columns c WHERE c.project_id = tasks.project_id)"
    );
    exec(&app.db, &sql, params![id.as_str(), user.id.as_str(), r.column_id.as_str()]).await?;
    let others = "SELECT id FROM tasks WHERE column_id = ?1 AND id != ?2 ORDER BY position, created";
    let mut order = ids(&app.db, others, &r.column_id, &id).await?;
    let at = r.before.and_then(|b| order.iter().position(|t| *t == b)).unwrap_or(order.len());
    order.insert(at, id);
    renumber(&app.db, "UPDATE tasks SET position = ?1 WHERE id = ?2", &order).await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn task_action(app: St, user: User, Path((id, action)): Path<(String, String)>) -> Res<Json<Value>> {
    let sql = format!("UPDATE tasks SET {} WHERE id = ?1 AND {OWNED}", archive_set(&action)?);
    exec(&app.db, &sql, params![id.as_str(), user.id.as_str()]).await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn remove_task(app: St, user: User, Path(id): Path<String>) -> Res<Json<Value>> {
    exec(&app.db, &format!("DELETE FROM tasks WHERE id = ?1 AND {OWNED}"), params![id.as_str(), user.id.as_str()]).await?;
    app.db.execute("DELETE FROM subtasks WHERE task_id = ?1", params![id.as_str()]).await?;
    app.db.execute("DELETE FROM reminders WHERE task_id = ?1", params![id.as_str()]).await?;
    app.db.execute("DELETE FROM task_field_values WHERE task_id = ?1", params![id.as_str()]).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Serialize, Deserialize)]
pub struct Subtask {
    id: String,
    task_id: String,
    title: String,
    done: i64,
    description: String,
    due: Option<String>,
    priority: i64,
}

pub async fn subtasks(app: St, user: User, Path(id): Path<String>) -> Res<Json<Vec<Subtask>>> {
    let sql = format!("SELECT id, task_id, title, done, description, due, priority FROM subtasks WHERE task_id = ?1 AND task_id IN {OWNED_TASKS} ORDER BY position, created");
    Ok(Json(all(&app.db, &sql, params![id.as_str(), user.id.as_str()]).await?))
}

#[derive(Deserialize)]
pub struct SubtaskReq {
    title: String,
    #[serde(default)]
    done: bool,
    #[serde(default)]
    description: String,
    due: Option<String>,
    #[serde(default)]
    priority: i64,
}

impl SubtaskReq {
    fn clean_title(&self) -> Res<&str> {
        let title = self.title.trim();
        if title.is_empty() { Err(bad("Subtask title is required")) } else { Ok(title) }
    }
}

pub async fn create_subtask(app: St, user: User, Path(id): Path<String>, Json(r): Json<SubtaskReq>) -> Res<Json<Value>> {
    let sql = format!(
        "INSERT INTO subtasks (id, task_id, title, position) \
         SELECT ?3, ?1, ?4, (SELECT COALESCE(MAX(position) + 1, 0) FROM subtasks WHERE task_id = ?1) WHERE ?1 IN {OWNED_TASKS}"
    );
    exec(&app.db, &sql, params![id.as_str(), user.id.as_str(), uuid(), r.clean_title()?]).await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn update_subtask(app: St, user: User, Path(id): Path<String>, Json(r): Json<SubtaskReq>) -> Res<Json<Value>> {
    let sql = format!(
        "UPDATE subtasks SET title = ?3, done = ?4, description = ?5, due = ?6, priority = ?7 WHERE id = ?1 AND task_id IN {OWNED_TASKS}"
    );
    let p = params![id.as_str(), user.id.as_str(), r.clean_title()?, r.done as i64, r.description, r.due, r.priority.clamp(0, 3)];
    exec(&app.db, &sql, p).await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn remove_subtask(app: St, user: User, Path(id): Path<String>) -> Res<Json<Value>> {
    let sql = format!("DELETE FROM subtasks WHERE id = ?1 AND task_id IN {OWNED_TASKS}");
    exec(&app.db, &sql, params![id.as_str(), user.id.as_str()]).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct SharedProject {
    id: String,
    user_id: String,
    name: String,
    description: String,
}

const SHARED_PROJECT: &str = "SELECT id, user_id, name, description FROM projects \
     WHERE share_token = ?1 AND (share_expires IS NULL OR share_expires > unixepoch())";

pub async fn public_board(app: St, Path(t): Path<String>) -> Res<Json<Value>> {
    let p: SharedProject = one(&app.db, SHARED_PROJECT, params![t.as_str()]).await?;
    let owner = || params![p.user_id.as_str(), p.id.as_str()];
    let columns: Vec<Column> = all(&app.db, &format!("{COLUMN_SELECT} AND p.id = ?2 ORDER BY c.position"), owner()).await?;
    let fields: Vec<Field> = all(&app.db, FIELD_SELECT, owner()).await?;
    let sql = format!("{TASK_SELECT} AND p.id = ?2 AND t.archived = 0 ORDER BY t.position, t.created");
    let mut tasks: Vec<Task> = all(&app.db, &sql, owner()).await?;
    let mut subtasks: Vec<Subtask> = all(
        &app.db,
        "SELECT s.id, s.task_id, s.title, s.done, s.description, s.due, s.priority FROM subtasks s \
         JOIN tasks t ON t.id = s.task_id WHERE t.project_id = ?1 AND t.archived = 0 ORDER BY s.position, s.created",
        params![p.id.as_str()],
    )
    .await?;
    let files: Vec<PublicFile> = all(
        &app.db,
        "SELECT id, name, mime, size, created, folder_id FROM files WHERE user_id = ?1 AND project_id = ?2 AND archived = 0 ORDER BY name",
        owner(),
    )
    .await?;
    let public = format!("/api/public/projects/{t}/files/");
    for d in tasks.iter_mut().map(|x| &mut x.description).chain(subtasks.iter_mut().map(|x| &mut x.description)) {
        *d = d.replace("/api/files/", &public);
    }
    Ok(Json(json!({
        "name": p.name, "description": p.description, "columns": columns, "fields": fields, "tasks": tasks, "subtasks": subtasks, "files": files,
    })))
}

pub async fn public_raw(app: St, Path((t, id)): Path<(String, String)>, req: Request) -> Res<Response> {
    let sql = format!("SELECT f.name, f.mime, f.key FROM files f JOIN ({SHARED_PROJECT}) p ON p.id = f.project_id AND p.user_id = f.user_id WHERE f.id = ?2");
    let f: Stored = one(&app.db, &sql, params![t.as_str(), id.as_str()]).await?;
    serve(&app, f, req).await
}
