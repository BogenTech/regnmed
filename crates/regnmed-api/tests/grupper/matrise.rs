//! Tilgangsmatrisen i docs/auth.md er MASKINSJEKKET (#58).
//!
//! docs/ er revisorvendt dokumentasjon. En tilgangstabell som er blitt
//! feil er verre enn ingen tabell: den blir sitert i en revisjon, og
//! ingen leser koden for å kontrollere den. Derfor genereres tabellen
//! her fra `Rolle` og `Rett`, og testen krever at dokumentet inneholder
//! nøyaktig den.
//!
//! Feiler testen, skriver den ut blokken som skal limes inn. Den er
//! altså ikke bare en sperre, men også verktøyet som holder dokumentet
//! oppdatert.
//!
//! Krever ingen database.

use regnmed_api::tilgang::{Rett, Rolle};

const START: &str = "<!-- MATRISE: generert av crates/regnmed-api/tests/grupper/matrise.rs -->";
const SLUTT: &str = "<!-- /MATRISE -->";

/// Rollene i tabellen, i den rekkefølgen de gir mening å lese: fra minst
/// til mest. `revisor` står mellom `les` og `bokforing` fordi den er
/// lesing pluss lønn.
const ROLLER: [Rolle; 5] = [
    Rolle::Ansatt,
    Rolle::Les,
    Rolle::Revisor,
    Rolle::Bokforing,
    Rolle::Admin,
];

fn matrise() -> String {
    let mut ut = String::new();
    ut.push_str(START);
    ut.push_str("\n\n| Rettighet | Hva den gir |");
    for r in ROLLER {
        ut.push_str(&format!(" {} |", r.slug()));
    }
    ut.push_str("\n| --- | --- |");
    for _ in ROLLER {
        ut.push_str(" --- |");
    }
    ut.push('\n');

    // Gruppert som portalen, og innenfor gruppen i vokabularets egen
    // rekkefølge — den er skrevet for å leses.
    let mut grupper: Vec<&'static str> = Rett::ALLE.iter().map(|r| r.gruppe()).collect();
    grupper.dedup();
    let mut sett = Vec::new();
    for g in grupper {
        if sett.contains(&g) {
            continue;
        }
        sett.push(g);
        ut.push_str(&format!("| **{g}** | | | | | | |\n"));
        for rett in Rett::ALLE.iter().filter(|r| r.gruppe() == g) {
            ut.push_str(&format!("| `{}` | {} |", rett.slug(), rett.beskrivelse()));
            for rolle in ROLLER {
                ut.push_str(if rolle.har(*rett) { " ✅ |" } else { " — |" });
            }
            ut.push('\n');
        }
    }
    ut.push('\n');
    ut.push_str(SLUTT);
    ut
}

#[test]
fn matrisen_i_docs_stemmer_med_koden() {
    let sti = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/auth.md");
    let doc = std::fs::read_to_string(sti).expect("docs/auth.md");
    let forventet = matrise();

    if !doc.contains(&forventet) {
        // Skriv ut hele blokken, så rettingen er å lime inn.
        eprintln!("\n=== docs/auth.md skal inneholde denne blokken ===\n");
        eprintln!("{forventet}");
        eprintln!("\n=== slutt ===\n");
        panic!(
            "tilgangsmatrisen i docs/auth.md stemmer ikke med koden — \
             lim inn blokken over (den er skrevet ut i sin helhet)"
        );
    }
}

/// Matrisen skal dekke HELE vokabularet. Uten dette kunne en ny
/// rettighet bli borte fra dokumentet uten at noe sa fra — og en
/// tilgangstabell med hull er en tabell man ikke kan stole på.
#[test]
fn matrisen_dekker_hele_vokabularet() {
    let m = matrise();
    for rett in Rett::ALLE {
        assert!(
            m.contains(&format!("| `{}` |", rett.slug())),
            "{} mangler i matrisen",
            rett.slug()
        );
    }
}
