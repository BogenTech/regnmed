//! Utlegg og kjøregodtgjørelse, pure side (docs/utlegg.md, #42).
//!
//! Kjøregodtgjørelse: km × statens sats, med den trekkfrie delen
//! (Skattedirektoratets forskuddssats) skilt ut fra den trekkpliktige.
//! Satsene er regelverksdata i satsregisteret (`km_godtgjorelse`,
//! `km_godtgjorelse_trekkfri`, øre per km) — aldri hardkodet her.
//! Den trekkpliktige delen blir lønnsinnberetning den dagen lønn/
//! a-melding finnes (#46); til da rapporteres den som varsel, aldri
//! skjult (issuen).
//!
//! Mva-splitten for utlegg (brutto kvittering → netto + mva) er
//! [`crate::mva::split_gross`] — samme avrunding som all annen mva.

/// En beregnet kjøregodtgjørelse: alt i heltall øre, lagret på kravet
/// ved registrering så raden er selvstendig bevis (satsendringer rører
/// aldri innsendte krav).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Kjoregodtgjorelse {
    pub belop_ore: i64,
    pub trekkfri_ore: i64,
    pub trekkpliktig_ore: i64,
}

/// km × sats, split i trekkfri og trekkpliktig del. Er den trekkfrie
/// satsen høyere enn statens sats (har skjedd historisk), er hele
/// beløpet trekkfritt — aldri negativ trekkplikt.
pub fn kjoregodtgjorelse(
    km: i64,
    sats_ore_per_km: i64,
    trekkfri_ore_per_km: i64,
) -> Kjoregodtgjorelse {
    let belop = km * sats_ore_per_km;
    let trekkfri = (km * trekkfri_ore_per_km).min(belop);
    Kjoregodtgjorelse {
        belop_ore: belop,
        trekkfri_ore: trekkfri,
        trekkpliktig_ore: belop - trekkfri,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn satser_2026_gir_trekkpliktig_del() {
        // 120 km à 5,30 kr, trekkfritt 3,50 kr.
        let k = kjoregodtgjorelse(120, 530, 350);
        assert_eq!(k.belop_ore, 63_600);
        assert_eq!(k.trekkfri_ore, 42_000);
        assert_eq!(k.trekkpliktig_ore, 21_600);
    }

    #[test]
    fn trekkfri_sats_over_statens_gir_null_trekkplikt() {
        let k = kjoregodtgjorelse(100, 350, 500);
        assert_eq!(k.belop_ore, 35_000);
        assert_eq!(k.trekkfri_ore, 35_000, "aldri mer enn beløpet");
        assert_eq!(k.trekkpliktig_ore, 0);
    }

    #[test]
    fn null_km_er_null() {
        assert_eq!(
            kjoregodtgjorelse(0, 530, 350),
            Kjoregodtgjorelse {
                belop_ore: 0,
                trekkfri_ore: 0,
                trekkpliktig_ore: 0
            }
        );
    }
}
