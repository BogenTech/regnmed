//! The revisor's verification report over the web API: a revisor whose
//! only path to the company is a 'revisjon' engagement (read-only)
//! generates the report; every kontroll passes on a healthy ledger; a
//! planted anchor mismatch turns the verdict; the text download renders;
//! outsiders get 404. Requires DATABASE_URL (skips otherwise).

use crate::common::{TestIdp, test_state, unique_orgnr};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::NaiveDate;
use regnmed_core::Ore;
use regnmed_core::voucher::{EntryDraft, VoucherDraft};
use tower::ServiceExt;
use uuid::Uuid;

use regnmed_api::{AppState, router};

async fn get_raw(state: &AppState, uri: &str, bearer: &str) -> (StatusCode, String, String) {
    let response = router(state.clone())
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(uri)
                .header("authorization", format!("Bearer {bearer}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let content_type = response
        .headers()
        .get("content-type")
        .map(|v| v.to_str().unwrap().to_string())
        .unwrap_or_default();
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    (
        status,
        content_type,
        String::from_utf8(bytes.to_vec()).unwrap(),
    )
}

#[tokio::test]
async fn revisor_generates_the_verification_report() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };

    // The revisor reaches the company ONLY through her firm's revisjon
    // engagement — the marketplace path, not a direct membership.
    let revisor_sub = format!("test|{}", Uuid::new_v4());
    let revisor = regnmed_db::ensure_person(&state.pool, &revisor_sub, Some("Randi Revisor"), None)
        .await
        .unwrap();
    let firm =
        regnmed_db::ensure_firm(&state.pool, &unique_orgnr(), "Revisjon & Co AS", "revisjon")
            .await
            .unwrap();
    regnmed_db::ensure_firm_member(&state.pool, firm, revisor, "ansatt")
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Kontrollert AS")
        .await
        .unwrap();
    regnmed_db::ensure_engagement(&state.pool, firm, company, "revisjon")
        .await
        .unwrap();
    let token = idp.token(&revisor_sub, "Randi Revisor");

    // A small ledger with reskontro, a period lock and an anchor.
    regnmed_db::ensure_journal(&state.pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (number, name) in [
        ("1500", "Kundefordringer"),
        ("1920", "Bank"),
        ("3000", "Salg"),
    ] {
        regnmed_db::ensure_account(&state.pool, company, number, name)
            .await
            .unwrap();
    }
    regnmed_db::set_account_reskontro(&state.pool, company, "1500", Some("kunde"))
        .await
        .unwrap();
    let (_, party_no) =
        regnmed_db::create_party(&state.pool, company, "kunde", "Kunde AS", None, None)
            .await
            .unwrap();
    let draft = VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: NaiveDate::from_ymd_opt(2026, 5, 10).unwrap(),
        description: "Faktura".into(),
        reverses: None,
        entries: vec![
            EntryDraft {
                account_number: "1500".into(),
                amount: Ore(12_500_00),
                vat_code: None,
                description: None,
                party_no: Some(party_no),
                avdeling: None,
                prosjekt: None,
                valuta: None,
            },
            EntryDraft {
                account_number: "3000".into(),
                amount: Ore(-12_500_00),
                vat_code: None,
                description: None,
                party_no: None,
                avdeling: None,
                prosjekt: None,
                valuta: None,
            },
        ],
    };
    regnmed_db::post_voucher(&state.pool, company, &draft, "test")
        .await
        .unwrap();
    regnmed_db::set_period_lock(
        &state.pool,
        company,
        NaiveDate::from_ymd_opt(2026, 5, 31).unwrap(),
        "test",
        false,
    )
    .await
    .unwrap();
    regnmed_db::create_anchor_snapshot(&state.pool)
        .await
        .unwrap()
        .expect("ledger has vouchers");

    // Healthy ledger: every kontroll OK, anchors listed.
    let uri = format!("/companies/{company}/reports/revisjon");
    let (status, _, body) = get_raw(&state, &uri, &token).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let report: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(report["alle_ok"], true, "{report}");
    let kontroller = report["kontroller"].as_array().unwrap();
    assert_eq!(kontroller.len(), 8);
    for kontroll in kontroller {
        assert_eq!(kontroll["ok"], true, "{kontroll}");
    }
    assert!(
        report["kontroller"]
            .as_array()
            .unwrap()
            .iter()
            .any(|k| k["navn"] == "Reskontro mot hovedbok"
                && k["detalj"].as_str().unwrap().contains("1 reskontrokonto")),
        "{report}"
    );
    // A ledger without imports says so — the kontroll never hides.
    assert!(
        kontroller.iter().any(|k| k["navn"] == "Importert historikk"
            && k["detalj"]
                .as_str()
                .unwrap()
                .contains("ingen importert historikk")),
        "{report}"
    );
    assert!(!report["ankere"].as_array().unwrap().is_empty());
    assert_eq!(report["kjede_sekvens"], 1);

    // The text rendering downloads with the verdict stated.
    let (status, content_type, text) =
        get_raw(&state, &format!("{uri}?format=tekst"), &token).await;
    assert_eq!(status, StatusCode::OK);
    assert!(content_type.starts_with("text/plain"), "{content_type}");
    assert!(text.contains("VERIFIKASJONSRAPPORT"));
    assert!(text.contains("ALLE KONTROLLER OK"));
    assert!(text.contains("Kontrollert AS"));

    // A planted anchor claiming a different head turns the verdict —
    // the report reports, it never hides.
    let fake = Uuid::now_v7();
    sqlx::query("insert into anchor_snapshot (id, root_hash, leaf_count) values ($1, $2, 1)")
        .bind(fake)
        .bind([0xAA_u8; 32].as_slice())
        .execute(&state.pool)
        .await
        .unwrap();
    sqlx::query(
        "insert into anchor_leaf (snapshot_id, company_id, last_seq, last_hash)
         values ($1, $2, 1, $3)",
    )
    .bind(fake)
    .bind(company)
    .bind([0xAA_u8; 32].as_slice())
    .execute(&state.pool)
    .await
    .unwrap();
    let (_, _, body) = get_raw(&state, &uri, &token).await;
    let report: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(report["alle_ok"], false);
    let forankring = report["kontroller"]
        .as_array()
        .unwrap()
        .iter()
        .find(|k| k["navn"] == "Ekstern forankring")
        .unwrap();
    assert_eq!(forankring["ok"], false);

    // No path to the company → 404, never a hint that it exists.
    let stranger_sub = format!("test|{}", Uuid::new_v4());
    regnmed_db::ensure_person(&state.pool, &stranger_sub, Some("Fremmed"), None)
        .await
        .unwrap();
    let stranger_token = idp.token(&stranger_sub, "Fremmed");
    let (status, _, _) = get_raw(&state, &uri, &stranger_token).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Kontroll 4 is a real tie-out: Σ reskontro against the account's own
/// saldo, konto for konto. Every divergence below is reachable WITHOUT
/// editing the ledger — the reskontro flag is what moves (åpningsbalanse
/// and SAF-T import clear it, an admin can set it again), and the
/// postings then sit on the wrong side of the equation.
#[tokio::test]
async fn the_reskontro_tie_out_names_the_konto_and_the_difference() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };

    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Bea Bokfører"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Avstemt AS")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, person, "admin")
        .await
        .unwrap();
    let token = idp.token(&sub, "Bea Bokfører");
    regnmed_db::ensure_journal(&state.pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (number, name) in [
        ("1500", "Kundefordringer"),
        ("2400", "Leverandørgjeld"),
        ("2500", "Gammel gjeld"),
        ("3000", "Salg"),
        ("4000", "Varekjøp"),
    ] {
        regnmed_db::ensure_account(&state.pool, company, number, name)
            .await
            .unwrap();
    }

    let post = |konto: &'static str,
                ore: i64,
                motkonto: &'static str,
                party_no: Option<String>,
                dag: u32| {
        let pool = state.pool.clone();
        async move {
            let draft = VoucherDraft {
                journal_code: "GL".into(),
                voucher_date: NaiveDate::from_ymd_opt(2026, 5, dag).unwrap(),
                description: "Bilag".into(),
                reverses: None,
                entries: vec![
                    EntryDraft {
                        account_number: konto.into(),
                        amount: Ore(ore),
                        vat_code: None,
                        description: None,
                        party_no,
                        avdeling: None,
                        prosjekt: None,
                        valuta: None,
                    },
                    EntryDraft {
                        account_number: motkonto.into(),
                        amount: Ore(-ore),
                        vat_code: None,
                        description: None,
                        party_no: None,
                        avdeling: None,
                        prosjekt: None,
                        valuta: None,
                    },
                ],
            };
            regnmed_db::post_voucher(&pool, company, &draft, "test")
                .await
                .unwrap();
        }
    };

    // (a) 1500 flagged: one invoice with a party, then the flag is
    // cleared (as åpningsbalanse does), 2 500,00 is posted without a
    // party, and the flag comes back. The saldo now exceeds what the
    // kundespesifikasjon holds.
    regnmed_db::set_account_reskontro(&state.pool, company, "1500", Some("kunde"))
        .await
        .unwrap();
    let (_, kunde_no) =
        regnmed_db::create_party(&state.pool, company, "kunde", "Kunde AS", None, None)
            .await
            .unwrap();
    post("1500", 12_500_00, "3000", Some(kunde_no), 10).await;
    regnmed_db::set_account_reskontro(&state.pool, company, "1500", None)
        .await
        .unwrap();
    post("1500", 2_500_00, "3000", None, 11).await;
    regnmed_db::set_account_reskontro(&state.pool, company, "1500", Some("kunde"))
        .await
        .unwrap();

    // (b) 2400 carries leverandør postings, then someone flags it
    // 'kunde' — the amounts land in the spesifikasjon the account is not.
    regnmed_db::set_account_reskontro(&state.pool, company, "2400", Some("leverandor"))
        .await
        .unwrap();
    let (_, lev_no) = regnmed_db::create_party(
        &state.pool,
        company,
        "leverandor",
        "Leverandør AS",
        None,
        None,
    )
    .await
    .unwrap();
    post("2400", -8_000_00, "4000", Some(lev_no.clone()), 12).await;
    regnmed_db::set_account_reskontro(&state.pool, company, "2400", Some("kunde"))
        .await
        .unwrap();

    // (c) 2500 holds party postings but the flag was cleared and never
    // restored — the amount is in the leverandørspesifikasjon while no
    // reskontro account in the hovedbok holds it.
    regnmed_db::set_account_reskontro(&state.pool, company, "2500", Some("leverandor"))
        .await
        .unwrap();
    post("2500", -3_000_00, "4000", Some(lev_no), 13).await;
    regnmed_db::set_account_reskontro(&state.pool, company, "2500", None)
        .await
        .unwrap();

    let uri = format!("/companies/{company}/reports/revisjon");
    let (status, _, body) = get_raw(&state, &uri, &token).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let report: serde_json::Value = serde_json::from_str(&body).unwrap();
    let kontroll = report["kontroller"]
        .as_array()
        .unwrap()
        .iter()
        .find(|k| k["navn"] == "Reskontro mot hovedbok")
        .unwrap();
    assert_eq!(kontroll["ok"], false, "{report}");
    assert_eq!(report["alle_ok"], false);
    let detalj = kontroll["detalj"].as_str().unwrap();

    // (a) the difference, in kroner and in øre, with the konto named.
    assert!(
        detalj.contains("konto 1500: hovedbok 15000,00 mot reskontro 12500,00 — differanse 2500,00 (250000 øre), 1 postering uten part"),
        "{detalj}"
    );
    // (b) the party of the wrong kind.
    assert!(
        detalj.contains(
            "konto 2400 er merket kunde, men 1 postering bærer part av typen leverandor (-8000,00)"
        ),
        "{detalj}"
    );
    // (c) the party postings on an account nobody flagged.
    assert!(
        detalj.contains("konto 2500 er ikke merket som reskontrokonto, men 1 postering bærer part (-3000,00, leverandor)"),
        "{detalj}"
    );
    // Every finding on its own line, and only the reskontro kontroll
    // fails — the tie-out never drags the other checks down with it.
    assert_eq!(detalj.lines().count(), 3, "{detalj}");
    for other in report["kontroller"].as_array().unwrap() {
        if other["navn"] != "Reskontro mot hovedbok" {
            assert_eq!(other["ok"], true, "{other}");
        }
    }

    // The text rendering carries each finding as its own indented line.
    let (status, _, text) = get_raw(&state, &format!("{uri}?format=tekst"), &token).await;
    assert_eq!(status, StatusCode::OK);
    assert!(text.contains("[AVVIK] Reskontro mot hovedbok"), "{text}");
    assert!(text.contains("       konto 2500 er ikke merket"), "{text}");

    // Restoring the flags and posting the missing party binding is not
    // possible (the ledger is append-only) — but a company where every
    // party posting sits on its own flagged account ties out, and says
    // so with the total it reconciled.
    let clean = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Ren AS")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, clean, person, "admin")
        .await
        .unwrap();
    regnmed_db::ensure_journal(&state.pool, clean, "GL", "Hovedbok")
        .await
        .unwrap();
    for (number, name) in [
        ("1500", "Kundefordringer"),
        ("2400", "Leverandørgjeld"),
        ("3000", "Salg"),
    ] {
        regnmed_db::ensure_account(&state.pool, clean, number, name)
            .await
            .unwrap();
    }
    regnmed_db::set_account_reskontro(&state.pool, clean, "1500", Some("kunde"))
        .await
        .unwrap();
    // 2400 is flagged and never posted to: zero is a saldo like any
    // other and ties out. (Counting rows rather than amounts made an
    // untouched account look like it held a party-less posting, because
    // the left join hands the account back with a null entry.)
    regnmed_db::set_account_reskontro(&state.pool, clean, "2400", Some("leverandor"))
        .await
        .unwrap();
    let (_, ren_kunde) =
        regnmed_db::create_party(&state.pool, clean, "kunde", "Kunde AS", None, None)
            .await
            .unwrap();
    let draft = VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: NaiveDate::from_ymd_opt(2026, 5, 10).unwrap(),
        description: "Faktura".into(),
        reverses: None,
        entries: vec![
            EntryDraft {
                account_number: "1500".into(),
                amount: Ore(7_000_00),
                vat_code: None,
                description: None,
                party_no: Some(ren_kunde),
                avdeling: None,
                prosjekt: None,
                valuta: None,
            },
            EntryDraft {
                account_number: "3000".into(),
                amount: Ore(-7_000_00),
                vat_code: None,
                description: None,
                party_no: None,
                avdeling: None,
                prosjekt: None,
                valuta: None,
            },
        ],
    };
    regnmed_db::post_voucher(&state.pool, clean, &draft, "test")
        .await
        .unwrap();
    let (_, _, body) = get_raw(
        &state,
        &format!("/companies/{clean}/reports/revisjon"),
        &token,
    )
    .await;
    let report: serde_json::Value = serde_json::from_str(&body).unwrap();
    let kontroll = report["kontroller"]
        .as_array()
        .unwrap()
        .iter()
        .find(|k| k["navn"] == "Reskontro mot hovedbok")
        .unwrap();
    assert_eq!(kontroll["ok"], true, "{report}");
    let detalj = kontroll["detalj"].as_str().unwrap();
    assert!(detalj.contains("2 reskontrokontoer avstemt"), "{detalj}");
    assert!(detalj.contains("7000,00"), "{detalj}");
}
