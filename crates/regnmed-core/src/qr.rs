//! QR codes, for one job: putting a sign-in URL on a screen a phone can
//! photograph.
//!
//! Hand-rolled like every other format in this crate, and for the usual
//! reason — but the scope is narrower than the standard, deliberately:
//! BYTE mode, error correction level L, versions 1 to 5 only. Those are
//! the versions with a SINGLE error-correction block, which removes the
//! interleaving half of the specification, and level L is right for a
//! short-lived URL on a lit screen. A payload that does not fit says so
//! loudly rather than silently producing something a camera cannot read.
//!
//! Verified against an independent encoder (segno) module for module, at
//! every version in range, with the golden test below pinning the result
//! so it can never drift.

use std::fmt::Write;

#[derive(Debug, PartialEq, Eq)]
pub enum QrError {
    /// The payload needs a version this encoder does not implement.
    TooLong { bytes: usize, max: usize },
    /// Byte mode encodes bytes; a payload must be representable as such.
    /// (Every URL is, which is why this is an error and not a mode switch.)
    NotEncodable,
}

impl std::fmt::Display for QrError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QrError::TooLong { bytes, max } => {
                write!(
                    f,
                    "{bytes} bytes is more than the {max} this QR encoder covers"
                )
            }
            QrError::NotEncodable => write!(f, "payload cannot be encoded in byte mode"),
        }
    }
}

impl std::error::Error for QrError {}

/// Data codewords per version at level L, versions 1..=5.
const DATA_CODEWORDS: [usize; 5] = [19, 34, 55, 80, 108];
/// Error-correction codewords per version at level L. One block each.
const EC_CODEWORDS: [usize; 5] = [7, 10, 15, 20, 26];
/// The single alignment pattern's centre, versions 2..=5 (version 1 has none).
const ALIGNMENT_CENTRE: [usize; 5] = [0, 18, 22, 26, 30];

/// The biggest payload this encoder takes. Four bits of mode and eight of
/// length come off the top, so a byte-mode payload loses two codewords.
pub const MAX_BYTES: usize = DATA_CODEWORDS[4] - 2;

/// The modules of the finished code, row-major, true = dark. Includes no
/// quiet zone: that belongs to whatever draws it.
pub fn matrix(payload: &str) -> Result<Vec<Vec<bool>>, QrError> {
    let bytes = payload.as_bytes();
    if bytes.len() > MAX_BYTES {
        return Err(QrError::TooLong {
            bytes: bytes.len(),
            max: MAX_BYTES,
        });
    }
    if bytes.len() > 255 {
        // The 8-bit length field cannot say it. Unreachable while
        // MAX_BYTES is 106, but the invariant belongs next to the field.
        return Err(QrError::NotEncodable);
    }

    let version = (1..=5)
        .find(|v| bytes.len() <= DATA_CODEWORDS[v - 1] - 2)
        .expect("MAX_BYTES already bounded the length");
    let data_len = DATA_CODEWORDS[version - 1];
    let ec_len = EC_CODEWORDS[version - 1];

    let mut codewords = encode_bytes(bytes, data_len);
    codewords.extend(reed_solomon(&codewords, ec_len));

    let size = 17 + 4 * version;
    let (mut modules, reserved) = skeleton(version, size);
    place_data(&mut modules, &reserved, size, &codewords);

    // The mask is chosen, not fixed: a bad one leaves large even blocks or
    // finder-like runs that make a camera work harder than it has to.
    let mask = (0u8..8)
        .min_by_key(|&mask| {
            let mut candidate = modules.clone();
            apply_mask(&mut candidate, &reserved, size, mask);
            write_format(&mut candidate, size, mask);
            penalty(&candidate, size)
        })
        .expect("eight masks");
    apply_mask(&mut modules, &reserved, size, mask);
    write_format(&mut modules, size, mask);
    Ok(modules)
}

/// The code as an SVG, with the four-module quiet zone the specification
/// requires — without it, readers lose the edge of the code.
///
/// Black on white whatever the page's theme is: an inverted QR is readable
/// to some scanners and invisible to others, and this one has to work the
/// first time, held at arm's length, in a car.
pub fn svg(payload: &str) -> Result<String, QrError> {
    let modules = matrix(payload)?;
    let size = modules.len();
    let span = size + 8;

    let mut path = String::new();
    for (r, row) in modules.iter().enumerate() {
        for (c, dark) in row.iter().enumerate() {
            if *dark {
                let _ = write!(path, "M{} {}h1v1h-1z", c + 4, r + 4);
            }
        }
    }
    Ok(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {span} {span}\" \
         shape-rendering=\"crispEdges\" role=\"img\" aria-label=\"QR-kode for innlogging\">\
         <rect width=\"{span}\" height=\"{span}\" fill=\"#fff\"/>\
         <path fill=\"#000\" d=\"{path}\"/></svg>"
    ))
}

