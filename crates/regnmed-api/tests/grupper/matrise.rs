//! The access matrix in docs/auth.md is MACHINE-CHECKED (#58).
//!
//! docs/ is audit-facing documentation. An access table that has gone
//! wrong is worse than no table: it gets quoted in an audit, and nobody
//! reads the code to check it. So the table is generated here from `Rolle`
//! and `Rett`, and the test requires the document to contain exactly
//! that.
//!
//! When the test fails it prints the block to paste in. It is therefore
//! not only a guard, but also the tool that keeps the document up to
//! date.
//!
//! Requires no database.

use regnmed_api::tilgang::{Rett, Rolle};

const START: &str = "<!-- MATRISE: generert av crates/regnmed-api/tests/grupper/matrise.rs -->";
const SLUTT: &str = "<!-- /MATRISE -->";

/// The roles in the table, in the order that makes sense to read: from
/// least to most. `revisor` sits between `les` and `bokforing` because it
/// is reading plus lønn.
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

    // Grouped like the portal, and within the group in the vocabulary's
    // own order — it is written to be read.
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
fn the_matrix_in_docs_matches_the_code() {
    let sti = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/auth.md");
    let doc = std::fs::read_to_string(sti).expect("docs/auth.md");
    let forventet = matrise();

    if !doc.contains(&forventet) {
        // Print the whole block, so the fix is a paste.
        eprintln!("\n=== docs/auth.md skal inneholde denne blokken ===\n");
        eprintln!("{forventet}");
        eprintln!("\n=== slutt ===\n");
        panic!(
            "tilgangsmatrisen i docs/auth.md stemmer ikke med koden — \
             lim inn blokken over (den er skrevet ut i sin helhet)"
        );
    }
}

/// The matrix must cover the WHOLE vocabulary. Without this a new
/// rettighet could vanish from the document with nothing saying so — and
/// an access table with holes is a table you cannot trust.
#[test]
fn the_matrix_covers_the_whole_vocabulary() {
    let m = matrise();
    for rett in Rett::ALLE {
        assert!(
            m.contains(&format!("| `{}` |", rett.slug())),
            "{} mangler i matrisen",
            rett.slug()
        );
    }
}
