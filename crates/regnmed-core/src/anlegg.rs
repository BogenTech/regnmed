//! Anleggsregister, pure side (docs/anlegg.md, #40): lineære
//! avskrivninger for regnskapet og saldoavskrivning per gruppe for
//! skattemeldingen — begge som rene funksjoner over registerdata, alt
//! i heltall øre.
//!
//! Regnskapsmessig: avskrivbart beløp (kostpris − restverdi) fordeles
//! over levetiden i faste månedsbeløp; siste måned tar resten, så
//! planen summerer EKSAKT til det avskrivbare beløpet.
//!
//! Skattemessig: saldometoden (skatteloven §14-40 flg.) — grunnlaget er
//! inngående saldo + tilganger − vederlag; årets avskrivning er
//! grunnlag × gruppens sats når grunnlaget er positivt. Negativ saldo
//! (vederlag over saldoen) avskrives ikke — den rapporteres som
//! inntektsføringskandidat og håndteres av regnskapsfører (bevisst
//! utenfor scope, se docs/anlegg.md).

/// Saldogruppene i skatteloven §14-41 med beskrivelse — satsene selv
/// er regelverksdata i satsregisteret (domene `saldogruppe_<bokstav>`),
/// aldri hardkodet her.
pub const SALDOGRUPPER: &[(&str, &str)] = &[
    ("a", "Kontormaskiner o.l."),
    ("b", "Ervervet forretningsverdi"),
    ("c", "Vogntog, lastebiler, busser, varebiler mv."),
    ("d", "Personbiler, maskiner, inventar mv."),
    ("e", "Skip, fartøyer, rigger mv."),
    ("f", "Fly, helikopter"),
    (
        "g",
        "Anlegg for overføring og distribusjon av elektrisk kraft mv.",
    ),
    ("h", "Bygg og anlegg, hoteller mv."),
    ("i", "Forretningsbygg"),
    ("j", "Fast teknisk installasjon i bygninger"),
];

pub fn gyldig_saldogruppe(gruppe: &str) -> bool {
    SALDOGRUPPER.iter().any(|(g, _)| *g == gruppe)
}

/// Månedsbeløpet for måned `maned_nr` (1-basert) i en lineær plan.
/// Alle måneder unntatt den siste får det avrundede grunnbeløpet; den
/// siste tar resten, så summen over levetiden er eksakt
/// `kostpris - restverdi`.
pub fn manedsbelop(
    kostpris_ore: i64,
    restverdi_ore: i64,
    levetid_maneder: i32,
    maned_nr: i32,
) -> i64 {
    debug_assert!(levetid_maneder > 0 && maned_nr >= 1 && maned_nr <= levetid_maneder);
    let avskrivbart = kostpris_ore - restverdi_ore;
    let basis = avskrivbart / levetid_maneder as i64;
    if maned_nr < levetid_maneder {
        basis
    } else {
        avskrivbart - basis * (levetid_maneder as i64 - 1)
    }
}

/// Ett saldoår: grunnlag, årets avskrivning og utgående saldo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaldoAr {
    pub grunnlag_ore: i64,
    pub avskrivning_ore: i64,
    pub utgaende_ore: i64,
}

/// Saldometoden for ett år. Negativt grunnlag avskrives aldri — det
/// rapporteres videre som utgående (inntektsføringskandidat).
pub fn saldo_ar(inngaende_ore: i64, tilgang_ore: i64, vederlag_ore: i64, sats_bp: i64) -> SaldoAr {
    let grunnlag = inngaende_ore + tilgang_ore - vederlag_ore;
    let avskrivning = if grunnlag > 0 {
        // Halvt vekk fra null; grunnlaget er positivt her.
        (grunnlag as i128 * sats_bp as i128 + 5_000) / 10_000
    } else {
        0
    } as i64;
    SaldoAr {
        grunnlag_ore: grunnlag,
        avskrivning_ore: avskrivning,
        utgaende_ore: grunnlag - avskrivning,
    }
}

/// Gevinst (positiv) eller tap (negativ) ved avhending.
pub fn gevinst_ved_avhending(bokfort_ore: i64, vederlag_ore: i64) -> i64 {
    vederlag_ore - bokfort_ore
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn planen_summerer_eksakt() {
        // 35 000 kr over 36 måneder deler ikke jevnt.
        let (kostpris, restverdi, levetid) = (3_500_000i64, 0i64, 36i32);
        let sum: i64 = (1..=levetid)
            .map(|m| manedsbelop(kostpris, restverdi, levetid, m))
            .sum();
        assert_eq!(sum, kostpris);
        assert_eq!(manedsbelop(kostpris, restverdi, levetid, 1), 97_222);
        assert_eq!(manedsbelop(kostpris, restverdi, levetid, 36), 97_230);
        // Restverdi reduserer det avskrivbare beløpet.
        let sum: i64 = (1..=60)
            .map(|m| manedsbelop(50_000_00, 5_000_00, 60, m))
            .sum();
        assert_eq!(sum, 45_000_00);
        assert_eq!(manedsbelop(36_000_00, 0, 36, 7), 1_000_00);
    }

    #[test]
    fn saldo_ar_regner_grunnlag_og_avskrivning() {
        // Gruppe d (20 %): inngående 0, tilgang 46 000, vederlag 33 000.
        let ar = saldo_ar(0, 46_000_00, 33_000_00, 2000);
        assert_eq!(ar.grunnlag_ore, 13_000_00);
        assert_eq!(ar.avskrivning_ore, 2_600_00);
        assert_eq!(ar.utgaende_ore, 10_400_00);
        // Neste år ruller utgående inn som inngående.
        let neste = saldo_ar(ar.utgaende_ore, 0, 0, 2000);
        assert_eq!(neste.avskrivning_ore, 2_080_00);
        assert_eq!(neste.utgaende_ore, 8_320_00);
    }

    #[test]
    fn negativt_grunnlag_avskrives_ikke() {
        let ar = saldo_ar(10_000_00, 0, 25_000_00, 2000);
        assert_eq!(ar.grunnlag_ore, -15_000_00);
        assert_eq!(ar.avskrivning_ore, 0);
        assert_eq!(ar.utgaende_ore, -15_000_00, "rapporteres, aldri gjettet");
    }

    #[test]
    fn avrunding_halvt_opp() {
        // 1,25 kr × 20 % = 0,25 → 25 øre / grunnlag 125 øre, sats 2000 bp
        assert_eq!(saldo_ar(0, 125, 0, 2000).avskrivning_ore, 25);
        // 33 øre × 30 % = 9,9 → 10 øre.
        assert_eq!(saldo_ar(0, 33, 0, 3000).avskrivning_ore, 10);
    }

    #[test]
    fn gevinst_og_tap() {
        assert_eq!(gevinst_ved_avhending(32_000_00, 33_000_00), 1_000_00);
        assert_eq!(gevinst_ved_avhending(32_000_00, 30_000_00), -2_000_00);
        assert_eq!(gevinst_ved_avhending(0, 0), 0);
    }

    #[test]
    fn saldogruppene_er_a_til_j() {
        assert_eq!(SALDOGRUPPER.len(), 10);
        assert!(gyldig_saldogruppe("a") && gyldig_saldogruppe("j"));
        assert!(!gyldig_saldogruppe("k") && !gyldig_saldogruppe(""));
    }
}
