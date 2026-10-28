use axum::Json;
use axum::extract::multipart::Field;
use axum::extract::{Multipart, Path};
use axum::http::StatusCode;
use libsql::params;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::auth::{User, login_allowed};
use crate::files::unique_name;
use crate::templates::notice_email;
use crate::util::{Res, all, bad, err, exec, hash, one, token, uuid, verify};
use crate::{App, St};

const NAME_LIMIT: usize = 100;
const NOTE_LIMIT: usize = 2000;
const OPEN: &str = "token = ?1 AND active = 1 AND (expires IS NULL OR expires > unixepoch())";

#[derive(Serialize, Deserialize)]
pub struct UploadLink {
    id: String,
    folder_id: Option<String>,
    token: String,
    title: String,
    description: String,
    locked: i64,
    expires: Option<i64>,
    active: i64,
    created: i64,
    files: i64,
    bytes: i64,
    last_upload: Option<i64>,
}

#[derive(Deserialize)]
pub struct LinkReq {
    title: String,
    #[serde(default)]
    description: String,
    folder: Option<String>,
    expires: Option<i64>,
    password: Option<String>,
    #[serde(default = "enabled")]
    active: bool,
}

fn enabled() -> bool {
    true
}

impl LinkReq {
    fn title(&self) -> Res<&str> {
        let title = self.title.trim();
        if title.is_empty() || title.chars().count() > 200 {
            return Err(bad("Give the request a title of at most 200 characters"));
        }
        Ok(title)
    }

    fn password_hash(&self) -> Option<String> {
        self.password.as_deref().map(|p| if p.is_empty() { String::new() } else { hash(p) })
    }
}

pub async fn list(app: St, user: User) -> Res<Json<Vec<UploadLink>>> {
    let rows = all(
        &app.db,
        "SELECT l.id, l.folder_id, l.token, l.title, l.description, l.password IS NOT NULL AS locked, l.expires, l.active, l.created, \
         COUNT(f.id) AS files, COALESCE(SUM(f.size), 0) AS bytes, MAX(f.created) AS last_upload \
         FROM upload_links l LEFT JOIN files f ON f.upload_link_id = l.id \
         WHERE l.user_id = ?1 GROUP BY l.id ORDER BY l.created DESC",
        params![user.id.as_str()],
    )
    .await?;
    Ok(Json(rows))
}

pub async fn create(app: St, user: User, Json(r): Json<LinkReq>) -> Res<Json<Value>> {
    let (id, t) = (uuid(), token());
    let password = r.password_hash().filter(|p| !p.is_empty());
    app.db
        .execute(
            "INSERT INTO upload_links (id, user_id, folder_id, token, title, description, password, expires, active) \
             VALUES (?1, ?2, (SELECT id FROM folders WHERE id = ?3 AND user_id = ?2), ?4, ?5, ?6, ?7, ?8, ?9)",
            params![id.as_str(), user.id.as_str(), r.folder.as_deref(), t.as_str(), r.title()?, r.description.trim(), password, r.expires, r.active as i64],
        )
        .await?;
    Ok(Json(json!({ "id": id, "token": t })))
}

