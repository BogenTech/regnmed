//! The aksjonærregisteroppgave against Skatteetatens REAL test environment
//! — rendered by `regnmed_core::shareholder_return`, filed by
//! `regnmed_gov::shareholder_return`, judged by Skatteetaten (#182).
//!
//! Never runs in CI: skips unless AKSJONAER_LIVE=1, and needs the
//! Maskinporten test client (docs/secrets.md):
//!
//!     set -a; . ~/.config/regnmed/maskinporten-test.env; set +a
//!     AKSJONAER_LIVE=1 cargo nextest run -p regnmed-gov --test shareholder_return_live --no-capture
//!
//! The test waits for Skatteetaten's answer in Dialogporten and requires
//! `Acceptance`. Each run FILES an oppgave for the synthetic Tenor company 311032240
//! (the systembruker approved 2026-11-11), so a run is a real delivery in
//! test.

use chrono::NaiveDate;
use regnmed_core::share_register::TransactionType;
use regnmed_core::shareholder_return::{
    Company, MainForm, NewIssue, PriorYear, ShareMovement, ShareholderId, SubForm,
    render_main_form, render_sub_form,
};
use regnmed_gov::maskinporten::{MaskinportenConfig, TokenProvider};
use regnmed_gov::shareholder_return::{Form, SCOPES, ShareholderReturnClient};
use uuid::Uuid;

const COMPANY: &str = "311032240";
const YEAR: i32 = 2026;

fn this_year(n: i64) -> PriorYear {
    PriorYear {
        last_year: 0,
        this_year: n,
    }
}

const GIVER: &str = "12855599579";
const RECEIVER: &str = "26829398612";

fn sub(nin: &str, last: i64, this: i64, movements: Vec<ShareMovement>) -> String {
    render_sub_form(&SubForm {
        orgnumber: COMPANY.into(),
        inntektsar: YEAR,
        id: Some(ShareholderId::Nin(nin.into())),
        name: "aksjonær (syntetisk)".into(),
        address: None,
        postal_code: None,
        postal_city: None,
        country_code: None,
        share_count: PriorYear {
            last_year: last,
            this_year: this,
        },
        dividend: vec![],
        movements,
    })
    .unwrap()
}

/// Stiftelse (T) to one owner, then a sale (K/R) and an overføring med
/// skattemessig kontinuitet (OK, both sides naming the other) to a
/// second — every code regnmed files beyond the founding, against the
/// real validator.
#[tokio::test]
async fn skatteetaten_takes_our_oppgave() {
    if std::env::var("AKSJONAER_LIVE").as_deref() != Ok("1") {
        eprintln!("skipping: AKSJONAER_LIVE=1 not set");
        return;
    }
    let d = |m, day| NaiveDate::from_ymd_opt(YEAR, m, day).unwrap();
    let movement = |m, day, type_, quantity, amount_ore, other: Option<&str>| ShareMovement {
        date: d(m, day),
        type_,
        quantity,
        amount_ore,
        counterparty: other.map(|n| ShareholderId::Nin(n.into())),
    };
    let main = render_main_form(&MainForm {
        company: Company {
            orgnumber: COMPANY.into(),
            name: "TENOR TESTSELSKAP AS".into(),
            address: Some("Testveien 1".into()),
            postal_code: Some("0150".into()),
            postal_city: Some("OSLO".into()),
            contact_name: None,
            contact_email: Some("test@regnmed.no".into()),
        },
        inntektsar: YEAR,
        // 100 shares at 300 kr: the 30 000 kr minimum.
        share_capital: this_year(3_000_000),
        nominal_value_ore: this_year(30_000),
        share_count: this_year(100),
        paid_in_share_capital: this_year(3_000_000),
        share_premium: this_year(0),
        dividend: vec![],
        new_issues: vec![NewIssue {
            date: d(1, 1),
            type_: TransactionType::Stiftelse,
            new_shares: 100,
            shares_after: 100,
            nominal_value_ore: 30_000,
            share_premium_ore: 0,
        }],
    })
    .unwrap();
    use TransactionType::*;
    let giver = sub(
        GIVER,
        0,
        60,
        vec![
            movement(1, 1, Stiftelse, 100, Some(3_000_000), None),
            movement(3, 1, Salg, 30, Some(1_200_000), None),
            movement(
                6,
                1,
                OverforingMedKontinuitetAvgang,
                10,
                None,
                Some(RECEIVER),
            ),
        ],
    );
    let receiver = sub(
        RECEIVER,
        0,
        40,
        vec![
            movement(3, 1, Kjop, 30, Some(1_200_000), None),
            movement(6, 1, OverforingMedKontinuitet, 10, None, Some(GIVER)),
        ],
    );

    let mut config = MaskinportenConfig::from_env().expect("MASKINPORTEN_* from the test env file");
    config.scopes = SCOPES.into();
    let tokens = TokenProvider::new(config);
    let client = ShareholderReturnClient::from_env();
    let k = Uuid::new_v4;
    let receipt = client
        .submit(
            &tokens,
            COMPANY,
            YEAR,
            Form {
                xml: &main,
                idempotency_key: k(),
            },
            &[
                Form {
                    xml: &giver,
                    idempotency_key: k(),
                },
                Form {
                    xml: &receiver,
                    idempotency_key: k(),
                },
            ],
            k(),
        )
        .await;
    eprintln!("{receipt:#?}");
    let receipt = receipt.unwrap();

    // Skatteetaten's answer arrived about a minute after filing in test;
    // allow three.
    for _ in 0..36 {
        let outcome = client
            .outcome(
                &tokens,
                COMPANY,
                &receipt.dialog_id,
                &receipt.forsendelse_id,
            )
            .await
            .unwrap();
        if let Some(o) = outcome {
            eprintln!("{o:#?}");
            assert_eq!(o.kind, "Acceptance", "Skatteetaten did not approve: {o:?}");
            return;
        }
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
    panic!("no answer from Skatteetaten within three minutes");
}
