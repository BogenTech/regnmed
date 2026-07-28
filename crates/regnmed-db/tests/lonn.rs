//! Lønnskjøring mot ekte Postgres (#46, docs/lonn.md).
//!
//! Det som må stemme: bilaget balanserer, forskuddstrekket følger
//! reglene (feriepenger trekkfrie, halv skatt i desember),
//! arbeidsgiveravgiften kommer fra satsregisteret, utbetalte feriepenger
//! trekker ned GJELDEN i stedet for å bli en ny kostnad, samme måned kan
//! ikke kjøres to ganger, og en kjøring kan ikke endres i ettertid.
//!
//! Krever DATABASE_URL; hopper over ellers.

use chrono::NaiveDate;
use regnmed_db::lonn::{self, Lonnspost, NyAnsatt};
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

fn dato(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

async fn selskap(pool: &PgPool) -> Uuid {
    let company = regnmed_db::create_company(pool, &unique_orgnr(), "Lønnstest AS")
        .await
        .unwrap();
    regnmed_db::ensure_journal(pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (nr, navn) in [
        ("5000", "Lønn til ansatte"),
        ("5090", "Feriepenger"),
        ("5400", "Arbeidsgiveravgift"),
        ("2600", "Forskuddstrekk"),
        ("2770", "Skyldig arbeidsgiveravgift"),
        ("2930", "Skyldig lønn"),
        ("2940", "Skyldige feriepenger"),
        ("1920", "Bankinnskudd"),
    ] {
        regnmed_db::ensure_account(pool, company, nr, navn)
            .await
            .unwrap();
    }
    company
}

async fn ansatt(pool: &PgPool, company: Uuid, fnr: &str, navn: &str, lonn: i64) -> Uuid {
    lonn::create_ansatt(
        pool,
        company,
        &NyAnsatt {
            fodselsnummer: fnr.into(),
            navn: navn.into(),
            stilling: Some("Utvikler".into()),
            ansatt_fra: dato(2025, 1, 1),
            manedslonn_ore: Some(lonn),
            timelonn_ore: None,
            trekk_type: "prosent".into(),
            trekk_prosent_bp: Some(3500),
            trekk_tabell: None,
            feriepenger_bp: 1020,
            bank_account: None,
            note: None,
        },
        "Test",
    )
    .await
    .unwrap()
}

/// Sum of a voucher's entries — must be exactly zero, always.
async fn voucher_sum(pool: &PgPool, voucher_id: Uuid) -> i64 {
    sqlx::query_scalar(
        "select coalesce(sum(amount_ore), 0)::bigint from entry where voucher_id = $1",
    )
    .bind(voucher_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn konto_belop(pool: &PgPool, voucher_id: Uuid, konto: &str) -> i64 {
    sqlx::query_scalar(
        "select coalesce(sum(e.amount_ore), 0)::bigint from entry e
         join account a on a.id = e.account_id
         where e.voucher_id = $1 and a.number = $2",
    )
    .bind(voucher_id)
    .bind(konto)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn lonnskjoring_bokfores_som_ett_balansert_bilag() {
    let Some(pool) = pool().await else { return };
    let company = selskap(&pool).await;
    // 50 000 kr, 35 % trekk, sone I (14,1 %).
    let a = ansatt(&pool, company, "26829398612", "Kari Utvikler", 5_000_000).await;

    let kjoring = lonn::kjor_lonn(
        &pool,
        company,
        2026,
        3,
        dato(2026, 3, 25),
        "I",
        &[Lonnspost {
            employee_id: a,
            brutto_ore: None,
            feriepenger_ore: 0,
            fra_timer: false,
        }],
        "Test",
    )
    .await
    .unwrap();

    assert_eq!(kjoring.sum.brutto_ore, 5_000_000);
    assert_eq!(kjoring.sum.forskuddstrekk_ore, 1_750_000);
    assert_eq!(kjoring.sum.netto_ore, 3_250_000);
    // 10,2 % feriepengeavsetning av bruttolønnen.
    assert_eq!(kjoring.sum.feriepengeavsetning_ore, 510_000);
    // 14,1 % arbeidsgiveravgift, fra satsregisteret — ikke fra koden.
    assert_eq!(kjoring.sum.aga_ore, 705_000);

    // Bilaget balanserer. Alt annet er uinteressant hvis dette svikter.
    assert_eq!(voucher_sum(&pool, kjoring.voucher_id).await, 0);

    assert_eq!(
        konto_belop(&pool, kjoring.voucher_id, "5000").await,
        5_000_000
    );
    assert_eq!(
        konto_belop(&pool, kjoring.voucher_id, "2600").await,
        -1_750_000
    );
    assert_eq!(
        konto_belop(&pool, kjoring.voucher_id, "2930").await,
        -3_250_000
    );
    assert_eq!(
        konto_belop(&pool, kjoring.voucher_id, "5090").await,
        510_000
    );
    assert_eq!(
        konto_belop(&pool, kjoring.voucher_id, "2940").await,
        -510_000
    );
    assert_eq!(
        konto_belop(&pool, kjoring.voucher_id, "5400").await,
        705_000
    );
    assert_eq!(
        konto_belop(&pool, kjoring.voucher_id, "2770").await,
        -705_000
    );
}

/// Utbetalte feriepenger er ikke en ny kostnad — de trekker ned gjelden
/// som ble avsatt i opptjeningsåret. Blir dette feil, kostnadsføres
/// feriepenger to ganger og resultatet er systematisk for lavt.
#[tokio::test]
async fn utbetalte_feriepenger_reduserer_gjeld_og_er_trekkfrie() {
    let Some(pool) = pool().await else { return };
    let company = selskap(&pool).await;
    let a = ansatt(&pool, company, "08888797336", "Ola Ferierende", 3_000_000).await;

    let kjoring = lonn::kjor_lonn(
        &pool,
        company,
        2026,
        6,
        dato(2026, 6, 20),
        "I",
        &[Lonnspost {
            employee_id: a,
            brutto_ore: None,
            feriepenger_ore: 4_000_000,
            fra_timer: false,
        }],
        "Test",
    )
    .await
    .unwrap();

    // Trekk bare av ordinær lønn: 35 % av 30 000, ikke av 70 000.
    assert_eq!(kjoring.sum.forskuddstrekk_ore, 1_050_000);
    assert_eq!(kjoring.sum.netto_ore, 3_000_000 + 4_000_000 - 1_050_000);
    assert_eq!(voucher_sum(&pool, kjoring.voucher_id).await, 0);

    // 5000 bærer BARE ordinær lønn — feriepengene er ingen ny kostnad.
    assert_eq!(
        konto_belop(&pool, kjoring.voucher_id, "5000").await,
        3_000_000
    );
    // 2940 debiteres 4 000 000 (uttak) og krediteres 306 000 (ny avsetning).
    assert_eq!(
        konto_belop(&pool, kjoring.voucher_id, "2940").await,
        4_000_000 - 306_000
    );
    // Avgift faller på alt som faktisk utbetales, feriepengene inkludert.
    assert_eq!(kjoring.sum.aga_ore, 987_000); // 14,1 % av 70 000
}

#[tokio::test]
async fn desember_gir_halvt_forskuddstrekk() {
    let Some(pool) = pool().await else { return };
    let company = selskap(&pool).await;
    let a = ansatt(&pool, company, "25927898821", "Nils Desember", 5_000_000).await;

    let kjoring = lonn::kjor_lonn(
        &pool,
        company,
        2026,
        12,
        dato(2026, 12, 15),
        "I",
        &[Lonnspost {
            employee_id: a,
            brutto_ore: None,
            feriepenger_ore: 0,
            fra_timer: false,
        }],
        "Test",
    )
    .await
    .unwrap();
    assert_eq!(
        kjoring.sum.forskuddstrekk_ore, 875_000,
        "halv skatt i desember"
    );
    assert_eq!(voucher_sum(&pool, kjoring.voucher_id).await, 0);
}

#[tokio::test]
async fn sone_v_er_nullsats_og_sone_ia_nektes() {
    let Some(pool) = pool().await else { return };
    let company = selskap(&pool).await;
    let a = ansatt(&pool, company, "26829398612", "Finnmarking", 4_000_000).await;

    let kjoring = lonn::kjor_lonn(
        &pool,
        company,
        2026,
        4,
        dato(2026, 4, 25),
        "V",
        &[Lonnspost {
            employee_id: a,
            brutto_ore: None,
            feriepenger_ore: 0,
            fra_timer: false,
        }],
        "Test",
    )
    .await
    .unwrap();
    assert_eq!(kjoring.sum.aga_ore, 0, "sone V er nullsats");
    assert_eq!(voucher_sum(&pool, kjoring.voucher_id).await, 0);

    // Sone Ia nektes: fribeløpet kan ikke leses ut av én sats.
    let feil = lonn::kjor_lonn(
        &pool,
        company,
        2026,
        5,
        dato(2026, 5, 25),
        "Ia",
        &[Lonnspost {
            employee_id: a,
            brutto_ore: None,
            feriepenger_ore: 0,
            fra_timer: false,
        }],
        "Test",
    )
    .await
    .unwrap_err();
    assert!(feil.to_string().contains("fribeløpet"), "{feil}");
}

#[tokio::test]
async fn tabelltrekk_stopper_kjoringen_i_stedet_for_a_gjette() {
    let Some(pool) = pool().await else { return };
    let company = selskap(&pool).await;
    let a = lonn::create_ansatt(
        &pool,
        company,
        &NyAnsatt {
            fodselsnummer: "08888797336".into(),
            navn: "Tabell Trekksen".into(),
            stilling: None,
            ansatt_fra: dato(2025, 1, 1),
            manedslonn_ore: Some(5_000_000),
            timelonn_ore: None,
            trekk_type: "tabell".into(),
            trekk_prosent_bp: None,
            trekk_tabell: Some(7100),
            feriepenger_bp: 1020,
            bank_account: None,
            note: None,
        },
        "Test",
    )
    .await
    .unwrap();

    let feil = lonn::kjor_lonn(
        &pool,
        company,
        2026,
        3,
        dato(2026, 3, 25),
        "I",
        &[Lonnspost {
            employee_id: a,
            brutto_ore: None,
            feriepenger_ore: 0,
            fra_timer: false,
        }],
        "Test",
    )
    .await
    .unwrap_err();
    assert!(feil.to_string().contains("tabelltrekk"), "{feil}");
    assert!(feil.to_string().contains("tilnærmer dem ikke"), "{feil}");
}

#[tokio::test]
async fn samme_maned_kan_ikke_kjores_to_ganger_og_kjoringer_er_uforanderlige() {
    let Some(pool) = pool().await else { return };
    let company = selskap(&pool).await;
    let a = ansatt(&pool, company, "26829398612", "Kari", 4_000_000).await;
    let post = || {
        vec![Lonnspost {
            employee_id: a,
            brutto_ore: None,
            feriepenger_ore: 0,
            fra_timer: false,
        }]
    };

    let forste = lonn::kjor_lonn(
        &pool,
        company,
        2026,
        7,
        dato(2026, 7, 25),
        "I",
        &post(),
        "Test",
    )
    .await
    .unwrap();
    assert!(lonn::kjort_maned(&pool, company, 2026, 7).await.unwrap());

    let feil = lonn::kjor_lonn(
        &pool,
        company,
        2026,
        7,
        dato(2026, 7, 25),
        "I",
        &post(),
        "Test",
    )
    .await
    .unwrap_err();
    assert!(feil.to_string().contains("allerede kjørt"), "{feil}");

    // Databasen selv nekter å endre eller slette en kjøring.
    let err = sqlx::query("update payroll_run set brutto_ore = 1 where id = $1")
        .bind(forste.id)
        .execute(&pool)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("innsettings-bare"), "{err}");

    let err = sqlx::query("delete from payroll_line where run_id = $1")
        .bind(forste.id)
        .execute(&pool)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("innsettings-bare"), "{err}");

    // Den ansattes identitet er heller ikke redigerbar.
    let err = sqlx::query("update employee set fodselsnummer = '08888797336' where id = $1")
        .bind(a)
        .execute(&pool)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("uforanderlig"), "{err}");
}

/// Listen viser fødselsdato, ikke fødselsnummer — samme personvernvalg
/// som i aksjeeierboken.
#[tokio::test]
async fn ansattlisten_viser_fodselsdato_ikke_fodselsnummer() {
    let Some(pool) = pool().await else { return };
    let company = selskap(&pool).await;
    ansatt(&pool, company, "26829398612", "Kari Utvikler", 4_000_000).await;

    let ansatte = lonn::list_ansatte(&pool, company).await.unwrap();
    assert_eq!(ansatte.len(), 1);
    assert_eq!(ansatte[0].fodselsdato, Some(dato(1993, 2, 26)));
    assert!(
        !format!("{:?}", ansatte[0]).contains("26829398612"),
        "fødselsnummeret skal ikke ligge i ansattlisten"
    );
}

#[tokio::test]
async fn ugyldig_fodselsnummer_avvises_ved_registrering() {
    let Some(pool) = pool().await else { return };
    let company = selskap(&pool).await;
    let feil = lonn::create_ansatt(
        &pool,
        company,
        &NyAnsatt {
            fodselsnummer: "26829398613".into(),
            navn: "Feil Nummer".into(),
            stilling: None,
            ansatt_fra: dato(2025, 1, 1),
            manedslonn_ore: Some(1),
            timelonn_ore: None,
            trekk_type: "ingen".into(),
            trekk_prosent_bp: None,
            trekk_tabell: None,
            feriepenger_bp: 1020,
            bank_account: None,
            note: None,
        },
        "Test",
    )
    .await
    .unwrap_err();
    assert!(feil.to_string().contains("fødselsnummer"), "{feil}");
}

/// Lønnsslippen bygges av den innsettings-bare lønnslinjen, så den kan
/// gjenskapes for alltid — og den skal forklare trekket, ikke bare
/// oppgi det.
#[tokio::test]
async fn lonnsslipp_bygges_fra_linjen_med_hittil_i_ar() {
    let Some(pool) = pool().await else { return };
    let company = selskap(&pool).await;
    let a = ansatt(&pool, company, "26829398612", "Kari Utvikler", 5_000_000).await;
    let post = |fp: i64| {
        vec![Lonnspost {
            employee_id: a,
            brutto_ore: None,
            feriepenger_ore: fp,
            fra_timer: false,
        }]
    };

    lonn::kjor_lonn(
        &pool,
        company,
        2026,
        5,
        dato(2026, 5, 25),
        "I",
        &post(0),
        "Test",
    )
    .await
    .unwrap();
    let juni = lonn::kjor_lonn(
        &pool,
        company,
        2026,
        6,
        dato(2026, 6, 20),
        "I",
        &post(4_000_000),
        "Test",
    )
    .await
    .unwrap();

    let slipp = lonn::lonnsslipp(&pool, company, juni.id, a).await.unwrap();
    assert_eq!(slipp.ansatt_navn, "Kari Utvikler");
    // Fødselsdato, ikke fødselsnummer — også på slippen.
    assert_eq!(slipp.ansatt_fodselsdato, Some(dato(1993, 2, 26)));
    // Brutto på slippen er ALT som utbetales; trekkgrunnlaget er mindre.
    assert_eq!(slipp.brutto_ore, 9_000_000);
    assert_eq!(slipp.trekkgrunnlag_ore, 5_000_000);
    assert_eq!(slipp.forskuddstrekk_ore, 1_750_000);
    assert_eq!(slipp.netto_ore, 7_250_000);
    assert_eq!(slipp.trekk_prosent_bp, Some(3500));
    // Hittil i år t.o.m. juni: to måneder lønn + juni-feriepengene.
    assert_eq!(slipp.hittil_brutto_ore, 5_000_000 + 9_000_000);
    assert_eq!(slipp.hittil_trekk_ore, 3_500_000);
    assert_eq!(slipp.hittil_feriepenger_ore, 1_020_000);

    // Og den rendrer til en PDF som forklarer trekkfriheten.
    let pdf = regnmed_core::lonnsslipp::render_lonnsslipp(&slipp);
    assert!(pdf.starts_with(b"%PDF-1.4"));
    let tekst = String::from_utf8_lossy(&pdf).to_string();
    assert!(tekst.contains("uten forskuddstrekk"), "{tekst}");
    assert!(!tekst.contains("26829398612"), "fnr skal ikke i slippen");

    // Mai-slippen ser bare mai i hittil-tallene.
    let mai_id = lonn::list_kjoringer(&pool, company, Some(2026))
        .await
        .unwrap()
        .into_iter()
        .find(|k| k.maned == 5)
        .unwrap();
    assert_eq!(mai_id.ansatte.len(), 1, "listingen kjenner deltakerne");
    let mai = lonn::lonnsslipp(&pool, company, mai_id.id, a)
        .await
        .unwrap();
    assert_eq!(mai.hittil_brutto_ore, 5_000_000);
}

/// Timelønn fra timeføringen. Det viktigste her er NEKTELSEN: timer som
/// fortsatt kan endres skal ikke kunne betales, fordi lønnskjøringen er
/// innsettings-bar og de to da spriker for alltid.
#[tokio::test]
async fn timelonn_krever_at_maneden_er_last() {
    let Some(pool) = pool().await else { return };
    let company = selskap(&pool).await;

    let person = regnmed_db::ensure_person(
        &pool,
        &format!("test|{}", Uuid::new_v4()),
        Some("Timelønnet"),
        None,
    )
    .await
    .unwrap();
    let a = lonn::create_ansatt(
        &pool,
        company,
        &NyAnsatt {
            fodselsnummer: "25927898821".into(),
            navn: "Timelønnet".into(),
            stilling: None,
            ansatt_fra: dato(2025, 1, 1),
            manedslonn_ore: None,
            timelonn_ore: Some(45_000), // 450 kr/t
            trekk_type: "prosent".into(),
            trekk_prosent_bp: Some(3000),
            trekk_tabell: None,
            feriepenger_bp: 1020,
            bank_account: None,
            note: None,
        },
        "Test",
    )
    .await
    .unwrap();
    sqlx::query("update employee set person_id = $2 where id = $1")
        .bind(a)
        .bind(person)
        .execute(&pool)
        .await
        .unwrap();

    // 20 timer i mars.
    for dag in [2u32, 3, 4] {
        regnmed_db::timesheet::create_time_entry(
            &pool,
            company,
            person,
            &regnmed_db::timesheet::TimeEntryDraft {
                dato: dato(2026, 3, dag),
                minutter: 400,
                beskrivelse: "Arbeid".into(),
                prosjekt: None,
                fakturerbar: false,
                timesats_ore: None,
            },
            "Test",
        )
        .await
        .unwrap();
    }

    let g = lonn::timegrunnlag(&pool, company, a, 2026, 3)
        .await
        .unwrap();
    assert_eq!(g.minutter, 1200, "20 timer");
    assert_eq!(g.belop_ore, 900_000, "20 t x 450 kr = 9 000 kr");
    assert!(!g.laast, "ikke låst ennå");

    let post = || {
        vec![Lonnspost {
            employee_id: a,
            brutto_ore: None,
            feriepenger_ore: 0,
            fra_timer: true,
        }]
    };

    // Ulåst: nektes.
    let feil = lonn::kjor_lonn(
        &pool,
        company,
        2026,
        3,
        dato(2026, 3, 25),
        "I",
        &post(),
        "Test",
    )
    .await
    .unwrap_err();
    assert!(feil.to_string().contains("ikke låst"), "{feil}");

    // Lås måneden, og kjøringen går.
    regnmed_db::timesheet::set_timesheet_lock(&pool, company, dato(2026, 3, 31), "Test", None)
        .await
        .unwrap();
    let g = lonn::timegrunnlag(&pool, company, a, 2026, 3)
        .await
        .unwrap();
    assert!(g.laast);

    let kjoring = lonn::kjor_lonn(
        &pool,
        company,
        2026,
        3,
        dato(2026, 3, 25),
        "I",
        &post(),
        "Test",
    )
    .await
    .unwrap();
    assert_eq!(kjoring.sum.brutto_ore, 900_000, "timene, ikke månedslønn");
    assert_eq!(kjoring.sum.forskuddstrekk_ore, 270_000, "30 % av 9 000 kr");
    assert_eq!(voucher_sum(&pool, kjoring.voucher_id).await, 0);
}

/// Uten kobling til en portalbruker vet timeføringen ikke hvem den
/// ansatte er — og da sier vi det, i stedet for å betale null.
#[tokio::test]
async fn ansatt_uten_portalbruker_gir_tydelig_feil() {
    let Some(pool) = pool().await else { return };
    let company = selskap(&pool).await;
    let a = ansatt(&pool, company, "26829398612", "Uten kobling", 4_000_000).await;

    let feil = lonn::timegrunnlag(&pool, company, a, 2026, 3)
        .await
        .unwrap_err();
    assert!(feil.to_string().contains("portalbruker"), "{feil}");
}

// ---------------------------------------------------------------------
// Arbeidsgiveravgift på feriepenger som er avsatt, men ikke utbetalt.
//
// Avgiften forfaller først når feriepengene utbetales, men forpliktelsen
// oppstår med opptjeningen. Modellen er et MÅL, ikke en strøm av
// tillegg: etter hver kjøring skal konto 2780 være satsen av det som
// faktisk skyldes, og kjøringen bokfører differansen.
// ---------------------------------------------------------------------

/// Saldoen på en konto for HELE selskapet, ikke bare ett bilag.
async fn konto_saldo(pool: &PgPool, company: Uuid, konto: &str) -> i64 {
    sqlx::query_scalar(
        "select coalesce(sum(e.amount_ore), 0)::bigint from entry e
         join account a on a.id = e.account_id
         where a.company_id = $1 and a.number = $2",
    )
    .bind(company)
    .bind(konto)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn kjor(
    pool: &PgPool,
    company: Uuid,
    a: Uuid,
    maned: u32,
    sone: &str,
    brutto: Option<i64>,
    feriepenger: i64,
) -> lonn::Lonnskjoring {
    lonn::kjor_lonn(
        pool,
        company,
        2026,
        maned,
        dato(2026, maned, 20),
        sone,
        &[Lonnspost {
            employee_id: a,
            brutto_ore: brutto,
            feriepenger_ore: feriepenger,
            fra_timer: false,
        }],
        "Test",
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn avsetning_pa_ikke_utbetalte_feriepenger_bokfores() {
    let Some(pool) = pool().await else { return };
    let company = selskap(&pool).await;
    let a = ansatt(&pool, company, "26829398612", "Kari Avsetning", 5_000_000).await;

    let k = kjor(&pool, company, a, 3, "I", None, 0).await;

    // 10,2 % av 50 000 = 5 100 kr feriepenger skyldes; 14,1 % av det er
    // 719,10 kr i avgift som påløper nå og forfaller ved utbetaling.
    assert_eq!(k.sum.feriepengeavsetning_ore, 510_000);
    assert_eq!(k.sum.aga_feriepenger_ore, 71_910);
    assert_eq!(voucher_sum(&pool, k.voucher_id).await, 0);
    assert_eq!(konto_belop(&pool, k.voucher_id, "5405").await, 71_910);
    assert_eq!(konto_belop(&pool, k.voucher_id, "2780").await, -71_910);
    assert!(k.advarsler.is_empty(), "{:?}", k.advarsler);
}

/// Livsløpet: avgiften avsettes ved opptjening og føres tilbake ved
/// utbetaling — for da er den ordinære aga-linjen den som bærer den.
/// Går dette galt, blir avgiften enten kostnadsført to ganger eller
/// stående som en gjeld som aldri forsvinner.
#[tokio::test]
async fn avsetningen_fores_tilbake_nar_feriepengene_utbetales() {
    let Some(pool) = pool().await else { return };
    let company = selskap(&pool).await;
    let a = ansatt(&pool, company, "08888797336", "Ola Feriepenger", 5_000_000).await;

    let opptjening = kjor(&pool, company, a, 3, "I", None, 0).await;
    assert_eq!(opptjening.sum.aga_feriepenger_ore, 71_910);

    // Ferieavvikling: ingen ordinær lønn, feriepengene utbetales.
    let utbetaling = kjor(&pool, company, a, 4, "I", Some(0), 510_000).await;

    assert_eq!(
        utbetaling.sum.aga_feriepenger_ore, -71_910,
        "avsetningen føres tilbake i sin helhet"
    );
    assert_eq!(voucher_sum(&pool, utbetaling.voucher_id).await, 0);
    // Avgiften på det utbetalte ligger nå i den ordinære aga-linjen.
    assert_eq!(utbetaling.sum.aga_ore, 71_910);

    // Og etterpå står begge kontoene på null: ingen gjeld igjen, ingen
    // avsetning igjen.
    assert_eq!(konto_saldo(&pool, company, "2940").await, 0);
    assert_eq!(konto_saldo(&pool, company, "2780").await, 0);
}

/// Feriepengegjeld som ikke bærer avsetning — fordi den ble opptjent før
/// funksjonen fantes, eller i en sone uten avgift — tas igjen ved neste
/// kjøring. Det er hele poenget med å sikte mot en saldo i stedet for å
/// legge til et beløp.
#[tokio::test]
async fn gjeld_uten_avsetning_tas_igjen_ved_neste_kjoring() {
    let Some(pool) = pool().await else { return };
    let company = selskap(&pool).await;
    let a = ansatt(&pool, company, "25927898821", "Nils Sonebytte", 5_000_000).await;

    // Sone V er nullsats: feriepenger opptjenes, ingen avgift avsettes.
    let uten = kjor(&pool, company, a, 3, "V", None, 0).await;
    assert_eq!(uten.sum.aga_feriepenger_ore, 0);
    assert_eq!(konto_saldo(&pool, company, "2780").await, 0);

    // Virksomheten flytter til sone I. Nå skylder den avgift på ALT som
    // står ubetalt, ikke bare på månedens opptjening.
    let med = kjor(&pool, company, a, 4, "I", None, 0).await;
    let skyldig = 510_000 + 510_000;
    assert_eq!(
        med.sum.aga_feriepenger_ore,
        skyldig * 1410 / 10_000,
        "hele gjelden får avsetning, ikke bare den nye måneden"
    );
    assert_eq!(konto_saldo(&pool, company, "2780").await, -143_820);
}

/// Invarianten som gjør at avsetningen ikke kan drive: etter enhver
/// kjøring er saldoen på 2780 nøyaktig satsen av saldoen på 2940.
#[tokio::test]
async fn avsetningen_er_alltid_satsen_av_feriepengegjelden() {
    let Some(pool) = pool().await else { return };
    let company = selskap(&pool).await;
    let a = ansatt(&pool, company, "03048810003", "Turid Invariant", 4_321_000).await;
    let b = ansatt(&pool, company, "03048810194", "Per Invariant", 2_777_700).await;

    for (maned, feriepenger) in [(3u32, 0i64), (4, 0), (5, 130_000), (6, 250_000)] {
        lonn::kjor_lonn(
            &pool,
            company,
            2026,
            maned,
            dato(2026, maned, 20),
            "I",
            &[
                Lonnspost {
                    employee_id: a,
                    brutto_ore: None,
                    feriepenger_ore: feriepenger,
                    fra_timer: false,
                },
                Lonnspost {
                    employee_id: b,
                    brutto_ore: None,
                    feriepenger_ore: feriepenger,
                    fra_timer: false,
                },
            ],
            "Test",
        )
        .await
        .unwrap();

        // Avrundingen skjer PER ANSATT, så fasiten må bygges per ansatt
        // — 14,1 % av totalen ville bommet med et øre eller to og gjort
        // testen til en tilnærming i stedet for en invariant.
        let per_ansatt: Vec<i64> = sqlx::query_scalar(
            "select coalesce(sum(l.feriepengeavsetning_ore - l.feriepenger_ore), 0)::bigint
             from payroll_line l join payroll_run r on r.id = l.run_id
             where r.company_id = $1 group by l.employee_id",
        )
        .bind(company)
        .fetch_all(&pool)
        .await
        .unwrap();
        let ventet: i64 = per_ansatt
            .iter()
            .map(|s| regnmed_core::lonn::aga_avsetning_mal(*s, 1410))
            .sum();

        assert_eq!(
            per_ansatt.iter().sum::<i64>(),
            -konto_saldo(&pool, company, "2940").await,
            "etter {maned:02}/2026: lønnshistorikken skal forklare hele 2940"
        );
        assert_eq!(
            -konto_saldo(&pool, company, "2780").await,
            ventet,
            "etter {maned:02}/2026: 2780 skal være 14,1 % av hver ansatts gjeld"
        );
    }
}

/// Feriepengegjeld som ikke stammer fra lønnskjøringene — en
/// åpningsbalanse, en manuell avsetning — kan ikke knyttes til noen
/// ansatt, og får derfor ingen avgiftsavsetning. Det er en reell
/// begrensning, og kjøringen sier fra om den i stedet for å late som.
#[tokio::test]
async fn ufordelt_feriepengegjeld_gir_advarsel_ikke_stillhet() {
    let Some(pool) = pool().await else { return };
    let company = selskap(&pool).await;
    let a = ansatt(&pool, company, "03048810275", "Åse Overtatt", 5_000_000).await;

    // Regnskapsføreren avsetter feriepenger manuelt, uten ansattkobling.
    use regnmed_core::Ore;
    use regnmed_core::voucher::{EntryDraft, VoucherDraft};
    let linje = |konto: &str, belop: i64| EntryDraft {
        account_number: konto.into(),
        amount: Ore(belop),
        vat_code: None,
        description: Some("Overtatt feriepengegjeld".into()),
        party_no: None,
        avdeling: None,
        prosjekt: None,
        valuta: None,
    };
    regnmed_db::post_voucher(
        &pool,
        company,
        &VoucherDraft {
            journal_code: "GL".into(),
            voucher_date: dato(2026, 1, 1),
            description: "Åpningsbalanse feriepenger".into(),
            reverses: None,
            entries: vec![linje("5090", 1_000_000), linje("2940", -1_000_000)],
        },
        "Test",
    )
    .await
    .unwrap();

    let k = kjor(&pool, company, a, 3, "I", None, 0).await;

    // Avsetningen dekker bare det lønnskjøringen selv har opptjent.
    assert_eq!(k.sum.aga_feriepenger_ore, 71_910);
    let advarsel = k.advarsler.join(" ");
    assert!(
        advarsel.contains("10000,00"),
        "differansen skal navngis: {advarsel}"
    );
    assert!(advarsel.contains("2780"), "{advarsel}");
}
