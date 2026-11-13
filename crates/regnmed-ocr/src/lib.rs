//! Text recognition for receipt photos (docs/bilagstolkning.md).
//!
//! The engine is `ocrs` -- pure Rust, so the binary stays static like the
//! rest of the cluster. Its recognition model knows only ASCII: «Ø» comes
//! out as «O» and «ø» as «?». That costs nothing where it matters, because
//! `voucher_hints` matches on normalized ASCII keywords and on digits; it
//! does cost the shop's name its letters, which the user corrects.
//!
//! What we add on top is the READING ORDER. ocrs groups text into blocks,
//! so a receipt's «TOTALT» and its «157,70» come out far apart -- and the
//! parser finds an amount by its label on the same line. `rows` puts each
//! printed line back together.

use std::io::Cursor;
use std::path::Path;

use anyhow::{Context, Result};
use image::{DynamicImage, ImageDecoder, ImageReader};
use ocrs::{ImageSource, OcrEngine, OcrEngineParams, TextItem};
use rten::Model;

/// A photo is scaled down to this on its long edge before reading: the
/// portal's editor saves at most 2400 px, and a raw 12 MP camera file
/// would only cost memory, not accuracy.
pub const MAX_EDGE: u32 = 2400;

/// One recognized stretch of text and where it sat.
#[derive(Debug, Clone)]
pub struct Piece {
    pub top: i32,
    pub bottom: i32,
    pub left: i32,
    pub text: String,
}

/// Puts pieces that share a printed line back on one line, left to right.
/// Two pieces share a line when they overlap vertically by at least half
/// the shorter one's height. The comparison is against the line's FIRST
/// piece, not the union of everything gathered so far: a growing union
/// swallows the next line down on a tightly printed receipt.
pub fn rows(mut pieces: Vec<Piece>) -> String {
    pieces.sort_by_key(|p| (p.top + p.bottom, p.left));
    let mut rows: Vec<Vec<Piece>> = Vec::new();
    for piece in pieces {
        let same_line = rows.last().is_some_and(|row| {
            let first = &row[0];
            let overlap = piece.bottom.min(first.bottom) - piece.top.max(first.top);
            overlap * 2 >= (piece.bottom - piece.top).min(first.bottom - first.top)
        });
        match rows.last_mut() {
            Some(row) if same_line => row.push(piece),
            _ => rows.push(vec![piece]),
        }
    }
    rows.into_iter()
        .map(|mut row| {
            row.sort_by_key(|p| p.left);
            row.into_iter()
                .map(|p| p.text)
                .collect::<Vec<_>>()
                .join("  ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub struct Reader {
    engine: OcrEngine,
}

impl Reader {
    /// Loads the two models from `dir` (text-detection.onnx,
    /// text-recognition.onnx -- fetched by scripts/fetch-ocr-models.sh).
    pub fn load(dir: &Path) -> Result<Self> {
        let model = |name: &str| {
            let path = dir.join(name);
            Model::load_file(&path).with_context(|| format!("loading {}", path.display()))
        };
        let engine = OcrEngine::new(OcrEngineParams {
            detection_model: Some(model("text-detection.onnx")?),
            recognition_model: Some(model("text-recognition.onnx")?),
            ..Default::default()
        })?;
        Ok(Self { engine })
    }

    /// The text of a JPEG or PNG, one printed line per line.
    pub fn read(&self, bytes: &[u8]) -> Result<String> {
        let image = decode(bytes)?.into_rgb8();
        let source = ImageSource::from_bytes(image.as_raw(), image.dimensions())?;
        let input = self.engine.prepare_input(source)?;
        let words = self.engine.detect_words(&input)?;
        let lines = self.engine.find_text_lines(&input, &words);
        let pieces = self
            .engine
            .recognize_text(&input, &lines)?
            .into_iter()
            .flatten()
            .map(|line| {
                let r = line.bounding_rect();
                Piece {
                    top: r.top(),
                    bottom: r.bottom(),
                    left: r.left(),
                    text: line.to_string(),
                }
            })
            .collect();
        Ok(rows(pieces))
    }
}

/// Decodes, turns the picture the way the camera said it was held (a
/// phone stores sideways pixels and an EXIF note), and scales it down.
fn decode(bytes: &[u8]) -> Result<DynamicImage> {
    let mut decoder = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()?
        .into_decoder()
        .context("not a JPEG or PNG image")?;
    let orientation = decoder.orientation()?;
    let mut image = DynamicImage::from_decoder(decoder)?;
    image.apply_orientation(orientation);
    if image.width().max(image.height()) > MAX_EDGE {
        image = image.resize(MAX_EDGE, MAX_EDGE, image::imageops::FilterType::Triangle);
    }
    Ok(image)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn piece(top: i32, bottom: i32, left: i32, text: &str) -> Piece {
        Piece {
            top,
            bottom,
            left,
            text: text.into(),
        }
    }

    #[test]
    fn a_label_and_its_amount_end_up_on_one_line() {
        // As ocrs returns them: the label column first, then the amounts.
        let text = rows(vec![
            piece(100, 120, 10, "TOTALT"),
            piece(124, 144, 10, "Bankkort"),
            piece(101, 121, 400, "157.70"),
            piece(125, 145, 400, "157.70"),
        ]);
        assert_eq!(text, "TOTALT  157.70\nBankkort  157.70");
    }

    #[test]
    fn tightly_printed_lines_stay_apart() {
        // Each line touches the next; a union-based row would swallow all three.
        let text = rows(vec![
            piece(0, 20, 0, "a"),
            piece(8, 28, 50, "b"),
            piece(18, 38, 0, "c"),
            piece(36, 56, 0, "d"),
        ]);
        assert_eq!(text, "a  b\nc\nd");
    }

    /// The real engine on a rendered receipt, when the models are present
    /// (OCR_MODELS_DIR); skipped politely otherwise, like the DB tests.
    #[test]
    fn reads_a_receipt_when_the_models_are_present() {
        let Ok(dir) = std::env::var("OCR_MODELS_DIR") else {
            eprintln!("OCR_MODELS_DIR unset -- skipping");
            return;
        };
        let reader = Reader::load(Path::new(&dir)).unwrap();
        let text = reader.read(include_bytes!("../tests/receipt.jpg")).unwrap();
        assert!(
            text.lines()
                .any(|l| l.starts_with("TOTALT") && l.contains("157")),
            "{text}"
        );
        assert!(text.contains("915 933 149"), "{text}");
    }
}
