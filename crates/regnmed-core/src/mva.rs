//! Merverdiavgift: terminer, dated rates and integer-øre beregning.
//!
//! Pure and deterministic like everything in this crate. Rates arrive as
//! data (loaded from the `vat_rate` table by regnmed-db) — the rate valid
//! on the voucher date decides the beregning, never a "current rate".

use chrono::{Datelike, NaiveDate};

/// A standard two-month mva-termin (1 = januar–februar … 6 = november–
/// desember). Årstermin and other special schemes are mva-melding
/// concerns, not ledger concerns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Termin {
    pub year: i32,
    /// 1–6.
    pub number: u8,
}

impl Termin {
    pub fn of(date: NaiveDate) -> Termin {
        Termin {
            year: date.year(),
            number: ((date.month() + 1) / 2) as u8,
        }
    }

    pub fn new(year: i32, number: u8) -> Option<Termin> {
        (1..=6).contains(&number).then_some(Termin { year, number })
    }

    pub fn start(self) -> NaiveDate {
        NaiveDate::from_ymd_opt(self.year, u32::from(self.number) * 2 - 1, 1)
            .expect("termin start is a valid date")
    }

    /// Last day of the termin's second month.
    pub fn end(self) -> NaiveDate {
        let next_month_start = if self.number == 6 {
            NaiveDate::from_ymd_opt(self.year + 1, 1, 1)
        } else {
            NaiveDate::from_ymd_opt(self.year, u32::from(self.number) * 2 + 1, 1)
        };
        next_month_start.expect("valid date") - chrono::Days::new(1)
    }
}

impl std::fmt::Display for Termin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}. termin {}", self.number, self.year)
    }
}

/// The company's mva-terminordning (docs/mva.md, #51). To-måneder is
/// the default; årstermin (turnover below the threshold, on application) and
/// primærnæring are yearly with their own frister. The ordning
/// Skatteetaten has GRANTED is recorded per company with valid_from —
/// eligibility is never auto-detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Terminordning {
    ToManeder,
    Arlig,
    Primaernaering,
}

impl Terminordning {
    pub fn parse(s: &str) -> Option<Terminordning> {
        match s {
            "to-maneder" => Some(Terminordning::ToManeder),
            "arlig" => Some(Terminordning::Arlig),
            "primaernaering" => Some(Terminordning::Primaernaering),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Terminordning::ToManeder => "to-maneder",
            Terminordning::Arlig => "arlig",
            Terminordning::Primaernaering => "primaernaering",
        }
    }

    /// Periods per year: 6 two-month terminer, otherwise a single årstermin.
    pub fn antall_perioder(self) -> u8 {
        match self {
            Terminordning::ToManeder => 6,
            _ => 1,
        }
    }

    pub fn periode_of(self, date: NaiveDate) -> Termin {
        match self {
            Terminordning::ToManeder => Termin::of(date),
            _ => Termin {
                year: date.year(),
                number: 1,
            },
        }
    }

    pub fn ny_periode(self, year: i32, number: u8) -> Option<Termin> {
        (1..=self.antall_perioder())
            .contains(&number)
            .then_some(Termin { year, number })
    }

    pub fn start(self, t: Termin) -> NaiveDate {
        match self {
            Terminordning::ToManeder => t.start(),
            _ => NaiveDate::from_ymd_opt(t.year, 1, 1).expect("valid date"),
        }
    }

    pub fn end(self, t: Termin) -> NaiveDate {
        match self {
            Terminordning::ToManeder => t.end(),
            _ => NaiveDate::from_ymd_opt(t.year, 12, 31).expect("valid date"),
        }
    }

    /// The filing deadline for the period (skatteforvaltningsforskriften
    /// §8-3): two-month = 1 month and 10 days after the end of the
    /// termin, with the special rule of 31 August for the 3rd termin;
    /// årstermin = 10 March the following year; primærnæring = 10 April
    /// the following year.
    pub fn frist(self, t: Termin) -> NaiveDate {
        let date = |y, m, d| NaiveDate::from_ymd_opt(y, m, d).expect("valid frist");
        match self {
            Terminordning::ToManeder => match t.number {
                1 => date(t.year, 4, 10),
                2 => date(t.year, 6, 10),
                3 => date(t.year, 8, 31),
                4 => date(t.year, 10, 10),
                5 => date(t.year, 12, 10),
                _ => date(t.year + 1, 2, 10),
            },
            Terminordning::Arlig => date(t.year + 1, 3, 10),
            Terminordning::Primaernaering => date(t.year + 1, 4, 10),
        }
    }

