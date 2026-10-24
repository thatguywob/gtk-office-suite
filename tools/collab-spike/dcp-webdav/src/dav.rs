//! A small HTTP client for Nextcloud's WebDAV (`/remote.php/dav`) and OCS
//! (`/ocs/v2.php`) endpoints. It returns what the server said, status and
//! headers included, so the measurement tests can quote it; the store maps
//! statuses onto [`StoreError`].

use std::time::{Duration, Instant};

use dcp_core::StoreError;
use reqwest::header::HeaderMap;
use reqwest::Method;
use serde_json::Value;

use crate::xml::{self, Entry};

/// One HTTP response, fully read.
#[derive(Debug, Clone)]
pub struct Resp {
    pub status: u16,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
    /// Wall time from sending the request to having read the whole body.
    pub elapsed: Duration,
}

impl Resp {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|v| v.to_str().ok())
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// The entity tag of the resource the response is about, quotes kept, as
    /// it must be sent back in `If-Match`.
    pub fn etag(&self) -> Option<String> {
        self.header("etag")
            .or_else(|| self.header("oc-etag"))
            .map(str::to_owned)
    }

    /// The `Date` header in milliseconds (whole seconds: the server floors it).
    pub fn date_ms(&self) -> Option<u64> {
        httpdate::parse_http_date(self.header("date")?)
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|d| d.as_millis() as u64)
    }

    /// The server's `Last-Modified` or, failing that, `Date`, in milliseconds.
    pub fn server_time_ms(&self) -> Option<u64> {
        let v = self.header("last-modified").or_else(|| self.header("date"))?;
        httpdate::parse_http_date(v)
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|d| d.as_millis() as u64)
    }
}

/// Map an unsuccessful WebDAV or OCS answer onto the store's error type.
pub fn status_error(what: &str, r: &Resp) -> StoreError {
    let detail = || {
        let t = r.text();
        let t = t.trim();
        let t: String = t.chars().take(300).collect();
        format!("{what}: HTTP {} {t}", r.status)
    };
    match r.status {
        401 | 403 => StoreError::Forbidden(detail()),
        404 => StoreError::NotFound(detail()),
        // 409 is what Sabre answers for a missing parent collection; 412 is a
        // failed precondition; 423 is Nextcloud's file lock (a WebDAV lock, or
        // two writers at once).
        409 | 412 | 423 => StoreError::Conflict(detail()),
        429 => StoreError::RateLimited {
            retry_after_ms: r
                .header("retry-after")
                .and_then(|v| v.parse::<u64>().ok())
                .map(|s| s * 1000),
        },
        _ => StoreError::Transport(detail()),
    }
}

pub fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// A user's authenticated view of one Nextcloud instance.
#[derive(Clone)]
pub struct Dav {
    http: reqwest::Client,
    /// `http://nc-a`, no trailing slash.
    pub base: String,
    pub user: String,
    password: String,
    /// `Some`: authenticate with `Authorization: Bearer` instead of Basic.
    bearer: Option<String>,
    /// Where relative names live: `/remote.php/dav/files/<user>/` for an account,
    /// `/public.php/webdav/` for a share token.
    root: String,
}

