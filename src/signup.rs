use std::net::SocketAddr;

use axum::Json;
use axum::extract::{ConnectInfo, Path};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::Html;
use libsql::params;
use serde_json::{Value, json};

use crate::templates::{Applicant, approval_email, message_page, notice_email, review_page};
use crate::util::{Res, bad, err, hash, one, token, uuid};
use crate::{App, St};

#[derive(serde::Deserialize)]
pub struct RegisterReq {
    email: String,
    name: String,
    password: String,
}

const MAX_PENDING: i64 = 10;

struct Meta {
    ip: String,
    agent: String,
    language: String,
    origin: String,
}

fn header_str(h: &HeaderMap, name: impl header::AsHeaderName) -> String {
    h.get(name).and_then(|v| v.to_str().ok()).unwrap_or("Unknown").chars().take(300).collect()
}

fn client_meta(h: &HeaderMap, addr: SocketAddr) -> Meta {
    let forwarded = h
        .get("x-forwarded-for")
        .or_else(|| h.get("x-real-ip"))
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(|v| v.trim().to_owned());
    Meta {
        ip: forwarded.unwrap_or_else(|| addr.ip().to_string()),
        agent: header_str(h, header::USER_AGENT),
        language: header_str(h, header::ACCEPT_LANGUAGE),
        origin: header_str(h, header::ORIGIN),
    }
}

async fn exists(app: &App, sql: &str, value: impl Into<libsql::Value>) -> Res<bool> {
    Ok(app.db.query(sql, [value.into()]).await?.next().await?.is_some())
}

const APPLICANT: &str = "SELECT u.id AS user_id, u.name, u.email, r.ip, r.agent, r.language, r.origin, \
     datetime(r.created, 'unixepoch') || ' UTC' AS created_at \
     FROM registrations r JOIN users u ON u.id = r.user_id WHERE r.token = ?1 AND u.approved = 0";

async fn applicant(app: &App, t: &str) -> Option<Applicant> {
    one(&app.db, APPLICANT, params![t]).await.ok()
}

pub async fn register(
    app: St,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(r): Json<RegisterReq>,
) -> Res<Json<Value>> {
    let email = r.email.trim().to_lowercase();
    if !email.contains('@') || r.name.trim().is_empty() || r.password.len() < 8 {
        return Err(bad("Provide a valid email, a name and a password of at least 8 characters"));
    }
    if exists(&app, "SELECT 1 WHERE (SELECT COUNT(*) FROM users WHERE approved = 0) >= ?1", MAX_PENDING).await? {
        return Err(err(StatusCode::TOO_MANY_REQUESTS, "Too many pending requests, try again later"));
    }
    if exists(&app, "SELECT 1 FROM blocked_emails WHERE email = ?1", email.as_str()).await? {
        return Err(err(StatusCode::FORBIDDEN, "This email address cannot be used to register"));
    }
    if exists(&app, "SELECT 1 FROM users WHERE email = ?1", email.as_str()).await? {
        return Err(err(StatusCode::CONFLICT, "This email is already registered"));
    }
    let t = token();
    let meta = client_meta(&headers, addr);
    let user_id = uuid();
    app.db
        .execute(
            "INSERT INTO users (id, email, name, pass, approve_token) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![user_id.as_str(), email, r.name.trim(), hash(&r.password), t.clone()],
        )
        .await?;
    app.db
        .execute(
            "INSERT INTO registrations (token, user_id, ip, agent, language, origin) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![t.clone(), user_id, meta.ip, meta.agent, meta.language, meta.origin],
        )
        .await?;
    if let Some(a) = applicant(&app, &t).await {
        let html = approval_email(&a, &app.url(&format!("/api/auth/approve/{t}")), &app.url("/icon-192.png"));
        app.mail(&app.cfg.verify_email, &format!("New account request: {}", a.name), html).await;
    }
    Ok(Json(json!({ "message": "Registration received. You can sign in once your account is approved." })))
}

fn invalid_link() -> (StatusCode, Html<String>) {
    let html = message_page("Link no longer valid", "This request was already handled or the link is incorrect.");
    (StatusCode::NOT_FOUND, Html(html))
}

pub async fn review(app: St, Path(t): Path<String>) -> (StatusCode, Html<String>) {
    let Some(a) = applicant(&app, &t).await else { return invalid_link() };
    let approve = format!("{}/api/auth/approve/{t}", app.cfg.base);
    let reject = format!("{}/api/auth/reject/{t}", app.cfg.base);
    (StatusCode::OK, Html(review_page(&a, &approve, &reject)))
}

pub async fn approve(app: St, Path(t): Path<String>) -> Res<(StatusCode, Html<String>)> {
    let Some(a) = applicant(&app, &t).await else { return Ok(invalid_link()) };
    app.db
        .execute("UPDATE users SET approved = 1, approve_token = NULL WHERE id = ?1", params![a.user_id])
        .await?;
    app.db.execute("DELETE FROM registrations WHERE token = ?1", params![t]).await?;
    let html = notice_email(
        "Your account is ready",
        "Your RustKeep account has been approved. You can sign in now.",
        "Sign in",
        &app.url("/login"),
        &app.url("/icon-192.png"),
    );
    app.mail(&a.email, "Your RustKeep account has been approved", html).await;
    let text = format!("{} can now sign in with {}.", a.name, a.email);
    Ok((StatusCode::OK, Html(message_page("Account approved", &text))))
}

pub async fn reject(app: St, Path(t): Path<String>) -> Res<(StatusCode, Html<String>)> {
    let Some(a) = applicant(&app, &t).await else { return Ok(invalid_link()) };
    app.db.execute("DELETE FROM users WHERE id = ?1 AND approved = 0", params![a.user_id]).await?;
    app.db.execute("DELETE FROM registrations WHERE token = ?1", params![t]).await?;
    app.db.execute("INSERT OR IGNORE INTO blocked_emails (email) VALUES (?1)", params![a.email.clone()]).await?;
    let text = format!("The request from {} was removed and this address can no longer sign up.", a.email);
    Ok((StatusCode::OK, Html(message_page("Request rejected", &text))))
}
