//! Document bytes in object storage (docs/blob-store.md, migration 0070).
//!
//! A document is stored under its SHA-256 and its database row keeps the
//! hash with the content column NULL. Content-addressed, so the same bytes
//! are one object however many rows name them (an inbox document booked as
//! a voucher attachment is not copied), a retried upload overwrites itself
//! with identical bytes, and every read is checked against the row's hash —
//! a changed or missing object fails as loudly as altered bytes in the row.
//!
//! Without `BLOB_S3_*` configured the bytes stay in the row, as before
//! (local development, tests, any environment that has not moved).
//!
//! The S3 client is hand-rolled — PUT and GET with AWS Signature V4, a few
//! dozen lines over sha2 and reqwest — for the same reason the PDF, XML and
//! DER writers are: an SDK would dwarf the binary for two calls. The signer
//! is tested against AWS's own published example.

use anyhow::{Context, Result, bail, ensure};
use regnmed_core::hash::sha256;
use sha2::{Digest, Sha256};
use std::sync::Arc;

/// This process's document store, read from the environment once. API,
/// CLI and jobs all reach documents through it, so no call site can pick a
/// different store than the process was configured with. Call it at
/// startup: a half-configured environment then fails there, not on the
/// first upload.
pub fn blobs() -> &'static Blobs {
    static BLOBS: std::sync::OnceLock<Blobs> = std::sync::OnceLock::new();
    BLOBS.get_or_init(|| Blobs::from_env().unwrap_or_else(|e| panic!("{e:#}")))
}

/// Where document bytes live.
#[derive(Clone)]
pub enum Blobs {
    /// In the row's content column (no object store configured).
    Db,
    /// In an S3-compatible object store, keyed by SHA-256.
    S3(Arc<S3>),
}

pub struct S3 {
    client: reqwest::Client,
    /// Scheme and host, e.g. `https://hel1.your-objectstorage.com`.
    endpoint: String,
    bucket: String,
    region: String,
    /// Key prefix per environment, e.g. `regnmed-prod/documents/`.
    prefix: String,
    access_key: String,
    secret_key: String,
}

const ENV: [&str; 6] = [
    "BLOB_S3_ENDPOINT",
    "BLOB_S3_BUCKET",
    "BLOB_S3_REGION",
    "BLOB_S3_PREFIX",
    "BLOB_S3_ACCESS_KEY",
    "BLOB_S3_SECRET_KEY",
];

impl Blobs {
    /// All six `BLOB_S3_*` variables, or none (bytes stay in the row). Half
    /// a configuration is refused: it would put new documents in the
    /// database while the operator believes they go to the object store.
    pub fn from_env() -> Result<Self> {
        let values: Vec<Option<String>> = ENV
            .iter()
            .map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
            .collect();
        if values.iter().all(Option::is_none) {
            return Ok(Blobs::Db);
        }
        let missing: Vec<&str> = ENV
            .iter()
            .zip(&values)
            .filter(|(_, v)| v.is_none())
            .map(|(k, _)| *k)
            .collect();
        ensure!(
            missing.is_empty(),
            "object store half-configured, missing {}",
            missing.join(", ")
        );
        let v: Vec<String> = values.into_iter().flatten().collect();
        Ok(Blobs::S3(Arc::new(S3 {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .context("object store client")?,
            endpoint: v[0].trim_end_matches('/').to_string(),
            bucket: v[1].clone(),
            region: v[2].clone(),
            prefix: v[3].clone(),
            access_key: v[4].clone(),
            secret_key: v[5].clone(),
        })))
    }

    /// A one-line description for startup logs (never the keys).
    pub fn describe(&self) -> String {
        match self {
            Blobs::Db => "documents are stored in the database (no BLOB_S3_* set)".into(),
            Blobs::S3(s) => format!(
                "documents are stored in {}/{}/{}",
                s.endpoint, s.bucket, s.prefix
            ),
        }
    }

