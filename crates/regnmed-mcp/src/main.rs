//! `regnmed-mcp` — the MCP server, on stdio.
//!
//! One JSON object per line in, one per line out, which is what the MCP
//! stdio transport prescribes. Everything diagnostic goes to stderr:
//! stdout is the protocol, and a stray println there is a parse error at
//! the other end.

use anyhow::Result;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[tokio::main]
async fn main() -> Result<()> {
    let client = regnmed_mcp::client::Client::from_env().await?;
    let server = regnmed_mcp::Server::new(client);

    let mut inn = BufReader::new(tokio::io::stdin()).lines();
    let mut ut = tokio::io::stdout();
    while let Some(linje) = inn.next_line().await? {
        let linje = linje.trim();
        if linje.is_empty() {
            continue;
        }
        let message = match serde_json::from_str(linje) {
            Ok(v) => v,
            Err(e) => {
                // A malformed line has no id, so there is nobody to
                // answer; say so on stderr and keep the session alive.
                eprintln!("regnmed-mcp: ugyldig JSON på stdin: {e}");
                continue;
            }
        };
        if let Some(svar) = server.handle(message).await {
            ut.write_all(serde_json::to_string(&svar)?.as_bytes())
                .await?;
            ut.write_all(b"\n").await?;
            ut.flush().await?;
        }
    }
    Ok(())
}