    /// Human label for the periode under this ordning.
    pub fn label(self, t: Termin) -> String {
        match self {
            Terminordning::ToManeder => t.to_string(),
            Terminordning::Arlig => format!("Årstermin {}", t.year),
            Terminordning::Primaernaering => format!("Årstermin {} (primærnæring)", t.year),
        }
    }
}

/// One row of the dated rate table: `rate_class` charges `rate_bp`
/// (basis points, 25 % = 2500) from `valid_from` until superseded.
#[derive(Debug, Clone)]
pub struct RatePeriod {
    pub rate_class: String,
    pub valid_from: NaiveDate,
    pub rate_bp: i64,
}

/// The rate in force for a class on a date: the latest `valid_from` that
/// is not after the date. `None` before the table's history starts.
pub fn rate_on(rates: &[RatePeriod], rate_class: &str, date: NaiveDate) -> Option<i64> {
    rates
        .iter()
        .filter(|r| r.rate_class == rate_class && r.valid_from <= date)
        .max_by_key(|r| r.valid_from)
        .map(|r| r.rate_bp)
}

/// VAT in øre from a base (grunnlag) in øre, rounded half away from zero.
/// The result carries the base's sign, so ledger conventions (positive =
/// debit) survive the beregning.
pub fn vat_of_base(base_ore: i64, rate_bp: i64) -> i64 {
    let vat = (i128::from(base_ore.unsigned_abs()) * i128::from(rate_bp) + 5_000) / 10_000;
    i64::try_from(vat).expect("vat amount fits in i64") * base_ore.signum()
}

/// Splits a VAT-inclusive amount into (base, vat): base rounds half away
/// from zero, vat is the exact remainder so base + vat == gross always.
pub fn split_gross(gross_ore: i64, rate_bp: i64) -> (i64, i64) {
    let denominator = 10_000 + i128::from(rate_bp);
    let base = (i128::from(gross_ore.unsigned_abs()) * 10_000 + denominator / 2) / denominator;
    let base = i64::try_from(base).expect("base fits in i64") * gross_ore.signum();
    (base, gross_ore - base)
}

/// One line of the mva-spesifikasjon: grunnlag and beregnet avgift for a
/// standard code at one rate, in ledger signs (positive = debit). Built
/// by regnmed-db from the ledger; consumed by reports and the
/// mva-melding builder.
#[derive(Debug, Clone)]
pub struct SpesLine {
    pub code: String,
    pub description: String,
    pub rate_bp: i64,
    pub grunnlag_ore: i64,
    /// `vat_of_base(grunnlag, rate)` — beregnet, not posted; comparing it
    /// against the posted VAT accounts is the accountant's control.
    pub avgift_ore: i64,
}

/// How a standard code participates in the mva-oppgjør.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Output VAT, payable (codes 3, 31, 32, 33).
    Utgaende,
    /// Input VAT, deductible (codes 1, 11, 12, 13, 14, 15).
    Inngaende,
    /// Reverse charge / import basis, WITH deduction right (81, 83, 86,
    /// 88, 91). The buyer both computes the output tax and deducts it —
    /// mval. §11-1 (2) and (3), jf. §3-30 — so the net effect on the
    /// amount payable is ZERO.
    OmvendtMedFradrag,
    /// The same, WITHOUT deduction right (82, 84, 87, 89, 92): the
    /// computed output tax stands alone and is payable in full.
    OmvendtUtenFradrag,
    /// «Kostnad ved innførsel av varer» (20, 21, 22): a marker on the
    /// supplier invoice, NOT a tax calculation. Reported nowhere — the
    /// tax on that same import is calculated under 81/83 or 14/15, and a
    /// melding line here would charge it a second time.
    Kostnadsmarkor,
    /// No VAT effect (codes 0, 5, 51, 52, 6, 7, 85).
    Ingen,
}

