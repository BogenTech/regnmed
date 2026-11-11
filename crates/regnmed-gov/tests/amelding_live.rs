//! The a-melding against Skatteetatens REAL test environment — rendered by
//! `regnmed_core::amelding`, submitted by `regnmed_gov::amelding`, and
//! judged by Skatteetaten (docs/gov.md, #180).
//!
//! Never runs in CI: skips unless AMELDING_LIVE=1, and needs the
//! Maskinporten test client (docs/secrets.md):
//!
//!     set -a; . ~/.config/regnmed/maskinporten-test.env; set +a
//!     AMELDING_LIVE=1 cargo nextest run -p regnmed-gov --test amelding_live --no-capture
//!
//! Each run SUBMITS a melding for the synthetic Tenor company 311032240
//! (systembruker approved 2026-11-11), so a run is a real delivery in test.

use chrono::{NaiveDate, Utc};
use regnmed_core::amelding::{Employment, Income, IncomeKind, Melding, Recipient};
use regnmed_core::payroll::Zone;
use regnmed_gov::amelding::{AmeldingClient, SCOPES};
use regnmed_gov::maskinporten::{MaskinportenConfig, TokenProvider};
use uuid::Uuid;

const COMPANY: &str = "311032240";

fn melding() -> Melding {
    let d = |y, m, day| NaiveDate::from_ymd_opt(y, m, day).unwrap();
    Melding {
        message_id: Uuid::new_v4().to_string(),
        replaces_message_id: None,
        delivered_at: Utc::now(),
        source_system: "regnmed".into(),
        orgnumber: COMPANY.into(),
        business_orgnumber: Some("314752538".into()),
        payment_date: d(2026, 10, 25),
        zone: Zone::I,
        employer_contribution_rate_bp: 1410,
        // 40 000 × 14,1 %
        employer_contribution_ore: 564_000,
        recipients: vec![Recipient {
            name: "daglig leder (syntetisk)".into(),
            nin: "12855599579".into(),
            employment: Employment {
                id: "regnmed-live-1".into(),
                start_date: d(2026, 1, 1),
                end_date: None,
                full_time_hours_per_week_hundredths: Some(3750),
                occupation_code: Some("2512101".into()),
                working_hours_scheme: Some("ikkeSkift".into()),
                position_percent_bp: Some(10000),
                last_salary_change: Some(d(2026, 1, 1)),
                employment_form: Some("fast".into()),
            },
            income: vec![Income {
                kind: IncomeKind::FixedSalary,
                amount_ore: 4_000_000,
            }],
            withholding_ore: 1_400_000,
        }],
    }
}

#[tokio::test]
async fn skatteetaten_receives_our_melding_without_deviations() {
    if std::env::var("AMELDING_LIVE").as_deref() != Ok("1") {
        eprintln!("skipping: AMELDING_LIVE=1 not set");
        return;
    }
    let mut config = MaskinportenConfig::from_env().expect("MASKINPORTEN_* from the test env file");
    config.scopes = SCOPES.into();
    let tokens = TokenProvider::new(config);
    let client = AmeldingClient::from_env();

    let m = melding();
    let json = regnmed_core::amelding::render_json(&m).unwrap();
    let receipt = client
        .submit(
            &tokens,
            COMPANY,
            &regnmed_core::amelding::period(&m),
            json,
            Uuid::new_v4(),
        )
        .await
        .unwrap();
    assert_eq!(receipt.meldings_id, m.message_id);
    eprintln!("receipt: {receipt:?}");

    // The feedback took 21 s on the first try; allow three minutes.
    for _ in 0..36 {
        if let Some(feedback) = client.feedback(&tokens, COMPANY, &receipt).await.unwrap() {
            eprintln!("{}", serde_json::to_string_pretty(&feedback).unwrap());
            let received = &feedback["mottak"][0]["mottattLeveranse"]
                .as_array()
                .unwrap()
                .iter()
                .find(|l| l["meldingsId"] == m.message_id.as_str())
                .expect("our melding in the feedback")
                .clone();
            assert_eq!(received["mottakstatus"], "mottatt");
            assert_eq!(
                received["avvik"],
                serde_json::json!([]),
                "Skatteetaten found deviations"
            );
            assert_eq!(
                received["mottattAvgiftOgTrekkTotalt"]["sumArbeidsgiveravgift"],
                5640
            );
            return;
        }
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
    panic!("no feedback within three minutes");
}