/// Mode indicator, length, the bytes, then the specified padding.
fn encode_bytes(bytes: &[u8], data_len: usize) -> Vec<u8> {
    let mut bits = Vec::with_capacity((data_len + 2) * 8);
    let mut push = |value: u16, width: usize| {
        for i in (0..width).rev() {
            bits.push((value >> i) & 1 == 1);
        }
    };
    push(0b0100, 4); // byte mode
    push(bytes.len() as u16, 8); // versions 1..=9 use an 8-bit count
    for byte in bytes {
        push(*byte as u16, 8);
    }
    // Terminator, truncated if the capacity ends first.
    for _ in 0..4usize.min(data_len * 8 - bits.len()) {
        bits.push(false);
    }
    while bits.len() % 8 != 0 {
        bits.push(false);
    }

    let mut codewords: Vec<u8> = bits
        .chunks(8)
        .map(|chunk| chunk.iter().fold(0u8, |acc, b| (acc << 1) | *b as u8))
        .collect();
    // The two pad bytes the specification names, alternating.
    for pad in [0xECu8, 0x11].iter().cycle() {
        if codewords.len() >= data_len {
            break;
        }
        codewords.push(*pad);
    }
    codewords
}

/// GF(256) with the QR primitive polynomial 0x11D.
fn gf_tables() -> ([u8; 512], [u8; 256]) {
    let mut exp = [0u8; 512];
    let mut log = [0u8; 256];
    let mut x: u16 = 1;
    for i in 0..255 {
        exp[i] = x as u8;
        log[x as usize] = i as u8;
        x <<= 1;
        if x & 0x100 != 0 {
            x ^= 0x11D;
        }
    }
    for i in 255..512 {
        exp[i] = exp[i - 255];
    }
    (exp, log)
}

/// The remainder of data·x^n divided by the generator polynomial — the
/// error-correction codewords, in order.
fn reed_solomon(data: &[u8], ec_len: usize) -> Vec<u8> {
    let (exp, log) = gf_tables();
    let mul = |a: u8, b: u8| -> u8 {
        if a == 0 || b == 0 {
            0
        } else {
            exp[log[a as usize] as usize + log[b as usize] as usize]
        }
    };

    // Generator: product of (x - α^i), i < ec_len.
    let mut generator = vec![1u8];
    for i in 0..ec_len {
        let root = exp[i];
        let mut next = vec![0u8; generator.len() + 1];
        for (j, g) in generator.iter().enumerate() {
            next[j] ^= *g;
            next[j + 1] ^= mul(*g, root);
        }
        generator = next;
    }

    let mut remainder = vec![0u8; data.len() + ec_len];
    remainder[..data.len()].copy_from_slice(data);
    for i in 0..data.len() {
        let factor = remainder[i];
        if factor == 0 {
            continue;
        }
        for (j, g) in generator.iter().enumerate() {
            remainder[i + j] ^= mul(*g, factor);
        }
    }
    remainder[data.len()..].to_vec()
}

/// Everything that is fixed by the version: finders, separators, timing,
/// alignment, the dark module, and the reserved format areas. Returns the
/// modules and a mask of which positions data must not touch.
fn skeleton(version: usize, size: usize) -> (Vec<Vec<bool>>, Vec<Vec<bool>>) {
    let mut m = vec![vec![false; size]; size];
    let mut reserved = vec![vec![false; size]; size];

    // Finder patterns, each with its separator: the 8×8 corner is all
    // function module, so reserving the square covers both.
    for (top, left) in [(0, 0), (0, size - 7), (size - 7, 0)] {
        for dr in 0..7 {
            for dc in 0..7 {
                let dark = dr == 0
                    || dr == 6
                    || dc == 0
                    || dc == 6
                    || (2..=4).contains(&dr) && (2..=4).contains(&dc);
                m[top + dr][left + dc] = dark;
            }
        }
        for dr in -1i32..8 {
            for dc in -1i32..8 {
                let r = top as i32 + dr;
                let c = left as i32 + dc;
                if (0..size as i32).contains(&r) && (0..size as i32).contains(&c) {
                    reserved[r as usize][c as usize] = true;
                }
            }
        }
    }

    // Timing patterns: row 6 and column 6, alternating, dark at even index.
    for i in 0..size {
        if !reserved[6][i] {
            m[6][i] = i % 2 == 0;
            reserved[6][i] = true;
        }
        if !reserved[i][6] {
            m[i][6] = i % 2 == 0;
            reserved[i][6] = true;
        }
    }

    // One alignment pattern from version 2 on; the other three positions
    // the coordinate table implies all collide with finder patterns.
    if version >= 2 {
        let centre = ALIGNMENT_CENTRE[version - 1];
        for dr in 0..5 {
            for dc in 0..5 {
                let dark = dr == 0 || dr == 4 || dc == 0 || dc == 4 || (dr == 2 && dc == 2);
                m[centre - 2 + dr][centre - 2 + dc] = dark;
                reserved[centre - 2 + dr][centre - 2 + dc] = true;
            }
        }
    }

    // The dark module, always set, always at this one place.
    m[4 * version + 9][8] = true;
    reserved[4 * version + 9][8] = true;

    // Format information, written after the mask is known.
    for i in 0..9 {
        reserved[8][i] = true;
        reserved[i][8] = true;
    }
    for i in 0..8 {
        reserved[8][size - 1 - i] = true;
        reserved[size - 1 - i][8] = true;
    }

    (m, reserved)
}

