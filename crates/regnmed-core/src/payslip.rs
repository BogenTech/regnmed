//! The payslip as PDF (docs/lonn.md, #46).
//!
//! Rendered with the same hand-rolled, deterministic PDF writer as the
//! faktura (`regnmed-core::pdf`) — the same reasoning: a document we are
//! answerable for, without an engine that behaves in ways we cannot see.
//!
//! **The slip is not stored.** Unlike the faktura, where the PDF *is* the
//! sales document and is therefore posted as an attachment, the payslip
//! is derived from the payroll line — and the lines are insert-only. The
//! same line yields the same slip forever, so it renders on request
//! rather than storing yet another copy of personal data.
//!
//! The slip shows the **birth date, not the fødselsnummer**, as
//! everywhere else in the system. Employees know their own number; a file
//! that goes astray need not carry it.

use crate::money::Ore;
use crate::pdf::{Font, Pdf};

const MARGIN: f32 = 50.0;
const RIGHT: f32 = 545.0;

const MANEDER: [&str; 12] = [
    "januar",
    "februar",
    "mars",
    "april",
    "mai",
    "juni",
    "juli",
    "august",
    "september",
    "oktober",
    "november",
    "desember",
];

/// One line on the slip: a pay element or a deduction.
#[derive(Debug, Clone)]
pub struct PayslipLine {
    pub tekst: String,
    /// Positive adds to what is paid, negative is a deduction.
    pub amount_ore: i64,
}

#[derive(Debug, Clone)]
pub struct PayslipInput {
    pub employer_name: String,
    pub arbeidsgiver_orgnr: String,
    pub arbeidsgiver_adresse: Option<String>,
    pub employee_name: String,
    pub ansatt_stilling: Option<String>,
    /// Aksjeloven-style restraint: the date identifies, the number is
    /// not needed on a document that travels.
    pub employee_birth_date: Option<chrono::NaiveDate>,
    pub ar: i32,
    pub month: u32,
    pub paid_date: chrono::NaiveDate,
    pub lines: Vec<PayslipLine>,
    /// Sum of every pay line — what "bruttolønn" means to the person
    /// reading the slip, feriepenger included.
    pub gross_ore: i64,
    /// What withholding was actually computed on. Lower than brutto
    /// exactly when trekkfrie feriepenger were paid, which is why the
    /// slip can explain the difference instead of leaving the employee
    /// to wonder.
    pub withholding_basis_ore: i64,
    pub withholding_ore: i64,
    /// The skattekort percentage in basis points, when withholding is by
    /// percentage. None for frikort.
    pub withholding_percent_bp: Option<i64>,
    /// December's half withholding was applied.
    pub half_withholding: bool,
    pub net_ore: i64,
    /// Feriepenger earned this month — money the employee has coming,
    /// not money paid now, so it is stated apart from the total.
    pub holiday_pay_accrual_ore: i64,
    pub holiday_pay_bp: i64,
    /// Year to date, through this run.
    pub hittil_brutto_ore: i64,
    pub hittil_trekk_ore: i64,
    pub hittil_feriepenger_ore: i64,
}

fn kr(ore: i64) -> String {
    let raw = Ore(ore).to_string();
    let (heltall, rest) = raw.split_once(',').expect("Ore always has decimals");
    let (sign, digits) = heltall
        .strip_prefix('-')
        .map_or(("", heltall), |d| ("-", d));
    let mut grouped = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            grouped.push(' ');
        }
        grouped.push(c);
    }
    format!("{sign}{grouped},{rest}")
}

fn prosent(bp: i64) -> String {
    let hele = bp / 100;
    let desimal = bp % 100;
    if desimal == 0 {
        format!("{hele} %")
    } else {
        format!("{hele},{desimal:02} %").replace(",00 %", " %")
    }
}

