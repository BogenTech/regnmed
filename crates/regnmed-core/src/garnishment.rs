//! Trekkpålegg (utleggstrekk i lønn): the rules the payroll run applies,
//! with no I/O (docs/trekkpalegg.md).
//!
//! Shared by orders registered by hand (#339) and orders fetched from
//! Skatteetaten (#334): both reduce to a [`Kind`], a [`Size`] and a
//! percentage [`Basis`], and both are withheld by [`withhold`] under the
//! same cap (§3, "trekke så mye som mulig").

use chrono::NaiveDate;

/// Skatteetaten's three types (docs/trekkpalegg.md §6, from the a-melding
/// guidance table). They differ in percentage basis, payment and
/// reporting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    /// The new scheme: one KID per employee, reported as
    /// `utleggstrekkSamordnet`. Every order from the API is one.
    Coordinated,
    /// Old scheme, tax and duties: paid as ONE amount per pay date with
    /// the company's own KID, reported as `utleggstrekkSkatt`.
    Tax,
    /// Old scheme, other claims: KID and account from the letter, never
    /// reported in the a-melding.
    Other,
}

impl Kind {
    /// The stored value (migration 0110).
    pub fn slug(self) -> &'static str {
        match self {
            Kind::Coordinated => "samordnet",
            Kind::Tax => "skatt",
            Kind::Other => "ovrige",
        }
    }

    pub fn from_slug(slug: &str) -> Option<Kind> {
        match slug {
            "samordnet" => Some(Kind::Coordinated),
            "skatt" => Some(Kind::Tax),
            "ovrige" => Some(Kind::Other),
            _ => None,
        }
    }

    /// The a-melding's `utleggstrekk/beskrivelse`; `None` = not reported.
    pub fn amelding_description(self) -> Option<&'static str> {
        match self {
            Kind::Coordinated => Some("utleggstrekkSamordnet"),
            Kind::Tax => Some("utleggstrekkSkatt"),
            Kind::Other => None,
        }
    }

    /// For the payslip and the preview. Norwegian: users read it.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Coordinated => "samordnet trekk",
            Kind::Tax => "utleggstrekk skatt",
            Kind::Other => "utleggstrekk, øvrige krav",
        }
    }
}

/// How much an order takes: a monthly amount or a percentage of the
/// month's basis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Size {
    AmountOre(i64),
    /// Basis points: two decimals of a percent, never more (§2 step 4).
    PercentBp(i64),
}

/// The month's pay, split the way the two percentage bases need it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Basis {
    pub pay_ore: i64,
    pub holiday_pay_ore: i64,
    /// The trekkpliktig part of kjøregodtgjørelse and diett. The trekkfri
    /// part is an expense allowance and never in any basis.
    pub taxable_allowance_ore: i64,
    /// Naturalytelser. Always 0 today: payroll has none (docs/lonn.md).
    /// Kept so the two bases below differ where Skatteetaten says they do
    /// the day benefits in kind arrive.
    pub benefits_in_kind_ore: i64,
}

impl Basis {
    /// Samordnet: gross pay without benefits in kind and expense
    /// allowances; holiday pay is in (§1).
    pub fn coordinated_ore(&self) -> i64 {
        self.pay_ore + self.holiday_pay_ore + self.taxable_allowance_ore
    }

    /// Skatt (old scheme): "bruttolønn, samme grunnlag som
    /// forskuddstrekket" — benefits in kind count, as they do for
    /// forskuddstrekk (§6 table).
    pub fn tax_ore(&self) -> i64 {
        self.coordinated_ore() + self.benefits_in_kind_ore
    }

    /// The basis a percentage of `kind` is taken of. Øvrige take an amount
    /// only ([`validate`]), since their basis is in the letter.
    pub fn for_kind(&self, kind: Kind) -> i64 {
        match kind {
            Kind::Coordinated => self.coordinated_ore(),
            Kind::Tax => self.tax_ore(),
            Kind::Other => 0,
        }
    }
}

fn whole_kroner(ore: i64) -> i64 {
    ore.max(0) / 100 * 100
}

/// What the order asks for this month, rounded DOWN to whole kroner like
/// forskuddstrekk: the a-melding's `beloep` is whole kroner, and what is
/// booked, paid and reported must be one number (§2 step 4).
pub fn wanted_ore(size: Size, basis_ore: i64) -> i64 {
    match size {
        Size::AmountOre(amount) => whole_kroner(amount),
        Size::PercentBp(bp) => {
            whole_kroner((basis_ore.max(0) as i128 * bp as i128 / 10_000) as i64)
        }
    }
}