pub async fn update(app: St, user: User, Path(id): Path<String>, Json(r): Json<LinkReq>) -> Res<Json<Value>> {
    exec(
        &app.db,
        "UPDATE upload_links SET title = ?1, description = ?2, folder_id = (SELECT id FROM folders WHERE id = ?3 AND user_id = ?8), \
         expires = ?4, active = ?5, password = CASE WHEN ?6 IS NULL THEN password WHEN ?6 = '' THEN NULL ELSE ?6 END \
         WHERE id = ?7 AND user_id = ?8",
        params![r.title()?, r.description.trim(), r.folder.as_deref(), r.expires, r.active as i64, r.password_hash(), id.as_str(), user.id.as_str()],
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn remove(app: St, user: User, Path(id): Path<String>) -> Res<Json<Value>> {
    let tx = app.db.transaction().await?;
    tx.execute("UPDATE files SET upload_link_id = NULL WHERE upload_link_id = ?1 AND user_id = ?2", params![id.as_str(), user.id.as_str()]).await?;
    exec(&tx, "DELETE FROM upload_links WHERE id = ?1 AND user_id = ?2", params![id.as_str(), user.id.as_str()]).await?;
    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct Target {
    id: String,
    user_id: String,
    folder_id: Option<String>,
    title: String,
    password: Option<String>,
}

async fn target(app: &App, t: &str) -> Res<Target> {
    let sql = format!("SELECT id, user_id, folder_id, title, password FROM upload_links WHERE {OPEN}");
    one(&app.db, &sql, params![t]).await
}

fn check_password(app: &App, link: &Target, given: &str) -> Res<()> {
    let Some(stored) = &link.password else { return Ok(()) };
    let key = format!("upload:{}", link.id);
    if !login_allowed(app, &key) {
        return Err(err(StatusCode::TOO_MANY_REQUESTS, "Too many attempts, try again later"));
    }
    if !verify(given, stored) {
        return Err(err(StatusCode::UNAUTHORIZED, "Wrong password"));
    }
    app.login_attempts.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(&key);
    Ok(())
}

#[derive(Serialize, Deserialize)]
pub struct PublicLink {
    title: String,
    description: String,
    owner: String,
    locked: i64,
}

pub async fn public_get(app: St, Path(t): Path<String>) -> Res<Json<PublicLink>> {
    let sql = format!(
        "SELECT l.title, l.description, u.name AS owner, l.password IS NOT NULL AS locked \
         FROM upload_links l JOIN users u ON u.id = l.user_id WHERE l.{OPEN}"
    );
    Ok(Json(one(&app.db, &sql, params![t.as_str()]).await?))
}

#[derive(Deserialize)]
pub struct UnlockReq {
    password: String,
}

pub async fn unlock(app: St, Path(t): Path<String>, Json(r): Json<UnlockReq>) -> Res<Json<Value>> {
    check_password(&app, &target(&app, &t).await?, &r.password)?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Default)]
struct Sender {
    name: String,
    note: String,
    password: String,
}

impl Sender {
    fn set(&mut self, field: &str, value: String) {
        match field {
            "name" => self.name = value.trim().to_owned(),
            "note" => self.note = value.trim().to_owned(),
            "password" => self.password = value,
            _ => {}
        }
    }

    fn validate(&self) -> Res<()> {
        if self.name.is_empty() || self.name.chars().count() > NAME_LIMIT {
            return Err(bad("Enter your name (at most 100 characters)"));
        }
        if self.note.chars().count() > NOTE_LIMIT {
            return Err(bad("The message can be at most 2000 characters"));
        }
        Ok(())
    }
}

fn base_name(raw: &str) -> String {
    let name = raw.rsplit(['/', '\\']).next().unwrap_or_default().trim();
    if name.is_empty() || name == "." || name == ".." { "file".to_owned() } else { name.chars().take(255).collect() }
}

#[derive(Default)]
pub struct Origin<'a> {
    link: Option<&'a str>,
    uploader: Option<&'a str>,
    note: Option<&'a str>,
}

pub async fn store(app: &App, owner: &str, folder: Option<&str>, origin: Origin<'_>, field: &mut Field<'_>) -> Res<String> {
    let mime = field.content_type().unwrap_or("application/octet-stream").to_owned();
    let raw = base_name(field.file_name().unwrap_or_default());
    let key = token();
    let size = app.storage.save(&key, field).await?;
    let name = unique_name(app, owner, folder, &raw).await?;
    app.db
        .execute(
            "INSERT INTO files (id, user_id, folder_id, name, mime, size, key, upload_link_id, uploader, upload_note) \
             VALUES (?1, ?2, (SELECT id FROM folders WHERE id = ?3 AND user_id = ?2), ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![uuid(), owner, folder, name.as_str(), mime, size, key, origin.link, origin.uploader, origin.note],
        )
        .await?;
    Ok(name)
}

pub async fn public_upload(app: St, Path(t): Path<String>, mut mp: Multipart) -> Res<Json<Value>> {
    let link = target(&app, &t).await?;
    let mut sender = Sender::default();
    let mut saved = Vec::new();
    while let Some(mut field) = mp.next_field().await? {
        if field.file_name().is_none() {
            let name = field.name().unwrap_or_default().to_owned();
            sender.set(&name, field.text().await?);
            continue;
        }
        if saved.is_empty() {
            sender.validate()?;
            check_password(&app, &link, &sender.password)?;
        }
        let origin = Origin {
            link: Some(&link.id),
            uploader: Some(&sender.name),
            note: Some(sender.note.as_str()).filter(|n| !n.is_empty()),
        };
        saved.push(store(&app, &link.user_id, link.folder_id.as_deref(), origin, &mut field).await?);
    }
    if saved.is_empty() {
        return Err(bad("No file in request"));
    }
    notify(&app, &link, &sender, saved.len()).await;
    Ok(Json(json!({ "saved": saved })))
}

#[derive(Deserialize)]
struct Owner {
    email: String,
}

async fn notify(app: &App, link: &Target, sender: &Sender, count: usize) {
    let Ok(owner) = one::<Owner>(&app.db, "SELECT email FROM users WHERE id = ?1", params![link.user_id.as_str()]).await else { return };
    let mut text = format!("{} uploaded {count} file(s) to your request \"{}\".", sender.name, link.title);
    if !sender.note.is_empty() {
        text.push_str(&format!(" Message: {}", sender.note));
    }
    let path = link.folder_id.as_deref().map_or("/files".to_owned(), |f| format!("/files?in={f}"));
    let html = notice_email("New files received", &text, "Open files", &app.url(&path), &app.url("/icon-192.png"));
    app.mail(&owner.email, &format!("{} sent you {count} file(s)", sender.name), html).await;
}

#[cfg(test)]
mod tests {
    use super::base_name;

    #[test]
    fn strips_paths_from_uploaded_names() {
        assert_eq!(base_name("report.pdf"), "report.pdf");
        assert_eq!(base_name("../../etc/passwd"), "passwd");
        assert_eq!(base_name("C:\\Users\\a\\photo.jpg"), "photo.jpg");
        assert_eq!(base_name(".."), "file");
        assert_eq!(base_name("  "), "file");
    }
}
