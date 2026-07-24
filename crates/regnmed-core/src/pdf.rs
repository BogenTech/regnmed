//! Minimal deterministic PDF writer (docs/faktura.md).
//!
//! regnmed's documents have one fixed layout, so the writer is
//! hand-rolled for exactly what they need — text in the three standard
//! fonts (Helvetica, Helvetica-Bold, Courier), horizontal rules, and
//! multiple pages — instead of pulling in a rendering engine
//! (frugality budget, docs/frugality.md). The standard 14 fonts need no
//! embedding, and WinAnsiEncoding covers æøå.
//!
//! Determinism is a feature: no timestamps, no randomness, no /Info —
//! the same input produces byte-identical output forever, so a stored
//! invoice PDF can be pinned by golden test and re-verified by hash
//! like every other piece of dokumentasjon.

/// A4 in PDF points.
pub const PAGE_WIDTH: f32 = 595.0;
pub const PAGE_HEIGHT: f32 = 842.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Font {
    Regular,
    Bold,
    Mono,
}

impl Font {
    fn resource(self) -> &'static str {
        match self {
            Font::Regular => "/F1",
            Font::Bold => "/F2",
            Font::Mono => "/F3",
        }
    }

    fn base_font(self) -> &'static str {
        match self {
            Font::Regular => "Helvetica",
            Font::Bold => "Helvetica-Bold",
            Font::Mono => "Courier",
        }
    }
}

/// Helvetica advance width in 1/1000 em for a WinAnsi character. The
/// table covers what our documents use; unknown characters assume the
/// average width — alignment is cosmetic, correctness never depends on
/// it. Courier is uniformly 600.
fn char_width(font: Font, c: char) -> u32 {
    if font == Font::Mono {
        return 600;
    }
    // Helvetica and Helvetica-Bold agree on the widths we align by
    // (digits, punctuation); the table is Helvetica's.
    match c {
        ' ' | ',' | '.' | ':' | ';' | '/' | '!' => 278,
        'i' | 'j' | 'l' => 222,
        'f' | 't' | 'I' => 278,
        'r' => 333,
        '(' | ')' | '-' | '[' | ']' => 333,
        'c' | 'k' | 's' | 'v' | 'x' | 'y' | 'z' | 'J' => 500,
        'm' | 'M' => 833,
        'w' => 722,
        'W' => 944,
        'æ' => 889,
        'ø' => 611,
        'Æ' => 1000,
        'Ø' | 'C' | 'D' | 'G' | 'O' | 'Q' => 778,
        'H' | 'K' | 'N' | 'R' | 'U' => 722,
        'F' | 'T' | 'Z' => 611,
        'L' => 556,
        '%' => 889,
        '\'' => 191,
        '"' => 355,
        '@' => 1015,
        _ if c.is_ascii_uppercase() => 667,
        _ => 556,
    }
}

/// Width of `s` at `size` points.
pub fn text_width(font: Font, size: f32, s: &str) -> f32 {
    let units: u32 = s.chars().map(|c| char_width(font, c)).sum();
    units as f32 * size / 1000.0
}

/// One page's accumulated content-stream operations. Bytes, not a
/// String: WinAnsi-encoded text is Latin-1, not UTF-8.
struct Page {
    ops: Vec<u8>,
}

pub struct Pdf {
    pages: Vec<Page>,
}

fn fmt_coord(v: f32) -> String {
    // Two decimals, no trailing float noise — part of the determinism.
    format!("{v:.2}")
}

/// Escapes and encodes a string for a PDF literal string in WinAnsi.
fn encode_text(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    for c in s.chars() {
        let byte: u8 = match c {
            '(' | ')' | '\\' => {
                out.push(b'\\');
                c as u8
            }
            c if (c as u32) < 0x80 => c as u8,
            // WinAnsi coincides with Latin-1 for 0xA0..=0xFF …
            c if (0xA0..=0xFF).contains(&(c as u32)) => c as u32 as u8,
            // … and maps common typographic characters into 0x80..0x9F.
            '€' => 0x80,
            '…' => 0x85,
            '\u{2018}' => 0x91,
            '\u{2019}' => 0x92,
            '\u{201C}' => 0x93,
            '\u{201D}' => 0x94,
            '•' => 0x95,
            '–' => 0x96,
            '—' => 0x97,
            _ => b'?',
        };
        out.push(byte);
    }
    out
}