/// How a standard code participates in the oppgjør.
///
/// The split between med/uten fradragsrett is not a modelling choice; it
/// is what Skatteetatens own code list says, per code. Verbatim for 81:
/// «Grunnlaget og beregnet utgående innførselsmerverdiavgift føres i
/// post 9, mens fradragsberettiget inngående innførselsmerverdiavgift
/// føres i post 17» — two posts, one code. For 82 the same sentence
/// stops after post 9. Codes 86/88 say the same about post 12 (7) and
/// post 17 (8), and 91 about post 13 and post 14.
///
/// 20/21/22 are «Kostnad ved innførsel av varer» — markers on the
/// SUPPLIER INVOICE, not tax calculations: «Ved selve avgiftsberegningen
/// benyttes kode 81 eller kode 14», and using them is not even
/// mandatory. They used to be routed with the 8x/9x codes, which made
/// every import generate tax twice: once under 21 and once under 81.
///
/// Source: Norwegian SAF-T Standard VAT/Tax codes v1.13, the document
/// the mva-melding XSD itself points to for `mvaKode`. The post numbers
/// belong to the old RF-0002; the two-sidedness they describe is a
/// property of the CODE and carries into the code-based melding.
pub fn direction(code: &str) -> Direction {
    match code {
        "3" | "31" | "32" | "33" => Direction::Utgaende,
        "1" | "11" | "12" | "13" | "14" | "15" => Direction::Inngaende,
        "81" | "83" | "86" | "88" | "91" => Direction::OmvendtMedFradrag,
        "82" | "84" | "87" | "89" | "92" => Direction::OmvendtUtenFradrag,
        "20" | "21" | "22" => Direction::Kostnadsmarkor,
        _ => Direction::Ingen,
    }
}

