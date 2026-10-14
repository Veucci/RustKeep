use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::extract::Request;
use axum::extract::multipart::Field;
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::response::Response;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tower::ServiceExt;
use tower_http::services::ServeFile;

use crate::util::{Res, not_found};

const SIGNED_HEADERS: &str = "host;x-amz-content-sha256;x-amz-date";
const PASSED_HEADERS: [header::HeaderName; 5] =
    [header::CONTENT_LENGTH, header::CONTENT_RANGE, header::ACCEPT_RANGES, header::ETAG, header::LAST_MODIFIED];

pub struct Storage {
    dir: PathBuf,
    s3: Option<S3>,
}

struct S3 {
    http: reqwest::Client,
    origin: String,
    host: String,
    prefix: String,
    region: String,
    access_key: String,
    secret_key: String,
}

impl Storage {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir, s3: S3::from_env() }
    }

    fn local(&self, key: &str) -> PathBuf {
        self.dir.join(key)
    }

    pub async fn save(&self, key: &str, field: &mut Field<'_>) -> Res<i64> {
        let path = self.local(key);
        let result = match (write_field(field, &path).await, &self.s3) {
            (Ok(size), Some(s3)) => s3.put(key, &path, size).await.map(|()| size),
            (written, _) => written,
        };
        if self.s3.is_some() || result.is_err() {
            let _ = tokio::fs::remove_file(&path).await;
        }
        result
    }

    pub async fn serve(&self, key: &str, req: Request) -> Res<Response> {
        let path = self.local(key);
        match &self.s3 {
            Some(s3) if !tokio::fs::try_exists(&path).await.unwrap_or(false) => s3.get(key, req.headers()).await,
            _ => Ok(ServeFile::new(path).oneshot(req).await?.map(Body::new)),
        }
    }

    pub async fn remove(&self, key: &str) {
        let _ = tokio::fs::remove_file(self.local(key)).await;
        if let Some(s3) = &self.s3 {
            s3.delete(key).await;
        }
    }
}

async fn write_field(field: &mut Field<'_>, path: &Path) -> Res<i64> {
    let mut f = tokio::fs::File::create(path).await?;
    let mut size = 0;
    while let Some(chunk) = field.chunk().await? {
        size += chunk.len() as i64;
        f.write_all(&chunk).await?;
    }
    f.flush().await?;
    Ok(size)
}

impl S3 {
    fn from_env() -> Option<Self> {
        let bucket = crate::env("S3_BUCKET")?;
        let region = crate::env("S3_REGION").unwrap_or_else(|| "us-east-1".into());
        let (origin, prefix) = match crate::env("S3_ENDPOINT") {
            Some(e) => (e.trim_end_matches('/').to_owned(), format!("/{bucket}")),
            None => (format!("https://{bucket}.s3.{region}.amazonaws.com"), String::new()),
        };
        Some(Self {
            http: reqwest::Client::new(),
            host: origin.split_once("://").map_or(origin.as_str(), |(_, h)| h).to_owned(),
            origin,
            prefix,
            region,
            access_key: crate::env("S3_ACCESS_KEY_ID").expect("S3_ACCESS_KEY_ID is required with S3_BUCKET"),
            secret_key: crate::env("S3_SECRET_ACCESS_KEY").expect("S3_SECRET_ACCESS_KEY is required with S3_BUCKET"),
        })
    }

    fn signed(&self, method: Method, key: &str) -> reqwest::RequestBuilder {
        let path = format!("{}/{key}", self.prefix);
        let (date, stamp) = amz_date(crate::auth::unix_now());
        let canonical = format!(
            "{method}\n{path}\n\nhost:{}\nx-amz-content-sha256:UNSIGNED-PAYLOAD\nx-amz-date:{stamp}\n\n{SIGNED_HEADERS}\nUNSIGNED-PAYLOAD",
            self.host
        );
        let scope = format!("{date}/{}/s3/aws4_request", self.region);
        let to_sign = format!("AWS4-HMAC-SHA256\n{stamp}\n{scope}\n{}", hex::encode(Sha256::digest(canonical)));
        let mut k = hmac(format!("AWS4{}", self.secret_key).as_bytes(), date.as_bytes());
        for part in [self.region.as_str(), "s3", "aws4_request"] {
            k = hmac(&k, part.as_bytes());
        }
        let auth = format!(
            "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={SIGNED_HEADERS}, Signature={}",
            self.access_key,
            hex::encode(hmac(&k, to_sign.as_bytes()))
        );
        self.http
            .request(method, format!("{}{path}", self.origin))
            .header("x-amz-date", stamp)
            .header("x-amz-content-sha256", "UNSIGNED-PAYLOAD")
            .header(header::AUTHORIZATION, auth)
    }

    async fn put(&self, key: &str, path: &Path, size: i64) -> Res<()> {
        let file = tokio::fs::File::open(path).await?;
        self.signed(Method::PUT, key).header(header::CONTENT_LENGTH, size).body(file).send().await?.error_for_status()?;
        Ok(())
    }

    async fn get(&self, key: &str, headers: &HeaderMap) -> Res<Response> {
        let mut req = self.signed(Method::GET, key);
        if let Some(range) = headers.get(header::RANGE) {
            req = req.header(header::RANGE, range);
        }
        let res = req.send().await?;
        if res.status() == StatusCode::NOT_FOUND {
            return Err(not_found());
        }
        let res = res.error_for_status()?;
        let mut out = Response::builder().status(res.status());
        for name in PASSED_HEADERS {
            if let Some(v) = res.headers().get(&name) {
                out = out.header(name, v);
            }
        }
        Ok(out.body(Body::from_stream(res.bytes_stream()))?)
    }

    async fn delete(&self, key: &str) {
        let res = self.signed(Method::DELETE, key).send().await.and_then(reqwest::Response::error_for_status);
        if let Err(e) = res {
            eprintln!("error: could not delete stored object {key}: {e}");
        }
    }
}

fn hmac(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn amz_date(unix: i64) -> (String, String) {
    let z = unix.div_euclid(86_400) + 719_468;
    let secs = unix.rem_euclid(86_400);
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let date = format!("{year:04}{month:02}{day:02}");
    let stamp = format!("{date}T{:02}{:02}{:02}Z", secs / 3600, secs % 3600 / 60, secs % 60);
    (date, stamp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_amz_dates() {
        assert_eq!(amz_date(0), ("19700101".into(), "19700101T000000Z".into()));
        assert_eq!(amz_date(951_782_400), ("20000229".into(), "20000229T000000Z".into()));
        assert_eq!(amz_date(1_792_000_000), ("20261014".into(), "20261014T174640Z".into()));
    }

    #[test]
    fn hmac_matches_rfc4231() {
        let mac = hmac(&[0x0b; 20], b"Hi There");
        assert_eq!(hex::encode(mac), "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7");
    }
}
