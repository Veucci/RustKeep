use axum::Json;
use axum::extract::Path;
use libsql::params;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::St;
use crate::auth::User;
use crate::projects::find;
use crate::util::{Res, all, bad, exec, one, uuid};

const OWNED: &str = "project_id IN (SELECT id FROM projects WHERE user_id = ?2)";
const COLUMN_IN_PROJECT: &str = "EXISTS (SELECT 1 FROM board_columns c JOIN projects p ON p.id = c.project_id \
     WHERE c.id = ?3 AND p.id = ?1 AND p.user_id = ?2)";

#[derive(Serialize, Deserialize)]
pub struct Column {
    id: String,
    name: String,
    position: i64,
}

pub async fn columns(app: St, user: User, Path(id): Path<String>) -> Res<Json<Vec<Column>>> {
    let sql = format!("SELECT id, name, position FROM board_columns WHERE project_id = ?1 AND {OWNED} ORDER BY position");
    Ok(Json(all(&app.db, &sql, params![id.as_str(), user.id.as_str()]).await?))
}

#[derive(Deserialize)]
pub struct ColumnReq {
    name: String,
}

fn column_name(r: &ColumnReq) -> Res<&str> {
    let name = r.name.trim();
    if name.is_empty() { Err(bad("Column name is required")) } else { Ok(name) }
}

pub async fn create_column(app: St, user: User, Path(id): Path<String>, Json(r): Json<ColumnReq>) -> Res<Json<Value>> {
    find(&app, &user, &id).await?;
    app.db
        .execute(
            "INSERT INTO board_columns (id, project_id, name, position) \
             SELECT ?3, ?1, ?2, COALESCE(MAX(position) + 1, 0) FROM board_columns WHERE project_id = ?1",
            params![id.as_str(), column_name(&r)?, uuid()],
        )
        .await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn rename_column(app: St, user: User, Path(id): Path<String>, Json(r): Json<ColumnReq>) -> Res<Json<Value>> {
    let sql = format!("UPDATE board_columns SET name = ?3 WHERE id = ?1 AND {OWNED}");
    exec(&app.db, &sql, params![id.as_str(), user.id.as_str(), column_name(&r)?]).await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn remove_column(app: St, user: User, Path(id): Path<String>) -> Res<Json<Value>> {
    exec(&app.db, &format!("DELETE FROM board_columns WHERE id = ?1 AND {OWNED}"), params![id.as_str(), user.id.as_str()]).await?;
    app.db.execute("DELETE FROM tasks WHERE column_id = ?1", params![id.as_str()]).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct Placed {
    id: String,
    project_id: String,
    position: i64,
}

pub async fn move_column(app: St, user: User, Path((id, dir)): Path<(String, String)>) -> Res<Json<Value>> {
    let neighbor_sql = match dir.as_str() {
        "left" => "position < ?2 ORDER BY position DESC",
        "right" => "position > ?2 ORDER BY position ASC",
        _ => return Err(bad("Invalid direction")),
    };
    let sql = format!("SELECT id, project_id, position FROM board_columns WHERE id = ?1 AND {OWNED}");
    let col: Placed = one(&app.db, &sql, params![id.as_str(), user.id.as_str()]).await?;
    let sql = format!("SELECT id, project_id, position FROM board_columns WHERE project_id = ?1 AND {neighbor_sql} LIMIT 1");
    let Ok(other) = one::<Placed>(&app.db, &sql, params![col.project_id, col.position]).await else {
        return Ok(Json(json!({ "ok": true })));
    };
    let swap = "UPDATE board_columns SET position = ?1 WHERE id = ?2";
    app.db.execute(swap, params![other.position, col.id]).await?;
    app.db.execute(swap, params![col.position, other.id]).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Serialize, Deserialize)]
pub struct Task {
    id: String,
    column_id: String,
    title: String,
    description: String,
    due: Option<String>,
    created: i64,
    priority: i64,
}

pub async fn tasks(app: St, user: User, Path(id): Path<String>) -> Res<Json<Vec<Task>>> {
    let sql = format!(
        "SELECT id, column_id, title, description, due, created, priority FROM tasks \
         WHERE project_id = ?1 AND {OWNED} ORDER BY position, created"
    );
    Ok(Json(all(&app.db, &sql, params![id.as_str(), user.id.as_str()]).await?))
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
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct MoveReq {
    column_id: String,
    before: Option<String>,
}

#[derive(Deserialize)]
struct TaskId {
    id: String,
}

pub async fn move_task(app: St, user: User, Path(id): Path<String>, Json(r): Json<MoveReq>) -> Res<Json<Value>> {
    let sql = format!(
        "UPDATE tasks SET column_id = ?3 WHERE id = ?1 AND {OWNED} \
         AND ?3 IN (SELECT c.id FROM board_columns c WHERE c.project_id = tasks.project_id)"
    );
    exec(&app.db, &sql, params![id.as_str(), user.id.as_str(), r.column_id.as_str()]).await?;
    let mut order: Vec<String> = all::<TaskId>(
        &app.db,
        "SELECT id FROM tasks WHERE column_id = ?1 AND id != ?2 ORDER BY position, created",
        params![r.column_id.as_str(), id.as_str()],
    )
    .await?
    .into_iter()
    .map(|t| t.id)
    .collect();
    let at = r.before.and_then(|b| order.iter().position(|t| *t == b)).unwrap_or(order.len());
    order.insert(at, id);
    let tx = app.db.transaction().await?;
    for (pos, task) in order.iter().enumerate() {
        tx.execute("UPDATE tasks SET position = ?1 WHERE id = ?2", params![pos as i64, task.as_str()]).await?;
    }
    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn remove_task(app: St, user: User, Path(id): Path<String>) -> Res<Json<Value>> {
    exec(&app.db, &format!("DELETE FROM tasks WHERE id = ?1 AND {OWNED}"), params![id.as_str(), user.id.as_str()]).await?;
    Ok(Json(json!({ "ok": true })))
}
