//! Resultatregnskap and balanse grouped per NS 4102 account classes —
//! the presentation layer over trial-balance lines (bokføringsforskriften
//! §3-1; the formal årsregnskap oppstillingsplan is a later, separate
//! concern).
//!
//! Input lines carry **ledger signs** (debit positive, credit negative,
//! integer øre). Presentation flips the sign where the reader expects
//! positive numbers: inntekter (class 3, credit balances) and
//! equity and liabilities (class 2) are shown negated; assets (class 1)
//! and kostnader (classes 4–8 debit) are shown as-is. The underlying
//! identity never changes: everything sums from `SUM(amount_ore)`.
//!
//! Class map (NS 4102 first digit):
//! 1 assets · 2 equity and liabilities · 3 operating revenue ·
//! 4 varekostnad · 5 lønnskostnad · 6–7 annen driftskostnad ·
//! 8 finansposter, skatt m.m.

/// One account's period balance, ledger sign.
#[derive(Debug, Clone)]
pub struct BalanceLine {
    pub number: String,
    pub name: String,
    pub balance_ore: i64,
}

/// A presentation section: heading, its account lines (display sign),
/// and their sum (display sign).
#[derive(Debug)]
pub struct Section {
    pub heading: &'static str,
    pub lines: Vec<BalanceLine>,
    pub sum_ore: i64,
}

#[derive(Debug)]
pub struct IncomeStatement {
    pub sections: Vec<Section>,
    pub operating_result_ore: i64,
    /// Positive = overskudd. `-(sum of classes 3–8, ledger sign)`.
    pub annual_result_ore: i64,
}

#[derive(Debug)]
pub struct BalanceSheet {
    pub assets: Section,
    pub equity_and_liabilities: Section,
    /// Result accumulated in classes 3–8 up to the balance date, shown
    /// on the egenkapital side ("udisponert resultat") so the balance
    /// balances mid-year without a closing entry.
    pub undistributed_result_ore: i64,
}

/// Resultatdisponering (#84): where the year's result is transferred to
/// equity at årsavslutning. NOT our choice of account — Skatteetaten's
/// næringsspesifikasjon code list gives 8800 its OWN grouping category,
/// `resultatDisponeringForSAF-T`, separate from every income-statement
/// line ("Disponering av årets overskudd/dekning av årets underskudd",
/// vendored in `saft/naeringsspesifikasjon_*.csv`).
///
/// It sits in class 8, and that is what makes the whole årsavslutning
/// work without new state: the disposition entry lowers `-sum(3..8)` by
/// exactly the amount it raises equity, so `undistributed_result_ore`
/// falls to zero for the closed year and the balanse still nets out.
/// The ledger carries it, like everything else.
///
/// The resultatregnskap must therefore EXCLUDE it: a disposition is not
/// a result line, and counting it would make last year's P&L read zero
/// the moment the year was closed.
pub const ALLOCATION_ACCOUNT: &str = "8800";

fn er_disponering(number: &str) -> bool {
    number == ALLOCATION_ACCOUNT
}

pub fn class_of(number: &str) -> Option<u32> {
    number.chars().next()?.to_digit(10)
}

/// The presentation sign rule in ONE place: inntekter (class 3) and
/// egenkapital/gjeld (class 2) read positive when negated; everything
/// else is shown as booked. Used by the sections below and by budsjett
/// (docs/budsjett.md), which stores what the reader sees.
pub fn presentation_ore(number: &str, ledger_ore: i64) -> i64 {
    match class_of(number) {
        Some(2) | Some(3) => -ledger_ore,
        _ => ledger_ore,
    }
}

fn section(lines: &[BalanceLine], classes: &[u32], heading: &'static str, negate: bool) -> Section {
    let mut selected: Vec<BalanceLine> = lines
        .iter()
        .filter(|l| !er_disponering(&l.number))
        .filter(|l| class_of(&l.number).is_some_and(|c| classes.contains(&c)))
        .filter(|l| l.balance_ore != 0)
        .cloned()
        .collect();
    if negate {
        for line in &mut selected {
            line.balance_ore = -line.balance_ore;
        }
    }
    let sum_ore = selected.iter().map(|l| l.balance_ore).sum();
    Section {
        heading,
        lines: selected,
        sum_ore,
    }
}