    /// Stores `bytes` and returns what the row's content column should hold:
    /// the bytes themselves without an object store, NULL with one (the
    /// object is written first, so a row never names a missing object; a
    /// failed transaction afterwards leaves only an unreferenced object).
    pub async fn store<'a>(&self, bytes: &'a [u8]) -> Result<Option<&'a [u8]>> {
        match self {
            Blobs::Db => Ok(Some(bytes)),
            Blobs::S3(s) => {
                s.put(&sha256(bytes), bytes).await?;
                Ok(None)
            }
        }
    }

    /// A document's bytes — from the row when it still holds them, otherwise
    /// from the object store — checked against the row's hash either way.
    pub async fn load(&self, in_row: Option<Vec<u8>>, stored_hash: &[u8]) -> Result<Vec<u8>> {
        let bytes = match (in_row, self) {
            (Some(bytes), _) => bytes,
            (None, Blobs::S3(s)) => s.get(stored_hash).await?,
            (None, Blobs::Db) => bail!(
                "document {} is in the object store, but none is configured (BLOB_S3_*)",
                hex::encode(stored_hash)
            ),
        };
        ensure!(
            sha256(&bytes).as_slice() == stored_hash,
            "document {}: content does not match its stored hash — altered",
            hex::encode(stored_hash)
        );
        Ok(bytes)
    }

    /// Writes the bytes to the object store (for moving a row's content
    /// out) and reads them back, so the caller clears the row only once the
    /// object provably holds the same bytes.
    pub async fn offload(&self, bytes: &[u8]) -> Result<()> {
        let Blobs::S3(s) = self else {
            bail!("no object store configured (BLOB_S3_*)");
        };
        let digest = sha256(bytes);
        s.put(&digest, bytes).await?;
        let back = s.get(&digest).await?;
        ensure!(
            sha256(&back) == digest,
            "object {} read back different bytes",
            hex::encode(digest)
        );
        Ok(())
    }
}

impl S3 {
    fn path(&self, digest: &[u8]) -> String {
        format!("/{}/{}{}", self.bucket, self.prefix, hex::encode(digest))
    }

    async fn put(&self, digest: &[u8; 32], bytes: &[u8]) -> Result<()> {
        let path = self.path(digest);
        self.send("PUT", &path, &hex::encode(digest), Some(bytes))
            .await
            .map(drop)
    }

    async fn get(&self, digest: &[u8]) -> Result<Vec<u8>> {
        let path = self.path(digest);
        self.send("GET", &path, EMPTY_SHA256, None).await
    }

    /// One signed request, retried on what is the store's trouble rather
    /// than ours: timeouts, connection errors, 429 and 5xx. Hetzner's
    /// object storage answers an occasional 504 for an object that is
    /// there (seen 2026-11-08); a 404 is not retried — that object is gone.
    ///
    /// It also sits on about one GET in 25 for 3-18 s before sending
    /// headers, and the stall belongs to the REQUEST, not the object: the
    /// same object answers at once the next time (measured 2026-11-25), and
    /// a GET that timed out after 30 s answered its retry in 3 s. So a GET
    /// with no answer after `HEDGE` gets a second, identical request beside
    /// it and the first to succeed wins. The first is never abandoned —
    /// v0.8.3 gave up after 3 s and resent, and five resends in a row
    /// stalled — so a hedged GET is never slower than an unhedged one.
    async fn send(
        &self,
        method: &str,
        path: &str,
        payload_hash: &str,
        body: Option<&[u8]>,
    ) -> Result<Vec<u8>> {
        use std::sync::atomic::{AtomicBool, Ordering};
        const ATTEMPTS: u32 = 5;
        const HEDGE: std::time::Duration = std::time::Duration::from_millis(1500);
        let mut last = String::new();
        for attempt in 1..=ATTEMPTS {
            if attempt > 1 {
                tokio::time::sleep(std::time::Duration::from_millis(200 << (attempt - 1))).await;
            }
            let label = format!("attempt {attempt}");
            // Only GETs: a PUT would upload its whole body twice.
            let outcome = if body.is_none() {
                let answered = tokio::sync::Notify::new();
                let hedged = AtomicBool::new(false);
                let first = self.request(method, path, payload_hash, None, label, Some(&answered));
                // Sent only while the first has no answer: a body that is
                // merely large is downloading, not stalled.
                let second = async {
                    tokio::select! {
                        _ = tokio::time::sleep(HEDGE) => {}
                        _ = answered.notified() => std::future::pending::<()>().await,
                    }
                    hedged.store(true, Ordering::SeqCst);
                    eprintln!(
                        "object store {method} {path} attempt {attempt}: no answer after {} ms, sending a second request",
                        HEDGE.as_millis()
                    );
                    let label = format!("attempt {attempt} (hedge)");
                    self.request(method, path, payload_hash, None, label, None)
                        .await
                };
                tokio::pin!(first, second);
                // The first success wins, and so does the first FINAL
                // failure: a 404 is the answer, not a reason to wait. A
                // retryable failure waits for the other request, if sent.
                tokio::select! {
                    r = &mut first => match r {
                        Err(Failure::Retry(_)) if hedged.load(Ordering::SeqCst) => second.await,
                        r => r,
                    },
                    r = &mut second => match r {
                        Err(Failure::Retry(_)) => first.await,
                        r => r,
                    },
                }
            } else {
                self.request(method, path, payload_hash, body, label, None)
                    .await
            };
            match outcome {
                Ok(bytes) => return Ok(bytes),
                Err(Failure::Final(e)) => return Err(e),
                Err(Failure::Retry(why)) => last = why,
            }
        }
        bail!("object store {method} {path}: unavailable after {ATTEMPTS} attempts ({last})")
    }

