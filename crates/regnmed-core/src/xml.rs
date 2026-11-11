//! Minimal deterministic XML writer shared by the export formats (SAF-T,
//! mva-melding). Hand-rolled on purpose: no dependency, byte-identical
//! output for identical input, and nothing this crate doesn't need.

pub(crate) struct Xml {
    pub(crate) out: String,
    pub(crate) depth: usize,
}

impl Xml {
    pub(crate) fn new() -> Self {
        Xml {
            out: String::new(),
            depth: 0,
        }
    }

    pub(crate) fn raw(&mut self, line: &str) {
        self.out.push_str(line);
        self.out.push('\n');
    }

    fn indent(&mut self) {
        for _ in 0..self.depth {
            self.out.push_str("  ");
        }
    }

    pub(crate) fn open(&mut self, tag: &str) {
        self.indent();
        self.out.push('<');
        self.out.push_str(tag);
        self.out.push_str(">\n");
        self.depth += 1;
    }

    pub(crate) fn close(&mut self, tag: &str) {
        self.depth -= 1;
        self.indent();
        self.out.push_str("</");
        self.out.push_str(tag);
        self.out.push_str(">\n");
    }

    /// Empty element, e.g. `<betalingsinformasjon/>`.
    pub(crate) fn empty(&mut self, tag: &str) {
        self.indent();
        self.out.push('<');
        self.out.push_str(tag);
        self.out.push_str("/>\n");
    }

    pub(crate) fn leaf(&mut self, tag: &str, value: &str) {
        self.indent();
        self.out.push('<');
        self.out.push_str(tag);
        self.out.push('>');
        for c in value.chars() {
            match c {
                '&' => self.out.push_str("&amp;"),
                '<' => self.out.push_str("&lt;"),
                '>' => self.out.push_str("&gt;"),
                _ => self.out.push(c),
            }
        }
        self.out.push_str("</");
        self.out.push_str(tag);
        self.out.push_str(">\n");
    }
}

/// Integer øre → "1234.56", sign included. XML decimals always use a dot,
/// whatever the locale of the reader.
pub(crate) fn decimal(ore: i64) -> String {
    let sign = if ore < 0 { "-" } else { "" };
    format!(
        "{sign}{}.{:02}",
        ore.unsigned_abs() / 100,
        ore.unsigned_abs() % 100
    )
}

/// Basis points as the Norwegian kodelister write a sats: "25", "14.1",
/// "11.11" (the mva-melding and the a-melding's aga table both use it).
pub(crate) fn rate(bp: i64) -> String {
    match (bp / 100, bp % 100) {
        (whole, 0) => format!("{whole}"),
        (whole, frac) if frac % 10 == 0 => format!("{whole}.{}", frac / 10),
        (whole, frac) => format!("{whole}.{frac:02}"),
    }
}