/// Ledger-sign sum over account classes, disposition INCLUDED.
///
/// `balanse` depends on that: the disposition entry is what makes
/// `undistributed_result_ore` falls to zero for a closed year. The
/// income statement uses `result_sum` instead.
fn ledger_sum(lines: &[BalanceLine], classes: &[u32]) -> i64 {
    lines
        .iter()
        .filter(|l| class_of(&l.number).is_some_and(|c| classes.contains(&c)))
        .map(|l| l.balance_ore)
        .sum()
}

/// The same sum with the disposition excluded — what the reader means
/// by "resultat".
fn result_sum(lines: &[BalanceLine], classes: &[u32]) -> i64 {
    lines
        .iter()
        .filter(|l| !er_disponering(&l.number))
        .filter(|l| class_of(&l.number).is_some_and(|c| classes.contains(&c)))
        .map(|l| l.balance_ore)
        .sum()
}

/// Resultatregnskap over the period's saldo lines (classes 3–8).
pub fn result(lines: &[BalanceLine]) -> IncomeStatement {
    let sections = vec![
        section(lines, &[3], "Driftsinntekter", true),
        section(lines, &[4], "Varekostnad", false),
        section(lines, &[5], "Lønnskostnad", false),
        section(lines, &[6, 7], "Annen driftskostnad", false),
        section(lines, &[8], "Finansposter, skatt m.m.", false),
    ];
    IncomeStatement {
        operating_result_ore: -result_sum(lines, &[3, 4, 5, 6, 7]),
        annual_result_ore: -result_sum(lines, &[3, 4, 5, 6, 7, 8]),
        sections,
    }
}

/// Profitability totals in presentation signs (#71): what a project (or
/// any dimension-filtered slice) earned and cost. Both numbers read
/// positive; the difference is the dekningsbidrag.
#[derive(Debug, PartialEq, Eq)]
pub struct Profitability {
    /// Class 3, negated to read positive.
    pub revenue_ore: i64,
    /// Classes 4–8 as booked (costs are debits, already positive).
    pub cost_ore: i64,
}

impl Profitability {
    pub fn result_ore(&self) -> i64 {
        self.revenue_ore - self.cost_ore
    }
}

pub fn profitability(lines: &[BalanceLine]) -> Profitability {
    Profitability {
        revenue_ore: -result_sum(lines, &[3]),
        cost_ore: result_sum(lines, &[4, 5, 6, 7, 8]),
    }
}

/// Balance sheet over balance lines accumulated from day one through the
/// balance date (classes 1–2, plus the running result on the EK side).
pub fn balance_sheet(lines: &[BalanceLine]) -> BalanceSheet {
    BalanceSheet {
        assets: section(lines, &[1], "Eiendeler", false),
        equity_and_liabilities: section(lines, &[2], "Egenkapital og gjeld", true),
        undistributed_result_ore: -ledger_sum(lines, &[3, 4, 5, 6, 7, 8]),
    }
}

