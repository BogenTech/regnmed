//! A throwaway nats-server for the queue tests: its own JetStream
//! directory (parallel servers sharing the default one fight over it),
//! killed and cleaned up on drop.
//!
//! The server picks its OWN port (`-p -1`) and writes it to a ports file.
//! Reserving a free port in the test and handing it over raced: macOS
//! hands out ephemeral ports in sequence, so the other test's client
//! socket could take the port first and the server died with «address
//! already in use».

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use async_nats::jetstream;
use uuid::Uuid;

pub struct TestNats {
    pub js: jetstream::Context,
    server: Child,
    dir: PathBuf,
}

impl Drop for TestNats {
    fn drop(&mut self) {
        stop(&mut self.server, &self.dir);
    }
}

fn stop(server: &mut Child, dir: &PathBuf) {
    let _ = server.kill();
    let _ = server.wait();
    let _ = std::fs::remove_dir_all(dir);
}

/// Starts one, or None when nats-server is not on PATH (the test skips).
pub async fn start() -> Option<TestNats> {
    let dir = std::env::temp_dir().join(format!("regnmed-nats-{}", Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir).unwrap();
    let Ok(mut server) = Command::new("nats-server")
        .args(["-js", "-a", "127.0.0.1", "-p", "-1", "-sd"])
        .arg(&dir)
        .arg("--ports_file_dir")
        .arg(&dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        eprintln!("skipping: nats-server not found on PATH");
        let _ = std::fs::remove_dir_all(&dir);
        return None;
    };
    let ports = dir.join(format!("nats-server_{}.ports", server.id()));
    for _ in 0..100 {
        let url = std::fs::read(&ports)
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
            .and_then(|v| v["nats"][0].as_str().map(str::to_string));
        if let Some(url) = url
            && let Ok(js) = crate::connect(&url, None, None).await
        {
            return Some(TestNats { js, server, dir });
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    stop(&mut server, &dir);
    panic!("nats-server did not come up");
}
