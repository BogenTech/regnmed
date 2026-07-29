//! HTTP response hardening (#64, docs/auth.md §9).
//!
//! A code review found no exploitable path today — the portal downloads
//! through `blob` + `a.download` and never navigates to a document, the
//! Bearer model means a link cannot be "opened authenticated", and Svelte
//! escapes what it renders. But that defence rests entirely on nobody
//! ever changing those habits, which is too thin for an audit-facing
//! product. These headers hold when the habits slip.
//!
//! Two seams, for the same reason the access guard is one seam (#56):
//!
//! 1. [`security_headers`] runs on **every** response, so no endpoint can
//!    forget `nosniff` or the CSP.
//! 2. [`file_response`] builds **every** byte-serving response, so no
//!    download can forget to sanitise a filename or to distrust an
//!    uploader's content type.
//!
//! The rule about uploaded content is worth stating plainly: **the
//! uploader does not get to decide what the server says the bytes are.**
//! An `ansatt` uploading a receipt, or an allowed e-mail sender, could
//! otherwise have us serve `text/html` — and `nosniff` alone would not
//! help, because we would be asserting the dangerous type ourselves.

use std::sync::OnceLock;

use axum::body::Body;
use axum::extract::Request;
use axum::http::{HeaderValue, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use sha2::{Digest, Sha256};

/// Content types we are willing to assert for **uploaded** bytes.
///
/// Everything else becomes `application/octet-stream`, which a browser
/// downloads instead of interpreting. The list is what an accounting
/// system actually receives as a bilag: documents, photographs of
/// receipts, and the machine formats we import.
///
/// Two absences are deliberate, not oversights:
///
/// - **`text/html`** — the whole point.
/// - **`image/svg+xml`** — an SVG is a document that can run script, so
///   it is the one image type that must never be served as an image.
///   It is easy to miss precisely because it looks like the others.
const SERVEABLE: &[&str] = &[
    "application/pdf",
    "image/png",
    "image/jpeg",
    "image/gif",
    "image/webp",
    "image/heic",
    "image/tiff",
    "application/xml",
    "text/xml",
    "text/plain",
    "text/csv",
    "message/rfc822",
    "application/json",
    "application/octet-stream",
];

/// What the server will claim these bytes are.
///
/// Parameters are dropped (`text/plain; charset=utf-8` → `text/plain`):
/// the parameter is another place to smuggle something, and we re-add
/// nothing we did not choose.
pub fn safe_content_type(declared: &str) -> &'static str {
    let base = declared
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    SERVEABLE
        .iter()
        .find(|s| **s == base)
        .copied()
        .unwrap_or("application/octet-stream")
}

/// Whether the document is shown in place or downloaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vis {
    /// Rendered in the browser — only ever for bytes WE generated.
    Inline,
    /// Downloaded. The right default for anything an uploader supplied.
    Attachment,
}

impl Vis {
    fn slug(self) -> &'static str {
        match self {
            Vis::Inline => "inline",
            Vis::Attachment => "attachment",
        }
    }
}

