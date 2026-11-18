#![forbid(unsafe_code)]

mod auth;
mod board;
mod calendar;
mod convert;
mod dashboard;
mod export;
mod fields;
mod files;
mod folders;
mod images;
mod markdown;
mod migrate;
mod notes;
mod projects;
mod reminders;
mod requests;
mod reset;
mod signup;
mod sites;
mod storage;
mod templates;
mod util;
mod vault;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::http::{StatusCode, Uri, header};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{delete, get, post, put};
use tower_http::services::ServeDir;

pub struct Config {
    pub base: String,
    pub public_url: String,
    pub data_dir: PathBuf,
    pub resend_key: Option<String>,
    pub mail_from: String,
    pub verify_email: String,
    pub secure_cookie: bool,
    pub google: Option<calendar::GoogleApp>,
}

pub struct App {
    pub db: libsql::Connection,
    pub cfg: Config,
    pub crypto: util::Crypto,
    pub storage: storage::Storage,
    pub login_attempts: Mutex<HashMap<String, (u32, i64)>>,
    pub index: String,
    _database: libsql::Database,
}

impl App {
    pub fn url(&self, path: &str) -> String {
        format!("{}{}{}", self.cfg.public_url, self.cfg.base, path)
    }

    pub async fn mail(&self, to: &str, subject: &str, html: String) -> bool {
        util::send_mail(self.cfg.resend_key.as_deref(), &self.cfg.mail_from, to, subject, html).await
    }
}

pub type St = axum::extract::State<Arc<App>>;

pub const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS users (
  id TEXT PRIMARY KEY, email TEXT NOT NULL UNIQUE, name TEXT NOT NULL, pass TEXT NOT NULL,
  approved INTEGER NOT NULL DEFAULT 0, approve_token TEXT, pin TEXT,
  pin_fails INTEGER NOT NULL DEFAULT 0, pin_block INTEGER NOT NULL DEFAULT 0,
  created INTEGER NOT NULL DEFAULT (unixepoch()), avatar BLOB);
