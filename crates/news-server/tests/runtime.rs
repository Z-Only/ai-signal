//! Smoke-test the actual executable using loopback only and scheduling disabled.
#![cfg(unix)]

use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
};

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[tokio::test]
async fn executable_migrates_serves_and_shuts_down_cleanly() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("index.html"),
        "AI Signal native smoke test",
    )
    .unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_ai-news-server"))
        .env("DATABASE_PATH", directory.path().join("news.sqlite3"))
        .env("STATIC_DIR", directory.path())
        .env("BIND_ADDR", "127.0.0.1:0")
        .env("ENABLE_SCHEDULER", "false")
        .env_remove("ADMIN_TOKEN")
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut child = ChildGuard(child);
    let stderr = child.0.stderr.take().unwrap();
    let mut line = String::new();
    BufReader::new(stderr).read_line(&mut line).unwrap();
    let address = line.trim().strip_prefix("AI Signal listening on ").unwrap();
    let client = reqwest::Client::new();
    let news = client
        .get(format!("http://{address}/api/news"))
        .send()
        .await
        .unwrap();
    assert_eq!(news.status(), 200);
    let body: serde_json::Value = serde_json::from_str(&news.text().await.unwrap()).unwrap();
    assert_eq!(body["schedule"], "尚未启用");
    assert_eq!(body["articles"], serde_json::json!([]));
    assert_eq!(
        client
            .post(format!("http://{address}/api/refresh"))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        client
            .get(format!("http://{address}/"))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap(),
        "AI Signal native smoke test"
    );
    assert!(directory.path().join("news.sqlite3").exists());
    assert!(Command::new("kill")
        .args(["-INT", &child.0.id().to_string()])
        .status()
        .unwrap()
        .success());
    assert!(child.0.wait().unwrap().success());
}

#[test]
fn executable_fails_fast_for_occupied_port() {
    let directory = tempfile::tempdir().unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_ai-news-server"))
        .env("DATABASE_PATH", directory.path().join("news.sqlite3"))
        .env("BIND_ADDR", listener.local_addr().unwrap().to_string())
        .env_remove("ENABLE_SCHEDULER")
        .env_remove("ADMIN_TOKEN")
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(!String::from_utf8_lossy(&result.stderr).contains("AI Signal listening"));
}
