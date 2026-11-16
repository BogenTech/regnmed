//! A throwaway nats-server for the queue tests: its own port and its
//! own JetStream directory (parallel servers sharing the default one
//! fight over it), killed and cleaned up on drop.

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
        let _ = self.server.kill();
        let _ = self.server.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Starts one, or None when nats-server is not on PATH (the test skips).
pub async fn start() -> Option<TestNats> {
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let dir = std::env::temp_dir().join(format!("regnmed-nats-{}", Uuid::new_v4().simple()));
    let Ok(server) = Command::new("nats-server")
        .args(["-js", "-p", &port.to_string(), "-sd"])
        .arg(&dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        eprintln!("skipping: nats-server not found on PATH");
        return None;
    };
    let mut nats = None;
    let url = format!("nats://127.0.0.1:{port}");
    // Up to 20 s: a freshly spawned binary on macOS sometimes takes
    // several seconds before it listens, and 5 s made both tests flaky.
    for _ in 0..200 {
        if let Ok(js) = crate::connect(&url, None, None).await {
            nats = Some(js);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let guard = TestNats {
        js: nats.expect("nats-server did not come up"),
        server,
        dir,
    };
    Some(guard)
}
