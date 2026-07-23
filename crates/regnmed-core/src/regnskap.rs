//! Resultatregnskap and balanse grouped per NS 4102 account classes —
//! the presentation layer over saldobalanse lines (bokføringsforskriften
//! §3-1; the formal årsregnskap oppstillingsplan is a later, separate
//! concern).
//!
//! Input lines carry **ledger signs** (debit positive, credit negative,
//! integer øre). Presentation flips the sign where the reader expects
//! positive numbers: inntekter (class 3, credit balances) and
//! egenkapital/gjeld (class 2) are shown negated; eiendeler (class 1)
//! and kostnader (classes 4–8 debit) are shown as-is. The underlying
//! identity never changes: everything sums from `SUM(amount_ore)`.
//!
//! Class map (NS 4102 first digit):
//! 1 eiendeler · 2 egenkapital og gjeld · 3 driftsinntekter ·
//! 4 varekostnad · 5 lønnskostnad · 6–7 annen driftskostnad ·
//! 8 finansposter, skatt m.m.

/// One account's period balance, ledger sign.
#[derive(Debug, Clone)]
pub struct SaldoLine {
    pub number: String,
    pub name: String,
    pub saldo_ore: i64,
}

/// A presentation section: heading, its account lines (display sign),
/// and their sum (display sign).
#[derive(Debug)]
pub struct Seksjon {
    pub heading: &'static str,
    pub lines: Vec<SaldoLine>,
    pub sum_ore: i64,
}

#[derive(Debug)]
pub struct Resultat {
    pub seksjoner: Vec<Seksjon>,
    pub driftsresultat_ore: i64,
    /// Positive = overskudd. `-(sum of classes 3–8, ledger sign)`.
    pub arsresultat_ore: i64,
}

#[derive(Debug)]
pub struct Balanse {
    pub eiendeler: Seksjon,
    pub egenkapital_gjeld: Seksjon,
    /// Result accumulated in classes 3–8 up to the balance date, shown
    /// on the egenkapital side ("udisponert resultat") so the balance
    /// balances mid-year without a closing entry.
    pub udisponert_resultat_ore: i64,
}

fn class_of(number: &str) -> Option<u32> {
    number.chars().next()?.to_digit(10)
}

fn section(lines: &[SaldoLine], classes: &[u32], heading: &'static str, negate: bool) -> Seksjon {
    let mut selected: Vec<SaldoLine> = lines
        .iter()
        .filter(|l| class_of(&l.number).is_some_and(|c| classes.contains(&c)))
        .filter(|l| l.saldo_ore != 0)
        .cloned()
        .collect();
    if negate {
        for line in &mut selected {
            line.saldo_ore = -line.saldo_ore;
        }
    }
    let sum_ore = selected.iter().map(|l| l.saldo_ore).sum();
    Seksjon {
        heading,
        lines: selected,
        sum_ore,
    }
}

fn ledger_sum(lines: &[SaldoLine], classes: &[u32]) -> i64 {
    lines
        .iter()
        .filter(|l| class_of(&l.number).is_some_and(|c| classes.contains(&c)))
        .map(|l| l.saldo_ore)
        .sum()
}

/// Resultatregnskap over the period's saldo lines (classes 3–8).
pub fn resultat(lines: &[SaldoLine]) -> Resultat {
    let seksjoner = vec![
        section(lines, &[3], "Driftsinntekter", true),
        section(lines, &[4], "Varekostnad", false),
        section(lines, &[5], "Lønnskostnad", false),
        section(lines, &[6, 7], "Annen driftskostnad", false),
        section(lines, &[8], "Finansposter, skatt m.m.", false),
    ];
    Resultat {
        driftsresultat_ore: -ledger_sum(lines, &[3, 4, 5, 6, 7]),
        arsresultat_ore: -ledger_sum(lines, &[3, 4, 5, 6, 7, 8]),
        seksjoner,
    }
}

/// Balanse over saldo lines accumulated from day one through the
/// balance date (classes 1–2, plus the running result on the EK side).
pub fn balanse(lines: &[SaldoLine]) -> Balanse {
    Balanse {
        eiendeler: section(lines, &[1], "Eiendeler", false),
        egenkapital_gjeld: section(lines, &[2], "Egenkapital og gjeld", true),
        udisponert_resultat_ore: -ledger_sum(lines, &[3, 4, 5, 6, 7, 8]),
    }
}

impl Balanse {
    /// Double-entry guarantees this is zero over a complete ledger:
    /// eiendeler − (egenkapital + gjeld + udisponert resultat).
    pub fn differanse_ore(&self) -> i64 {
        self.eiendeler.sum_ore - self.egenkapital_gjeld.sum_ore - self.udisponert_resultat_ore
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(number: &str, name: &str, saldo_ore: i64) -> SaldoLine {
        SaldoLine {
            number: number.into(),
            name: name.into(),
            saldo_ore,
        }
    }

    /// A small complete ledger: aksjekapital innskutt, ett salg m/mva,
    /// ett varekjøp, ett bankgebyr. Ledger signs throughout.
    fn saldo() -> Vec<SaldoLine> {
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
        let r = resultat(&saldo());
        assert_eq!(r.seksjoner[0].heading, "Driftsinntekter");
        assert_eq!(r.seksjoner[0].sum_ore, 10_000_00);
        assert_eq!(r.seksjoner[1].sum_ore, 8_000_00, "varekostnad");
        assert_eq!(r.seksjoner[3].sum_ore, 150_00, "annen driftskostnad");
        assert_eq!(r.driftsresultat_ore, 10_000_00 - 8_000_00 - 150_00);
        assert_eq!(r.arsresultat_ore, 1_850_00);
    }

    #[test]
    fn balanse_balances_via_udisponert_resultat() {
        let b = balanse(&saldo());
        assert_eq!(b.eiendeler.sum_ore, 104_350_00);
        assert_eq!(b.egenkapital_gjeld.sum_ore, 102_500_00);
        assert_eq!(b.udisponert_resultat_ore, 1_850_00);
        assert_eq!(b.differanse_ore(), 0);
    }

    #[test]
    fn zero_balance_accounts_are_omitted_but_sections_survive_empty() {
        let mut lines = saldo();
        lines.push(line("1500", "Kundefordringer", 0));
        let b = balanse(&lines);
        assert!(b.eiendeler.lines.iter().all(|l| l.number != "1500"));
        let r = resultat(&[]);
        assert_eq!(r.arsresultat_ore, 0);
        assert!(r.seksjoner.iter().all(|s| s.lines.is_empty()));
    }

    #[test]
    fn class_6_and_7_group_together() {
        let lines = vec![
            line("6300", "Leie", 5_000_00),
            line("7770", "Gebyr", 100_00),
        ];
        let r = resultat(&lines);
        let annen = &r.seksjoner[3];
        assert_eq!(annen.lines.len(), 2);
        assert_eq!(annen.sum_ore, 5_100_00);
    }
}
