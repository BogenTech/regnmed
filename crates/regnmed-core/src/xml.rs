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
