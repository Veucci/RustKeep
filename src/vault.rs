use axum::Json;
use axum::extract::Path;
use libsql::params;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::St;
use crate::auth::User;
use crate::util::{Res, all, bad, exec, one};

#[derive(Serialize, Deserialize)]
pub struct Item {
    id: i64,
    name: String,
    note: String,
    created: i64,
}

pub async fn list(app: St, user: User) -> Res<Json<Vec<Item>>> {
    user.need_pin()?;
    let rows = all(
        &app.db,
        "SELECT id, name, note, created FROM vault WHERE user_id = ?1 ORDER BY name",
        params![user.id],
    )
    .await?;
    Ok(Json(rows))
}

#[derive(Deserialize)]
struct Sealed {
    value: String,
}

pub async fn reveal(app: St, user: User, Path(id): Path<i64>) -> Res<Json<Value>> {
    user.need_pin()?;
    let s: Sealed = one(&app.db, "SELECT value FROM vault WHERE id = ?1 AND user_id = ?2", params![id, user.id]).await?;
    Ok(Json(json!({ "value": app.crypto.open(&s.value)? })))
}

#[derive(Deserialize)]
pub struct CreateReq {
    name: String,
    value: String,
    #[serde(default)]
    note: String,
}

pub async fn create(app: St, user: User, Json(r): Json<CreateReq>) -> Res<Json<Value>> {
    user.need_pin()?;
    if r.name.trim().is_empty() || r.value.is_empty() {
        return Err(bad("Name and value are required"));
    }
    app.db
        .execute(
            "INSERT INTO vault (user_id, name, value, note) VALUES (?1, ?2, ?3, ?4)",
            params![user.id, r.name.trim(), app.crypto.seal(&r.value), r.note],
        )
        .await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn remove(app: St, user: User, Path(id): Path<i64>) -> Res<Json<Value>> {
    user.need_pin()?;
    exec(&app.db, "DELETE FROM vault WHERE id = ?1 AND user_id = ?2", params![id, user.id]).await?;
    Ok(Json(json!({ "ok": true })))
}