    /// One signed request. Signed per request: the signature carries the
    /// time. Every failed or slow request is logged with its timing: a retry
    /// used to be silent, so a 48 s audit report in prod (2026-11-09)
    /// showed nothing but its total.
    async fn request(
        &self,
        method: &str,
        path: &str,
        payload_hash: &str,
        body: Option<&[u8]>,
        label: String,
        answered: Option<&tokio::sync::Notify>,
    ) -> Result<Vec<u8>, Failure> {
        const SLOW: std::time::Duration = std::time::Duration::from_secs(2);
        let url = format!("{}{path}", self.endpoint);
        let mut req = match body {
            Some(bytes) => self.client.put(url).body(bytes.to_vec()),
            None => self.client.get(url),
        };
        for (k, v) in self.signed_headers(method, path, payload_hash) {
            req = req.header(k, v);
        }
        let started = std::time::Instant::now();
        let outcome = req.send().await.map_err(anyhow::Error::from);
        let headers_ms = started.elapsed().as_millis();
        // notify_one keeps the signal if nobody is waiting yet.
        if let (Ok(resp), Some(answered)) = (&outcome, answered)
            && resp.status().is_success()
        {
            answered.notify_one();
        }
        let note = |what: &str| {
            eprintln!(
                "object store {method} {path} {label}: {what}, headers after {headers_ms} ms, {} ms in all",
                started.elapsed().as_millis()
            )
        };
        let retry = |why: String| {
            note(&why);
            Failure::Retry(why)
        };
        match outcome {
            Ok(resp) if resp.status().is_success() => match resp.bytes().await {
                Ok(bytes) => {
                    if started.elapsed() > SLOW {
                        note(&format!("slow, {} bytes", bytes.len()));
                    }
                    Ok(bytes.to_vec())
                }
                Err(e) => Err(retry(format!(
                    "reading the body: {:#}",
                    anyhow::Error::from(e)
                ))),
            },
            Ok(resp) if resp.status() == reqwest::StatusCode::NOT_FOUND => Err(Failure::Final(
                anyhow::anyhow!("object store {method} {path}: 404 — the object is missing"),
            )),
            Ok(resp)
                if resp.status().is_server_error()
                    || resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS =>
            {
                Err(retry(resp.status().to_string()))
            }
            Ok(resp) => Err(Failure::Final(anyhow::anyhow!(
                "object store {method} {path}: {}",
                resp.status()
            ))),
            Err(e) => Err(retry(format!("{e:#}"))),
        }
    }

    fn signed_headers(
        &self,
        method: &str,
        path: &str,
        payload_hash: &str,
    ) -> Vec<(String, String)> {
        let host = self
            .endpoint
            .split("://")
            .nth(1)
            .unwrap_or(&self.endpoint)
            .to_string();
        let amz_date = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
        let headers = vec![
            ("host".to_string(), host),
            ("x-amz-content-sha256".to_string(), payload_hash.to_string()),
            ("x-amz-date".to_string(), amz_date.clone()),
        ];
        let auth = authorization(
            method,
            path,
            &headers,
            payload_hash,
            &amz_date,
            &self.region,
            &self.access_key,
            &self.secret_key,
        );
        let mut out: Vec<(String, String)> =
            headers.into_iter().filter(|(k, _)| k != "host").collect();
        out.push(("authorization".to_string(), auth));
        out
    }
}

/// Why one request failed: worth another attempt, or not.
enum Failure {
    Retry(String),
    Final(anyhow::Error),
}

const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

