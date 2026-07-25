//! Tekstlaget ut av en PDF (docs/bilagstolkning.md, #34).
//!
//! De fleste fakturaer som kommer inn i innboksen er *genererte* PDF-er
//! med et tekstlag — teksten står allerede i filen, den skal bare
//! hentes ut. Det krever ingen OCR og ingen modell; det krever at vi
//! leser PDF-ens egne innholdsstrømmer.
//!
//! Dette er med vilje et **lite utsnitt** av PDF: objekter,
//! innholdsstrømmer (rå eller Flate-komprimert), og de tekstvisende
//! operatorene. Skannede bilder har ikke noe tekstlag, og PDF-er med
//! egne fontkodinger gir bytes vi ikke kan tolke. I begge tilfeller
//! returnerer vi **None** — aldri søppel som ser ut som et forslag.
//! Bildesiden (OCR) hører til en valgfri sidecar, ikke kjernen
//! (docs/frugality.md).

/// Extracted text, or None when the document has no readable text
/// layer. Lines are approximate: each text-positioning operator starts
/// a new one, which is enough for "the number next to this word".
pub fn extract(pdf: &[u8]) -> Option<String> {
    if !pdf.starts_with(b"%PDF") {
        return None;
    }
    let mut text = String::new();
    for stream in content_streams(pdf) {
        text.push_str(&text_from_content(&stream));
    }
    let text = text.trim().to_string();
    if !looks_like_text(&text) {
        return None;
    }
    Some(text)
}

/// Every stream in the file that could hold page content: skips images
/// and anything we cannot decompress.
fn content_streams(pdf: &[u8]) -> Vec<Vec<u8>> {
    let mut streams = Vec::new();
    let mut i = 0usize;
    while let Some(found) = find(pdf, b"stream", i) {
        // The dictionary in front of this stream, bounded by the
        // previous "obj" so we do not read the whole file.
        let dict_start = rfind(pdf, b"obj", found).map(|p| p + 3).unwrap_or(0);
        let dict = &pdf[dict_start..found];
        let mut data_start = found + b"stream".len();
        // "stream" is followed by CRLF or LF.
        if pdf.get(data_start) == Some(&b'\r') {
            data_start += 1;
        }
        if pdf.get(data_start) == Some(&b'\n') {
            data_start += 1;
        }
        let Some(end) = find(pdf, b"endstream", data_start) else {
            break;
        };
        i = end + b"endstream".len();

        if contains(dict, b"/Image") || contains(dict, b"/XObject") {
            continue;
        }
        let raw = &pdf[data_start..end];
        if contains(dict, b"/FlateDecode") {
            if let Some(inflated) = inflate(raw) {
                streams.push(inflated);
            }
        } else if !contains(dict, b"/Filter") {
            streams.push(raw.to_vec());
        }
    }
    streams
}

fn inflate(raw: &[u8]) -> Option<Vec<u8>> {
    use std::io::Read;
    // Streams may carry trailing EOL before "endstream"; zlib stops on
    // its own, so trailing bytes are harmless.
    let mut out = Vec::new();
    let mut decoder = flate2::read::ZlibDecoder::new(raw);
    match decoder.read_to_end(&mut out) {
        Ok(_) => Some(out),
        // Some producers write raw deflate without the zlib header.
        Err(_) => {
            let mut out = Vec::new();
            let mut decoder = flate2::read::DeflateDecoder::new(raw);
            decoder.read_to_end(&mut out).ok().map(|_| out)
        }
    }
}

