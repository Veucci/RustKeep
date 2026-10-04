use std::sync::Arc;

use axum::Json;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::IntoResponse;
use libsql::params;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::util::{AppError, Res, bad, cookie, err, exec, hash, one, token, verify};
use crate::{App, St};

const COOKIE: &str = "rk_session";
const SESSION_SECS: i64 = 30 * 86400;
const PIN_SECS: i64 = 15 * 60;

pub struct User {
    pub id: i64,
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
    user_id: i64,
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
    id: i64,
    pass: String,
    approved: i64,
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
    if !verify(&r.password, &u.pass) {
        return Err(invalid());
    }
    if u.approved == 0 {
        return Err(err(StatusCode::FORBIDDEN, "Your account has not been approved yet"));
    }
    let t = token();
    app.db
        .execute(
            "INSERT INTO sessions (token, user_id, expires) VALUES (?1, ?2, unixepoch() + ?3)",
            params![t.clone(), u.id, SESSION_SECS],
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
    id: i64,
    email: String,
    name: String,
    has_pin: i64,
    #[serde(default)]
    pin_ok: bool,
}

pub async fn me(app: St, user: User) -> Res<Json<Me>> {
    let mut me: Me = one(
        &app.db,
        "SELECT id, email, name, pin IS NOT NULL AS has_pin FROM users WHERE id = ?1",
        params![user.id],
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
    let row: LoginRow = one(&app.db, "SELECT id, pass, approved FROM users WHERE id = ?1", params![user.id]).await?;
    if !verify(&r.password, &row.pass) {
        return Err(err(StatusCode::UNAUTHORIZED, "Wrong password"));
    }
    exec(&app.db, "UPDATE users SET pin = ?1, pin_fails = 0 WHERE id = ?2", params![hash(&r.pin), user.id]).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct UnlockReq {
    pin: String,
}

#[derive(Deserialize)]
struct PinRow {
    pin: Option<String>,
    pin_block: i64,
}

pub async fn unlock(app: St, user: User, Json(r): Json<UnlockReq>) -> Res<Json<Value>> {
    let row: PinRow = one(&app.db, "SELECT pin, pin_block FROM users WHERE id = ?1", params![user.id]).await?;
    if row.pin_block > unix_now() {
        return Err(err(StatusCode::TOO_MANY_REQUESTS, "Too many failed attempts, wait 5 minutes"));
    }
    let Some(pin) = row.pin else { return Err(bad("Set a PIN first")) };
    if !verify(&r.pin, &pin) {
        app.db
            .execute(
                "UPDATE users SET pin_fails = (pin_fails + 1) % 5, \
                 pin_block = CASE WHEN pin_fails >= 4 THEN unixepoch() + 300 ELSE pin_block END WHERE id = ?1",
                params![user.id],
            )
            .await?;
        return Err(err(StatusCode::UNAUTHORIZED, "Wrong PIN"));
    }
    app.db.execute("UPDATE users SET pin_fails = 0 WHERE id = ?1", params![user.id]).await?;
    app.db
        .execute("UPDATE sessions SET pin_until = unixepoch() + ?1 WHERE token = ?2", params![PIN_SECS, user.token])
        .await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn lock(app: St, user: User) -> Res<Json<Value>> {
    app.db.execute("UPDATE sessions SET pin_until = 0 WHERE token = ?1", params![user.token]).await?;
    Ok(Json(json!({ "ok": true })))
}
