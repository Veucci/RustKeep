use std::sync::Arc;

use axum::Json;
use axum::body::Bytes;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use libsql::params;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::util::{AppError, Res, all, bad, cookie, err, exec, hash, one, token, verify};
use crate::{App, St};

const COOKIE: &str = "rk_session";
const SESSION_SECS: i64 = 30 * 86400;
const PIN_SECS: i64 = 15 * 60;
const LOGIN_MAX: u32 = 10;
const LOGIN_WINDOW: i64 = 15 * 60;

pub struct User {
    pub id: String,
    pub pin_ok: bool,
    token: String,
}

impl User {
    pub fn need_pin(&self) -> Res<()> {
        if self.pin_ok { Ok(()) } else { Err(err(StatusCode::FORBIDDEN, "PIN required")) }
    }
}

#[derive(Deserialize)]
struct SessionRow {
    user_id: String,
    pin_until: i64,
}

impl FromRequestParts<Arc<App>> for User {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, app: &Arc<App>) -> Res<Self> {
        let unauth = || err(StatusCode::UNAUTHORIZED, "Authentication required");
        let token = cookie(&parts.headers, COOKIE).ok_or_else(unauth)?;
        let s: SessionRow = one(
            &app.db,
            "SELECT user_id, pin_until FROM sessions WHERE token = ?1 AND expires > unixepoch()",
            params![token.clone()],
        )
        .await
        .map_err(|_| unauth())?;
        let now = unix_now();
        Ok(User { id: s.user_id, pin_ok: s.pin_until > now, token })
    }
}

pub fn unix_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

fn session_cookie(app: &App, value: &str, max_age: i64) -> HeaderMap {
    let path = if app.cfg.base.is_empty() { "/" } else { &app.cfg.base };
    let secure = if app.cfg.secure_cookie { "; Secure" } else { "" };
    let mut h = HeaderMap::new();
    let c = format!("{COOKIE}={value}; Path={path}; HttpOnly; SameSite=Lax; Max-Age={max_age}{secure}");
    h.insert(header::SET_COOKIE, c.parse().expect("cookie header"));
    h
}

#[derive(Deserialize)]
pub struct LoginReq {
    email: String,
    password: String,
}

#[derive(Deserialize)]
struct LoginRow {
    id: String,
    pass: String,
    approved: i64,
}

pub fn login_allowed(app: &App, user_id: &str) -> bool {
    let now = unix_now();
    let mut attempts = app.login_attempts.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    attempts.retain(|_, (_, start)| now - *start < LOGIN_WINDOW);
    let entry = attempts.entry(user_id.to_owned()).or_insert((0, now));
    entry.0 += 1;
    entry.0 <= LOGIN_MAX
}

pub async fn login(app: St, Json(r): Json<LoginReq>) -> Res<impl IntoResponse> {
    let invalid = || err(StatusCode::UNAUTHORIZED, "Invalid email or password");
    let u: LoginRow = one(
        &app.db,
        "SELECT id, pass, approved FROM users WHERE email = ?1",
        params![r.email.trim().to_lowercase()],
    )
    .await
    .map_err(|_| invalid())?;
    if !login_allowed(&app, &u.id) {
        return Err(err(StatusCode::TOO_MANY_REQUESTS, "Too many sign-in attempts, try again later"));
    }
    if !verify(&r.password, &u.pass) {
        return Err(invalid());
    }
    app.login_attempts.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(&u.id);
    if u.approved == 0 {
        return Err(err(StatusCode::FORBIDDEN, "Your account has not been approved yet"));
    }
    let t = token();
    app.db
        .execute(
            "INSERT INTO sessions (token, user_id, expires) VALUES (?1, ?2, unixepoch() + ?3)",
            params![t.clone(), u.id.as_str(), SESSION_SECS],
        )
        .await?;
    Ok((session_cookie(&app, &t, SESSION_SECS), Json(json!({ "ok": true }))))
}

pub async fn logout(app: St, user: User) -> Res<impl IntoResponse> {
    app.db.execute("DELETE FROM sessions WHERE token = ?1", params![user.token]).await?;
    Ok((session_cookie(&app, "", 0), Json(json!({ "ok": true }))))
}

#[derive(Serialize, Deserialize)]
pub struct Me {
    id: String,
    email: String,
    name: String,
    has_pin: i64,
    avatar: i64,
    #[serde(default)]
    pin_ok: bool,
}

