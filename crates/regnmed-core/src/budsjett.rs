//! Budsjett og avviksrapport (docs/budsjett.md, #41) — ren regning over
//! tall vi allerede stoler på.
//!
//! Beløpene her er i **presentasjonsfortegn** (som resultatrapporten:
//! inntekt positiv, kostnad positiv), fordi et budsjett skrives slik et
//! menneske leser det. Faktiske tall konverteres fra hovedbokens
//! debet/kredit med [`crate::regnskap::presentasjon_ore`] før de møter
//! budsjettet, så sammenligningen skjer i ett rom.
//!
//! `avvik = faktisk − budsjett` per konto. Fortegnet tolkes av leseren:
//! for en inntektskonto er positivt avvik bedre enn planlagt, for en
//! kostnadskonto er det dyrere. Rapporten later ikke som den vet hva
//! som er «bra» — den viser tallene.

use crate::regnskap::class_of;

/// One account's year: twelve months of budget and twelve of actuals,
/// both presentation sign.
#[derive(Debug, Clone)]
pub struct KontoTall {
    pub number: String,
    pub name: String,
    pub budsjett: [i64; 12],
    pub faktisk: [i64; 12],
}

#[derive(Debug, Clone)]
pub struct AvvikLinje {
    pub number: String,
    pub name: String,
    /// Sum of months 1..=t_o_m.
    pub budsjett_hittil_ore: i64,
    pub faktisk_hittil_ore: i64,
    /// `faktisk_hittil − budsjett_hittil`.
    pub avvik_hittil_ore: i64,
    /// The whole budgeted year, however far the ledger has come.
    pub budsjett_ar_ore: i64,
    pub budsjett_maaneder: [i64; 12],
    pub faktisk_maaneder: [i64; 12],
}

#[derive(Debug)]
pub struct AvvikSeksjon {
    pub heading: &'static str,
    pub linjer: Vec<AvvikLinje>,
    pub budsjett_hittil_ore: i64,
    pub faktisk_hittil_ore: i64,
    pub avvik_hittil_ore: i64,
    pub budsjett_ar_ore: i64,
}

#[derive(Debug)]
pub struct Avviksrapport {
    /// Months 1..=t_o_m are counted as "hittil".
    pub t_o_m_maned: u32,
    pub seksjoner: Vec<AvvikSeksjon>,
    /// Inntekter − kostnader, hittil.
    pub resultat_budsjett_hittil_ore: i64,
    pub resultat_faktisk_hittil_ore: i64,
    pub resultat_avvik_hittil_ore: i64,
    pub resultat_budsjett_ar_ore: i64,
    /// Per month: budgeted and actual result (inntekter − kostnader).
    pub resultat_budsjett_maaneder: [i64; 12],
    pub resultat_faktisk_maaneder: [i64; 12],
}

/// NS 4102-seksjonene, samme inndeling som resultatrapporten.
const SEKSJONER: [(&str, &[u32]); 5] = [
    ("Driftsinntekter", &[3]),
    ("Varekostnad", &[4]),
    ("Lønnskostnad", &[5]),
    ("Annen driftskostnad", &[6, 7]),
    ("Finansposter, skatt m.m.", &[8]),
];

/// Class 3 is income; 4–8 are costs. The result adds income and
/// subtracts costs — both are positive in presentation sign.
fn resultatfortegn(number: &str) -> i64 {
    match class_of(number) {
        Some(3) => 1,
        _ => -1,
    }
}