impl Pdf {
    pub fn new() -> Self {
        Pdf {
            pages: vec![Page { ops: Vec::new() }],
        }
    }

    fn page(&mut self) -> &mut Page {
        self.pages.last_mut().expect("always at least one page")
    }

    pub fn next_page(&mut self) {
        self.pages.push(Page { ops: Vec::new() });
    }

    /// Text with `(x, y)` measured from the TOP-left corner.
    pub fn text(&mut self, x: f32, y: f32, size: f32, font: Font, s: &str) {
        let baseline = PAGE_HEIGHT - y;
        let ops = &mut self.page().ops;
        ops.extend_from_slice(
            format!(
                "BT {} {} Tf {} {} Td (",
                font.resource(),
                fmt_coord(size),
                fmt_coord(x),
                fmt_coord(baseline)
            )
            .as_bytes(),
        );
        ops.extend_from_slice(&encode_text(s));
        ops.extend_from_slice(b") Tj ET\n");
    }

    /// Right-aligned text: `x_right` is where the text ENDS.
    pub fn text_right(&mut self, x_right: f32, y: f32, size: f32, font: Font, s: &str) {
        self.text(x_right - text_width(font, size, s), y, size, font, s);
    }

    /// Horizontal rule at `y` from the top.
    pub fn rule(&mut self, x1: f32, x2: f32, y: f32, width: f32) {
        let line_y = PAGE_HEIGHT - y;
        self.page().ops.extend_from_slice(
            format!(
                "{} w 0.6 G {} {} m {} {} l S 0 G\n",
                fmt_coord(width),
                fmt_coord(x1),
                fmt_coord(line_y),
                fmt_coord(x2),
                fmt_coord(line_y)
            )
            .as_bytes(),
        );
    }

    /// Serializes the document. Object layout: 1 catalog, 2 pages,
    /// 3..=5 fonts, then per page: page object + content stream.
    pub fn finish(self) -> Vec<u8> {
        let mut objects: Vec<Vec<u8>> = Vec::new();
        let page_count = self.pages.len();
        let first_page_obj = 6;

        let kids: Vec<String> = (0..page_count)
            .map(|i| format!("{} 0 R", first_page_obj + 2 * i))
            .collect();
        objects.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
        objects.push(
            format!(
                "<< /Type /Pages /Kids [{}] /Count {} >>",
                kids.join(" "),
                page_count
            )
            .into_bytes(),
        );
        for font in [Font::Regular, Font::Bold, Font::Mono] {
            objects.push(
                format!(
                    "<< /Type /Font /Subtype /Type1 /BaseFont /{} /Encoding /WinAnsiEncoding >>",
                    font.base_font()
                )
                .into_bytes(),
            );
        }
        for (i, page) in self.pages.iter().enumerate() {
            let content_obj = first_page_obj + 2 * i + 1;
            objects.push(
                format!(
                    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_WIDTH} {PAGE_HEIGHT}] \
                     /Resources << /Font << /F1 3 0 R /F2 4 0 R /F3 5 0 R >> >> \
                     /Contents {content_obj} 0 R >>"
                )
                .into_bytes(),
            );
            let mut content = format!("<< /Length {} >>\nstream\n", page.ops.len()).into_bytes();
            content.extend_from_slice(&page.ops);
            content.extend_from_slice(b"endstream");
            objects.push(content);
        }

        let mut out = Vec::with_capacity(4096);
        // The binary-marker comment line is conventional; keep it fixed.
        out.extend_from_slice(b"%PDF-1.4\n%\xc3\xa6\xc3\xb8\xc3\xa5\n");
        let mut offsets = Vec::with_capacity(objects.len());
        for (i, body) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
            out.extend_from_slice(body);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref_at = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        out.extend_from_slice(b"0000000000 65535 f \n");
        for offset in &offsets {
            out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        out
    }
}

