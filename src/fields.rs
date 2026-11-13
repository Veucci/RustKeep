use axum::Json;
use axum::extract::Path;
use libsql::params;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::St;
use crate::auth::User;
use crate::board::{NameReq, OWNED};
use crate::projects::find;
use crate::util::{Res, all, exec, uuid};

#[derive(Deserialize)]
pub struct FieldValue {
    field_id: String,
    value: String,
}

pub async fn replace_values(db: &libsql::Connection, task: &str, values: &[FieldValue]) -> Res<()> {
    let tx = db.transaction().await?;
    tx.execute("DELETE FROM task_field_values WHERE task_id = ?1", params![task]).await?;
    for v in values.iter().filter(|v| !v.value.trim().is_empty()) {
        tx.execute(
            "INSERT INTO task_field_values (id, task_id, field_id, value) SELECT ?1, t.id, f.id, ?4 \
             FROM tasks t JOIN custom_fields f ON f.project_id = t.project_id WHERE t.id = ?2 AND f.id = ?3",
            params![uuid(), task, v.field_id.as_str(), v.value.trim()],
        )
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub fn json_text<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Value, D::Error> {
    serde_json::from_str(&String::deserialize(d)?).map_err(serde::de::Error::custom)
}

#[derive(Serialize, Deserialize)]
pub struct Field {
    id: String,
    name: String,
}

pub const FIELD_SELECT: &str =
    "SELECT f.id, f.name FROM custom_fields f JOIN projects p ON p.id = f.project_id WHERE p.user_id = ?1 AND p.id = ?2 ORDER BY f.position, f.created";

pub async fn list(app: St, user: User, Path(id): Path<String>) -> Res<Json<Vec<Field>>> {
    Ok(Json(all(&app.db, FIELD_SELECT, params![user.id.as_str(), id.as_str()]).await?))
}

pub async fn create(app: St, user: User, Path(id): Path<String>, Json(r): Json<NameReq>) -> Res<Json<Value>> {
    find(&app, &user, &id).await?;
    app.db
        .execute(
            "INSERT INTO custom_fields (id, project_id, name, position) \
             SELECT ?3, ?1, ?2, COALESCE(MAX(position) + 1, 0) FROM custom_fields WHERE project_id = ?1",
            params![id.as_str(), r.required("Field name is required")?, uuid()],
        )
        .await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn rename(app: St, user: User, Path(id): Path<String>, Json(r): Json<NameReq>) -> Res<Json<Value>> {
    let sql = format!("UPDATE custom_fields SET name = ?3 WHERE id = ?1 AND {OWNED}");
    exec(&app.db, &sql, params![id.as_str(), user.id.as_str(), r.required("Field name is required")?]).await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn remove(app: St, user: User, Path(id): Path<String>) -> Res<Json<Value>> {
    exec(&app.db, &format!("DELETE FROM custom_fields WHERE id = ?1 AND {OWNED}"), params![id.as_str(), user.id.as_str()]).await?;
    app.db.execute("DELETE FROM task_field_values WHERE field_id = ?1", params![id.as_str()]).await?;
    Ok(Json(json!({ "ok": true })))
}