/// Builds the avviksrapport. `t_o_m_maned` (1..=12) decides how far
/// "hittil" reaches — normally the current month for the running year,
/// 12 for a finished one. Accounts with nothing budgeted and nothing
/// booked are left out; an account that appears on only one side is
/// kept, with zeroes on the other (a cost nobody planned is exactly
/// what an avviksrapport is for).
pub fn avvik(tall: &[KontoTall], t_o_m_maned: u32) -> Avviksrapport {
    let t_o_m = t_o_m_maned.clamp(1, 12) as usize;
    let hittil = |v: &[i64; 12]| v[..t_o_m].iter().sum::<i64>();

    let mut seksjoner = Vec::with_capacity(SEKSJONER.len());
    let mut resultat_budsjett_maaneder = [0i64; 12];
    let mut resultat_faktisk_maaneder = [0i64; 12];
    let mut resultat_budsjett_ar = 0i64;

    for (heading, classes) in SEKSJONER {
        let mut linjer: Vec<AvvikLinje> = tall
            .iter()
            .filter(|k| class_of(&k.number).is_some_and(|c| classes.contains(&c)))
            .filter(|k| k.budsjett.iter().any(|v| *v != 0) || k.faktisk.iter().any(|v| *v != 0))
            .map(|k| {
                let budsjett_hittil_ore = hittil(&k.budsjett);
                let faktisk_hittil_ore = hittil(&k.faktisk);
                AvvikLinje {
                    number: k.number.clone(),
                    name: k.name.clone(),
                    budsjett_hittil_ore,
                    faktisk_hittil_ore,
                    avvik_hittil_ore: faktisk_hittil_ore - budsjett_hittil_ore,
                    budsjett_ar_ore: k.budsjett.iter().sum(),
                    budsjett_maaneder: k.budsjett,
                    faktisk_maaneder: k.faktisk,
                }
            })
            .collect();
        linjer.sort_by(|a, b| a.number.cmp(&b.number));

        for linje in &linjer {
            let fortegn = resultatfortegn(&linje.number);
            for m in 0..12 {
                resultat_budsjett_maaneder[m] += fortegn * linje.budsjett_maaneder[m];
                resultat_faktisk_maaneder[m] += fortegn * linje.faktisk_maaneder[m];
            }
            resultat_budsjett_ar += fortegn * linje.budsjett_ar_ore;
        }

        let budsjett_hittil_ore = linjer.iter().map(|l| l.budsjett_hittil_ore).sum();
        let faktisk_hittil_ore = linjer.iter().map(|l| l.faktisk_hittil_ore).sum();
        seksjoner.push(AvvikSeksjon {
            heading,
            budsjett_hittil_ore,
            faktisk_hittil_ore,
            avvik_hittil_ore: faktisk_hittil_ore - budsjett_hittil_ore,
            budsjett_ar_ore: linjer.iter().map(|l| l.budsjett_ar_ore).sum(),
            linjer,
        });
    }

    let resultat_budsjett_hittil_ore = hittil(&resultat_budsjett_maaneder);
    let resultat_faktisk_hittil_ore = hittil(&resultat_faktisk_maaneder);
    Avviksrapport {
        t_o_m_maned: t_o_m as u32,
        seksjoner,
        resultat_budsjett_hittil_ore,
        resultat_faktisk_hittil_ore,
        resultat_avvik_hittil_ore: resultat_faktisk_hittil_ore - resultat_budsjett_hittil_ore,
        resultat_budsjett_ar_ore: resultat_budsjett_ar,
        resultat_budsjett_maaneder,
        resultat_faktisk_maaneder,
    }
}

