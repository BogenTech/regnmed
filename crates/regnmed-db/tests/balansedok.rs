//! Balansedokumentasjon (#88, bokføringsloven §11): what a balance
//! account consists of at period end. Requires DATABASE_URL (skips).

use chrono::NaiveDate;
use regnmed_core::Ore;
use regnmed_core::voucher::{EntryDraft, VoucherDraft};
use regnmed_db::balansedok::{avstem, balanse_status, hent_vedlegg};
use sqlx::PgPool;
use uuid::Uuid;

async fn pool() -> Option<PgPool> {
    dotenvy::dotenv().ok();
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("skipping: DATABASE_URL not set");
        return None;
    };
    let pool = regnmed_db::connect(&url).await.expect("connect to dev db");
    regnmed_db::MIGRATOR.run(&pool).await.expect("migrate");
    Some(pool)
}

fn unique_orgnr() -> String {
    let n = u32::from_be_bytes(Uuid::new_v4().as_bytes()[..4].try_into().unwrap());
    format!("{:09}", u64::from(n) % 1_000_000_000)
}

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

fn e(konto: &str, ore: i64) -> EntryDraft {
    EntryDraft {
        account_number: konto.into(),
        amount: Ore(ore),
        vat_code: None,
        description: None,
        party_no: None,
        avdeling: None,
        prosjekt: None,
        valuta: None,
    }
}