pub async fn me(app: St, user: User) -> Res<Json<Me>> {
    let mut me: Me = one(
        &app.db,
        "SELECT id, email, name, pin IS NOT NULL AS has_pin, COALESCE(length(avatar), 0) AS avatar FROM users WHERE id = ?1",
        params![user.id.as_str()],
    )
    .await?;
    me.pin_ok = user.pin_ok;
    Ok(Json(me))
}

#[derive(Deserialize)]
pub struct SetPinReq {
    password: String,
    pin: String,
}

pub async fn set_pin(app: St, user: User, Json(r): Json<SetPinReq>) -> Res<Json<Value>> {
    if r.pin.len() < 4 || r.pin.len() > 8 || !r.pin.chars().all(|c| c.is_ascii_digit()) {
        return Err(bad("PIN must be 4-8 digits"));
    }
    let row: LoginRow = one(&app.db, "SELECT id, pass, approved FROM users WHERE id = ?1", params![user.id.as_str()]).await?;
    if !verify(&r.password, &row.pass) {
        return Err(err(StatusCode::UNAUTHORIZED, "Wrong password"));
    }
    exec(&app.db, "UPDATE users SET pin = ?1, pin_fails = 0 WHERE id = ?2", params![hash(&r.pin), user.id.as_str()]).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct UnlockReq {
    pin: String,
}

#[derive(Deserialize)]
struct PinRow {
    pin: Option<String>,
}

#[derive(Deserialize)]
struct PinAttempt {
    pin: String,
}

pub async fn unlock(app: St, user: User, Json(r): Json<UnlockReq>) -> Res<Json<Value>> {
    let row: PinRow = one(&app.db, "SELECT pin FROM users WHERE id = ?1", params![user.id.as_str()]).await?;
    if row.pin.is_none() {
        return Err(bad("Set a PIN first"));
    }
    let attempt: Option<PinAttempt> = all(
        &app.db,
        "UPDATE users SET pin_fails = (pin_fails + 1) % 5, \
         pin_block = CASE WHEN pin_fails >= 4 THEN unixepoch() + 300 ELSE pin_block END \
         WHERE id = ?1 AND pin IS NOT NULL AND pin_block <= unixepoch() RETURNING pin",
        params![user.id.as_str()],
    )
    .await?
    .pop();
    let Some(attempt) = attempt else {
        return Err(err(StatusCode::TOO_MANY_REQUESTS, "Too many failed attempts, wait 5 minutes"));
    };
    if !verify(&r.pin, &attempt.pin) {
        return Err(err(StatusCode::UNAUTHORIZED, "Wrong PIN"));
    }
    app.db.execute("UPDATE users SET pin_fails = 0, pin_block = 0 WHERE id = ?1", params![user.id.as_str()]).await?;
    app.db
        .execute("UPDATE sessions SET pin_until = unixepoch() + ?1 WHERE token = ?2", params![PIN_SECS, user.token])
        .await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn lock(app: St, user: User) -> Res<Json<Value>> {
    app.db.execute("UPDATE sessions SET pin_until = 0 WHERE token = ?1", params![user.token]).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct ProfileReq {
    name: String,
    email: String,
    password: String,
}

pub async fn update_profile(app: St, user: User, Json(r): Json<ProfileReq>) -> Res<Json<Value>> {
    let email = r.email.trim().to_lowercase();
    let name = r.name.trim();
    if !email.contains('@') || name.is_empty() {
        return Err(bad("Provide a name and a valid email"));
    }
    let row: LoginRow = one(&app.db, "SELECT id, pass, approved FROM users WHERE id = ?1", params![user.id.as_str()]).await?;
    if !verify(&r.password, &row.pass) {
        return Err(err(StatusCode::UNAUTHORIZED, "Wrong password"));
    }
    let mut taken = app.db.query("SELECT 1 FROM users WHERE email = ?1 AND id != ?2", params![email.as_str(), user.id.as_str()]).await?;
    if taken.next().await?.is_some() {
        return Err(err(StatusCode::CONFLICT, "This email is already in use"));
    }
    exec(&app.db, "UPDATE users SET name = ?1, email = ?2 WHERE id = ?3", params![name, email, user.id.as_str()]).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct PasswordReq {
    password: String,
    new_password: String,
}

pub async fn change_password(app: St, user: User, Json(r): Json<PasswordReq>) -> Res<Json<Value>> {
    if r.new_password.len() < 8 {
        return Err(bad("New password must be at least 8 characters"));
    }
    let row: LoginRow = one(&app.db, "SELECT id, pass, approved FROM users WHERE id = ?1", params![user.id.as_str()]).await?;
    if !verify(&r.password, &row.pass) {
        return Err(err(StatusCode::UNAUTHORIZED, "Wrong password"));
    }
    exec(&app.db, "UPDATE users SET pass = ?1 WHERE id = ?2", params![hash(&r.new_password), user.id.as_str()]).await?;
    app.db
        .execute("DELETE FROM sessions WHERE user_id = ?1 AND token != ?2", params![user.id.as_str(), user.token])
        .await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Serialize, Deserialize)]
pub struct AccountStats {
    created: i64,
    notes: i64,
    projects: i64,
    tasks: i64,
    files: i64,
    folders: i64,
    vault: i64,
    reminders: i64,
    events: i64,
    file_bytes: i64,
    note_bytes: i64,
}

const ACCOUNT_STATS: &str = "SELECT \
    (SELECT created FROM users WHERE id = ?1) AS created, \
    (SELECT COUNT(*) FROM notes WHERE user_id = ?1 AND trashed_at IS NULL) AS notes, \
    (SELECT COUNT(*) FROM projects WHERE user_id = ?1) AS projects, \
    (SELECT COUNT(*) FROM tasks t JOIN projects p ON p.id = t.project_id WHERE p.user_id = ?1) AS tasks, \
    (SELECT COUNT(*) FROM files WHERE user_id = ?1) AS files, \
    (SELECT COUNT(*) FROM folders WHERE user_id = ?1) AS folders, \
    (SELECT COUNT(*) FROM vault WHERE user_id = ?1) AS vault, \
    (SELECT COUNT(*) FROM reminders WHERE user_id = ?1) AS reminders, \
    (SELECT COUNT(*) FROM events WHERE user_id = ?1) AS events, \
    (SELECT COALESCE(SUM(size), 0) FROM files WHERE user_id = ?1) AS file_bytes, \
    (SELECT COALESCE(SUM(length(CAST(body AS BLOB)) + length(CAST(title AS BLOB))), 0) FROM notes WHERE user_id = ?1) AS note_bytes";

pub async fn stats(app: St, user: User) -> Res<Json<AccountStats>> {
    Ok(Json(one(&app.db, ACCOUNT_STATS, params![user.id.as_str()]).await?))
}

pub const AVATAR_UPLOAD_LIMIT: usize = 15 * 1024 * 1024;
const AVATAR_SIDE: u32 = 256;

fn avatar_jpeg(bytes: &[u8]) -> Option<Vec<u8>> {
    let img = image::load_from_memory(bytes).ok()?;
    let square = img.resize_to_fill(AVATAR_SIDE, AVATAR_SIDE, image::imageops::FilterType::Lanczos3).to_rgb8();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85).encode_image(&square).ok()?;
    Some(out)
}

pub async fn set_avatar(app: St, user: User, bytes: Bytes) -> Res<Json<Value>> {
    let jpeg = tokio::task::spawn_blocking(move || avatar_jpeg(&bytes)).await?.ok_or_else(|| bad("Unsupported image"))?;
    exec(&app.db, "UPDATE users SET avatar = ?1 WHERE id = ?2", params![jpeg, user.id.as_str()]).await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn remove_avatar(app: St, user: User) -> Res<Json<Value>> {
    exec(&app.db, "UPDATE users SET avatar = NULL WHERE id = ?1", params![user.id.as_str()]).await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn avatar(app: St, user: User) -> Res<Response> {
    let mut rows = app.db.query("SELECT avatar FROM users WHERE id = ?1 AND avatar IS NOT NULL", params![user.id.as_str()]).await?;
    let row = rows.next().await?.ok_or_else(crate::util::not_found)?;
    let jpeg: Vec<u8> = row.get(0)?;
    Ok(([(header::CONTENT_TYPE, "image/jpeg"), (header::CACHE_CONTROL, "private, no-cache")], jpeg).into_response())
}