/// «Lag budsjett fra fjoråret ±X %»: scales an amount by basis points
/// (500 = +5 %, -1000 = −10 %), rounding half away from zero so the
/// suggestion never invents or loses an øre through truncation. Pure
/// integer arithmetic — no floats near money, ever.
pub fn juster_ore(belop_ore: i64, justering_bp: i64) -> i64 {
    let faktor = 10_000 + justering_bp;
    let produkt = (belop_ore as i128) * (faktor as i128);
    let half = 10_000i128 / 2;
    let rounded = if produkt >= 0 {
        (produkt + half) / 10_000
    } else {
        (produkt - half) / 10_000
    };
    rounded as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn konto(number: &str, budsjett: [i64; 12], faktisk: [i64; 12]) -> KontoTall {
        KontoTall {
            number: number.into(),
            name: format!("Konto {number}"),
            budsjett,
            faktisk,
        }
    }

    fn jevnt(per_maned: i64) -> [i64; 12] {
        [per_maned; 12]
    }

    #[test]
    fn hittil_stopper_ved_valgt_maned() {
        let tall = vec![konto("3000", jevnt(100_00), jevnt(90_00))];
        let rapport = avvik(&tall, 3);
        let linje = &rapport.seksjoner[0].linjer[0];
        assert_eq!(linje.budsjett_hittil_ore, 300_00);
        assert_eq!(linje.faktisk_hittil_ore, 270_00);
        assert_eq!(linje.avvik_hittil_ore, -30_00, "svikt i inntekt");
        assert_eq!(
            linje.budsjett_ar_ore, 1_200_00,
            "hele det budsjetterte året"
        );
    }

    #[test]
    fn resultatet_legger_til_inntekt_og_trekker_fra_kostnad() {
        let tall = vec![
            konto("3000", jevnt(100_00), jevnt(120_00)),
            konto("5000", jevnt(40_00), jevnt(45_00)),
            konto("6300", jevnt(10_00), jevnt(10_00)),
        ];
        let rapport = avvik(&tall, 12);
        // Budsjettert resultat: (100 − 40 − 10) × 12.
        assert_eq!(rapport.resultat_budsjett_hittil_ore, 600_00);
        assert_eq!(rapport.resultat_faktisk_hittil_ore, 780_00);
        assert_eq!(rapport.resultat_avvik_hittil_ore, 180_00);
        assert_eq!(rapport.resultat_budsjett_ar_ore, 600_00);
        assert_eq!(rapport.resultat_faktisk_maaneder[0], 65_00);
    }

    #[test]
    fn konto_som_bare_finnes_paa_en_side_blir_med() {
        let tall = vec![
            // Ubudsjettert kostnad — nettopp det rapporten er til for.
            konto("7770", [0; 12], jevnt(5_00)),
            // Budsjettert, men ingenting bokført ennå.
            konto("6300", jevnt(3_00), [0; 12]),
            // Verken plan eller virkelighet: utelates.
            konto("6400", [0; 12], [0; 12]),
        ];
        let rapport = avvik(&tall, 12);
        let annen = rapport
            .seksjoner
            .iter()
            .find(|s| s.heading == "Annen driftskostnad")
            .unwrap();
        assert_eq!(annen.linjer.len(), 2);
        assert_eq!(annen.linjer[0].number, "6300");
        assert_eq!(annen.linjer[0].faktisk_hittil_ore, 0);
        assert_eq!(annen.linjer[1].avvik_hittil_ore, 60_00);
    }

    #[test]
    fn seksjonene_folger_ns_4102() {
        let tall = vec![
            konto("3000", jevnt(1_00), [0; 12]),
            konto("4300", jevnt(1_00), [0; 12]),
            konto("5000", jevnt(1_00), [0; 12]),
            konto("6300", jevnt(1_00), [0; 12]),
            konto("7100", jevnt(1_00), [0; 12]),
            konto("8050", jevnt(1_00), [0; 12]),
        ];
        let rapport = avvik(&tall, 12);
        let headings: Vec<_> = rapport.seksjoner.iter().map(|s| s.heading).collect();
        assert_eq!(
            headings,
            vec![
                "Driftsinntekter",
                "Varekostnad",
                "Lønnskostnad",
                "Annen driftskostnad",
                "Finansposter, skatt m.m.",
            ]
        );
        // 6 og 7 deler seksjon.
        assert_eq!(rapport.seksjoner[3].linjer.len(), 2);
    }

    #[test]
    fn justering_runder_halve_ore_bort_fra_null() {
        assert_eq!(juster_ore(100_00, 0), 100_00);
        assert_eq!(juster_ore(100_00, 500), 105_00);
        assert_eq!(juster_ore(100_00, -1000), 90_00);
        // 1 øre + 5 % = 1,05 øre → 1; 1 øre + 50 % = 1,5 → 2 (bort fra null).
        assert_eq!(juster_ore(1, 500), 1);
        assert_eq!(juster_ore(1, 5000), 2);
        assert_eq!(juster_ore(-1, 5000), -2);
    }
}