/// AWS Signature V4 `Authorization` header for an S3 request with no query
/// string. `headers` are the signed headers, lower-case, values trimmed;
/// `path` is already URI-safe (hex keys and plain prefixes are).
#[allow(clippy::too_many_arguments)]
fn authorization(
    method: &str,
    path: &str,
    headers: &[(String, String)],
    payload_hash: &str,
    amz_date: &str,
    region: &str,
    access_key: &str,
    secret_key: &str,
) -> String {
    let mut sorted: Vec<&(String, String)> = headers.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let canonical_headers: String = sorted.iter().map(|(k, v)| format!("{k}:{v}\n")).collect();
    let signed: Vec<&str> = sorted.iter().map(|(k, _)| k.as_str()).collect();
    let signed = signed.join(";");
    let canonical = format!("{method}\n{path}\n\n{canonical_headers}\n{signed}\n{payload_hash}");
    let date = &amz_date[..8];
    let scope = format!("{date}/{region}/s3/aws4_request");
    let to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
        hex::encode(Sha256::digest(canonical.as_bytes()))
    );
    let mut key = hmac_sha256(format!("AWS4{secret_key}").as_bytes(), date.as_bytes());
    for part in [region, "s3", "aws4_request"] {
        key = hmac_sha256(&key, part.as_bytes());
    }
    let signature = hex::encode(hmac_sha256(&key, to_sign.as_bytes()));
    format!(
        "AWS4-HMAC-SHA256 Credential={access_key}/{scope}, SignedHeaders={signed}, Signature={signature}"
    )
}

/// HMAC-SHA256 over sha2 (RFC 2104, block size 64).
fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        k[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    inner.update(k.map(|b| b ^ 0x36));
    inner.update(msg);
    let inner = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(k.map(|b| b ^ 0x5c));
    outer.update(inner);
    outer.finalize().into()
}

/// The document columns, with the column holding each one's hash.
const DOCUMENT_COLUMNS: [(&str, &str, &str); 6] = [
    ("attachment", "content", "sha256"),
    ("inbox_document", "content", "sha256"),
    ("inbox_mail_attachment", "content", "sha256"),
    ("inbox_mail", "raw", "raw_sha256"),
    ("expense", "receipt_content", "receipt_sha256"),
    ("balanse_dokumentasjon", "vedlegg", "vedlegg_sha256"),
];

