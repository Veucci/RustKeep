use axum::Json;
use axum::extract::{Path, Query};
use axum::http::StatusCode;
use axum::response::Redirect;
use libsql::params;
use pulldown_cmark::{Event as Md, LinkType, Options, Parser, Tag, TagEnd, html};
use reqwest::{Client, RequestBuilder, Url};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::auth::{User, unix_now};
use crate::util::{AppError, Res, all, bad, err, exec, one, uuid};
use crate::{App, St};

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const REVOKE_URL: &str = "https://oauth2.googleapis.com/revoke";
const USERINFO_URL: &str = "https://openidconnect.googleapis.com/v1/userinfo";
const EVENTS_URL: &str = "https://www.googleapis.com/calendar/v3/calendars/primary/events";
const SCOPES: &str = "openid email https://www.googleapis.com/auth/calendar.events";
const STATE_SECS: i64 = 600;
const COLORS: [&str; 6] = ["blue", "green", "amber", "red", "violet", "slate"];

pub struct GoogleApp {
    pub client_id: String,
    pub client_secret: String,
}

const EVENT_SELECT: &str = "SELECT id, title, description, location, start_at, end_at, all_day, color, google_link, \
     IFNULL(google_id IS NOT NULL AND google_account = (SELECT g.email FROM google_links g WHERE g.user_id = events.user_id), 0) AS in_google \
     FROM events WHERE user_id = ?1 ORDER BY start_at";

const ITEM_SELECT: &str = "SELECT 'task' AS kind, t.id, t.title, t.due AS day, NULL AS at, p.id AS project_id, t.id AS task_id, \
       NULL AS note_id, p.name AS context, \
       t.column_id = (SELECT c.id FROM board_columns c WHERE c.project_id = p.id ORDER BY c.position DESC LIMIT 1) AS done \
     FROM tasks t JOIN projects p ON p.id = t.project_id WHERE p.user_id = ?1 AND p.archived = 0 AND t.due IS NOT NULL \
     UNION ALL \
     SELECT 'subtask', s.id, s.title, s.due, NULL, p.id, t.id, NULL, p.name || ' / ' || t.title, s.done \
     FROM subtasks s JOIN tasks t ON t.id = s.task_id JOIN projects p ON p.id = t.project_id \
     WHERE p.user_id = ?1 AND p.archived = 0 AND s.due IS NOT NULL \
     UNION ALL \
     SELECT 'reminder', r.id, r.title, NULL, r.remind_at, NULL, NULL, r.note_id, \
       IFNULL(CASE WHEN n.secret = 0 THEN n.title END, ''), r.sent \
     FROM reminders r LEFT JOIN notes n ON n.id = r.note_id WHERE r.user_id = ?1 AND r.archived = 0";

#[derive(Serialize, Deserialize)]
pub struct Event {
    id: String,
    title: String,
    description: String,
    location: String,
    start_at: i64,
    end_at: i64,
    all_day: i64,
    color: String,
    google_link: Option<String>,
    in_google: i64,
}

#[derive(Serialize, Deserialize)]
pub struct Item {
    kind: String,
    id: String,
    title: String,
    day: Option<String>,
    at: Option<i64>,
    project_id: Option<String>,
    task_id: Option<String>,
    note_id: Option<String>,
    context: String,
    done: i64,
}

#[derive(Serialize, Deserialize)]
pub struct GoogleStatus {
    email: String,
    synced: Option<i64>,
}

#[derive(Serialize)]
pub struct CalendarData {
    configured: bool,
    google: Option<GoogleStatus>,
    events: Vec<Event>,
    items: Vec<Item>,
}

pub async fn get(app: St, user: User) -> Res<Json<CalendarData>> {
    let u = user.id.as_str();
    let google = all(&app.db, "SELECT email, synced FROM google_links WHERE user_id = ?1", params![u]).await?.pop();
    Ok(Json(CalendarData {
        configured: app.cfg.google.is_some(),
        google,
        events: all(&app.db, EVENT_SELECT, params![u]).await?,
        items: all(&app.db, ITEM_SELECT, params![u]).await?,
    }))
}

#[derive(Deserialize)]
pub struct EventReq {
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    location: String,
    start_at: i64,
    end_at: i64,
    #[serde(default)]
    all_day: bool,
    #[serde(default)]
    color: String,
}

impl EventReq {
    fn title(&self) -> Res<&str> {
        let title = self.title.trim();
        if title.is_empty() {
            return Err(bad("Title is required"));
        }
        if self.end_at < self.start_at {
            return Err(bad("The event must end after it starts"));
        }
        Ok(title)
    }