impl Dav {
    pub fn new(base: &str, user: &str, password: &str) -> Dav {
        let http = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(120))
            .user_agent("dcp-webdav-spike/0")
            .build()
            .expect("an http client with no TLS builds");
        Dav {
            http,
            base: base.trim_end_matches('/').to_owned(),
            user: user.to_owned(),
            password: password.to_owned(),
            bearer: None,
            root: format!("/remote.php/dav/files/{user}/"),
        }
    }

    /// The same instance as another user.
    pub fn as_user(&self, user: &str, password: &str) -> Dav {
        let mut d = Dav::new(&self.base, user, password);
        d.http = self.http.clone();
        d
    }

    /// What an OCM receiving server holds: a share's secret and nothing else,
    /// sent as `Basic <secret>:` (Nextcloud's legacy form) against
    /// `/public.php/webdav/`.
    pub fn share_basic(base: &str, secret: &str) -> Dav {
        let mut d = Dav::new(base, secret, "");
        d.root = "/public.php/webdav/".to_owned();
        d
    }

    /// The same, sent as `Authorization: Bearer <secret>` (OCM's Resource Access).
    pub fn share_bearer(base: &str, secret: &str) -> Dav {
        let mut d = Dav::share_basic(base, secret);
        d.bearer = Some(secret.to_owned());
        d
    }

    /// `host[:port]` of the instance: the part of a federated address after `@`.
    pub fn host(&self) -> String {
        self.base
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .to_owned()
    }

    /// This user's Open Cloud Mesh address (`user@host[:port]`).
    pub fn cloud_id(&self) -> String {
        format!("{}@{}", self.user, self.host())
    }

    pub fn files_path(&self, rel: &str) -> String {
        format!("{}{}", self.root, rel.trim_start_matches('/'))
    }

    /// Send any request. `path` starts with `/`.
    pub async fn request(
        &self,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: Option<Vec<u8>>,
    ) -> Result<Resp, StoreError> {
        let method = Method::from_bytes(method.as_bytes())
            .map_err(|e| StoreError::Invalid(format!("method {method}: {e}")))?;
        let mut rb = self
            .http
            .request(method, format!("{}{}", self.base, path))
            .header("OCS-APIRequest", "true");
        rb = match &self.bearer {
            Some(t) => rb.bearer_auth(t),
            None => rb.basic_auth(&self.user, Some(&self.password)),
        };
        for (k, v) in headers {
            rb = rb.header(*k, *v);
        }
        if let Some(b) = body {
            rb = rb.body(b);
        }
        let t0 = Instant::now();
        let resp = rb
            .send()
            .await
            .map_err(|e| StoreError::Transport(format!("{path}: {e}")))?;
        let status = resp.status().as_u16();
        let headers = resp.headers().clone();
        let body = resp
            .bytes()
            .await
            .map_err(|e| StoreError::Transport(format!("{path}: reading the body: {e}")))?
            .to_vec();
        Ok(Resp {
            status,
            headers,
            body,
            elapsed: t0.elapsed(),
        })
    }

    pub async fn get(&self, rel: &str) -> Result<Resp, StoreError> {
        self.request("GET", &self.files_path(rel), &[], None).await
    }

    /// `PUT` that creates and never replaces (`If-None-Match: *`). A `412` (or the
    /// `423` of a concurrent creator) means the name is taken.
    pub async fn put_new(&self, rel: &str, bytes: Vec<u8>) -> Result<Resp, StoreError> {
        self.request(
            "PUT",
            &self.files_path(rel),
            &[("If-None-Match", "*"), ("Content-Type", "application/octet-stream")],
            Some(bytes),
        )
        .await
    }

    pub async fn mkcol(&self, rel: &str) -> Result<Resp, StoreError> {
        self.request("MKCOL", &self.files_path(rel), &[], None).await
    }

    pub async fn delete(&self, rel: &str) -> Result<Resp, StoreError> {
        self.request("DELETE", &self.files_path(rel), &[], None).await
    }

    /// `PROPFIND` with the named properties, written `prefix:name` with the
    /// prefixes `d` (DAV:), `oc` (owncloud), `nc` (nextcloud) and `ocm`
    /// (open-cloud-mesh.org/ns).
    pub async fn propfind(
        &self,
        path: &str,
        depth: u8,
        props: &[&str],
    ) -> Result<(Resp, Vec<Entry>), StoreError> {
        let mut body = String::from(
            "<?xml version=\"1.0\"?><d:propfind xmlns:d=\"DAV:\" xmlns:oc=\"http://owncloud.org/ns\" xmlns:nc=\"http://nextcloud.org/ns\" xmlns:ocm=\"http://open-cloud-mesh.org/ns\"><d:prop>",
        );
        for p in props {
            body.push_str(&format!("<{p}/>"));
        }
        body.push_str("</d:prop></d:propfind>");
        let depth = depth.to_string();
        let r = self
            .request(
                "PROPFIND",
                path,
                &[("Depth", depth.as_str()), ("Content-Type", "application/xml")],
                Some(body.into_bytes()),
            )
            .await?;
        if r.status != 207 {
            return Err(status_error(&format!("PROPFIND {path}"), &r));
        }
        let entries = xml::parse_multistatus(&r.body).map_err(StoreError::Transport)?;
        Ok((r, entries))
    }

    /// An OCS call. Returns `ocs.data` of a successful answer. The v2 API
    /// mirrors the OCS status code in the HTTP status.
    pub async fn ocs(
        &self,
        method: &str,
        path_and_query: &str,
        form: &[(&str, &str)],
    ) -> Result<Value, StoreError> {
        let sep = if path_and_query.contains('?') { '&' } else { '?' };
        let path = format!("/ocs/v2.php/{}{sep}format=json", path_and_query.trim_start_matches('/'));
        let body = (!form.is_empty()).then(|| {
            form.iter()
                .map(|(k, v)| format!("{}={}", percent_encode(k), percent_encode(v)))
                .collect::<Vec<_>>()
                .join("&")
                .into_bytes()
        });
        let headers: &[(&str, &str)] = if body.is_some() {
            &[
                ("Content-Type", "application/x-www-form-urlencoded"),
                ("Accept", "application/json"),
            ]
        } else {
            &[("Accept", "application/json")]
        };
        let r = self.request(method, &path, headers, body).await?;
        let v: Value = serde_json::from_slice(&r.body).map_err(|_| {
            StoreError::Transport(format!("OCS {path}: HTTP {} not JSON: {}", r.status, r.text().chars().take(200).collect::<String>()))
        })?;
        let meta = &v["ocs"]["meta"];
        let code = meta["statuscode"].as_u64().unwrap_or(0);
        if matches!(code, 100 | 200) {
            return Ok(v["ocs"]["data"].clone());
        }
        let message = meta["message"].as_str().unwrap_or("").to_owned();
        Err(match (r.status, code) {
            (401 | 403, _) | (_, 997 | 403) => StoreError::Forbidden(format!("OCS {path}: {code} {message}")),
            (404, _) | (_, 404) => StoreError::NotFound(format!("OCS {path}: {code} {message}")),
            _ => StoreError::Invalid(format!("OCS {path}: HTTP {} {code} {message}", r.status)),
        })
    }
}
