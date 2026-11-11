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
//! Each run FILES an oppgave for the synthetic Tenor company 311032240
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

#[tokio::test]
async fn skatteetaten_takes_our_oppgave() {
    if std::env::var("AKSJONAER_LIVE").as_deref() != Ok("1") {
        eprintln!("skipping: AKSJONAER_LIVE=1 not set");
        return;
    }
    let founded = NaiveDate::from_ymd_opt(YEAR, 1, 1).unwrap();
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
            date: founded,
            type_: TransactionType::Stiftelse,
            new_shares: 100,
            shares_after: 100,
            nominal_value_ore: 30_000,
            share_premium_ore: 0,
        }],
    })
    .unwrap();
    let sub = render_sub_form(&SubForm {
        orgnumber: COMPANY.into(),
        inntektsar: YEAR,
        id: Some(ShareholderId::Nin("12855599579".into())),
        name: "daglig leder (syntetisk)".into(),
        address: None,
        postal_code: None,
        postal_city: None,
        country_code: None,
        share_count: this_year(100),
        dividend: vec![],
        movements: vec![ShareMovement {
            date: founded,
            type_: TransactionType::Stiftelse,
            quantity: 100,
            amount_ore: Some(3_000_000),
        }],
    })
    .unwrap();

    let mut config = MaskinportenConfig::from_env().expect("MASKINPORTEN_* from the test env file");
    config.scopes = SCOPES.into();
    let receipt = ShareholderReturnClient::from_env()
        .submit(
            &TokenProvider::new(config),
            COMPANY,
            YEAR,
            Form {
                xml: &main,
                idempotency_key: Uuid::new_v4(),
            },
            &[Form {
                xml: &sub,
                idempotency_key: Uuid::new_v4(),
            }],
            Uuid::new_v4(),
        )
        .await;
    eprintln!("{receipt:#?}");
    let receipt = receipt.unwrap();
    assert!(!receipt.dialog_id.is_empty() && !receipt.forsendelse_id.is_empty());
}