/// The ceiling for all of an employee's orders together (§3): net pay
/// after forskuddstrekk, without the trekkfri allowances that are paid
/// with it — they are expense refunds, not pay. Never negative.
pub fn cap_ore(net_ore: i64, tax_free_allowance_ore: i64) -> i64 {
    whole_kroner(net_ore - tax_free_allowance_ore)
}

/// Withholds as much as possible, never more than each order asks: the
/// orders are taken in the order given until the cap is spent.
pub fn withhold(cap_ore: i64, wanted: &[i64]) -> Vec<i64> {
    let mut left = whole_kroner(cap_ore);
    wanted
        .iter()
        .map(|&w| {
            let take = w.min(left).max(0);
            left -= take;
            take
        })
        .collect()
}

/// One order as typed from a letter, before it is stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    pub kind: Kind,
    pub size: Size,
    pub valid_from: NaiveDate,
    pub valid_to: Option<NaiveDate>,
    pub kid: Option<String>,
    pub payee_orgnumber: Option<String>,
    pub payee_account: Option<String>,
    pub due_days: Option<i32>,
}

/// Checks a draft against §6's table and normalizes it (account number
/// without dots or spaces, empty strings as none). The messages are
/// Norwegian: the person typing from the letter reads them.
pub fn validate(mut d: Draft) -> Result<Draft, String> {
    let blank = |v: Option<String>| v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    d.kid = blank(d.kid);
    d.payee_orgnumber = blank(d.payee_orgnumber);
    d.payee_account = blank(d.payee_account).map(|a| crate::pain001::normalize_account_number(&a));
    match d.size {
        Size::AmountOre(a) if a <= 0 => return Err("beløpet må være større enn null".into()),
        Size::PercentBp(bp) if !(1..=10_000).contains(&bp) => {
            return Err("prosenten må være mellom 0,01 og 100".into());
        }
        Size::PercentBp(_) if d.kind == Kind::Other => {
            return Err(
                "øvrige krav registreres med beløp: grunnlaget for en prosent står i \
                        brevet, og regnmed gjetter det ikke"
                    .into(),
            );
        }
        _ => {}
    }
    if d.valid_to.is_some_and(|to| to < d.valid_from) {
        return Err("«gjelder til» kan ikke være før «gjelder fra»".into());
    }
    if d.kind == Kind::Tax {
        // Paid with the company's own KID and account (the payroll
        // settings), never the letter's.
        if d.kid.is_some() || d.payee_orgnumber.is_some() || d.payee_account.is_some() {
            return Err(
                "utleggstrekk skatt betales med selskapets egen KID og konto fra \
                        lønnsinnstillingene — oppgi ikke KID, mottaker eller konto på pålegget"
                    .into(),
            );
        }
    } else {
        let kid = d.kid.as_deref().ok_or("oppgi KID fra brevet")?;
        if !crate::kid::is_valid(kid) {
            return Err(format!("KID {kid} har feil kontrollsiffer"));
        }
        let org = d
            .payee_orgnumber
            .as_deref()
            .ok_or("oppgi betalingsmottakerens organisasjonsnummer fra brevet")?;
        if !crate::orgnumber::is_valid(org) {
            return Err(format!("organisasjonsnummer {org} er ugyldig (MOD11)"));
        }
        let account = d
            .payee_account
            .as_deref()
            .ok_or("oppgi kontonummeret fra brevet")?;
        if !crate::pain001::is_valid_account_number(account) {
            return Err(format!(
                "kontonummer {account} er ugyldig (11 siffer, MOD11)"
            ));
        }
    }
    match (d.kind, d.due_days) {
        (Kind::Other, None) => return Err("oppgi betalingsfristen fra brevet".into()),
        (Kind::Other, Some(days)) if !(0..=365).contains(&days) => {
            return Err("betalingsfristen må være 0–365 dager etter utbetaling".into());
        }
        (Kind::Other, Some(_)) => {}
        // Samordnet and skatt fall due on the pay date (§2 step 5).
        (_, Some(_)) => return Err("betalingsfrist oppgis bare for øvrige krav".into()),
        (_, None) => {}
    }
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn coordinated() -> Draft {
        Draft {
            kind: Kind::Coordinated,
            size: Size::AmountOre(400_000),
            valid_from: date(2026, 1, 1),
            valid_to: None,
            // The documentation example's KID and account; Skatteetaten's orgnr
            // (the example's 971648199 fails MOD11).
            kid: Some("6487719756".into()),
            payee_orgnumber: Some("974761076".into()),
            payee_account: Some("7021.39.97155".into()),
            due_days: None,
        }
    }

    #[test]
    fn the_two_bases_differ_only_by_benefits_in_kind() {
        let b = Basis {
            pay_ore: 4_000_000,
            holiday_pay_ore: 500_000,
            taxable_allowance_ore: 10_000,
            benefits_in_kind_ore: 300_000,
        };
        assert_eq!(b.coordinated_ore(), 4_510_000);
        assert_eq!(b.tax_ore(), 4_810_000);
        assert_eq!(b.for_kind(Kind::Coordinated), 4_510_000);
        assert_eq!(b.for_kind(Kind::Tax), 4_810_000);
    }

    #[test]
    fn the_wanted_amount_is_whole_kroner_rounded_down() {
        assert_eq!(wanted_ore(Size::AmountOre(123_456), 0), 123_400);
        // 12,34 % of 45 678,90 kr = 5 636,77 kr -> 5 636 kr.
        assert_eq!(wanted_ore(Size::PercentBp(1234), 4_567_890), 563_600);
        assert_eq!(wanted_ore(Size::PercentBp(1000), -100), 0);
    }

    #[test]
    fn as_much_as_possible_never_more() {
        // Cap: 10 000 kr net less 1 500 kr trekkfri allowance.
        let cap = cap_ore(1_000_050, 150_000);
        assert_eq!(cap, 850_000);
        assert_eq!(withhold(cap, &[600_000, 400_000]), vec![600_000, 250_000]);
        assert_eq!(withhold(cap, &[100_000]), vec![100_000]);
        assert_eq!(withhold(cap_ore(-5, 0), &[100_000]), vec![0]);
    }

    #[test]
    fn a_coordinated_order_needs_valid_kid_payee_and_account() {
        let d = validate(coordinated()).unwrap();
        assert_eq!(d.payee_account.as_deref(), Some("70213997155"));
        for (bad, message) in [
            (
                Draft {
                    kid: Some("6487719757".into()),
                    ..coordinated()
                },
                "kontrollsiffer",
            ),
            (
                Draft {
                    payee_orgnumber: Some("974761077".into()),
                    ..coordinated()
                },
                "MOD11",
            ),
            (
                Draft {
                    payee_account: Some("70213997156".into()),
                    ..coordinated()
                },
                "MOD11",
            ),
            (
                Draft {
                    kid: Some(" ".into()),
                    ..coordinated()
                },
                "oppgi KID",
            ),
        ] {
            let e = validate(bad).unwrap_err();
            assert!(e.contains(message), "{e}");
        }
    }

    #[test]
    fn other_claims_take_an_amount_and_a_due_date() {
        let other = Draft {
            kind: Kind::Other,
            due_days: Some(10),
            ..coordinated()
        };
        assert!(validate(other.clone()).is_ok());
        assert!(
            validate(Draft {
                size: Size::PercentBp(1000),
                ..other.clone()
            })
            .unwrap_err()
            .contains("beløp")
        );
        assert!(
            validate(Draft {
                due_days: None,
                ..other
            })
            .unwrap_err()
            .contains("frist")
        );
        assert!(
            validate(Draft {
                due_days: Some(3),
                ..coordinated()
            })
            .is_err()
        );
    }

    #[test]
    fn a_tax_order_carries_no_payment_details_of_its_own() {
        let tax = Draft {
            kind: Kind::Tax,
            size: Size::PercentBp(1550),
            kid: None,
            payee_orgnumber: None,
            payee_account: None,
            ..coordinated()
        };
        assert!(validate(tax.clone()).is_ok());
        assert!(
            validate(Draft {
                kid: Some("6487719756".into()),
                ..tax
            })
            .is_err()
        );
    }

    #[test]
    fn dates_and_sizes_are_checked() {
        assert!(
            validate(Draft {
                valid_to: Some(date(2025, 12, 31)),
                ..coordinated()
            })
            .is_err()
        );
        assert!(
            validate(Draft {
                size: Size::AmountOre(0),
                ..coordinated()
            })
            .is_err()
        );
        assert!(
            validate(Draft {
                size: Size::PercentBp(10_001),
                ..coordinated()
            })
            .is_err()
        );
    }

    #[test]
    fn only_old_and_new_tax_claims_are_reported() {
        assert_eq!(
            Kind::Coordinated.amelding_description(),
            Some("utleggstrekkSamordnet")
        );
        assert_eq!(Kind::Tax.amelding_description(), Some("utleggstrekkSkatt"));
        assert_eq!(Kind::Other.amelding_description(), None);
        for k in [Kind::Coordinated, Kind::Tax, Kind::Other] {
            assert_eq!(Kind::from_slug(k.slug()), Some(k));
        }
    }
}