/// The avgiftsposteringer a reverse-charge / import basis line must
/// carry, so the hovedbok holds the tax the mva-melding reports
/// (bokføringsforskriften §3-1 nr. 8; the melding XSD calls
/// `merverdiavgift` «Bokført beløp for merverdiavgift», which it was
/// not — the amount existed only as a computation in the report).
///
/// Both shapes net to zero, so a voucher that balanced before still
/// balances after:
/// - **Med fradragsrett**: the computed tax is a liability (2701) and
///   an equal deduction (2711). Nothing is owed, and both sides are
///   visible instead of silently cancelling.
/// - **Uten fradragsrett**: the liability stands, and the offsetting
///   debit goes to the BASIS ACCOUNT — non-deductible import tax is
///   part of what the thing cost, not a separate expense.
///
/// Returns empty for every other code, so callers can apply it blindly.
pub fn omvendt_entries(
    basis: &crate::voucher::EntryDraft,
    rate_bp: i64,
    utgaende_konto: &str,
    fradrag_konto: &str,
) -> Vec<crate::voucher::EntryDraft> {
    let retning = basis
        .vat_code
        .as_deref()
        .map_or(Direction::Ingen, direction);
    if !matches!(
        retning,
        Direction::OmvendtMedFradrag | Direction::OmvendtUtenFradrag
    ) {
        return Vec::new();
    }
    // Sign-preserving: a reversing voucher reverses the tax with it.
    let avgift = vat_of_base(basis.amount.0, rate_bp);
    if avgift == 0 {
        return Vec::new();
    }
    let entry = |konto: &str, belop: i64, tekst: &str, dims: bool| crate::voucher::EntryDraft {
        account_number: konto.to_string(),
        amount: crate::Ore(belop),
        // Uncoded, exactly like the invoice engine's mva entry: the
        // spesifikasjon derives grunnlag from the CODED lines, and a code
        // here would count the same basis twice.
        vat_code: None,
        description: Some(tekst.to_string()),
        party_no: None,
        avdeling: if dims { basis.avdeling.clone() } else { None },
        prosjekt: if dims { basis.prosjekt.clone() } else { None },
        valuta: None,
    };
    let mut ut = vec![entry(
        utgaende_konto,
        -avgift,
        "Beregnet mva, omvendt avgiftsplikt/innførsel",
        false,
    )];
    match retning {
        Direction::OmvendtMedFradrag => ut.push(entry(
            fradrag_konto,
            avgift,
            "Fradrag for beregnet mva",
            false,
        )),
        // No deduction right: the tax becomes cost, on the same account
        // and therefore under the same avdeling/prosjekt as the basis.
        _ => ut.push(entry(
            &basis.account_number,
            avgift,
            "Ikke-fradragsberettiget beregnet mva",
            true,
        )),
    }
    ut
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn termin_boundaries() {
        assert_eq!(Termin::of(date(2026, 1, 1)).number, 1);
        assert_eq!(Termin::of(date(2026, 2, 28)).number, 1);
        assert_eq!(Termin::of(date(2026, 3, 1)).number, 2);
        assert_eq!(Termin::of(date(2026, 12, 31)).number, 6);

        let t1 = Termin::new(2024, 1).unwrap();
        assert_eq!(t1.start(), date(2024, 1, 1));
        assert_eq!(t1.end(), date(2024, 2, 29), "leap year");
        let t6 = Termin::new(2026, 6).unwrap();
        assert_eq!(t6.start(), date(2026, 11, 1));
        assert_eq!(t6.end(), date(2026, 12, 31));

        assert!(Termin::new(2026, 0).is_none());
        assert!(Termin::new(2026, 7).is_none());
    }

    #[test]
    fn rate_lookup_respects_history() {
        let rates = vec![
            RatePeriod {
                rate_class: "low".into(),
                valid_from: date(2016, 1, 1),
                rate_bp: 1000,
            },
            RatePeriod {
                rate_class: "low".into(),
                valid_from: date(2018, 1, 1),
                rate_bp: 1200,
            },
            RatePeriod {
                rate_class: "low".into(),
                valid_from: date(2020, 4, 1),
                rate_bp: 600,
            },
            RatePeriod {
                rate_class: "low".into(),
                valid_from: date(2021, 10, 1),
                rate_bp: 1200,
            },
        ];
        assert_eq!(rate_on(&rates, "low", date(2017, 6, 1)), Some(1000));
        assert_eq!(rate_on(&rates, "low", date(2019, 6, 1)), Some(1200));
        assert_eq!(rate_on(&rates, "low", date(2020, 6, 1)), Some(600));
        assert_eq!(rate_on(&rates, "low", date(2026, 1, 1)), Some(1200));
        assert_eq!(rate_on(&rates, "low", date(2015, 1, 1)), None);
        assert_eq!(rate_on(&rates, "regular", date(2026, 1, 1)), None);
    }

    #[test]
    fn beregning_is_integer_and_sign_preserving() {
        assert_eq!(vat_of_base(1_000_000, 2500), 250_000);
        assert_eq!(vat_of_base(-1_000_000, 2500), -250_000);
        assert_eq!(vat_of_base(2, 2500), 1, "0,5 øre rounds away from zero");
        assert_eq!(vat_of_base(1_000_000, 1111), 111_100, "råfisk 11,11 %");
    }

    #[test]
    fn split_gross_reconstructs_exactly() {
        assert_eq!(split_gross(1_250_000, 2500), (1_000_000, 250_000));
        assert_eq!(split_gross(-1_250_000, 2500), (-1_000_000, -250_000));
        for gross in [1, 99, 100, 12_345, 999_999_999] {
            let (base, vat) = split_gross(gross, 2500);
            assert_eq!(base + vat, gross, "base + vat must equal gross");
        }
    }

    #[test]
    fn directions_classify_the_standard_codes() {
        assert_eq!(direction("3"), Direction::Utgaende);
        assert_eq!(direction("1"), Direction::Inngaende);
        // The deduction right is in the code, and it decides whether the
        // computed tax is payable or a wash.
        assert_eq!(direction("86"), Direction::OmvendtMedFradrag);
        assert_eq!(direction("87"), Direction::OmvendtUtenFradrag);
        assert_eq!(direction("81"), Direction::OmvendtMedFradrag);
        assert_eq!(direction("82"), Direction::OmvendtUtenFradrag);
        assert_eq!(direction("91"), Direction::OmvendtMedFradrag);
        // Import COST markers compute nothing — 81/14 do.
        assert_eq!(direction("21"), Direction::Kostnadsmarkor);
        // A zero-rated import basis is still a basis, not a marker.
        assert_eq!(direction("85"), Direction::Ingen);
        assert_eq!(direction("14"), Direction::Inngaende, "fradragssiden alene");
        assert_eq!(direction("5"), Direction::Ingen);
        assert_eq!(direction("0"), Direction::Ingen);
    }

    /// The computed tax must reach the hovedbok, and the voucher must
    /// still balance — otherwise the posting transaction would reject it.
    #[test]
    fn omvendt_avgift_is_posted_and_nets_to_zero() {
        let basis = |code: &str| crate::voucher::EntryDraft {
            account_number: "6800".into(),
            amount: crate::Ore(100_000_00),
            vat_code: Some(code.into()),
            description: None,
            party_no: None,
            avdeling: None,
            prosjekt: Some("P1".into()),
            valuta: None,
        };

        // Full deduction right: liability and deduction, both visible.
        let med = omvendt_entries(&basis("86"), 2500, "2701", "2711");
        assert_eq!(med.len(), 2);
        assert_eq!(med[0].account_number, "2701");
        assert_eq!(med[0].amount.0, -25_000_00, "beregnet utgående avgift");
        assert_eq!(med[1].account_number, "2711");
        assert_eq!(med[1].amount.0, 25_000_00, "og fradraget for den");
        assert_eq!(med.iter().map(|e| e.amount.0).sum::<i64>(), 0, "balanserer");
        assert!(
            med.iter().all(|e| e.vat_code.is_none()),
            "kodede linjer ville telt grunnlaget to ganger"
        );

        // No deduction right: the tax is part of what it cost, so it
        // lands on the basis account — and carries its prosjekt.
        let uten = omvendt_entries(&basis("87"), 2500, "2701", "2711");
        assert_eq!(uten[1].account_number, "6800");
        assert_eq!(uten[1].prosjekt.as_deref(), Some("P1"));
        assert_eq!(uten.iter().map(|e| e.amount.0).sum::<i64>(), 0);

        // A reversing voucher reverses the tax with it.
        let mut kreditert = basis("86");
        kreditert.amount = crate::Ore(-100_000_00);
        let rev = omvendt_entries(&kreditert, 2500, "2701", "2711");
        assert_eq!(rev[0].amount.0, 25_000_00);

        // Everything else is untouched — callers apply this blindly.
        assert!(omvendt_entries(&basis("1"), 2500, "2701", "2711").is_empty());
        assert!(omvendt_entries(&basis("21"), 2500, "2701", "2711").is_empty());
    }

    #[test]
    fn terminordning_periods_and_deadlines() {
        let date = |y, m, d| NaiveDate::from_ymd_opt(y, m, d).unwrap();
        let to = Terminordning::ToManeder;
        assert_eq!(to.antall_perioder(), 6);
        assert_eq!(to.periode_of(date(2026, 7, 25)).number, 4);
        assert_eq!(
            to.frist(Termin {
                year: 2026,
                number: 1
            }),
            date(2026, 4, 10)
        );
        assert_eq!(
            to.frist(Termin {
                year: 2026,
                number: 3
            }),
            date(2026, 8, 31),
            "saerregelen for 3. termin"
        );
        assert_eq!(
            to.frist(Termin {
                year: 2026,
                number: 6
            }),
            date(2027, 2, 10)
        );

        let ar = Terminordning::Arlig;
        assert_eq!(ar.antall_perioder(), 1);
        let periode = ar.periode_of(date(2026, 11, 3));
        assert_eq!(periode.number, 1);
        assert_eq!(ar.start(periode), date(2026, 1, 1));
        assert_eq!(ar.end(periode), date(2026, 12, 31));
        assert_eq!(ar.frist(periode), date(2027, 3, 10));
        assert_eq!(ar.label(periode), "\u{c5}rstermin 2026");
        assert!(
            ar.ny_periode(2026, 2).is_none(),
            "aarstermin har ingen 2. periode"
        );

        let pn = Terminordning::Primaernaering;
        assert_eq!(pn.frist(pn.periode_of(date(2026, 5, 1))), date(2027, 4, 10));

        assert_eq!(Terminordning::parse("arlig"), Some(Terminordning::Arlig));
        assert_eq!(Terminordning::parse("kvartal"), None);
        assert_eq!(Terminordning::Primaernaering.as_str(), "primaernaering");
    }
}
