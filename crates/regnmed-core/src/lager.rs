//! Enkelt varelager (docs/produkter.md, #39): beholdning og verdi som
//! ren funksjon over bevegelsene — aldri lagret, samme filosofi som
//! saldoer. Verdsettelse etter gjennomsnittsmetoden: innkjøp til
//! anskaffelseskost, uttak til løpende gjennomsnittskost. Alt i heltall
//! (milli-enheter og øre); avrunding halvt vekk fra null per bevegelse
//! så statusen er deterministisk uansett hvor den beregnes.
//!
//! Regler i randtilfellene (bevisst enkle, dokumentert her og i testene):
//! - Inngang uten kostpris (varetelling opp, kreditnota-retur) tas inn
//!   til løpende gjennomsnittskost; er beholdningen tom, til verdi 0.
//! - Uttak utover beholdningen fjerner hele verdien og lar antallet gå
//!   negativt — negativ beholdning er et telleavvik som skal synes,
//!   ikke skjules.
//! - Tømmes beholdningen eksakt, fjernes verdien eksakt (proporsjonen
//!   er 1), så ingen rest-øre blir igjen.

/// One inventory movement, chronological order is the caller's job.
/// Quantities are milli-units (1000 = one unit) matching invoice line
/// quantities; `kostpris_ore` is the acquisition cost PER UNIT and is
/// only meaningful on inbound movements.
#[derive(Debug, Clone, Copy)]
pub struct Bevegelse {
    pub antall_milli: i64,
    pub kostpris_ore: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LagerStatus {
    pub antall_milli: i64,
    pub verdi_ore: i64,
}

impl LagerStatus {
    /// Løpende gjennomsnittskost per unit, when there is stock to price.
    pub fn gjennomsnitt_ore(&self) -> Option<i64> {
        if self.antall_milli > 0 {
            Some(div_round(self.verdi_ore as i128 * 1000, self.antall_milli as i128))
        } else {
            None
        }
    }
}

/// Integer division rounded half away from zero.
fn div_round(n: i128, d: i128) -> i64 {
    debug_assert!(d != 0);
    let (n, d, sign) = if (n < 0) != (d < 0) {
        (n.abs(), d.abs(), -1)
    } else {
        (n.abs(), d.abs(), 1)
    };
    (sign * ((n + d / 2) / d)) as i64
}

/// Folds movements (in the order given) into beholdning + verdi.
pub fn verdsett<'a>(bevegelser: impl IntoIterator<Item = &'a Bevegelse>) -> LagerStatus {
    let mut status = LagerStatus::default();
    for b in bevegelser {
        if b.antall_milli > 0 {
            status.verdi_ore += match b.kostpris_ore {
                Some(kost) => div_round(b.antall_milli as i128 * kost as i128, 1000),
                None if status.antall_milli > 0 => div_round(
                    status.verdi_ore as i128 * b.antall_milli as i128,
                    status.antall_milli as i128,
                ),
                None => 0,
            };
        } else if b.antall_milli < 0 && status.antall_milli > 0 {
            let ut = (-b.antall_milli).min(status.antall_milli);
            status.verdi_ore -= div_round(
                status.verdi_ore as i128 * ut as i128,
                status.antall_milli as i128,
            );
        }
        status.antall_milli += b.antall_milli;
    }
    status
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inn(antall_milli: i64, kost: i64) -> Bevegelse {
        Bevegelse {
            antall_milli,
            kostpris_ore: Some(kost),
        }
    }

    fn bev(antall_milli: i64) -> Bevegelse {
        Bevegelse {
            antall_milli,
            kostpris_ore: None,
        }
    }

    #[test]
    fn gjennomsnitt_over_to_innkjop() {
        // 10 stk à 100 kr + 10 stk à 200 kr → snitt 150 kr; selg 5.
        let status = verdsett(&[inn(10_000, 100_00), inn(10_000, 200_00), bev(-5_000)]);
        assert_eq!(status.antall_milli, 15_000);
        assert_eq!(status.verdi_ore, 3_000_00 - 750_00);
        assert_eq!(status.gjennomsnitt_ore(), Some(150_00));
    }

    #[test]
    fn eksakt_tomming_etterlater_null_verdi() {
        let status = verdsett(&[inn(3_000, 99_99), bev(-3_000)]);
        assert_eq!(status, LagerStatus::default());
    }

    #[test]
    fn avrunding_halvt_vekk_fra_null() {
        // 3 stk à 100 kr = 300 kr; selg 1 → fjern 100,00 eksakt.
        // 3 stk til samlet 100,00 (33,3333 kr snitt); selg 1 → 33,33.
        let status = verdsett(&[inn(3_000, 33_33), bev(-1_000)]);
        assert_eq!(status.verdi_ore, 99_99 - 33_33);
        // Odd case: verdi 1,01 kr over 2 stk, selg 1 → fjern 0,51 (halvt opp).
        let status = verdsett(&[inn(1_000, 33), inn(1_000, 68), bev(-1_000)]);
        assert_eq!(status.verdi_ore, 101 - 51);
    }

    #[test]
    fn inngang_uten_kost_tas_til_snitt() {
        // Varetelling opp / kreditnota-retur: inn til løpende snitt.
        let status = verdsett(&[inn(10_000, 100_00), bev(-4_000), bev(2_000)]);
        assert_eq!(status.antall_milli, 8_000);
        assert_eq!(status.verdi_ore, 800_00);
        // Inn i tom beholdning uten kost: verdi 0.
        let status = verdsett(&[bev(5_000)]);
        assert_eq!(status.verdi_ore, 0);
        assert_eq!(status.antall_milli, 5_000);
    }

    #[test]
    fn oversalg_gir_negativ_beholdning_og_null_verdi() {
        let status = verdsett(&[inn(2_000, 100_00), bev(-3_000)]);
        assert_eq!(status.antall_milli, -1_000);
        assert_eq!(status.verdi_ore, 0);
        // Salg fra tom beholdning endrer bare antallet.
        let status = verdsett(&[bev(-1_000)]);
        assert_eq!(status.antall_milli, -1_000);
        assert_eq!(status.verdi_ore, 0);
    }

    #[test]
    fn brokdels_antall() {
        // 2,5 kg à 40 kr/kg = 100 kr.
        let status = verdsett(&[inn(2_500, 40_00)]);
        assert_eq!(status.verdi_ore, 100_00);
        assert_eq!(status.gjennomsnitt_ore(), Some(40_00));
    }
}
