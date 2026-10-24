//! Shared plumbing for the Nextcloud tests: where the instances are, who the
//! users are, and the rule that a missing instance FAILS the test unless
//! `DCP_SKIP_NEXTCLOUD=1` says to skip (and then the test says so).
#![allow(dead_code)]

use std::io::Write;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use dcp_webdav::{Dav, ServerClock, WebdavConfig, WebdavStore};

pub const PASSWORD: &str = "dcp-user-Passw0rd-spike";

pub fn url_a() -> String {
    std::env::var("DCP_NEXTCLOUD_A_URL").unwrap_or_else(|_| "http://nc-a".into())
}

pub fn url_b() -> String {
    std::env::var("DCP_NEXTCLOUD_B_URL").unwrap_or_else(|_| "http://nc-b".into())
}

pub fn password() -> String {
    std::env::var("DCP_NC_USER_PASSWORD").unwrap_or_else(|_| PASSWORD.into())
}

static TOKENS: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, String>>> =
    std::sync::OnceLock::new();

fn tokens() -> &'static std::sync::Mutex<std::collections::HashMap<String, String>> {
    TOKENS.get_or_init(Default::default)
}

/// Every request with the account password makes Nextcloud verify it with a slow
/// password hash, about 200 ms. An app password is checked against a token and is
/// the way a client is meant to authenticate; the tests use one per user.
/// `DCP_NC_NO_APP_PASSWORDS=1` keeps the account password (the latency test
/// measures both).
pub async fn prime_app_passwords(base: &str) {
    if std::env::var("DCP_NC_NO_APP_PASSWORDS").as_deref() == Ok("1") {
        return;
    }
    for user in ["alice", "bob", "carol", "dave"] {
        let key = format!("{base}|{user}");
        if tokens().lock().unwrap().contains_key(&key) {
            continue;
        }
        let d = Dav::new(base, user, &password());
        match d.ocs("GET", "core/getapppassword", &[]).await {
            Ok(v) => {
                if let Some(t) = v["apppassword"].as_str() {
                    tokens().lock().unwrap().insert(key, t.to_owned());
                }
            }
            Err(e) => println!("no app password for {user}@{base}: {e}"),
        }
    }
}

pub fn password_for(base: &str, user: &str) -> String {
    tokens()
        .lock()
        .unwrap()
        .get(&format!("{base}|{user}"))
        .cloned()
        .unwrap_or_else(password)
}

/// `Some(())` if the test should run. With `DCP_SKIP_NEXTCLOUD=1` it prints
/// `SKIPPED <name>` and returns `None`. Otherwise it panics if `base` does not
/// answer `status.php` with `"installed":true`.
pub async fn require(name: &str, bases: &[String]) -> Option<()> {
    if std::env::var("DCP_SKIP_NEXTCLOUD").as_deref() == Ok("1") {
        println!("SKIPPED {name}: DCP_SKIP_NEXTCLOUD=1");
        return None;
    }
    for base in bases {
        let dav = Dav::new(base, "nobody", "nobody");
        let ok = match dav.request("GET", "/status.php", &[], None).await {
            Ok(r) => r.ok() && r.text().contains("\"installed\":true"),
            Err(_) => false,
        };
        assert!(
            ok,
            "Nextcloud is not reachable at {base} (start it with scripts/nextcloud-up.sh, or set DCP_SKIP_NEXTCLOUD=1 to skip {name} explicitly)"
        );
    }
    for base in bases {
        prime_app_passwords(base).await;
    }
    Some(())
}

pub fn dav(base: &str, user: &str) -> Dav {
    Dav::new(base, user, &password_for(base, user))
}

pub fn config(base: &str, user: &str) -> WebdavConfig {
    WebdavConfig {
        base_url: base.to_owned(),
        user: user.to_owned(),
        password: password_for(base, user),
        device: format!("{user}-device"),
        remote_scheme: "http".into(),
    }
}

/// A calibrated clock for `base`, shared by the handles of one test.
pub async fn clock(base: &str) -> Arc<ServerClock> {
    ServerClock::calibrate(&dav(base, "alice")).await.expect("clock calibration")
}

pub fn store(base: &str, user: &str, clock: &Arc<ServerClock>, fault: &Arc<AtomicBool>) -> WebdavStore {
    WebdavStore::with_parts(&config(base, user), clock.clone(), fault.clone())
}

pub fn unique(prefix: &str) -> String {
    let mut b = [0u8; 5];
    getrandom_fill(&mut b);
    format!("{prefix}-{}", b.iter().map(|x| format!("{x:02x}")).collect::<String>())
}

fn getrandom_fill(b: &mut [u8]) {
    // /dev/urandom: the test crate does not depend on getrandom directly.
    use std::io::Read;
    std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(b)).expect("urandom");
}

/// Append one JSON measurement line to `$DCP_MEASURE_OUT` (if set) and print it.
pub fn measure(kind: &str, value: serde_json::Value) {
    let line = serde_json::json!({ "measure": kind, "value": value }).to_string();
    println!("MEASURE {line}");
    if let Ok(path) = std::env::var("DCP_MEASURE_OUT") {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "{line}");
        }
    }
}

pub fn ms(d: Duration) -> f64 {
    (d.as_secs_f64() * 1000.0 * 10.0).round() / 10.0
}

/// median and p95 of samples (milliseconds).
pub fn stats(mut v: Vec<f64>) -> serde_json::Value {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = v.len();
    let p = |q: f64| v[((n as f64 - 1.0) * q).round() as usize];
    serde_json::json!({ "n": n, "min": v[0], "median": p(0.5), "p95": p(0.95), "max": v[n - 1] })
}