impl Default for Pdf {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<u8> {
        let mut pdf = Pdf::new();
        pdf.text(50.0, 50.0, 14.0, Font::Bold, "Faktura æøå ÆØÅ");
        pdf.text(
            50.0,
            70.0,
            10.0,
            Font::Regular,
            "Linje (med) \\parenteser\\",
        );
        pdf.text_right(545.0, 70.0, 10.0, Font::Regular, "12 500,00");
        pdf.rule(50.0, 545.0, 80.0, 0.5);
        pdf.next_page();
        pdf.text(50.0, 50.0, 9.0, Font::Mono, "PURRING");
        pdf.finish()
    }

    /// Byte-based substring search — offsets in the file are byte
    /// offsets, and content streams are not valid UTF-8.
    fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack.windows(needle.len()).position(|w| w == needle)
    }

    #[test]
    fn structure_is_valid_pdf() {
        let bytes = sample();
        assert!(bytes.starts_with(b"%PDF-1.4"));
        assert!(bytes.ends_with(b"%%EOF\n"));
        assert!(find(&bytes, b"/Type /Catalog").is_some());
        assert!(find(&bytes, b"/Count 2").is_some(), "two pages");
        assert!(find(&bytes, b"/BaseFont /Helvetica-Bold").is_some());
        assert!(find(&bytes, b"/BaseFont /Courier").is_some());
        // xref offsets must actually point at their objects.
        let xref_at = find(&bytes, b"\nxref\n0 ").unwrap() + 1;
        let trailer = String::from_utf8_lossy(&bytes[xref_at..]);
        assert!(trailer.contains(&format!("startxref\n{xref_at}\n")));
        for i in 1..=9usize {
            let at = find(&bytes, format!("{i} 0 obj").as_bytes()).unwrap();
            let xref_line = trailer.lines().nth(i + 2).unwrap();
            let offset: usize = xref_line.split(' ').next().unwrap().parse().unwrap();
            assert_eq!(at, offset, "xref offset for object {i}");
        }
    }

    #[test]
    fn escaping_and_winansi_encoding() {
        let bytes = sample();
        assert!(
            find(&bytes, br"Linje \(med\) \\parenteser\\").is_some(),
            "escaped"
        );
        // æøå encoded as single Latin-1 bytes inside the string.
        assert!(find(&bytes, &[0xe6, 0xf8, 0xe5]).is_some(), "WinAnsi æøå");
        assert!(find(&bytes, &[0xc6, 0xd8, 0xc5]).is_some(), "WinAnsi ÆØÅ");
        // Typographic characters land in CP1252's 0x80..0x9F range.
        assert_eq!(encode_text("a — b"), vec![b'a', b' ', 0x97, b' ', b'b']);
        assert_eq!(encode_text("\u{2603}"), vec![b'?'], "unmappable becomes ?");
    }

    #[test]
    fn rendering_is_deterministic() {
        assert_eq!(sample(), sample());
    }

    #[test]
    fn right_alignment_uses_widths() {
        // Digits are 556/1000 em; "00" at 10pt = 11.12pt wide.
        assert!((text_width(Font::Regular, 10.0, "00") - 11.12).abs() < 0.01);
        assert_eq!(text_width(Font::Mono, 10.0, "00"), 12.0);
        let mut pdf = Pdf::new();
        pdf.text_right(545.0, 50.0, 10.0, Font::Regular, "00");
        let out = String::from_utf8(pdf.finish()).unwrap();
        assert!(
            out.contains("533.88 792.00 Td"),
            "545 - 11.12 = 533.88:\n{out}"
        );
    }
}