async fn oppsett(pool: &PgPool) -> (Uuid, Uuid) {
    let company = regnmed_db::create_company(pool, &unique_orgnr(), "Balanse AS")
        .await
        .unwrap();
    regnmed_db::ensure_journal(pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (nr, navn) in [
        ("1920", "Bank"),
        ("2400", "Leverandørgjeld"),
        ("3000", "Salg"),
    ] {
        regnmed_db::ensure_account(pool, company, nr, navn)
            .await
            .unwrap();
    }
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(pool, &sub, Some("Rev Isor"), None)
        .await
        .unwrap();
    (company, person)
}

#[tokio::test]
async fn only_balance_accounts_with_a_saldo_are_listed_and_documented() {
    let Some(pool) = pool().await else { return };
    let (company, person) = oppsett(&pool).await;
    regnmed_db::post_voucher(
        &pool,
        company,
        &VoucherDraft {
            journal_code: "GL".into(),
            voucher_date: d(2026, 1, 20),
            description: "Salg".into(),
            reverses: None,
            entries: vec![e("1920", 50_000_00), e("3000", -50_000_00)],
        },
        "test",
    )
    .await
    .unwrap();

    let periode = d(2026, 1, 31);
    let linjer = balanse_status(&pool, company, periode).await.unwrap();
    // 3000 is a resultatkonto and 2400 has no saldo: neither belongs on
    // a §11 list.
    assert_eq!(linjer.len(), 1, "{linjer:?}");
    assert_eq!(linjer[0].konto, "1920");
    assert_eq!(linjer[0].saldo_ore, 50_000_00);
    assert!(linjer[0].avstemt.is_none());

    // A resultatkonto cannot be reconciled here at all.
    let feil = avstem(
        &pool, company, "3000", periode, "forsøk", None, person, periode,
    )
    .await
    .expect_err("§11 gjelder balansepostene");
    assert!(feil.to_string().contains("balansekonto"), "{feil}");

    avstem(
        &pool,
        company,
        "1920",
        periode,
        "Kontoutskrift fra banken pr. 31.01",
        None,
        person,
        periode,
    )
    .await
    .unwrap();
    let linjer = balanse_status(&pool, company, periode).await.unwrap();
    let a = linjer[0].avstemt.as_ref().expect("avstemt");
    assert_eq!(a.saldo_ore, 50_000_00, "saldoen lagres slik den var");
    assert_eq!(a.avstemt_av, "Rev Isor");
    assert_eq!(linjer[0].avvik_ore(), None, "ingen bevegelse etterpå");
}

/// The reason the saldo is stored: posting to the account after it was
/// reconciled must be VISIBLE, not silently re-baselined.
#[tokio::test]
async fn posting_after_the_avstemming_shows_up_as_a_difference() {
    let Some(pool) = pool().await else { return };
    let (company, person) = oppsett(&pool).await;
    let periode = d(2026, 3, 31);
    regnmed_db::post_voucher(
        &pool,
        company,
        &VoucherDraft {
            journal_code: "GL".into(),
            voucher_date: d(2026, 3, 10),
            description: "Salg".into(),
            reverses: None,
            entries: vec![e("1920", 10_000_00), e("3000", -10_000_00)],
        },
        "test",
    )
    .await
    .unwrap();
    avstem(
        &pool,
        company,
        "1920",
        periode,
        "Kontoutskrift",
        None,
        person,
        periode,
    )
    .await
    .unwrap();

    // A late voucher lands inside the already-reconciled period.
    regnmed_db::post_voucher(
        &pool,
        company,
        &VoucherDraft {
            journal_code: "GL".into(),
            voucher_date: d(2026, 3, 25),
            description: "Etterslept salg".into(),
            reverses: None,
            entries: vec![e("1920", 2_500_00), e("3000", -2_500_00)],
        },
        "test",
    )
    .await
    .unwrap();

    let linjer = balanse_status(&pool, company, periode).await.unwrap();
    assert_eq!(
        linjer[0].avvik_ore(),
        Some(2_500_00),
        "differansen skal være synlig, ikke skjult"
    );

    // Re-reconciling is a NEW row, and the newest one applies.
    avstem(
        &pool,
        company,
        "1920",
        periode,
        "Ny kontoutskrift etter etterslepet",
        None,
        person,
        periode,
    )
    .await
    .unwrap();
    let linjer = balanse_status(&pool, company, periode).await.unwrap();
    assert_eq!(linjer[0].avvik_ore(), None);
    let historikk = regnmed_db::balansedok::historikk(&pool, company, "1920", periode)
        .await
        .unwrap();
    assert_eq!(historikk.len(), 2, "rettingen sletter ikke den forrige");
    assert_eq!(historikk[0].saldo_ore, 12_500_00, "nyeste først");
    assert_eq!(historikk[1].saldo_ore, 10_000_00);
}

/// The vedlegg IS the documentation, so it comes back byte for byte —
/// and the hash is re-checked on the way out, like bilagsvedleggene.
#[tokio::test]
async fn the_vedlegg_round_trips_and_its_hash_is_verified() {
    let Some(pool) = pool().await else { return };
    let (company, person) = oppsett(&pool).await;
    let periode = d(2026, 6, 30);
    regnmed_db::post_voucher(
        &pool,
        company,
        &VoucherDraft {
            journal_code: "GL".into(),
            voucher_date: d(2026, 6, 1),
            description: "Salg".into(),
            reverses: None,
            entries: vec![e("1920", 1_000_00), e("3000", -1_000_00)],
        },
        "test",
    )
    .await
    .unwrap();
    let id = avstem(
        &pool,
        company,
        "1920",
        periode,
        "Kontoutskrift vedlagt",
        Some(("kontoutskrift.pdf", "application/pdf", b"%PDF-1.4 saldo")),
        person,
        periode,
    )
    .await
    .unwrap();

    let (navn, ct, bytes) = hent_vedlegg(&pool, company, id).await.unwrap();
    assert_eq!(navn, "kontoutskrift.pdf");
    assert_eq!(ct, "application/pdf");
    assert_eq!(bytes, b"%PDF-1.4 saldo");

    // Tamper with the stored bytes: the download must refuse rather than
    // hand back something that is no longer what was documented.
    sqlx::query("update balanse_dokumentasjon set vedlegg = $2 where id = $1")
        .bind(id)
        .bind(b"%PDF-1.4 forfalsket".as_slice())
        .execute(&pool)
        .await
        .expect_err("tabellen er innsettings-bar");
}