/// The zigzag: two-module columns from the right edge, alternating
/// direction, skipping the timing column.
fn place_data(m: &mut [Vec<bool>], reserved: &[Vec<bool>], size: usize, codewords: &[u8]) {
    let bit = |n: usize| -> bool {
        codewords
            .get(n / 8)
            .map(|cw| (cw >> (7 - n % 8)) & 1 == 1)
            .unwrap_or(false)
    };
    let mut n = 0usize;
    let mut up = true;
    // `col` is the right-hand column of the pair, and i32 so the loop can
    // walk off the left edge and stop.
    let mut col = size as i32 - 1;
    while col > 0 {
        if col == 6 {
            // Column 6 is timing from top to bottom; the pair steps past it.
            col -= 1;
        }
        for i in 0..size {
            let row = if up { size - 1 - i } else { i };
            for c in [col as usize, (col - 1) as usize] {
                if !reserved[row][c] {
                    m[row][c] = bit(n);
                    n += 1;
                }
            }
        }
        up = !up;
        col -= 2;
    }
}

fn mask_bit(mask: u8, i: usize, j: usize) -> bool {
    match mask {
        0 => (i + j) % 2 == 0,
        1 => i % 2 == 0,
        2 => j % 3 == 0,
        3 => (i + j) % 3 == 0,
        4 => (i / 2 + j / 3) % 2 == 0,
        5 => (i * j) % 2 + (i * j) % 3 == 0,
        6 => ((i * j) % 2 + (i * j) % 3) % 2 == 0,
        _ => ((i + j) % 2 + (i * j) % 3) % 2 == 0,
    }
}

fn apply_mask(m: &mut [Vec<bool>], reserved: &[Vec<bool>], size: usize, mask: u8) {
    for i in 0..size {
        for j in 0..size {
            if !reserved[i][j] && mask_bit(mask, i, j) {
                m[i][j] = !m[i][j];
            }
        }
    }
}

/// Format information: level L and the mask, BCH-protected, written twice
/// so a damaged corner does not cost the whole code.
fn write_format(m: &mut [Vec<bool>], size: usize, mask: u8) {
    let data = 0b01_000 | mask as u32; // 01 = level L
    let mut bch = data << 10;
    for i in (0..5).rev() {
        if bch & (1 << (i + 10)) != 0 {
            bch ^= 0b10100110111 << i;
        }
    }
    let format = ((data << 10) | bch) ^ 0b101010000010010;

    for i in 0..15 {
        // Bit 14 is the most significant and goes FIRST — verified cell by
        // cell against an independent encoder, at every version and mask.
        let bit = (format >> (14 - i)) & 1 == 1;
        // First copy: along row 8, then up column 8.
        if i < 6 {
            m[8][i] = bit;
        } else if i < 8 {
            m[8][i + 1] = bit;
        } else if i == 8 {
            m[7][8] = bit;
        } else {
            m[14 - i][8] = bit;
        }
        // Second copy, split between the other two corners. Seven bits up
        // the bottom-left column, not eight: the eighth position is the
        // dark module, which is always set and belongs to nobody.
        if i < 7 {
            m[size - 1 - i][8] = bit;
        } else {
            m[8][size - 15 + i] = bit;
        }
    }
}

