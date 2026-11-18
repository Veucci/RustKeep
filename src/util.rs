use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};
use axum::Json;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use chacha20poly1305::aead::{Aead, AeadCore, KeyInit, OsRng};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use hmac::{Hmac, Mac};
use libsql::Connection;
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};

#[derive(Debug)]
pub struct AppError(pub StatusCode, pub String);
pub type Res<T> = Result<T, AppError>;

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({ "error": self.1 }))).into_response()
    }
}

impl<E: std::error::Error> From<E> for AppError {
    fn from(e: E) -> Self {
        eprintln!("error: {e}");
        AppError(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error".into())
    }
}

pub fn err(code: StatusCode, msg: &str) -> AppError {
    AppError(code, msg.into())
}

pub fn not_found() -> AppError {
    err(StatusCode::NOT_FOUND, "Not found")
}

pub fn bad(msg: &str) -> AppError {
    err(StatusCode::BAD_REQUEST, msg)
}

pub fn archive_set(action: &str) -> Res<&'static str> {
    match action {
        "archive" => Ok("archived = 1"),
        "unarchive" => Ok("archived = 0"),
        _ => Err(bad("Invalid action")),
    }
}

pub fn token() -> String {
    hex::encode(rand::random::<[u8; 24]>())
}

pub fn uuid() -> String {
    let mut b: [u8; 16] = rand::random();
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h = hex::encode(b);
    format!("{}-{}-{}-{}-{}", &h[..8], &h[8..12], &h[12..16], &h[16..20], &h[20..])
}

pub const PREVIEW_SECS: i64 = 3600;

const URL_PATH: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'.').remove(b'_').remove(b'~').remove(b'/');

pub fn encode_path(path: &str) -> String {
    utf8_percent_encode(path, URL_PATH).to_string()
}

pub fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

pub fn cookie(h: &HeaderMap, name: &str) -> Option<String> {
    h.get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|c| c.trim().strip_prefix(name)?.strip_prefix('=').map(str::to_owned))
}

pub async fn all<T: DeserializeOwned>(db: &Connection, sql: &str, p: impl libsql::params::IntoParams) -> Res<Vec<T>> {
    let mut rows = db.query(sql, p).await?;
    let mut out = Vec::new();
    while let Some(row) = rows.next().await? {
        out.push(libsql::de::from_row(&row)?);
    }
    Ok(out)
}

pub async fn one<T: DeserializeOwned>(db: &Connection, sql: &str, p: impl libsql::params::IntoParams) -> Res<T> {
    all(db, sql, p).await?.into_iter().next().ok_or_else(not_found)
}

pub async fn exec(db: &Connection, sql: &str, p: impl libsql::params::IntoParams) -> Res<u64> {
    let n = db.execute(sql, p).await?;
    if n == 0 {
        return Err(not_found());
    }
    Ok(n)
}

fn argon() -> Argon2<'static> {
    Argon2::new(Algorithm::Argon2id, Version::V0x13, Params::new(7168, 5, 1, None).expect("argon params"))
}

pub fn hash(p: &str) -> String {
    argon().hash_password(p.as_bytes(), &SaltString::generate(&mut OsRng)).expect("hash").to_string()
}

pub fn verify(p: &str, h: &str) -> bool {
    PasswordHash::new(h).is_ok_and(|h| argon().verify_password(p.as_bytes(), &h).is_ok())
}

pub struct Crypto(ChaCha20Poly1305, Vec<u8>);

impl Crypto {
    pub fn new(secret: &str) -> Self {
        let cipher = ChaCha20Poly1305::new_from_slice(&Sha256::digest(secret)).expect("key");
        Self(cipher, Sha256::digest(format!("mac:{secret}")).to_vec())
    }

