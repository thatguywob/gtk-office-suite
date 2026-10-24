//! A Durable Streams server this test process starts and stops itself, for
//! the tests that need to restart or kill it.

use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

pub struct ServerProc {
    child: Option<Child>,
    pub port: u16,
    pub started_in: Duration,
}

pub fn server_dir() -> PathBuf {
    std::env::var("DCP_STREAMS_SERVER_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../scripts/streams-server")
        })
}

/// A free port, found by binding to 0 and letting go.
pub fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

impl ServerProc {
    pub async fn start(port: u16, data_dir: Option<PathBuf>) -> Self {
        let dir = server_dir();
        assert!(
            dir.join("node_modules/@durable-streams/server").exists(),
            "the reference server is not installed in {dir:?}: run `npm ci` there (or set DCP_SKIP_STREAMS=1)"
        );
        let mut cmd = Command::new("node");
        cmd.current_dir(&dir)
            .arg("server.mjs")
            .args(["--port", &port.to_string()])
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true);
        if let Some(d) = &data_dir {
            cmd.args(["--data-dir", d.to_str().unwrap()]);
        }
        let t0 = Instant::now();
        let mut child = cmd.spawn().expect("cannot run node");
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        let ready = tokio::time::timeout(Duration::from_secs(30), async {
            while let Some(l) = lines.next_line().await.unwrap() {
                if l.starts_with("READY ") {
                    return true;
                }
            }
            false
        })
        .await
        .unwrap_or(false);
        assert!(ready, "the server did not print READY within 30 s");
        // Keep draining so the pipe never fills.
        tokio::spawn(async move { while let Ok(Some(_)) = lines.next_line().await {} });
        ServerProc {
            child: Some(child),
            port,
            started_in: t0.elapsed(),
        }
    }

    pub fn base(&self) -> String {
        format!("http://127.0.0.1:{}/v1/stream/", self.port)
    }

    /// SIGTERM, wait for the exit.
    pub async fn stop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let pid = c.id().expect("pid");
            let _ = Command::new("kill")
                .arg("-TERM")
                .arg(pid.to_string())
                .status()
                .await;
            let _ = tokio::time::timeout(Duration::from_secs(10), c.wait()).await;
        }
    }

    /// SIGKILL: no flush, no goodbye.
    pub async fn kill9(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.start_kill();
            let _ = c.wait().await;
        }
    }
}