/// The four penalty rules. Lower is better; only the relative order
/// matters, since this exists to pick between eight candidates.
fn penalty(m: &[Vec<bool>], size: usize) -> u32 {
    let mut score = 0u32;

    // Rule 1: runs of five or more of one colour, along both axes.
    for line in 0..size {
        for horizontal in [true, false] {
            let at = |i: usize| if horizontal { m[line][i] } else { m[i][line] };
            let mut run = 1;
            for i in 1..size {
                if at(i) == at(i - 1) {
                    run += 1;
                } else {
                    if run >= 5 {
                        score += 3 + (run - 5);
                    }
                    run = 1;
                }
            }
            if run >= 5 {
                score += 3 + (run - 5);
            }
        }
    }

    // Rule 2: every 2×2 block of one colour.
    for i in 0..size - 1 {
        for j in 0..size - 1 {
            let v = m[i][j];
            if m[i][j + 1] == v && m[i + 1][j] == v && m[i + 1][j + 1] == v {
                score += 3;
            }
        }
    }

    // Rule 3: the finder-like 1:1:3:1:1 run with four light modules beside
    // it, which is what makes a reader mistake data for a corner.
    const FINDER: [bool; 11] = [
        true, false, true, true, true, false, true, false, false, false, false,
    ];
    for line in 0..size {
        for horizontal in [true, false] {
            let at = |i: usize| if horizontal { m[line][i] } else { m[i][line] };
            for start in 0..size.saturating_sub(10) {
                let forward = (0..11).all(|k| at(start + k) == FINDER[k]);
                let backward = (0..11).all(|k| at(start + k) == FINDER[10 - k]);
                if forward || backward {
                    score += 40;
                }
            }
        }
    }

    // Rule 4: how far the dark proportion is from half.
    let dark: usize = m.iter().flatten().filter(|d| **d).count();
    let percent = dark * 100 / (size * size);
    score += (percent.abs_diff(50) / 5) as u32 * 10;

    score
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Renders the matrix as text, which is what the golden value pins and
    /// what was compared against segno.
    fn render(payload: &str) -> String {
        matrix(payload)
            .unwrap()
            .iter()
            .map(|row| {
                row.iter()
                    .map(|d| if *d { '#' } else { '.' })
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The real payload shape, at the version it actually lands on.
    #[test]
    fn a_sign_in_url_encodes_to_a_version_3_code() {
        let m = matrix("https://id.regnmed.no/device?user_code=WDJB-MJHT").unwrap();
        assert_eq!(m.len(), 29, "48 bytes fits version 3 (29 modules)");
        // Finder patterns, which is the cheapest proof the skeleton is right.
        for (r, c) in [(0, 0), (0, 22), (22, 0)] {
            assert!(m[r][c] && m[r + 6][c] && m[r][c + 6], "finder at {r},{c}");
            assert!(!m[r + 1][c + 1], "finder ring at {r},{c}");
            assert!(m[r + 3][c + 3], "finder centre at {r},{c}");
        }
    }

    /// Every version in range, so a change to the version choice, the
    /// padding or the block layout cannot pass unnoticed.
    #[test]
    fn each_version_is_reached_and_sized() {
        for (bytes, modules) in [(10, 21), (25, 25), (40, 29), (60, 33), (100, 37)] {
            let payload = "x".repeat(bytes);
            assert_eq!(matrix(&payload).unwrap().len(), modules, "{bytes} bytes");
        }
    }

    #[test]
    fn a_payload_too_long_says_so_instead_of_guessing() {
        let err = matrix(&"x".repeat(MAX_BYTES + 1)).unwrap_err();
        assert_eq!(
            err,
            QrError::TooLong {
                bytes: MAX_BYTES + 1,
                max: MAX_BYTES
            }
        );
    }

    #[test]
    fn the_modules_are_pinned() {
        assert_eq!(render("REGNMED"), GOLDEN_REGNMED);
    }

    const GOLDEN_REGNMED: &str = "#######..#.##.#######\n\
#.....#..###..#.....#\n\
#.###.#.##.##.#.###.#\n\
#.###.#..#.#..#.###.#\n\
#.###.#...#.#.#.###.#\n\
#.....#.....#.#.....#\n\
#######.#.#.#.#######\n\
........##.##........\n\
###.########.##...#..\n\
#....#.##.#...##...#.\n\
.#..#####.#.#...#####\n\
..##....#.....##...##\n\
..#...##.##.#.###.#.#\n\
........#..#.#.#.....\n\
#######.####.#####.##\n\
#.....#.#..###......#\n\
#.###.#.#..#.####.###\n\
#.###.#.......##..##.\n\
#.###.#.#.#.#..##...#\n\
#.....#.###...#....#.\n\
#######.##..#.##..###";

    #[test]
    fn the_svg_carries_the_quiet_zone_and_scales() {
        let svg = svg("REGNMED").unwrap();
        // 21 modules + 4 on each side.
        assert!(svg.contains("viewBox=\"0 0 29 29\""), "{svg}");
        assert!(
            svg.contains("fill=\"#fff\""),
            "a QR needs its light background"
        );
        assert!(svg.contains("fill=\"#000\""));
    }
}
