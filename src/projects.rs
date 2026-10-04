use axum::Json;
use axum::extract::Path;
use axum::http::header;
use axum::response::IntoResponse;
use libsql::params;
use rust_xlsxwriter::{Format, Workbook, Worksheet};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::St;
use crate::auth::User;
use crate::util::{Res, all, bad, exec, one};

pub const PROJECT_SELECT: &str = "SELECT p.id, p.name, p.description, p.status, p.created, \
     (SELECT COUNT(*) FROM tasks t WHERE t.project_id = p.id) AS tasks, \
     (SELECT COUNT(*) FROM tasks t WHERE t.column_id = \
        (SELECT c.id FROM board_columns c WHERE c.project_id = p.id ORDER BY c.position DESC LIMIT 1)) AS done \
     FROM projects p";

const DEFAULT_COLUMNS: [&str; 3] = ["To do", "In progress", "Done"];

#[derive(Serialize, Deserialize)]
pub struct Project {
    id: i64,
    name: String,
    description: String,
    status: String,
    created: i64,
    tasks: i64,
    done: i64,
}

pub async fn list(app: St, user: User) -> Res<Json<Vec<Project>>> {
    let sql = format!("{PROJECT_SELECT} WHERE p.user_id = ?1 ORDER BY p.created DESC");
    Ok(Json(all(&app.db, &sql, params![user.id]).await?))
}

pub async fn find(app: &St, user: &User, id: i64) -> Res<Project> {
    let sql = format!("{PROJECT_SELECT} WHERE p.id = ?1 AND p.user_id = ?2");
    one(&app.db, &sql, params![id, user.id]).await
}

pub async fn get(app: St, user: User, Path(id): Path<i64>) -> Res<Json<Project>> {
    Ok(Json(find(&app, &user, id).await?))
}

#[derive(Deserialize)]
pub struct ProjectReq {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    status: Option<String>,
}

pub async fn create(app: St, user: User, Json(r): Json<ProjectReq>) -> Res<Json<Value>> {
    if r.name.trim().is_empty() {
        return Err(bad("Project name is required"));
    }
    let mut rows = app
        .db
        .query(
            "INSERT INTO projects (user_id, name, description) VALUES (?1, ?2, ?3) RETURNING id",
            params![user.id, r.name.trim(), r.description],
        )
        .await?;
    let id: i64 = rows.next().await?.map(|r| r.get(0)).transpose()?.unwrap_or_default();
    for (pos, name) in DEFAULT_COLUMNS.iter().enumerate() {
        app.db
            .execute(
                "INSERT INTO board_columns (project_id, name, position) VALUES (?1, ?2, ?3)",
                params![id, *name, pos as i64],
            )
            .await?;
    }
    Ok(Json(json!({ "id": id })))
}

pub async fn update(app: St, user: User, Path(id): Path<i64>, Json(r): Json<ProjectReq>) -> Res<Json<Value>> {
    exec(
        &app.db,
        "UPDATE projects SET name = ?1, description = ?2, status = COALESCE(?3, status) WHERE id = ?4 AND user_id = ?5",
        params![r.name, r.description, r.status, id, user.id],
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn remove(app: St, user: User, Path(id): Path<i64>) -> Res<Json<Value>> {
    exec(&app.db, "DELETE FROM projects WHERE id = ?1 AND user_id = ?2", params![id, user.id]).await?;
    app.db
        .execute_batch(&format!(
            "DELETE FROM tasks WHERE project_id = {id}; \
             DELETE FROM board_columns WHERE project_id = {id}; \
             UPDATE notes SET project_id = NULL WHERE project_id = {id}; \
             UPDATE files SET project_id = NULL WHERE project_id = {id};"
        ))
        .await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct TaskRow {
    title: String,
    description: String,
    column: String,
    due: Option<String>,
}

#[derive(Deserialize)]
struct NoteRow {
    title: String,
    updated: i64,
}

#[derive(Deserialize)]
struct FileRow {
    name: String,
    mime: String,
    size: i64,
}

fn sheet<'a>(wb: &'a mut Workbook, name: &str, headers: &[&str]) -> Res<&'a mut Worksheet> {
    let ws = wb.add_worksheet().set_name(name)?;
    let bold = Format::new().set_bold();
    for (i, h) in headers.iter().enumerate() {
        ws.write_string_with_format(0, i as u16, *h, &bold)?;
    }
    Ok(ws)
}

pub async fn export(app: St, user: User, Path(id): Path<i64>) -> Res<impl IntoResponse> {
    let p = find(&app, &user, id).await?;
    let tasks: Vec<TaskRow> = all(
        &app.db,
        "SELECT t.title, t.description, c.name AS column, t.due FROM tasks t \
         JOIN board_columns c ON c.id = t.column_id WHERE t.project_id = ?1 ORDER BY c.position, t.id",
        params![id],
    )
    .await?;
    let notes: Vec<NoteRow> = all(
        &app.db,
        "SELECT title, updated FROM notes WHERE project_id = ?1 AND user_id = ?2 AND secret = 0 AND trashed_at IS NULL",
        params![id, user.id],
    )
    .await?;
    let files: Vec<FileRow> =
        all(&app.db, "SELECT name, mime, size FROM files WHERE project_id = ?1 AND user_id = ?2", params![id, user.id]).await?;

    let mut wb = Workbook::new();
    let ws = sheet(&mut wb, "Project", &["Name", "Description", "Status", "Tasks", "Completed"])?;
    ws.write_row(1, 0, [p.name.as_str(), p.description.as_str(), p.status.as_str()])?;
    ws.write_number(1, 3, p.tasks as f64)?.write_number(1, 4, p.done as f64)?;

    let ws = sheet(&mut wb, "Tasks", &["Title", "Column", "Due date", "Description"])?;
    for (i, t) in tasks.iter().enumerate() {
        let due = t.due.as_deref().unwrap_or("");
        ws.write_row(i as u32 + 1, 0, [t.title.as_str(), t.column.as_str(), due, t.description.as_str()])?;
    }

    let date = Format::new().set_num_format("yyyy-mm-dd hh:mm");
    let ws = sheet(&mut wb, "Notes", &["Title", "Updated"])?;
    for (i, n) in notes.iter().enumerate() {
        let row = i as u32 + 1;
        ws.write_string(row, 0, &n.title)?;
        ws.write_number_with_format(row, 1, n.updated as f64 / 86400.0 + 25569.0, &date)?;
    }

    let ws = sheet(&mut wb, "Files", &["Name", "Type", "Size (bytes)"])?;
    for (i, f) in files.iter().enumerate() {
        let row = i as u32 + 1;
        ws.write_row(row, 0, [f.name.as_str(), f.mime.as_str()])?;
        ws.write_number(row, 2, f.size as f64)?;
    }

    let buf = wb.save_to_buffer()?;
    let headers = [
        (header::CONTENT_TYPE, "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".to_owned()),
        (header::CONTENT_DISPOSITION, format!("attachment; filename=\"project-{id}.xlsx\"")),
    ];
    Ok((headers, buf))
}