/// Pulls the strings out of a content stream: `(literal) Tj`,
/// `<hex> Tj`, and `[(a) -250 (b)] TJ`. Positioning operators become
/// line breaks.
fn text_from_content(content: &[u8]) -> String {
    let mut out = String::new();
    let mut i = 0usize;
    let mut pending_newline = false;
    while i < content.len() {
        match content[i] {
            b'(' => {
                let (s, next) = literal_string(content, i + 1);
                if pending_newline {
                    out.push('\n');
                    pending_newline = false;
                }
                out.push_str(&s);
                i = next;
            }
            b'<' if content.get(i + 1) != Some(&b'<') => {
                let (s, next) = hex_string(content, i + 1);
                if pending_newline {
                    out.push('\n');
                    pending_newline = false;
                }
                out.push_str(&s);
                i = next;
            }
            b'T' => {
                // Td / TD / T* / TJ / Tj all end a run of text; the
                // positioning ones start a new line.
                if matches!(content.get(i + 1), Some(b'd') | Some(b'D') | Some(b'*')) {
                    pending_newline = true;
                }
                i += 2;
            }
            b'E' if content[i..].starts_with(b"ET") => {
                pending_newline = true;
                i += 2;
            }
            _ => i += 1,
        }
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

fn literal_string(content: &[u8], mut i: usize) -> (String, usize) {
    let mut bytes: Vec<u8> = Vec::new();
    let mut depth = 1usize;
    while i < content.len() {
        match content[i] {
            b'\\' => {
                i += 1;
                match content.get(i) {
                    Some(b'n') => bytes.push(b'\n'),
                    Some(b'r') => bytes.push(b'\r'),
                    Some(b't') => bytes.push(b'\t'),
                    Some(b'b') => bytes.push(8),
                    Some(b'f') => bytes.push(12),
                    Some(c @ b'0'..=b'7') => {
                        // Octal escape, up to three digits.
                        let mut value = (c - b'0') as u32;
                        let mut digits = 1;
                        while digits < 3 {
                            match content.get(i + 1) {
                                Some(d @ b'0'..=b'7') => {
                                    value = value * 8 + (d - b'0') as u32;
                                    i += 1;
                                    digits += 1;
                                }
                                _ => break,
                            }
                        }
                        bytes.push(value as u8);
                    }
                    Some(c) => bytes.push(*c),
                    None => break,
                }
                i += 1;
            }
            b'(' => {
                depth += 1;
                bytes.push(b'(');
                i += 1;
            }
            b')' => {
                depth -= 1;
                i += 1;
                if depth == 0 {
                    break;
                }
                bytes.push(b')');
            }
            c => {
                bytes.push(c);
                i += 1;
            }
        }
    }
    (decode_winansi(&bytes), i)
}

fn hex_string(content: &[u8], mut i: usize) -> (String, usize) {
    let mut digits = String::new();
    while i < content.len() && content[i] != b'>' {
        let c = content[i] as char;
        if c.is_ascii_hexdigit() {
            digits.push(c);
        }
        i += 1;
    }
    let mut bytes = Vec::new();
    let mut chars = digits.chars();
    while let Some(a) = chars.next() {
        let b = chars.next().unwrap_or('0');
        if let Ok(byte) = u8::from_str_radix(&format!("{a}{b}"), 16) {
            bytes.push(byte);
        }
    }
    (decode_winansi(&bytes), i + 1)
}

/// WinAnsi (CP1252) → Rust chars. Our own PDF writer emits WinAnsi, and
/// it is by far the most common simple-font encoding; the 0x80–0x9F
/// range is where it differs from Latin-1.
fn decode_winansi(bytes: &[u8]) -> String {
    const HIGH: [char; 32] = [
        '€', '\u{fffd}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{fffd}', 'Ž',
        '\u{fffd}', '\u{fffd}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ',
        '\u{fffd}', 'ž', 'Ÿ',
    ];
    bytes
        .iter()
        .map(|b| match b {
            0x80..=0x9f => HIGH[(b - 0x80) as usize],
            _ => *b as char,
        })
        .collect()
}

/// Guards against emitting mojibake as if it were a document: a
/// subset-encoded font yields bytes that are not text at all. We want a
/// clear "no readable text layer" instead of nonsense suggestions.
fn looks_like_text(text: &str) -> bool {
    if text.chars().count() < 20 {
        return false;
    }
    let interesting = text
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace() || ".,-:/()%&+".contains(*c))
        .count();
    let total = text.chars().count();
    let letters = text.chars().filter(|c| c.is_alphabetic()).count();
    // Mostly recognizable characters, and actually containing words.
    interesting * 10 >= total * 9 && letters * 10 >= total
}

fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if from >= haystack.len() {
        return None;
    }
    haystack[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}

fn rfind(haystack: &[u8], needle: &[u8], before: usize) -> Option<usize> {
    haystack[..before.min(haystack.len())]
        .windows(needle.len())
        .rposition(|w| w == needle)
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    find(haystack, needle, 0).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdf::{Font, Pdf};

    /// Round-trip against our own writer: what we print, we can read.
    #[test]
    fn leser_teksten_fra_var_egen_pdf() {
        let mut pdf = Pdf::new();
        pdf.text(50.0, 700.0, 12.0, Font::Bold, "Faktura 1001");
        pdf.text(50.0, 680.0, 10.0, Font::Regular, "Grossisten AS");
        pdf.text(50.0, 660.0, 10.0, Font::Regular, "Å betale: 6 250,00");
        let bytes = pdf.finish();
        let text = extract(&bytes).expect("vår egen PDF har et tekstlag");
        assert!(text.contains("Faktura 1001"), "{text}");
        assert!(text.contains("Grossisten AS"), "{text}");
        assert!(text.contains("Å betale: 6 250,00"), "{text}");
        assert!(
            text.lines().count() >= 3,
            "hver tekstoperasjon blir sin egen linje: {text}"
        );
    }

    #[test]
    fn parenteser_og_escapes_overlever() {
        let mut pdf = Pdf::new();
        pdf.text(50.0, 700.0, 10.0, Font::Regular, "Vare (rabatt 10%)");
        pdf.text(50.0, 680.0, 10.0, Font::Regular, "Levert av Grossisten AS");
        let text = extract(&pdf.finish()).unwrap();
        assert!(text.contains("Vare (rabatt 10%)"), "{text}");
        assert!(text.contains("Levert av Grossisten AS"), "{text}");
    }

    #[test]
    fn ikke_pdf_gir_ingenting() {
        assert!(extract(b"dette er en tekstfil, ikke en PDF").is_none());
        assert!(extract(&[]).is_none());
    }

    #[test]
    fn pdf_uten_lesbart_tekstlag_gir_ingenting() {
        // A "scan": a PDF whose only stream is an image.
        let fake = b"%PDF-1.4\n1 0 obj\n<< /Subtype /Image /Length 8 >>\nstream\n\x00\x01\x02\x03\x04\x05\x06\x07\nendstream\nendobj\n";
        assert!(extract(fake).is_none());
    }

    #[test]
    fn mojibake_regnes_ikke_som_tekst() {
        // Subset-encoded font: bytes that decode to control characters.
        let content = b"BT /F1 10 Tf 50 700 Td (\x01\x02\x03\x04\x05\x06\x07\x08\x0b\x0c\x0e\x0f\x10\x11\x12\x13\x14\x15\x16\x17\x18\x19\x1a) Tj ET";
        let mut fake = b"%PDF-1.4\n1 0 obj\n<< /Length 99 >>\nstream\n".to_vec();
        fake.extend_from_slice(content);
        fake.extend_from_slice(b"\nendstream\nendobj\n");
        assert!(extract(&fake).is_none(), "søppel skal aldri bli et forslag");
    }

    #[test]
    fn flate_komprimert_strom_leses() {
        use std::io::Write;
        let content = b"BT /F1 10 Tf 50 700 Td (Fakturanr 90210 fra Handelshuset AS) Tj ET";
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(content).unwrap();
        let compressed = encoder.finish().unwrap();
        let mut fake = format!(
            "%PDF-1.4\n1 0 obj\n<< /Filter /FlateDecode /Length {} >>\nstream\n",
            compressed.len()
        )
        .into_bytes();
        fake.extend_from_slice(&compressed);
        fake.extend_from_slice(b"\nendstream\nendobj\n");
        let text = extract(&fake).expect("flate-strøm skal leses");
        assert!(text.contains("Fakturanr 90210"), "{text}");
    }
}
