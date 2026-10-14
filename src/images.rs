use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use libsql::params;
use percent_encoding::percent_decode_str;
use reqwest::header::LOCATION;
use reqwest::{Response, Url, redirect};
use serde::Deserialize;
use tokio::task::JoinSet;

use crate::App;
use crate::auth::User;
use crate::markdown::{self, Images};
use crate::util::all;

const IMAGE_LIMIT: usize = 20 * 1024 * 1024;
const REDIRECTS: usize = 3;

#[derive(Deserialize)]
struct Blob {
    key: String,
    size: i64,
}

#[derive(Deserialize)]
struct Folder {
    id: Option<String>,
}

pub async fn load(app: &App, user: &User, folder: Option<&str>, markdown: &str) -> Images {
    let mut images = Images::new();
    let mut remote = JoinSet::new();
    for source in markdown::image_sources(markdown) {
        if source.starts_with("http://") || source.starts_with("https://") || source.starts_with("//") {
            remote.spawn(async move { (fetch(&source).await, source) });
        } else if let Some(bytes) = local(app, user, folder, &source).await {
            images.insert(source, bytes);
        }
    }
    while let Some(done) = remote.join_next().await {
        if let Ok((Some(bytes), source)) = done {
            images.insert(source, bytes);
        }
    }
    images
}

fn inline(data: &str) -> Option<Vec<u8>> {
    let (meta, payload) = data.split_once(',')?;
    if meta.ends_with(";base64") { B64.decode(payload.trim()).ok() } else { Some(percent_decode_str(payload).collect()) }
}

async fn local(app: &App, user: &User, folder: Option<&str>, source: &str) -> Option<Vec<u8>> {
    if let Some(data) = source.strip_prefix("data:") {
        return inline(data);
    }
    let file_id = source.split_once("/api/files/").and_then(|(_, rest)| rest.strip_suffix("/raw")).filter(|id| !id.contains('/'));
    let blob = match file_id {
        Some(id) => first(app, "SELECT key, size FROM files WHERE id = ?1 AND user_id = ?2", params![id, user.id.as_str()]).await,
        None => by_path(app, user, folder, source).await,
    }?;
    let size = usize::try_from(blob.size).ok().filter(|s| *s <= IMAGE_LIMIT)?;
    app.storage.read(&blob.key).await.ok().filter(|bytes| bytes.len() == size)
}

async fn first<T: serde::de::DeserializeOwned>(app: &App, sql: &str, params: impl libsql::params::IntoParams) -> Option<T> {
    all(&app.db, sql, params).await.ok()?.into_iter().next()
}

async fn by_path(app: &App, user: &User, folder: Option<&str>, source: &str) -> Option<Blob> {
    let path = percent_decode_str(source.split(['?', '#']).next()?).decode_utf8().ok()?;
    let mut current = folder.filter(|_| !path.starts_with('/')).map(str::to_owned);
    let mut parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty() && *p != ".").collect();
    let name = parts.pop()?;
    for part in parts {
        let next: Folder = if part == ".." {
            first(app, "SELECT parent_id AS id FROM folders WHERE id = ?1 AND user_id = ?2", params![current, user.id.as_str()]).await?
        } else {
            first(app, "SELECT id FROM folders WHERE user_id = ?1 AND name = ?2 AND parent_id IS ?3", params![user.id.as_str(), part, current]).await?
        };
        current = next.id;
    }
    let sql = "SELECT key, size FROM files WHERE user_id = ?1 AND name = ?2 AND folder_id IS ?3 ORDER BY archived LIMIT 1";
    first(app, sql, params![user.id.as_str(), name, current]).await
}

fn public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    !(ip.is_private() || ip.is_loopback() || ip.is_link_local() || ip.is_documentation() || a == 0 || a >= 240 || (a == 100 && b & 0xc0 == 64))
}

fn public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => public_v4(v4),
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => public_v4(v4),
            None => !(v6.is_loopback() || v6.is_unspecified() || v6.is_unique_local() || v6.is_unicast_link_local()),
        },
    }
}

async fn request(url: &Url) -> Option<Response> {
    let host = url.host_str().filter(|_| matches!(url.scheme(), "http" | "https"))?;
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host, url.port_or_known_default()?)).await.ok()?.collect();
    if addrs.is_empty() || !addrs.iter().all(|a| public(a.ip())) {
        return None;
    }
    let client = reqwest::Client::builder()
        .redirect(redirect::Policy::none())
        .timeout(Duration::from_secs(15))
        .resolve_to_addrs(host, &addrs)
        .build()
        .ok()?;
    client.get(url.clone()).send().await.ok()
}

async fn read(mut response: Response) -> Option<Vec<u8>> {
    if !response.status().is_success() {
        return None;
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        body.extend_from_slice(&chunk);
        if body.len() > IMAGE_LIMIT {
            return None;
        }
    }
    Some(body)
}

async fn fetch(source: &str) -> Option<Vec<u8>> {
    let mut url = Url::parse(&if source.starts_with("//") { format!("https:{source}") } else { source.to_owned() }).ok()?;
    for _ in 0..=REDIRECTS {
        let response = request(&url).await?;
        if !response.status().is_redirection() {
            return read(response).await;
        }
        url = url.join(response.headers().get(LOCATION)?.to_str().ok()?).ok()?;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_allows_public_addresses() {
        let blocked = ["127.0.0.1", "10.1.2.3", "172.16.0.1", "192.168.1.1", "169.254.169.254", "100.64.0.1", "0.0.0.0", "::1", "fd00::1", "fe80::1", "::ffff:127.0.0.1"];
        assert!(blocked.iter().all(|ip| !public(ip.parse().unwrap())));
        assert!(["1.1.1.1", "140.82.112.3", "2606:4700::1111"].iter().all(|ip| public(ip.parse().unwrap())));
    }

    #[test]
    fn decodes_inline_images() {
        assert_eq!(inline("image/svg+xml;base64,PHN2Zz4=").unwrap(), b"<svg>");
        assert_eq!(inline("image/svg+xml,%3Csvg%3E").unwrap(), b"<svg>");
    }
}