CREATE TABLE IF NOT EXISTS sessions (
  token TEXT PRIMARY KEY, user_id TEXT NOT NULL REFERENCES users(id), expires INTEGER NOT NULL, pin_until INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS projects (
  id TEXT PRIMARY KEY, user_id TEXT NOT NULL REFERENCES users(id), name TEXT NOT NULL, description TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL DEFAULT 'active', created INTEGER NOT NULL DEFAULT (unixepoch()),
  archived INTEGER NOT NULL DEFAULT 0, share_token TEXT UNIQUE, share_expires INTEGER);
CREATE TABLE IF NOT EXISTS board_columns (
  id TEXT PRIMARY KEY, project_id TEXT NOT NULL REFERENCES projects(id), name TEXT NOT NULL, position INTEGER NOT NULL,
  done INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS tasks (
  id TEXT PRIMARY KEY, project_id TEXT NOT NULL REFERENCES projects(id),
  column_id TEXT NOT NULL REFERENCES board_columns(id), title TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '', due TEXT, created INTEGER NOT NULL DEFAULT (unixepoch()),
  position INTEGER NOT NULL DEFAULT 0, priority INTEGER NOT NULL DEFAULT 0, archived INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS subtasks (
  id TEXT PRIMARY KEY, task_id TEXT NOT NULL REFERENCES tasks(id), title TEXT NOT NULL,
  done INTEGER NOT NULL DEFAULT 0, position INTEGER NOT NULL DEFAULT 0, created INTEGER NOT NULL DEFAULT (unixepoch()),
  description TEXT NOT NULL DEFAULT '', due TEXT, priority INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS custom_fields (
  id TEXT PRIMARY KEY, project_id TEXT NOT NULL REFERENCES projects(id), name TEXT NOT NULL,
  position INTEGER NOT NULL DEFAULT 0, created INTEGER NOT NULL DEFAULT (unixepoch()));
CREATE TABLE IF NOT EXISTS task_field_values (
  id TEXT PRIMARY KEY, task_id TEXT NOT NULL REFERENCES tasks(id), field_id TEXT NOT NULL REFERENCES custom_fields(id),
  value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS notes (
  id TEXT PRIMARY KEY, user_id TEXT NOT NULL REFERENCES users(id), project_id TEXT REFERENCES projects(id),
  title TEXT NOT NULL DEFAULT '',
  body TEXT NOT NULL DEFAULT '', secret INTEGER NOT NULL DEFAULT 0, archived INTEGER NOT NULL DEFAULT 0,
  trashed_at INTEGER, rev INTEGER NOT NULL DEFAULT 0, rev_client TEXT, updated INTEGER NOT NULL DEFAULT (unixepoch()),
  share_token TEXT UNIQUE, share_expires INTEGER, pinned INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS folders (
  id TEXT PRIMARY KEY, user_id TEXT NOT NULL REFERENCES users(id), parent_id TEXT REFERENCES folders(id),
  name TEXT NOT NULL, created INTEGER NOT NULL DEFAULT (unixepoch()), starred INTEGER NOT NULL DEFAULT 0,
  share_token TEXT UNIQUE, share_expires INTEGER, share_uploads INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS files (
  id TEXT PRIMARY KEY, user_id TEXT NOT NULL REFERENCES users(id), project_id TEXT REFERENCES projects(id),
  folder_id TEXT REFERENCES folders(id), name TEXT NOT NULL, mime TEXT NOT NULL,
  size INTEGER NOT NULL, key TEXT NOT NULL, created INTEGER NOT NULL DEFAULT (unixepoch()),
  share_token TEXT UNIQUE, share_expires INTEGER, archived INTEGER NOT NULL DEFAULT 0, starred INTEGER NOT NULL DEFAULT 0,
  upload_link_id TEXT REFERENCES upload_links(id), uploader TEXT, upload_note TEXT);
CREATE TABLE IF NOT EXISTS upload_links (
  id TEXT PRIMARY KEY, user_id TEXT NOT NULL REFERENCES users(id), folder_id TEXT REFERENCES folders(id),
  token TEXT NOT NULL UNIQUE, title TEXT NOT NULL, description TEXT NOT NULL DEFAULT '', password TEXT,
  expires INTEGER, active INTEGER NOT NULL DEFAULT 1, created INTEGER NOT NULL DEFAULT (unixepoch()));
CREATE TABLE IF NOT EXISTS vault (
  id TEXT PRIMARY KEY, user_id TEXT NOT NULL REFERENCES users(id), name TEXT NOT NULL, value TEXT NOT NULL,
  note TEXT NOT NULL DEFAULT '', created INTEGER NOT NULL DEFAULT (unixepoch()));
CREATE TABLE IF NOT EXISTS reminders (
  id TEXT PRIMARY KEY, user_id TEXT NOT NULL REFERENCES users(id), note_id TEXT REFERENCES notes(id), title TEXT NOT NULL,
  remind_at INTEGER NOT NULL, sent INTEGER NOT NULL DEFAULT 0, archived INTEGER NOT NULL DEFAULT 0,
  description TEXT NOT NULL DEFAULT '', utc_offset INTEGER NOT NULL DEFAULT 0, retry_at INTEGER, task_id TEXT REFERENCES tasks(id));
CREATE TABLE IF NOT EXISTS registrations (
  token TEXT PRIMARY KEY, user_id TEXT NOT NULL REFERENCES users(id), ip TEXT NOT NULL, agent TEXT NOT NULL, language TEXT NOT NULL,
  origin TEXT NOT NULL, created INTEGER NOT NULL DEFAULT (unixepoch()));
CREATE TABLE IF NOT EXISTS password_resets (
  token TEXT PRIMARY KEY, user_id TEXT NOT NULL REFERENCES users(id), expires INTEGER NOT NULL,
  created INTEGER NOT NULL DEFAULT (unixepoch()));
CREATE TABLE IF NOT EXISTS blocked_emails (email TEXT PRIMARY KEY, created INTEGER NOT NULL DEFAULT (unixepoch()));
CREATE TABLE IF NOT EXISTS events (
  id TEXT PRIMARY KEY, user_id TEXT NOT NULL REFERENCES users(id), title TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '', location TEXT NOT NULL DEFAULT '',
  start_at INTEGER NOT NULL, end_at INTEGER NOT NULL, all_day INTEGER NOT NULL DEFAULT 0,
  color TEXT NOT NULL DEFAULT 'blue', google_id TEXT, google_account TEXT, google_link TEXT,
  created INTEGER NOT NULL DEFAULT (unixepoch()), updated INTEGER NOT NULL DEFAULT (unixepoch()));
CREATE TABLE IF NOT EXISTS google_links (
  id TEXT PRIMARY KEY, user_id TEXT NOT NULL UNIQUE REFERENCES users(id), email TEXT NOT NULL,
  refresh_token TEXT NOT NULL, access_token TEXT, access_expires INTEGER NOT NULL DEFAULT 0, synced INTEGER,
  created INTEGER NOT NULL DEFAULT (unixepoch()));
CREATE TABLE IF NOT EXISTS sites (
  id TEXT PRIMARY KEY, user_id TEXT NOT NULL REFERENCES users(id), name TEXT NOT NULL UNIQUE,
  enabled INTEGER NOT NULL DEFAULT 1, password TEXT, expires INTEGER, views INTEGER NOT NULL DEFAULT 0,
  created INTEGER NOT NULL DEFAULT (unixepoch()), updated INTEGER NOT NULL DEFAULT (unixepoch()));
CREATE TABLE IF NOT EXISTS site_files (
  id TEXT PRIMARY KEY, site_id TEXT NOT NULL REFERENCES sites(id), path TEXT NOT NULL, key TEXT NOT NULL,
  size INTEGER NOT NULL, mime TEXT NOT NULL, updated INTEGER NOT NULL DEFAULT (unixepoch()),
  created INTEGER NOT NULL DEFAULT (unixepoch()));
CREATE INDEX IF NOT EXISTS notes_user ON notes(user_id);
CREATE INDEX IF NOT EXISTS files_user ON files(user_id, folder_id);
CREATE INDEX IF NOT EXISTS folders_parent ON folders(parent_id);
CREATE INDEX IF NOT EXISTS upload_links_user ON upload_links(user_id);
CREATE INDEX IF NOT EXISTS reminders_due ON reminders(sent, remind_at);
CREATE INDEX IF NOT EXISTS tasks_project ON tasks(project_id, column_id, position);
CREATE INDEX IF NOT EXISTS subtasks_task ON subtasks(task_id, position);
CREATE INDEX IF NOT EXISTS custom_fields_project ON custom_fields(project_id, position);
CREATE INDEX IF NOT EXISTS task_field_values_task ON task_field_values(task_id);
CREATE INDEX IF NOT EXISTS events_user ON events(user_id, start_at);
CREATE UNIQUE INDEX IF NOT EXISTS events_google ON events(user_id, google_id);
CREATE INDEX IF NOT EXISTS sites_user ON sites(user_id);
CREATE UNIQUE INDEX IF NOT EXISTS site_files_path ON site_files(site_id, path);
";

fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

fn secret_key(data_dir: &std::path::Path) -> String {
    if let Some(k) = env("SECRET_KEY") {
        return k;
    }
    let remote = env("TURSO_DATABASE_URL").is_some_and(|u| u.contains("://"));
    assert!(!remote, "SECRET_KEY is required when using a remote database");
    let path = data_dir.join("secret.key");
    std::fs::read_to_string(&path).unwrap_or_else(|_| {
        let k = util::token();
        std::fs::write(&path, &k).expect("write secret.key");
        k
    })
}

async fn open_db(data_dir: &std::path::Path) -> libsql::Database {
    let url = env("TURSO_DATABASE_URL").unwrap_or_else(|| data_dir.join("rustkeep.db").to_string_lossy().into_owned());
    let built = if url.contains("://") {
        libsql::Builder::new_remote(url, env("TURSO_AUTH_TOKEN").unwrap_or_default()).build().await
    } else {
        libsql::Builder::new_local(url).build().await
    };
    built.expect("open database")
}

fn spa_page(index: &str, uri: &Uri) -> Response {
    if Path::new(uri.path()).extension().is_some() {
        return (StatusCode::NOT_FOUND, [(header::CACHE_CONTROL, "no-store")]).into_response();
    }
    Html(index.to_owned()).into_response()
}

fn api() -> Router<Arc<App>> {
    Router::new()
        .route("/api/auth/register", post(signup::register))
        .route("/api/auth/login", post(auth::login))
        .route("/api/auth/logout", post(auth::logout))
        .route("/api/auth/forgot", post(reset::forgot))
        .route("/api/auth/reset", post(reset::reset))
        .route("/api/auth/approve/{token}", get(signup::review).post(signup::approve))
        .route("/api/auth/reject/{token}", post(signup::reject))
        .route("/api/me", get(auth::me).put(auth::update_profile))
        .route("/api/me/password", put(auth::change_password))
        .route("/api/me/stats", get(auth::stats))
        .route(
            "/api/me/avatar",
            get(auth::avatar).put(auth::set_avatar).delete(auth::remove_avatar).layer(DefaultBodyLimit::max(auth::AVATAR_UPLOAD_LIMIT)),
        )
        .route("/api/pin", post(auth::set_pin))
        .route("/api/pin/unlock", post(auth::unlock))
        .route("/api/pin/lock", post(auth::lock))
        .route("/api/notes", get(notes::list).post(notes::create))
        .route("/api/notes/{id}", get(notes::get).put(notes::update).delete(notes::remove))
        .route("/api/notes/{id}/action/{action}", post(notes::action))
        .route("/api/notes/{id}/export/{format}", get(notes::export))
        .route("/api/notes/{id}/share", post(notes::share).delete(notes::unshare))
        .route("/api/public/notes/{token}", get(notes::public_get).put(notes::public_update))
        .route("/api/files", get(files::list).post(files::upload).layer(DefaultBodyLimit::disable()))
        .route("/api/files/conflicts", post(files::conflicts))
        .route("/api/files/{id}", put(files::update).delete(files::remove))
        .route("/api/files/{id}/action/{action}", post(files::action))
        .route("/api/files/{id}/raw", get(files::raw))
        .route("/api/files/{id}/preview", get(files::preview))
        .route("/api/files/{id}/content", put(files::put_content).layer(DefaultBodyLimit::disable()))
        .route("/api/files/{id}/convert", get(files::formats).post(files::convert))
        .route("/api/files/{id}/share", post(files::share).delete(files::unshare))
        .route("/api/public/files/{token}", get(files::public_raw))
        .route("/api/folders", get(folders::list).post(folders::create))
        .route("/api/folders/{id}", put(folders::update).delete(folders::remove))
        .route("/api/folders/{id}/share", post(folders::share).delete(folders::unshare))
        .route("/api/public/folders/{token}", get(folders::public_list))
        .route("/api/public/folders/{token}/files/{id}", get(folders::public_raw))
        .route("/api/public/folders/{token}/upload", post(folders::public_upload).layer(DefaultBodyLimit::disable()))
        .route("/api/upload-links", get(requests::list).post(requests::create))
        .route("/api/upload-links/{id}", put(requests::update).delete(requests::remove))
        .route("/api/public/uploads/{token}", get(requests::public_get).post(requests::public_upload).layer(DefaultBodyLimit::disable()))
        .route("/api/public/uploads/{token}/unlock", post(requests::unlock))
        .route("/api/sites", get(sites::list).post(sites::create))
        .route("/api/sites/{id}", put(sites::update).delete(sites::remove))
        .route("/api/sites/{id}/files", get(sites::files).post(sites::upload).layer(DefaultBodyLimit::disable()))
        .route("/api/sites/{id}/file", get(sites::raw).put(sites::put_file).layer(DefaultBodyLimit::disable()))
        .route("/api/sites/{id}/move", post(sites::move_files))
        .route("/api/sites/{id}/delete", post(sites::delete_files))
        .route("/api/sites/{id}/preview", get(sites::preview))
        .route("/api/public/sites/{name}", get(sites::public_get))
        .route("/api/public/sites/{name}/unlock", post(sites::unlock))
        .route("/site/{name}", get(sites::redirect))
        .route("/site/{name}/", get(sites::serve))
        .route("/site/{name}/{*path}", get(sites::serve))
        .route("/preview/s/{token}", get(sites::redirect))
        .route("/preview/s/{token}/", get(sites::preview_serve))
        .route("/preview/s/{token}/{*path}", get(sites::preview_serve))
        .route("/preview/f/{token}/{*path}", get(files::preview_serve))
        .route("/api/vault", get(vault::list).post(vault::create))
        .route("/api/vault/{id}", get(vault::reveal).delete(vault::remove))
        .route("/api/reminders", get(reminders::list).post(reminders::create))
        .route("/api/reminders/{id}", put(reminders::update).delete(reminders::remove))
        .route("/api/reminders/{id}/action/{action}", post(reminders::action))
        .route("/api/projects", get(projects::list).post(projects::create))
        .route("/api/projects/{id}", get(projects::get).put(projects::update).delete(projects::remove))
        .route("/api/projects/{id}/export", get(projects::export))
        .route("/api/projects/{id}/action/{action}", post(projects::action))
        .route("/api/projects/{id}/share", post(projects::share).delete(projects::unshare))
        .route("/api/public/projects/{token}", get(board::public_board))
        .route("/api/public/projects/{token}/files/{id}/raw", get(board::public_raw))
        .route("/api/projects/{id}/columns", get(board::columns).post(board::create_column))
        .route("/api/projects/{id}/tasks", get(board::tasks).post(board::create_task))
        .route("/api/board/columns", get(board::overview_columns))
        .route("/api/board/tasks", get(board::overview_tasks))
        .route("/api/projects/{id}/fields", get(fields::list).post(fields::create))
        .route("/api/fields/{id}", put(fields::rename).delete(fields::remove))
        .route("/api/columns/{id}", put(board::rename_column).delete(board::remove_column))
        .route("/api/columns/{id}/move", post(board::move_column))
        .route("/api/columns/{id}/done", put(board::set_column_done))
        .route("/api/tasks/{id}", put(board::update_task).delete(board::remove_task))
        .route("/api/tasks/{id}/move", post(board::move_task))
        .route("/api/tasks/{id}/action/{action}", post(board::task_action))
        .route("/api/tasks/{id}/subtasks", get(board::subtasks).post(board::create_subtask))
        .route("/api/subtasks/{id}", put(board::update_subtask).delete(board::remove_subtask))
        .route("/api/dashboard", get(dashboard::get))
        .route("/api/calendar", get(calendar::get))
        .route("/api/calendar/sync", post(calendar::sync))
        .route("/api/events", post(calendar::create))
        .route("/api/events/{id}", put(calendar::update).delete(calendar::remove))
        .route("/api/events/{id}/google", post(calendar::push))
        .route("/api/google", delete(calendar::disconnect))
        .route("/api/google/connect", get(calendar::connect))
        .route("/api/google/callback", get(calendar::callback))
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    dotenvy::dotenv().ok();
    let data_dir = PathBuf::from(env("DATA_DIR").unwrap_or_else(|| "data".into()));
    std::fs::create_dir_all(data_dir.join("files")).expect("create data dir");
    let public_url = env("PUBLIC_URL").unwrap_or_else(|| "http://localhost:8080".into());
    let cfg = Config {
        base: env("BASE_PATH").unwrap_or_default().trim_end_matches('/').to_owned(),
        secure_cookie: public_url.starts_with("https://"),
        public_url: public_url.trim_end_matches('/').to_owned(),
        resend_key: env("RESEND_API_KEY"),
        mail_from: env("MAIL_FROM").expect("MAIL_FROM is required"),
        verify_email: env("VERIFY_EMAIL").expect("VERIFY_EMAIL is required"),
        google: env("GOOGLE_CLIENT_ID")
            .zip(env("GOOGLE_CLIENT_SECRET"))
            .map(|(client_id, client_secret)| calendar::GoogleApp { client_id, client_secret }),
        data_dir,
    };

    let database = open_db(&cfg.data_dir).await;
    let db = database.connect().expect("connect database");
    let _ = db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA cache_size=-1024;").await;
    let local = !env("TURSO_DATABASE_URL").is_some_and(|u| u.contains("://"));
    let backup = local.then(|| cfg.data_dir.join(format!("rustkeep-backup-{}.db", auth::unix_now())));
    if let Err(e) = migrate::migrate(&db, SCHEMA, backup.as_deref()).await {
        panic!("schema migration failed: {}", e.1);
    }
    db.execute("UPDATE site_files SET created = updated WHERE created > updated", ()).await.expect("align site file dates");

    let static_dir = PathBuf::from(env("STATIC_DIR").unwrap_or_else(|| "web/dist".into()));
    let index = std::fs::read_to_string(static_dir.join("index.html"))
        .unwrap_or_else(|_| "RustKeep: web/dist not found, run `trunk build` first.".into())
        .replace("__BASE__", &cfg.base);
    let page = {
        let index = index.clone();
        move || std::future::ready(Html(index.clone()))
    };
    let spa = ServeDir::new(static_dir)
        .precompressed_gzip()
        .append_index_html_on_directories(false)
        .fallback(get({
            let index = index.clone();
            move |uri: Uri| std::future::ready(spa_page(&index, &uri))
        }));

    let port = env("PORT").unwrap_or_else(|| "8080".into());
    let base = cfg.base.clone();
    let crypto = util::Crypto::new(&secret_key(&cfg.data_dir));
    let storage = storage::Storage::new(cfg.data_dir.join("files"));
    let app = Arc::new(App { db, crypto, storage, cfg, login_attempts: Mutex::default(), index, _database: database });
    tokio::spawn(reminders::run(app.clone()));

    let routes = api().route("/", get(page.clone())).fallback_service(spa).with_state(app);
    let routes = if base.is_empty() {
        routes
    } else {
        let target = base.clone();
        Router::new()
            .route("/", get(move || std::future::ready(Redirect::to(&target))))
            .route(&format!("{base}/"), get(page))
            .nest(&base, routes)
    };

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await.expect("bind");
    println!("RustKeep listening on :{port}{base}");
    axum::serve(listener, routes.into_make_service_with_connect_info::<std::net::SocketAddr>()).await.expect("serve");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spa_page_serves_index_for_routes_and_404_for_missing_assets() {
        let status = |path: &str| spa_page("index", &path.parse().unwrap()).status();
        assert_eq!(status("/notes/0b5c"), StatusCode::OK);
        assert_eq!(status("/s/abc_DEF-123"), StatusCode::OK);
        assert_eq!(status("/output-163047b258112fe9.css"), StatusCode::NOT_FOUND);
        assert_eq!(status("/web-f826d70aaa6867d9_bg.wasm"), StatusCode::NOT_FOUND);
    }
}