/// The ASCII fallback filename: quotes, backslashes, control characters
/// and path separators removed.
///
/// A quote in the name would otherwise close the quoted-string and let
/// the uploader append header parameters; CR/LF would end the header
/// line entirely (today that yields a 500, because axum rejects the
/// invalid value — an outage rather than an injection, but neither is
/// acceptable).
fn ascii_filename(filename: &str) -> String {
    let cleaned: String = filename
        .chars()
        .map(|c| match c {
            '"' | '\\' | '/' => '_',
            c if c.is_control() => '_',
            c if c.is_ascii() => c,
            // Non-ASCII is carried by `filename*` below; the fallback
            // must stay ASCII to be a valid quoted-string.
            _ => '_',
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').to_string();
    if trimmed.is_empty() {
        "dokument".into()
    } else {
        trimmed
    }
}

/// Percent-encodes for RFC 5987 `filename*`, so `Kvittering æøå.pdf`
/// survives with its own letters instead of arriving as underscores.
fn rfc5987(filename: &str) -> String {
    let mut out = String::new();
    for b in filename.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn content_disposition(vis: Vis, filename: &str) -> String {
    // Control characters go first, and go from BOTH forms. Percent-encoding
    // would make a CRLF header-safe, but no filename legitimately contains
    // one, and a client that decodes `filename*` should not be handed it.
    let filename: String = filename.replace(|c: char| c.is_control(), "_");
    format!(
        "{}; filename=\"{}\"; filename*=UTF-8''{}",
        vis.slug(),
        ascii_filename(&filename),
        rfc5987(&filename)
    )
}

/// The one way this API serves a file.
///
/// `declared_type` passes through [`safe_content_type`] whether it came
/// from an uploader or from us — our own PDFs are on the list anyway, and
/// a single path means there is no "trusted" variant for someone to reach
/// for by mistake later.
pub fn file_response(
    vis: Vis,
    filename: &str,
    declared_type: &str,
    bytes: impl Into<Body>,
) -> Response {
    let content_type = safe_content_type(declared_type);
    // Bytes we would not assert a type for are never rendered in place,
    // whatever the caller asked: `inline` plus an unknown type is exactly
    // the combination that lets an upload act like a page.
    let vis = if content_type == "application/octet-stream" {
        Vis::Attachment
    } else {
        vis
    };
    (
        [
            (header::CONTENT_TYPE, content_type.to_string()),
            (
                header::CONTENT_DISPOSITION,
                content_disposition(vis, filename),
            ),
        ],
        bytes.into(),
    )
        .into_response()
}

/// The Content-Security-Policy, with the hash of the portal's one inline
/// script computed from the served HTML itself.
///
/// index.html carries a small inline script that applies the theme before
/// first paint (without it the page flashes). A hash pinned in source
/// would silently stop matching the day somebody edits that script — the
/// theme flash would come back and nobody would connect it to a header.
/// Deriving the hash from the bytes we actually serve means the policy
/// cannot drift from the page.
///
/// `style-src` allows inline styles: Svelte sets style attributes, and
/// the key-figures bars are CSS-only by design (#36). Injected CSS is a
/// far smaller problem than injected script, and `script-src` carries no
/// `unsafe-inline` — which is the clause that matters for the risk this
/// addresses, a forgotten escape becoming XSS.
fn csp(index_html: &str) -> String {
    let mut directives = vec![
        "default-src 'self'".to_string(),
        "img-src 'self' data: blob:".to_string(),
        "style-src 'self' 'unsafe-inline'".to_string(),
        "connect-src 'self'".to_string(),
        "object-src 'none'".to_string(),
        "base-uri 'self'".to_string(),
        "frame-ancestors 'none'".to_string(),
        "form-action 'self'".to_string(),
    ];
    let mut script = "script-src 'self'".to_string();
    for body in inline_scripts(index_html) {
        let digest = Sha256::digest(body.as_bytes());
        script.push_str(&format!(
            " 'sha256-{}'",
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, digest)
        ));
    }
    directives.insert(1, script);
    directives.join("; ")
}

/// The bodies of `<script>` elements with no `src`, exactly as served.
fn inline_scripts(html: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(start) = rest.find("<script") {
        let after = &rest[start..];
        let Some(open_end) = after.find('>') else {
            break;
        };
        let tag = &after[..open_end];
        let Some(close) = after.find("</script>") else {
            break;
        };
        if !tag.contains(" src=") {
            out.push(&after[open_end + 1..close]);
        }
        rest = &after[close + "</script>".len()..];
    }
    out
}

fn csp_header(index_html: &str) -> &'static HeaderValue {
    static CSP: OnceLock<HeaderValue> = OnceLock::new();
    CSP.get_or_init(|| {
        HeaderValue::from_str(&csp(index_html)).expect("CSP is a valid header value")
    })
}

/// Sets the headers that hold when a habit slips.
///
/// `nosniff` and `Referrer-Policy` go on everything; the CSP goes on HTML
/// only, because it is meaningless on a PDF and would only be one more
/// header on every download.
pub async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    let is_html = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("text/html"));
    if is_html {
        headers.insert(
            header::CONTENT_SECURITY_POLICY,
            csp_header(crate::portal::index_html()).clone(),
        );
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_and_svg_are_never_asserted() {
        // The two that would turn an upload into a page.
        assert_eq!(safe_content_type("text/html"), "application/octet-stream");
        assert_eq!(
            safe_content_type("image/svg+xml"),
            "application/octet-stream"
        );
        assert_eq!(
            safe_content_type("application/xhtml+xml"),
            "application/octet-stream"
        );
    }

    #[test]
    fn known_types_survive_and_parameters_do_not() {
        assert_eq!(safe_content_type("application/pdf"), "application/pdf");
        assert_eq!(safe_content_type("APPLICATION/PDF"), "application/pdf");
        assert_eq!(safe_content_type("text/plain; charset=utf-8"), "text/plain");
        assert_eq!(safe_content_type(""), "application/octet-stream");
    }

    #[test]
    fn a_filename_cannot_break_out_of_the_header() {
        let d = content_disposition(Vis::Attachment, "evil\".html");
        assert!(d.starts_with("attachment; filename=\"evil_.html\""), "{d}");
        // CR/LF would end the header line; they must not survive at all.
        let d = content_disposition(Vis::Attachment, "a\r\nX-Evil: 1.pdf");
        assert!(!d.contains('\r') && !d.contains('\n'), "{d}");
        // A name that is only punctuation still yields something usable.
        assert!(content_disposition(Vis::Attachment, "...").contains("dokument"));
    }

    #[test]
    fn norwegian_filenames_survive_in_the_extended_form() {
        let d = content_disposition(Vis::Attachment, "Kvittering æøå.pdf");
        // ASCII fallback for old clients, real letters in filename*.
        assert!(d.contains("filename=\"Kvittering ___.pdf\""), "{d}");
        assert!(
            d.contains("filename*=UTF-8''Kvittering%20%C3%A6%C3%B8%C3%A5.pdf"),
            "{d}"
        );
    }

    /// `inline` plus a type we refuse to assert is the combination that
    /// lets an upload behave like a page — so it cannot be requested.
    #[test]
    fn unknown_types_are_forced_to_download() {
        let response = file_response(Vis::Inline, "x.html", "text/html", Vec::new());
        let disposition = response
            .headers()
            .get(header::CONTENT_DISPOSITION)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(disposition.starts_with("attachment;"), "{disposition}");
    }

    #[test]
    fn the_csp_pins_the_portals_own_inline_script() {
        let html = r#"<script>var t = 1;</script><script type="module" src="/a.js"></script>"#;
        let policy = csp(html);
        // The module script has a src and needs no hash; the inline one does.
        assert_eq!(policy.matches("'sha256-").count(), 1, "{policy}");
        assert!(
            !policy.contains("unsafe-inline")
                || !policy.contains("script-src 'self' 'unsafe-inline'")
        );
        assert!(policy.contains("object-src 'none'"), "{policy}");
        assert!(policy.contains("frame-ancestors 'none'"), "{policy}");
    }

    /// The hash must be of the script the browser actually runs, so the
    /// real index.html has to produce exactly one.
    #[test]
    fn the_real_portal_html_has_one_hashable_script() {
        let scripts = inline_scripts(crate::portal::index_html());
        assert_eq!(scripts.len(), 1, "found {} inline scripts", scripts.len());
        assert!(scripts[0].contains("regnmed-theme"), "{}", scripts[0]);
    }
}