/// Moves every document still held in a row to the object store: write,
/// read back and compare, and only then empty the row's content column
/// (migration 0070 allows exactly that update, and only when the stored
/// hash is the hash of the bytes removed). Safe to re-run and to interrupt:
/// a row is either untouched or provably in the store. Returns rows moved
/// per table.
pub async fn offload_documents(pool: &sqlx::PgPool) -> Result<Vec<(&'static str, u64)>> {
    use sqlx::Row;
    let store = blobs();
    ensure!(
        matches!(store, Blobs::S3(_)),
        "no object store configured (BLOB_S3_*)"
    );
    let mut moved = Vec::new();
    for (table, content, hash) in DOCUMENT_COLUMNS {
        let mut count = 0u64;
        loop {
            // Batches keep memory bounded however large the documents are.
            let rows = sqlx::query(&format!(
                "select id, {content} as content from {table}
                 where {content} is not null order by id limit 50"
            ))
            .fetch_all(pool)
            .await?;
            if rows.is_empty() {
                break;
            }
            for row in rows {
                let id: uuid::Uuid = row.get("id");
                let bytes: Vec<u8> = row.get("content");
                store.offload(&bytes).await?;
                sqlx::query(&format!(
                    "update {table} set {content} = null, {hash} = $2 where id = $1"
                ))
                .bind(id)
                .bind(sha256(&bytes).as_slice())
                .execute(pool)
                .await
                .with_context(|| format!("emptying {table} {id}"))?;
                count += 1;
            }
        }
        moved.push((table, count));
    }
    Ok(moved)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AWS's published example: "GET Object" in the Signature V4 header
    /// documentation (examplebucket, test.txt, bytes 0-9, 2013-05-24).
    #[test]
    fn signature_matches_aws_published_example() {
        let headers = vec![
            (
                "host".to_string(),
                "examplebucket.s3.amazonaws.com".to_string(),
            ),
            ("range".to_string(), "bytes=0-9".to_string()),
            ("x-amz-content-sha256".to_string(), EMPTY_SHA256.to_string()),
            ("x-amz-date".to_string(), "20130524T000000Z".to_string()),
        ];
        let auth = authorization(
            "GET",
            "/test.txt",
            &headers,
            EMPTY_SHA256,
            "20130524T000000Z",
            "us-east-1",
            "AKIAIOSFODNN7EXAMPLE",
            "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
        );
        assert_eq!(
            auth,
            "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request, \
             SignedHeaders=host;range;x-amz-content-sha256;x-amz-date, \
             Signature=f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41"
        );
    }

    #[test]
    fn hmac_matches_rfc4231_case_2() {
        assert_eq!(
            hex::encode(hmac_sha256(b"Jefe", b"what do ya want for nothing?")),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[tokio::test]
    async fn without_an_object_store_bytes_stay_in_the_row_and_are_checked() {
        let blobs = Blobs::Db;
        let bytes = b"kvittering".as_slice();
        assert_eq!(blobs.store(bytes).await.unwrap(), Some(bytes));
        let hash = sha256(bytes);
        assert_eq!(
            blobs.load(Some(bytes.to_vec()), &hash).await.unwrap(),
            bytes
        );
        assert!(blobs.load(Some(b"endret".to_vec()), &hash).await.is_err());
        assert!(blobs.load(None, &hash).await.is_err());
    }

    /// How the stand-in store answers a GET.
    #[derive(Clone, Copy)]
    enum Store {
        /// "hello" at once.
        Prompt,
        /// The first GET gets nothing for a minute, the rest "hello".
        StallFirst,
        /// Headers at once, the body three seconds later.
        SlowBody,
        /// The first GET gets 404 at once, any other 504.
        Missing,
    }

    /// A local stand-in for the object store. Returns the store and a count
    /// of the requests it received.
    async fn fake_store(mode: Store) -> (S3, Arc<std::sync::atomic::AtomicUsize>) {
        use std::sync::atomic::Ordering;
        use std::time::Duration;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        const HEAD: &[u8] = b"HTTP/1.1 200 OK\r\ncontent-length: 5\r\nconnection: close\r\n\r\n";
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let seen = count.clone();
        tokio::spawn(async move {
            loop {
                let (mut sock, _) = listener.accept().await.unwrap();
                let n = seen.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    let _ = sock.read(&mut buf).await;
                    let status = |code: &str| {
                        format!("HTTP/1.1 {code}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n")
                    };
                    let reply = match (mode, n) {
                        (Store::StallFirst, 0) => {
                            tokio::time::sleep(Duration::from_secs(60)).await;
                            [HEAD, b"hello"].concat()
                        }
                        (Store::SlowBody, _) => {
                            let _ = sock.write_all(HEAD).await;
                            let _ = sock.flush().await;
                            tokio::time::sleep(Duration::from_secs(3)).await;
                            b"hello".to_vec()
                        }
                        (Store::Missing, 0) => status("404 Not Found").into_bytes(),
                        (Store::Missing, _) => status("504 Gateway Timeout").into_bytes(),
                        _ => [HEAD, b"hello"].concat(),
                    };
                    let _ = sock.write_all(&reply).await;
                });
            }
        });
        let s3 = S3 {
            client: reqwest::Client::new(),
            endpoint: format!("http://{addr}"),
            bucket: "b".into(),
            region: "r".into(),
            prefix: "p/".into(),
            access_key: "k".into(),
            secret_key: "s".into(),
        };
        (s3, count)
    }

    #[tokio::test]
    async fn a_stalled_get_is_answered_by_a_second_request() {
        let (s3, count) = fake_store(Store::StallFirst).await;
        let started = std::time::Instant::now();
        assert_eq!(s3.get(&[0; 32]).await.unwrap(), b"hello");
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    /// A store that answers at once gets one request, not two.
    #[tokio::test]
    async fn a_prompt_get_sends_one_request() {
        let (s3, count) = fake_store(Store::Prompt).await;
        assert_eq!(s3.get(&[0; 32]).await.unwrap(), b"hello");
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    /// A body that takes long to arrive is downloading, not stalled: no
    /// second download.
    #[tokio::test]
    async fn a_slow_body_is_not_downloaded_twice() {
        let (s3, count) = fake_store(Store::SlowBody).await;
        assert_eq!(s3.get(&[0; 32]).await.unwrap(), b"hello");
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    /// A 404 is the answer: reported at once, never waiting for a second
    /// request whose 504 would turn "missing" into "unavailable".
    #[tokio::test]
    async fn a_missing_object_is_reported_at_once() {
        let (s3, count) = fake_store(Store::Missing).await;
        let started = std::time::Instant::now();
        let err = s3.get(&[0; 32]).await.unwrap_err().to_string();
        assert!(err.contains("404"), "{err}");
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    /// Round trip against a real object store, only when one is configured
    /// (like the database tests and DATABASE_URL).
    #[tokio::test]
    async fn round_trip_through_a_configured_object_store() {
        let Ok(blobs @ Blobs::S3(_)) = Blobs::from_env() else {
            eprintln!("skipping: BLOB_S3_* not set");
            return;
        };
        let bytes = format!("regnmed blob test {}", chrono::Utc::now()).into_bytes();
        assert_eq!(blobs.store(&bytes).await.unwrap(), None);
        assert_eq!(blobs.load(None, &sha256(&bytes)).await.unwrap(), bytes);
        blobs.offload(&bytes).await.unwrap();
    }
}
