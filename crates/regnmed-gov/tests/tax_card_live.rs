//! Skattekort against Skatteetatens REAL test environment, through
//! `regnmed_gov::tax_card` (docs/lonn.md).
//!
//! Never runs in CI: skips unless SKATTEKORT_LIVE=1, and needs the
//! Maskinporten test client (docs/secrets.md):
//!
//!     set -a; . ~/.config/regnmed/maskinporten-test.env; set +a
//!     SKATTEKORT_LIVE=1 cargo nextest run -p regnmed-gov --test tax_card_live --no-capture
//!
//! Orders cards for Skatteetaten's published test persons as the
//! synthetic Tenor company 311032240 (the systembruker approved
//! 2026-11-11) and checks that each kind of card is read as the module
//! says. Reading cards changes nothing at Skatteetaten.

use regnmed_gov::maskinporten::{MaskinportenConfig, TokenProvider};
use regnmed_gov::tax_card::{Decision, SCOPES, TaxCardClient, decide, parse_answer};

const COMPANY: &str = "311032240";

#[tokio::test]
async fn cards_from_the_test_environment() {
    if std::env::var("SKATTEKORT_LIVE").as_deref() != Ok("1") {
        eprintln!("skipping: SKATTEKORT_LIVE=1 not set");
        return;
    }
    let mut config = MaskinportenConfig::from_env().expect("MASKINPORTEN_* from the test env file");
    config.scopes = SCOPES.into();
    let tokens = TokenProvider::new(config);
    let client = TaxCardClient::from_env();

    // From testpersoner_med_skattekort_EI_2026.xlsx: prosent lønn, tabell
    // lønn, kildeskatt lønn, ikke skattekort.
    let nins: Vec<String> = ["15929998666", "20857597940", "05850298728", "19910298057"]
        .map(String::from)
        .to_vec();
    let order = client
        .order(
            &tokens,
            COMPANY,
            2026,
            &nins,
            "drift@regnmed.no",
            uuid::Uuid::new_v4(),
        )
        .await
        .expect("order");
    assert!(order.reference.starts_with("BR"), "{order:?}");

    let mut answer = None;
    for _ in 0..20 {
        answer = client
            .answer(&tokens, COMPANY, &order.reference)
            .await
            .expect("answer");
        if answer.is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    }
    let cards = parse_answer(&answer.expect("an answer within a minute")).expect("FORESPOERSEL_OK");
    let decision = |nin: &str| {
        let card = cards.iter().find(|c| c.nin == nin).expect(nin);
        println!("{nin}: {} -> {:?}", card.result, decide(card));
        decide(card)
    };
    assert!(matches!(
        decision("15929998666"),
        Decision::Apply(regnmed_core::payroll::Withholding::Percent(_))
    ));
    assert!(matches!(
        decision("20857597940"),
        Decision::Apply(regnmed_core::payroll::Withholding::Table(_))
    ));
    assert!(matches!(decision("05850298728"), Decision::Keep(_)));
    assert!(matches!(decision("19910298057"), Decision::Keep(_)));
}