    fn color(&self) -> &str {
        COLORS.iter().copied().find(|c| *c == self.color).unwrap_or(COLORS[0])
    }
}

pub async fn create(app: St, user: User, Json(r): Json<EventReq>) -> Res<Json<Value>> {
    let id = uuid();
    app.db
        .execute(
            "INSERT INTO events (id, user_id, title, description, location, start_at, end_at, all_day, color) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![id.as_str(), user.id.as_str(), r.title()?, r.description.trim(), r.location.trim(), r.start_at, r.end_at, r.all_day as i64, r.color()],
        )
        .await?;
    Ok(Json(json!({ "id": id })))
}

pub async fn update(app: St, user: User, Path(id): Path<String>, Json(r): Json<EventReq>) -> Res<Json<Value>> {
    exec(
        &app.db,
        "UPDATE events SET title = ?3, description = ?4, location = ?5, start_at = ?6, end_at = ?7, all_day = ?8, color = ?9, \
         updated = unixepoch() WHERE id = ?1 AND user_id = ?2",
        params![id.as_str(), user.id.as_str(), r.title()?, r.description.trim(), r.location.trim(), r.start_at, r.end_at, r.all_day as i64, r.color()],
    )
    .await?;
    let Some(g) = google(&app, &user.id).await? else { return Ok(Json(json!({ "ok": true }))) };
    let ev = outgoing(&app, &user, &id).await?;
    if ev.linked_to(&g.email).is_some() {
        upload(&app, &g, &id, &ev).await?;
    }
    Ok(Json(json!({ "ok": true })))
}