    pub fn mac(&self, data: &[u8]) -> Hmac<Sha256> {
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&self.1).expect("HMAC accepts any key length");
        mac.update(data);
        mac
    }

    pub fn sign_preview(&self, kind: &str, scope: &str, expires: i64) -> String {
        let sig = self.mac(format!("preview:{kind}:{scope}:{expires}").as_bytes()).finalize().into_bytes();
        format!("{scope}.{expires}.{}", hex::encode(sig))
    }

    pub fn check_preview(&self, kind: &str, token: &str, now: i64) -> Option<String> {
        let mut parts = token.rsplitn(3, '.');
        let (sig, expires, scope) = (parts.next()?, parts.next()?, parts.next()?);
        let fresh = expires.parse::<i64>().is_ok_and(|e| e > now);
        let sig = hex::decode(sig).ok().filter(|_| fresh)?;
        self.mac(format!("preview:{kind}:{scope}:{expires}").as_bytes()).verify_slice(&sig).ok()?;
        Some(scope.to_owned())
    }

    pub fn seal(&self, plain: &str) -> String {
        let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
        let mut out = nonce.to_vec();
        out.extend(self.0.encrypt(&nonce, plain.as_bytes()).expect("encrypt"));
        B64.encode(out)
    }

    pub fn open(&self, sealed: &str) -> Res<String> {
        let corrupt = || err(StatusCode::INTERNAL_SERVER_ERROR, "Decryption failed");
        let raw = B64.decode(sealed).map_err(|_| corrupt())?;
        if raw.len() < 12 {
            return Err(corrupt());
        }
        let (nonce, ct) = raw.split_at(12);
        let plain = self.0.decrypt(Nonce::from_slice(nonce), ct).map_err(|_| corrupt())?;
        String::from_utf8(plain).map_err(|_| corrupt())
    }
}

pub async fn send_mail(key: Option<&str>, from: &str, to: &str, subject: &str, html: String) -> bool {
    let Some(key) = key else {
        println!("[mail:dev] to={to} subject={subject}\n{html}");
        return true;
    };
    let body = serde_json::json!({ "from": from, "to": [to], "subject": subject, "html": html });
    let res = reqwest::Client::new()
        .post("https://api.resend.com/emails")
        .bearer_auth(key)
        .json(&body)
        .send()
        .await
        .and_then(|r| r.error_for_status());
    if let Err(e) = &res {
        eprintln!("mail error: {e}");
    }
    res.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crypto_roundtrip() {
        let c = Crypto::new("k");
        let s = c.seal("secret");
        assert_eq!(c.open(&s).ok().as_deref(), Some("secret"));
        assert!(Crypto::new("other").open(&s).is_err());
    }

    #[test]
    fn preview_tokens() {
        let c = Crypto::new("k");
        let t = c.sign_preview("f", "user:root", 1000);
        assert_eq!(c.check_preview("f", &t, 999).as_deref(), Some("user:root"));
        assert!(c.check_preview("f", &t, 1000).is_none());
        assert!(c.check_preview("s", &t, 999).is_none());
        assert!(Crypto::new("other").check_preview("f", &t, 999).is_none());
        assert!(c.check_preview("f", &t.replace("user:root", "user:other"), 999).is_none());
        assert!(c.check_preview("f", &t.replacen("1000", "2000", 1), 999).is_none());
        assert!(c.check_preview("f", "garbage", 0).is_none());
        assert_eq!(encode_path("a b/c#d.html"), "a%20b/c%23d.html");
    }

    #[test]
    fn uuid_v4() {
        let u = uuid();
        assert_eq!(u.len(), 36);
        assert_eq!(&u[14..15], "4");
        assert_ne!(u, uuid());
    }

    #[test]
    fn cookie_parse() {
        let mut h = HeaderMap::new();
        h.insert(header::COOKIE, "a=1; rk_session=abc".parse().unwrap());
        assert_eq!(cookie(&h, "rk_session").as_deref(), Some("abc"));
        assert!(cookie(&h, "rk").is_none());
    }

    #[test]
    fn password() {
        let h = hash("password123");
        assert!(verify("password123", &h) && !verify("x", &h));
    }
}