/// Renders the payslip. Deterministic: same input, same bytes.
pub fn render_payslip(input: &PayslipInput) -> Vec<u8> {
    let mut pdf = Pdf::new();

    // Employer, top left.
    pdf.text(MARGIN, 60.0, 13.0, Font::Bold, &input.employer_name);
    let mut y = 76.0;
    if let Some(adresse) = &input.arbeidsgiver_adresse {
        pdf.text(MARGIN, y, 9.0, Font::Regular, adresse);
        y += 12.0;
    }
    pdf.text(
        MARGIN,
        y,
        9.0,
        Font::Regular,
        &format!("Org.nr {}", input.arbeidsgiver_orgnr),
    );

    // Title and period, top right.
    pdf.text_right(RIGHT, 60.0, 16.0, Font::Bold, "LØNNSSLIPP");
    let month_label = MANEDER
        .get((input.month as usize).saturating_sub(1))
        .copied()
        .unwrap_or("");
    let fakta = [
        ("Periode", format!("{month_label} {}", input.ar)),
        ("Utbetalt", input.paid_date.to_string()),
    ];
    let mut fy = 82.0;
    for (label, value) in &fakta {
        pdf.text_right(RIGHT - 70.0, fy, 9.0, Font::Regular, label);
        pdf.text_right(RIGHT, fy, 9.0, Font::Bold, value);
        fy += 12.0;
    }

    // Employee.
    pdf.text(MARGIN, 130.0, 8.0, Font::Bold, "ARBEIDSTAKER");
    pdf.text(MARGIN, 144.0, 11.0, Font::Regular, &input.employee_name);
    let mut ay = 158.0;
    if let Some(stilling) = &input.ansatt_stilling {
        pdf.text(MARGIN, ay, 9.0, Font::Regular, stilling);
        ay += 12.0;
    }
    if let Some(fodt) = input.employee_birth_date {
        pdf.text(MARGIN, ay, 9.0, Font::Regular, &format!("Født {fodt}"));
    }

    // Pay elements and deductions.
    let mut ly = 210.0;
    pdf.text(MARGIN, ly, 8.0, Font::Bold, "BESKRIVELSE");
    pdf.text_right(RIGHT, ly, 8.0, Font::Bold, "BELØP");
    ly += 6.0;
    pdf.rule(MARGIN, RIGHT, ly, 0.7);
    ly += 16.0;

    for line in &input.lines {
        pdf.text(MARGIN, ly, 10.0, Font::Regular, &line.tekst);
        pdf.text_right(RIGHT, ly, 10.0, Font::Regular, &kr(line.amount_ore));
        ly += 15.0;
    }

    ly += 4.0;
    pdf.rule(MARGIN, RIGHT, ly, 0.5);
    ly += 16.0;
    pdf.text(MARGIN, ly, 10.0, Font::Bold, "Bruttolønn");
    pdf.text_right(RIGHT, ly, 10.0, Font::Bold, &kr(input.gross_ore));
    ly += 18.0;

    // The withholding with its own explanation — the employee should
    // be able to see WHY it came out the way it did.
    let mut trekktekst = String::from("Forskuddstrekk");
    if let Some(bp) = input.withholding_percent_bp {
        trekktekst.push_str(&format!(
            " ({} av {})",
            prosent(bp),
            kr(input.withholding_basis_ore)
        ));
    } else {
        trekktekst.push_str(" (frikort)");
    }
    pdf.text(MARGIN, ly, 10.0, Font::Regular, &trekktekst);
    pdf.text_right(RIGHT, ly, 10.0, Font::Regular, &kr(-input.withholding_ore));
    ly += 14.0;
    if input.half_withholding {
        pdf.text(
            MARGIN,
            ly,
            8.0,
            Font::Regular,
            "Halvt forskuddstrekk i desember.",
        );
        ly += 12.0;
    }
    if input.gross_ore != input.withholding_basis_ore {
        pdf.text(
            MARGIN,
            ly,
            8.0,
            Font::Regular,
            "Feriepenger utbetales uten forskuddstrekk.",
        );
        ly += 12.0;
    }

    ly += 6.0;
    pdf.rule(MARGIN, RIGHT, ly, 1.2);
    ly += 18.0;
    pdf.text(MARGIN, ly, 12.0, Font::Bold, "Til utbetaling");
    pdf.text_right(RIGHT, ly, 12.0, Font::Bold, &kr(input.net_ore));

    // Earned, not paid: kept clearly outside the total.
    ly += 34.0;
    pdf.text(MARGIN, ly, 8.0, Font::Bold, "OPPTJENT DENNE MÅNEDEN");
    ly += 14.0;
    pdf.text(
        MARGIN,
        ly,
        9.0,
        Font::Regular,
        &format!(
            "Feriepenger ({} av bruttolønn) — utbetales neste år",
            prosent(input.holiday_pay_bp)
        ),
    );
    pdf.text_right(
        RIGHT,
        ly,
        9.0,
        Font::Regular,
        &kr(input.holiday_pay_accrual_ore),
    );

    // Year to date.
    ly += 30.0;
    pdf.text(
        MARGIN,
        ly,
        8.0,
        Font::Bold,
        &format!("HITTIL I {}", input.ar),
    );
    ly += 14.0;
    for (tekst, amount) in [
        ("Bruttolønn", input.hittil_brutto_ore),
        ("Forskuddstrekk", input.hittil_trekk_ore),
        ("Feriepenger opptjent", input.hittil_feriepenger_ore),
    ] {
        pdf.text(MARGIN, ly, 9.0, Font::Regular, tekst);
        pdf.text_right(RIGHT, ly, 9.0, Font::Regular, &kr(amount));
        ly += 13.0;
    }

    pdf.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> chrono::NaiveDate {
        chrono::NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn input() -> PayslipInput {
        PayslipInput {
            employer_name: "Testselskap AS".into(),
            arbeidsgiver_orgnr: "923 609 016".into(),
            arbeidsgiver_adresse: Some("Storgata 1, 0155 OSLO".into()),
            employee_name: "Kari Utvikler".into(),
            ansatt_stilling: Some("Utvikler".into()),
            employee_birth_date: Some(date(1993, 2, 26)),
            ar: 2026,
            month: 3,
            paid_date: date(2026, 3, 25),
            lines: vec![PayslipLine {
                tekst: "Fastlønn".into(),
                amount_ore: 5_000_000,
            }],
            gross_ore: 5_000_000,
            withholding_basis_ore: 5_000_000,
            withholding_ore: 1_750_000,
            withholding_percent_bp: Some(3500),
            half_withholding: false,
            net_ore: 3_250_000,
            holiday_pay_accrual_ore: 510_000,
            holiday_pay_bp: 1020,
            hittil_brutto_ore: 15_000_000,
            hittil_trekk_ore: 5_250_000,
            hittil_feriepenger_ore: 1_530_000,
        }
    }

    #[test]
    fn renders_a_wellformed_pdf() {
        let bytes = render_payslip(&input());
        assert!(bytes.starts_with(b"%PDF-1.4"), "PDF header");
        assert!(bytes.ends_with(b"%%EOF\n"), "PDF trailer");
        assert!(bytes.windows(9).any(|w| w == b"startxref"), "xref table");
        // Small enough to mail without thinking about it.
        assert!(bytes.len() < 8_000, "{} bytes", bytes.len());
    }

    #[test]
    fn is_deterministic() {
        assert_eq!(render_payslip(&input()), render_payslip(&input()));
    }

    /// The employee should be able to see why the withholding came out
    /// as it did, and why the feriepenger were not withheld from.
    #[test]
    fn the_slip_explains_the_withholding() {
        let tekst = String::from_utf8_lossy(&render_payslip(&input())).to_string();
        assert!(tekst.contains("Forskuddstrekk"), "{tekst}");
        assert!(tekst.contains("35 %"), "trekksatsen skal stå");

        let mut juni = input();
        juni.month = 6;
        // Brutto is everything paid out; the trekkgrunnlag is only the
        // ordinary pay.
        juni.gross_ore = 7_000_000;
        juni.withholding_basis_ore = 3_000_000;
        juni.withholding_ore = 1_050_000;
        juni.lines.push(PayslipLine {
            tekst: "Feriepenger".into(),
            amount_ore: 4_000_000,
        });
        juni.net_ore = 5_950_000;
        let tekst = String::from_utf8_lossy(&render_payslip(&juni)).to_string();
        assert!(
            tekst.contains("uten forskuddstrekk"),
            "feriepengenes trekkfrihet skal forklares"
        );

        let mut desember = input();
        desember.month = 12;
        desember.half_withholding = true;
        let tekst = String::from_utf8_lossy(&render_payslip(&desember)).to_string();
        assert!(tekst.contains("Halvt forskuddstrekk"), "{tekst}");
    }

    /// The fødselsnummer must never end up in a document that travels.
    #[test]
    fn the_slip_shows_the_birth_date_not_the_fodselsnummer() {
        let tekst = String::from_utf8_lossy(&render_payslip(&input())).to_string();
        assert!(tekst.contains("1993-02-26"));
        assert!(!tekst.contains("26829398612"));
    }

    #[test]
    fn frikort_says_frikort() {
        let mut fri = input();
        fri.withholding_percent_bp = None;
        fri.withholding_ore = 0;
        fri.net_ore = 5_000_000;
        let tekst = String::from_utf8_lossy(&render_payslip(&fri)).to_string();
        assert!(tekst.contains("frikort"), "{tekst}");
    }

    #[test]
    fn amounts_and_percentages_are_formatted_norwegian_style() {
        assert_eq!(kr(5_000_000), "50 000,00");
        assert_eq!(kr(-1_750_000), "-17 500,00");
        assert_eq!(kr(123_455), "1 234,55");
        assert_eq!(prosent(3500), "35 %");
        assert_eq!(prosent(1020), "10,20 %");
        assert_eq!(prosent(1250), "12,50 %");
    }
}