pub async fn remove(app: St, user: User, Path(id): Path<String>) -> Res<Json<Value>> {
    let ev = outgoing(&app, &user, &id).await?;
    if let Some(g) = google(&app, &user.id).await?
        && let Some(gid) = ev.linked_to(&g.email)
    {
        let res = g.http.delete(format!("{EVENTS_URL}/{gid}")).bearer_auth(&g.token).send().await.map_err(upstream)?;
        if !res.status().is_success() && !matches!(res.status().as_u16(), 404 | 410) {
            return Err(upstream(res.status()));
        }
    }
    exec(&app.db, "DELETE FROM events WHERE id = ?1 AND user_id = ?2", params![id.as_str(), user.id.as_str()]).await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn push(app: St, user: User, Path(id): Path<String>) -> Res<Json<Value>> {
    let g = google(&app, &user.id).await?.ok_or_else(|| bad("Connect Google Calendar first"))?;
    let ev = outgoing(&app, &user, &id).await?;
    upload(&app, &g, &id, &ev).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct Outgoing {
    title: String,
    description: String,
    location: String,
    all_day: i64,
    start_time: String,
    end_time: String,
    start_date: String,
    end_date: String,
    google_id: Option<String>,
    google_account: Option<String>,
}

impl Outgoing {
    fn linked_to(&self, email: &str) -> Option<&str> {
        self.google_id.as_deref().filter(|_| self.google_account.as_deref() == Some(email))
    }

    fn body(&self, public_url: &str) -> Value {
        let (start, end) = if self.all_day == 1 {
            (json!({ "date": self.start_date }), json!({ "date": self.end_date }))
        } else {
            (json!({ "dateTime": self.start_time }), json!({ "dateTime": self.end_time }))
        };
        json!({ "summary": self.title, "description": google_html(&self.description, public_url), "location": self.location, "start": start, "end": end })
    }
}

fn google_html(markdown: &str, public_url: &str) -> String {
    let absolute = |url: &str| if url.starts_with('/') { format!("{public_url}{url}") } else { url.to_owned() };
    let events = Parser::new_ext(markdown, Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS).map(|e| match e {
        Md::Start(Tag::Image { dest_url, title, id, .. }) | Md::Start(Tag::Link { dest_url, title, id, .. }) => {
            Md::Start(Tag::Link { link_type: LinkType::Inline, dest_url: absolute(&dest_url).into(), title, id })
        }
        Md::End(TagEnd::Image) => Md::End(TagEnd::Link),
        Md::Html(raw) | Md::InlineHtml(raw) => Md::Text(raw),
        other => other,
    });
    let mut out = String::new();
    html::push_html(&mut out, events);
    out
}

const OUTGOING: &str = "SELECT title, description, location, all_day, \
         strftime('%Y-%m-%dT%H:%M:%SZ', start_at, 'unixepoch') AS start_time, \
         strftime('%Y-%m-%dT%H:%M:%SZ', end_at, 'unixepoch') AS end_time, \
         date(start_at, 'unixepoch') AS start_date, date(end_at, 'unixepoch', '+1 day') AS end_date, \
         google_id, google_account FROM events WHERE id = ?1 AND user_id = ?2";

async fn outgoing(app: &App, user: &User, id: &str) -> Res<Outgoing> {
    one(&app.db, OUTGOING, params![id, user.id.as_str()]).await
}

#[derive(Deserialize)]
struct Saved {
    id: String,
    #[serde(rename = "htmlLink")]
    html_link: Option<String>,
}

async fn upload(app: &App, g: &Google, id: &str, ev: &Outgoing) -> Res<()> {
    let req = match ev.linked_to(&g.email) {
        Some(gid) => g.http.patch(format!("{EVENTS_URL}/{gid}")),
        None => g.http.post(EVENTS_URL),
    };
    let saved: Saved = read(req.bearer_auth(&g.token).json(&ev.body(&app.cfg.public_url))).await?;
    app.db
        .execute(
            "UPDATE events SET google_id = ?2, google_account = ?3, google_link = ?4 WHERE id = ?1",
            params![id, saved.id, g.email.as_str(), saved.html_link],
        )
        .await?;
    Ok(())
}

fn upstream(e: impl std::fmt::Display) -> AppError {
    eprintln!("google error: {e}");
    err(StatusCode::BAD_GATEWAY, "Google Calendar request failed")
}

async fn read<T: DeserializeOwned>(req: RequestBuilder) -> Res<T> {
    let res = req.send().await.and_then(|r| r.error_for_status()).map_err(upstream)?;
    res.json().await.map_err(upstream)
}

fn google_app(app: &App) -> Res<&GoogleApp> {
    app.cfg.google.as_ref().ok_or_else(|| bad("Google Calendar is not configured on this server"))
}

struct Google {
    http: Client,
    token: String,
    email: String,
}

#[derive(Deserialize)]
struct Link {
    id: String,
    email: String,
    refresh_token: String,
    access_token: Option<String>,
    access_expires: i64,
}

#[derive(Deserialize)]
struct TokenRes {
    access_token: String,
    expires_in: i64,
    refresh_token: Option<String>,
}

async fn google(app: &App, user_id: &str) -> Res<Option<Google>> {
    let sql = "SELECT id, email, refresh_token, access_token, access_expires FROM google_links WHERE user_id = ?1";
    let Some(link) = all::<Link>(&app.db, sql, params![user_id]).await?.pop() else { return Ok(None) };
    let http = Client::new();
    let cached = link.access_token.as_deref().filter(|_| link.access_expires > unix_now() + 60);
    let token = match cached {
        Some(sealed) => app.crypto.open(sealed)?,
        None => refresh(app, &http, &link).await?,
    };
    Ok(Some(Google { http, token, email: link.email }))
}

async fn refresh(app: &App, http: &Client, link: &Link) -> Res<String> {
    let cfg = google_app(app)?;
    let refresh_token = app.crypto.open(&link.refresh_token)?;
    let form = [
        ("client_id", cfg.client_id.as_str()),
        ("client_secret", cfg.client_secret.as_str()),
        ("refresh_token", refresh_token.as_str()),
        ("grant_type", "refresh_token"),
    ];
    let res = http.post(TOKEN_URL).form(&form).send().await.map_err(upstream)?;
    if matches!(res.status().as_u16(), 400 | 401) {
        app.db.execute("DELETE FROM google_links WHERE id = ?1", params![link.id.as_str()]).await?;
        return Err(bad("Google Calendar access expired, connect again"));
    }
    let t: TokenRes = res.error_for_status().map_err(upstream)?.json().await.map_err(upstream)?;
    app.db
        .execute(
            "UPDATE google_links SET access_token = ?2, access_expires = ?3 WHERE id = ?1",
            params![link.id.as_str(), app.crypto.seal(&t.access_token), unix_now() + t.expires_in],
        )
        .await?;
    Ok(t.access_token)
}

pub async fn connect(app: St, user: User) -> Res<Redirect> {
    let cfg = google_app(&app)?;
    let state = app.crypto.seal(&format!("{}:{}", user.id, unix_now()));
    let redirect = app.url("/api/google/callback");
    let url = Url::parse_with_params(
        AUTH_URL,
        [
            ("client_id", cfg.client_id.as_str()),
            ("redirect_uri", redirect.as_str()),
            ("response_type", "code"),
            ("scope", SCOPES),
            ("access_type", "offline"),
            ("prompt", "consent"),
            ("state", state.as_str()),
        ],
    )
    .map_err(upstream)?;
    Ok(Redirect::to(url.as_str()))
}

#[derive(Deserialize)]
pub struct CallbackQ {
    code: Option<String>,
    #[serde(default)]
    state: String,
}

#[derive(Deserialize)]
struct UserInfo {
    email: String,
}

fn state_valid(app: &App, user: &User, state: &str) -> bool {
    let Ok(opened) = app.crypto.open(state) else { return false };
    let Some((id, at)) = opened.split_once(':') else { return false };
    id == user.id && at.parse::<i64>().is_ok_and(|at| unix_now() - at < STATE_SECS)
}

async fn link_account(app: &App, user: &User, q: CallbackQ) -> Res<()> {
    let code = q.code.filter(|_| state_valid(app, user, &q.state)).ok_or_else(|| bad("Google sign-in was cancelled"))?;
    let cfg = google_app(app)?;
    let http = Client::new();
    let redirect = app.url("/api/google/callback");
    let form = [
        ("code", code.as_str()),
        ("client_id", cfg.client_id.as_str()),
        ("client_secret", cfg.client_secret.as_str()),
        ("redirect_uri", redirect.as_str()),
        ("grant_type", "authorization_code"),
    ];
    let t: TokenRes = read(http.post(TOKEN_URL).form(&form)).await?;
    let refresh_token = t.refresh_token.ok_or_else(|| bad("Google did not grant offline access"))?;
    let info: UserInfo = read(http.get(USERINFO_URL).bearer_auth(&t.access_token)).await?;
    app.db
        .execute(
            "INSERT INTO google_links (id, user_id, email, refresh_token, access_token, access_expires) VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
             ON CONFLICT(user_id) DO UPDATE SET email = excluded.email, refresh_token = excluded.refresh_token, \
             access_token = excluded.access_token, access_expires = excluded.access_expires, synced = NULL",
            params![
                uuid(),
                user.id.as_str(),
                info.email,
                app.crypto.seal(&refresh_token),
                app.crypto.seal(&t.access_token),
                unix_now() + t.expires_in
            ],
        )
        .await?;
    Ok(())
}

pub async fn callback(app: St, user: User, Query(q): Query<CallbackQ>) -> Redirect {
    let outcome = match link_account(&app, &user, q).await {
        Ok(()) => "connected",
        Err(e) => {
            eprintln!("google connect failed: {}", e.1);
            "failed"
        }
    };
    Redirect::to(&format!("{}/calendar?google={outcome}", app.cfg.base))
}

pub async fn disconnect(app: St, user: User) -> Res<Json<Value>> {
    let sql = "SELECT id, email, refresh_token, access_token, access_expires FROM google_links WHERE user_id = ?1";
    let link: Link = one(&app.db, sql, params![user.id.as_str()]).await?;
    if let Ok(token) = app.crypto.open(&link.refresh_token) {
        let revoked = Client::new().post(REVOKE_URL).form(&[("token", token)]).send().await;
        if let Err(e) = revoked {
            eprintln!("google revoke failed: {e}");
        }
    }
    exec(&app.db, "DELETE FROM google_links WHERE id = ?1", params![link.id]).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct GoogleTime {
    date: Option<String>,
    #[serde(rename = "dateTime")]
    date_time: Option<String>,
}

#[derive(Deserialize)]
struct GoogleEvent {
    id: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    location: String,
    start: GoogleTime,
    end: GoogleTime,
    #[serde(rename = "htmlLink")]
    html_link: Option<String>,
}

#[derive(Deserialize)]
struct Page {
    #[serde(default)]
    items: Vec<GoogleEvent>,
    #[serde(rename = "nextPageToken")]
    next_page_token: Option<String>,
}

#[derive(Deserialize)]
struct Window {
    time_min: String,
    time_max: String,
    from_at: i64,
    to_at: i64,
}

const WINDOW: &str = "SELECT strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-365 days') AS time_min, \
     strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '+730 days') AS time_max, \
     unixepoch('now', '-365 days') AS from_at, unixepoch('now', '+730 days') AS to_at";

async fn fetch(g: &Google, w: &Window) -> Res<Vec<GoogleEvent>> {
    let mut events = Vec::new();
    let mut page_token: Option<String> = None;
    loop {
        let mut query = vec![
            ("timeMin", w.time_min.as_str()),
            ("timeMax", w.time_max.as_str()),
            ("singleEvents", "true"),
            ("maxResults", "2500"),
        ];
        query.extend(page_token.as_deref().map(|t| ("pageToken", t)));
        let page: Page = read(g.http.get(EVENTS_URL).bearer_auth(&g.token).query(&query)).await?;
        events.extend(page.items);
        page_token = page.next_page_token;
        if page_token.is_none() {
            return Ok(events);
        }
    }
}

const UPSERT: &str = "INSERT INTO events (id, user_id, title, description, location, start_at, end_at, all_day, \
       google_id, google_account, google_link) \
     VALUES (?1, ?2, ?3, ?4, ?5, unixepoch(COALESCE(?6, ?7)), \
       CASE WHEN ?9 IS NULL THEN unixepoch(?8) ELSE unixepoch(?9, '-1 day') END, ?7 IS NOT NULL, ?10, ?11, ?12) \
     ON CONFLICT(user_id, google_id) DO UPDATE SET title = excluded.title, description = excluded.description, \
       location = excluded.location, start_at = excluded.start_at, end_at = excluded.end_at, all_day = excluded.all_day, \
       google_account = excluded.google_account, google_link = excluded.google_link, updated = unixepoch()";

const SWEEP: &str = "DELETE FROM events WHERE user_id = ?1 AND google_account = ?2 AND google_id IS NOT NULL \
     AND start_at BETWEEN ?3 AND ?4 AND google_id NOT IN (SELECT value FROM json_each(?5))";

pub async fn sync(app: St, user: User) -> Res<Json<Value>> {
    let g = google(&app, &user.id).await?.ok_or_else(|| bad("Connect Google Calendar first"))?;
    let window: Window = one(&app.db, WINDOW, ()).await?;
    let events = fetch(&g, &window).await?;
    let ids: Vec<&str> = events.iter().map(|e| e.id.as_str()).collect();
    let tx = app.db.transaction().await?;
    for e in &events {
        let title = if e.summary.trim().is_empty() { "(No title)" } else { e.summary.trim() };
        let p = params![
            uuid(),
            user.id.as_str(),
            title,
            e.description.as_str(),
            e.location.as_str(),
            e.start.date_time.as_deref(),
            e.start.date.as_deref(),
            e.end.date_time.as_deref(),
            e.end.date.as_deref(),
            e.id.as_str(),
            g.email.as_str(),
            e.html_link.as_deref()
        ];
        tx.execute(UPSERT, p).await?;
    }
    tx.execute(
        SWEEP,
        params![user.id.as_str(), g.email.as_str(), window.from_at, window.to_at, serde_json::to_string(&ids)?],
    )
    .await?;
    tx.execute("UPDATE google_links SET synced = unixepoch() WHERE user_id = ?1", params![user.id.as_str()]).await?;
    tx.commit().await?;
    Ok(Json(json!({ "count": events.len() })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn google_html_links_images() {
        let out = google_html("**Hi** ![photo](/rk/api/files/1/raw) <b>x</b>", "https://a.b");
        assert_eq!(out, "<p><strong>Hi</strong> <a href=\"https://a.b/rk/api/files/1/raw\">photo</a> &lt;b&gt;x&lt;/b&gt;</p>\n");
    }

    #[tokio::test]
    async fn google_times_roundtrip() {
        let db = libsql::Builder::new_local(":memory:").build().await.unwrap().connect().unwrap();
        crate::migrate::migrate(&db, crate::SCHEMA, None).await.unwrap_or_else(|e| panic!("{}", e.1));
        db.execute("INSERT INTO users (id, email, name, pass) VALUES ('u', 'a@b.c', 'A', 'x')", ()).await.unwrap();
        let none: Option<&str> = None;
        let timed = params!["e1", "u", "Meet", "", "", "2026-10-07T10:00:00+03:00", none, "2026-10-07T11:30:00+03:00", none, "g1", "a@b.c", none];
        let day = params!["e2", "u", "Trip", "", "", none, "2026-10-07", none, "2026-10-10", "g2", "a@b.c", none];
        db.execute(UPSERT, timed).await.unwrap();
        db.execute(UPSERT, day).await.unwrap();
        let ev: Outgoing = one(&db, OUTGOING, params!["e1", "u"]).await.ok().unwrap();
        assert_eq!((ev.start_time.as_str(), ev.end_time.as_str()), ("2026-10-07T07:00:00Z", "2026-10-07T08:30:00Z"));
        let ev: Outgoing = one(&db, OUTGOING, params!["e2", "u"]).await.ok().unwrap();
        assert_eq!((ev.all_day, ev.start_date.as_str(), ev.end_date.as_str()), (1, "2026-10-07", "2026-10-10"));
        db.execute(SWEEP, params!["u", "a@b.c", 0, i64::MAX, r#"["g1"]"#]).await.unwrap();
        let left: Vec<Saved> = all(&db, "SELECT google_id AS id, NULL AS htmlLink FROM events", ()).await.ok().unwrap();
        assert_eq!(left.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), ["g1"]);
    }
}