impl BalanceSheet {
    /// Double-entry guarantees this is zero over a complete ledger:
    /// assets − (equity + liabilities + undistributed result).
    pub fn difference_ore(&self) -> i64 {
        self.assets.sum_ore - self.equity_and_liabilities.sum_ore - self.undistributed_result_ore
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(number: &str, name: &str, balance_ore: i64) -> BalanceLine {
        BalanceLine {
            number: number.into(),
            name: name.into(),
            balance_ore,
        }
    }

    #[test]
    fn lonnsomhet_reads_positive_and_subtracts() {
        // Revenue booked as credit (negative), costs as debit, and a
        // balance line that must not leak into the result.
        let lines = [
            line("3000", "Salg", -10_000_00),
            line("4300", "Varekjøp", 3_000_00),
            line("7790", "Annet", 1_500_00),
            line("1920", "Bank", 99_00),
        ];
        let l = profitability(&lines);
        assert_eq!(l.revenue_ore, 10_000_00);
        assert_eq!(l.cost_ore, 4_500_00);
        assert_eq!(l.result_ore(), 5_500_00);
    }

    /// A small complete ledger: aksjekapital innskutt, ett salg m/mva,
    /// one purchase, one bank fee. Ledger signs throughout.
    fn balance() -> Vec<BalanceLine> {
        vec![
            line("1920", "Bank", 100_000_00 + 12_500_00 - 8_000_00 - 150_00),
            line("2000", "Aksjekapital", -100_000_00),
            line("2700", "Utgående mva", -2_500_00),
            line("3000", "Salgsinntekt", -10_000_00),
            line("4300", "Varekjøp", 8_000_00),
            line("7770", "Bankgebyr", 150_00),
        ]
    }

    #[test]
    fn resultat_shows_inntekter_positive_and_computes_arsresultat() {
        let r = result(&balance());
        assert_eq!(r.sections[0].heading, "Driftsinntekter");
        assert_eq!(r.sections[0].sum_ore, 10_000_00);
        assert_eq!(r.sections[1].sum_ore, 8_000_00, "varekostnad");
        assert_eq!(r.sections[3].sum_ore, 150_00, "annen driftskostnad");
        assert_eq!(r.operating_result_ore, 10_000_00 - 8_000_00 - 150_00);
        assert_eq!(r.annual_result_ore, 1_850_00);
    }

    /// Årsavslutning (#84): the disposition moves the year's result to
    /// equity, and the two reports must react DIFFERENTLY to it.
    ///
    /// - The balanse must see it, or the profit is counted twice: once
    ///   in equity and once still sitting in classes 3–8.
    /// - The resultatregnskap must NOT, or last year's P&L reads zero
    ///   the moment the year is closed.
    ///
    /// Both fall out of one fact: 8800 is class 8 with its own grouping
    /// category in Skatteetatens code list, so the ledger carries the
    /// disposition without any stored "this year is closed" flag.
    #[test]
    fn a_disposition_leaves_the_resultat_alone_and_empties_udisponert() {
        let mut lines = balance();
        let annual_result = result(&lines).annual_result_ore;
        assert_eq!(annual_result, 1_850_00);
        assert_eq!(
            balance_sheet(&lines).undistributed_result_ore,
            annual_result
        );

        // Close the year: debit 8800, credit 2050 (equity), same amount.
        lines.push(line("8800", "Disponering av årets resultat", annual_result));
        lines.push(line("2050", "Annen egenkapital", -annual_result));

        assert_eq!(
            result(&lines).annual_result_ore,
            annual_result,
            "disponeringen er ikke en resultatlinje — fjorårets resultat står"
        );
        assert!(
            !result(&lines)
                .sections
                .iter()
                .any(|s| s.lines.iter().any(|l| l.number == "8800")),
            "8800 skal ikke vises som en resultatlinje"
        );
        assert_eq!(
            balance_sheet(&lines).undistributed_result_ore,
            0,
            "resultatet er disponert — det kan ikke også ligge udisponert"
        );
        assert_eq!(
            balance_sheet(&lines).difference_ore(),
            0,
            "balansen går fortsatt i null"
        );
    }

    #[test]
    fn balanse_balances_via_udisponert_resultat() {
        let b = balance_sheet(&balance());
        assert_eq!(b.assets.sum_ore, 104_350_00);
        assert_eq!(b.equity_and_liabilities.sum_ore, 102_500_00);
        assert_eq!(b.undistributed_result_ore, 1_850_00);
        assert_eq!(b.difference_ore(), 0);
    }

    #[test]
    fn zero_balance_accounts_are_omitted_but_sections_survive_empty() {
        let mut lines = balance();
        lines.push(line("1500", "Kundefordringer", 0));
        let b = balance_sheet(&lines);
        assert!(b.assets.lines.iter().all(|l| l.number != "1500"));
        let r = result(&[]);
        assert_eq!(r.annual_result_ore, 0);
        assert!(r.sections.iter().all(|s| s.lines.is_empty()));
    }

    #[test]
    fn class_6_and_7_group_together() {
        let lines = vec![
            line("6300", "Leie", 5_000_00),
            line("7770", "Gebyr", 100_00),
        ];
        let r = result(&lines);
        let other = &r.sections[3];
        assert_eq!(other.lines.len(), 2);
        assert_eq!(other.sum_ore, 5_100_00);
    }
}
