use axum::Json;
use axum::http::StatusCode;
use libsql::params;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::St;
use crate::templates::notice_email;
use crate::util::{Res, all, bad, err, exec, hash, one, token};

const RESET_SECS: i64 = 60 * 60;
const RESEND_COOLDOWN: i64 = 2 * 60;

fn digest(t: &str) -> String {
    hex::encode(Sha256::digest(t.as_bytes()))
}

#[derive(Deserialize)]
pub struct ForgotReq {
    email: String,
}

#[derive(Deserialize)]
struct Account {
    id: String,
    name: String,
}

pub async fn forgot(app: St, Json(r): Json<ForgotReq>) -> Res<Json<Value>> {
    let email = r.email.trim().to_lowercase();
    let sent = Json(json!({ "message": "If an account exists for this email, we sent a link to reset your password." }));
    let found: Vec<Account> = all(
        &app.db,
        "SELECT id, name FROM users WHERE email = ?1 AND approved = 1 AND NOT EXISTS \
         (SELECT 1 FROM password_resets r WHERE r.user_id = users.id AND r.created > unixepoch() - ?2)",
        params![email.as_str(), RESEND_COOLDOWN],
    )
    .await?;
    let Some(user) = found.into_iter().next() else { return Ok(sent) };
    let t = token();
    app.db.execute("DELETE FROM password_resets WHERE user_id = ?1 OR expires <= unixepoch()", params![user.id.as_str()]).await?;
    app.db
        .execute(
            "INSERT INTO password_resets (token, user_id, expires) VALUES (?1, ?2, unixepoch() + ?3)",
            params![digest(&t), user.id.as_str(), RESET_SECS],
        )
        .await?;
    let text = format!(
        "Hi {}, we received a request to reset your RustKeep password. The link is valid for one hour. If you did not ask for this, you can ignore this email.",
        user.name
    );
    let html = notice_email("Reset your password", &text, "Reset password", &app.url(&format!("/reset/{t}")), &app.url("/icon-192.png"));
    let app = app.0.clone();
    tokio::spawn(async move { app.mail(&email, "Reset your RustKeep password", html).await });
    Ok(sent)
}

#[derive(Deserialize)]
pub struct ResetReq {
    token: String,
    password: String,
}

#[derive(Deserialize)]
struct ResetRow {
    user_id: String,
}

pub async fn reset(app: St, Json(r): Json<ResetReq>) -> Res<Json<Value>> {
    if r.password.len() < 8 {
        return Err(bad("Password must be at least 8 characters"));
    }
    let row: ResetRow = one(
        &app.db,
        "DELETE FROM password_resets WHERE token = ?1 AND expires > unixepoch() RETURNING user_id",
        params![digest(&r.token)],
    )
    .await
    .map_err(|_| err(StatusCode::BAD_REQUEST, "This reset link is invalid or has expired"))?;
    let user = row.user_id.as_str();
    exec(&app.db, "UPDATE users SET pass = ?1 WHERE id = ?2", params![hash(&r.password), user]).await?;
    app.db.execute("DELETE FROM sessions WHERE user_id = ?1", params![user]).await?;
    app.db.execute("DELETE FROM password_resets WHERE user_id = ?1", params![user]).await?;
    app.login_attempts.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(user);
    Ok(Json(json!({ "ok": true })))
}
