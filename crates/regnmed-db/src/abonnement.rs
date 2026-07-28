//! Abonnement (#65, docs/abonnement.md): dekning, status og den
//! månedlige fakturakjøringen.
//!
//! Statusregelen bor i `regnmed-core::abonnement`; her hentes bare
//! faktaene den trenger. Fakturaen utstedes av den ordinære motoren
//! (`create_invoice_in`) i DRIFTSSELSKAPETS hovedbok — regnmed er sin
//! egen kunde nummer én, med gap-frie nummer, KID og reskontro som alle
//! andre.

use anyhow::{Context, Result, ensure};
use chrono::{Datelike, NaiveDate, Utc};
use regnmed_core::abonnement::Status;
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// Faktaene statusregelen trenger, hentet i én spørring.
async fn fakta(
    pool: &PgPool,
    company_id: Uuid,
    idag: NaiveDate,
) -> Result<(NaiveDate, bool, Option<NaiveDate>)> {
    let row = sqlx::query(
        "select c.created_at::date as opprettet,
                exists(select 1 from abonnement a
                        where a.company_id = c.id
                          and a.valid_from <= $2
                          and (a.valid_to is null or a.valid_to > $2)) as dekket,
                (select max(a.valid_to) from abonnement a
                  where a.company_id = c.id and a.valid_to is not null
                    and a.valid_to <= $2) as siste_slutt
         from company c where c.id = $1",
    )
    .bind(company_id)
    .bind(idag)
    .fetch_optional(pool)
    .await?
    .context("ukjent selskap")?;
    Ok((
        row.get("opprettet"),
        row.get("dekket"),
        row.get("siste_slutt"),
    ))
}

/// Statusen i dag for ett selskap.
pub async fn status_for(pool: &PgPool, company_id: Uuid) -> Result<Status> {
    let idag = Utc::now().date_naive();
    let (opprettet, dekket, siste_slutt) = fakta(pool, company_id, idag).await?;
    Ok(regnmed_core::abonnement::status(
        opprettet,
        dekket,
        siste_slutt,
        idag,
    ))
}

/// Skal skrivende handlinger avvises? Kalles fra tilgangsvakten på
/// endrende rettigheter — én søm, som resten av vakten.
pub async fn sperret(pool: &PgPool, company_id: Uuid) -> Result<bool> {
    Ok(status_for(pool, company_id).await?.sperret())
}

/// Tegner en dekning fra `fra`. Åpen (`til videre`) når `til` er None.
pub async fn tegn(
    pool: &PgPool,
    company_id: Uuid,
    plan: &str,
    fra: NaiveDate,
    til: Option<NaiveDate>,
    note: &str,
    av: &str,
) -> Result<Uuid> {
    ensure!(!note.trim().is_empty(), "tegningen må ha en referanse");
    let id = Uuid::new_v4();
    sqlx::query(
        "insert into abonnement (id, company_id, plan, valid_from, valid_to, note, created_by)
         values ($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(id)
    .bind(company_id)
    .bind(plan)
    .bind(fra)
    .bind(til)
    .bind(note.trim())
    .bind(av)
    .execute(pool)
    .await?;
    Ok(id)
}

/// Avslutter den åpne dekningen: setter `valid_to` (EKSKLUSIV) på
/// selskapets åpne rad. Historikken røres aldri.
pub async fn avslutt(pool: &PgPool, company_id: Uuid, til: NaiveDate) -> Result<()> {
    let n = sqlx::query(
        "update abonnement set valid_to = $2
         where company_id = $1 and valid_to is null and valid_from < $2",
    )
    .bind(company_id)
    .bind(til)
    .execute(pool)
    .await?
    .rows_affected();
    ensure!(
        n > 0,
        "ingen åpen dekning å avslutte (eller sluttdatoen er før startdatoen)"
    );
    Ok(())
}

/// Prisen som gjelder på en dato, i øre per måned eks. mva.
pub async fn pris_pa(pool: &PgPool, plan: &str, dato: NaiveDate) -> Result<i64> {
    let pris: Option<i64> = sqlx::query_scalar(
        "select pris_ore_per_mnd from abonnement_pris
         where plan = $1 and valid_from <= $2
         order by valid_from desc limit 1",
    )
    .bind(plan)
    .bind(dato)
    .fetch_optional(pool)
    .await?;
    pris.with_context(|| format!("ingen pris for planen «{plan}» per {dato} — prislisten er data (abonnement_pris) og må dekke datoen"))
}

#[derive(Debug)]
pub struct FakturaUtfall {
    pub company_id: Uuid,
    pub company_navn: String,
    /// Fakturanummer i driftsselskapet når fakturaen ble utstedt; None
    /// med `detail` når selskapet ble hoppet over eller feilet.
    pub invoice_no: Option<i64>,
    pub detail: Option<String>,
}

/// Fakturerer måneden `idag` ligger i, for hvert selskap med dekning på
/// månedens første dag — inn i DRIFTSSELSKAPETS hovedbok. Idempotent:
/// kjøringsraden er unik per (selskap, år, måned) og skrives i samme
/// transaksjon som fakturaen. `bare` avgrenser til ett kundeselskap
/// (etterfakturering, testing); None = alle med dekning.
pub async fn fakturer_maned(
    pool: &PgPool,
    drift_company_id: Uuid,
    idag: NaiveDate,
    bare: Option<Uuid>,
) -> Result<Vec<FakturaUtfall>> {
    let forste = NaiveDate::from_ymd_opt(idag.year(), idag.month(), 1).unwrap();

    // Driftsselskapet trenger konto og journal for salget; ensure er
    // idempotent og gjør første kjøring selvoppsettende.
    crate::ensure_journal(pool, drift_company_id, "GL", "Hovedbok").await?;
    for (nr, navn) in [
        ("1500", "Kundefordringer"),
        ("2700", "Utgående mva"),
        ("3000", "Salgsinntekt, avgiftspliktig"),
    ] {
        crate::ensure_account(pool, drift_company_id, nr, navn).await?;
    }
    // Fordringskontoen må bære reskontro, ellers nekter posteringen
    // partslinjen — og abonnementsfakturaen SKAL på reskontro, det er
    // slik innbetalingen (KID via OCR/bank) lukker den.
    crate::reskontro::set_account_reskontro(pool, drift_company_id, "1500", Some("kunde")).await?;

    let kunder = sqlx::query(
        "select c.id, c.orgnr, c.name, a.plan
         from company c
         join abonnement a on a.company_id = c.id
         where c.id <> $1
           and a.valid_from <= $2
           and (a.valid_to is null or a.valid_to > $2)
           and ($3::uuid is null or c.id = $3)
         order by c.name",
    )
    .bind(drift_company_id)
    .bind(forste)
    .bind(bare)
    .fetch_all(pool)
    .await?;

    let mut utfall = Vec::new();
    for kunde in &kunder {
        let company_id: Uuid = kunde.get("id");
        let navn: String = kunde.get("name");
        let resultat = fakturer_en(
            pool,
            drift_company_id,
            company_id,
            kunde.get("orgnr"),
            &navn,
            kunde.get("plan"),
            idag,
        )
        .await;
        utfall.push(match resultat {
            Ok(nr) => FakturaUtfall {
                company_id,
                company_navn: navn,
                invoice_no: nr,
                // Ok(None) er de ufarlige tilfellene: måneden er alt
                // fakturert, eller planen koster ingenting.
                detail: nr.is_none().then(|| "hoppet over".into()),
            },
            Err(e) => FakturaUtfall {
                company_id,
                company_navn: navn,
                invoice_no: None,
                detail: Some(format!("{e:#}")),
            },
        });
    }
    Ok(utfall)
}

async fn fakturer_en(
    pool: &PgPool,
    drift: Uuid,
    kunde: Uuid,
    kunde_orgnr: String,
    kunde_navn: &str,
    plan: String,
    idag: NaiveDate,
) -> Result<Option<i64>> {
    // Kundeparten i driftsselskapets reskontro, nøklet på orgnr.
    let party_no: Option<String> = sqlx::query_scalar(
        "select party_no from party
         where company_id = $1 and kind = 'kunde' and orgnr = $2",
    )
    .bind(drift)
    .bind(&kunde_orgnr)
    .fetch_optional(pool)
    .await?;
    let party_no = match party_no {
        Some(no) => no,
        None => {
            crate::reskontro::create_party(
                pool,
                drift,
                "kunde",
                kunde_navn,
                Some(&kunde_orgnr),
                None,
            )
            .await?
            .1
        }
    };

    let pris = pris_pa(pool, &plan, idag).await?;
    if pris == 0 {
        return Ok(None); // en gratisplan fakturerer ingenting
    }

    // Rask vei: måneden er alt fakturert. Kappløpet mellom to
    // samtidige kjøringer avgjøres ikke her, men av unikheten på
    // kjøringsraden i transaksjonen under.
    let finnes: Option<Uuid> = sqlx::query_scalar(
        "select invoice_id from abonnement_faktura_run
         where company_id = $1 and ar = $2 and maned = $3",
    )
    .bind(kunde)
    .bind(idag.year())
    .bind(idag.month() as i32)
    .fetch_optional(pool)
    .await?;
    if finnes.is_some() {
        return Ok(None);
    }

    let mut tx = pool.begin().await?;
    let draft = crate::invoice::InvoiceDraft {
        party_no,
        invoice_date: idag,
        due_date: idag + chrono::Days::new(14),
        journal_code: "GL".into(),
        receivable_account: "1500".into(),
        vat_account: "2700".into(),
        valuta: None,
        valuta_kurs_micro: None,
        lines: vec![crate::invoice::InvoiceLineDraft {
            description: format!(
                "regnmed {plan} — {} {}",
                regnmed_core::invoice::maanedsnavn(idag.month()),
                idag.year()
            ),
            account_number: "3000".into(),
            quantity_milli: 1000,
            unit_price_ore: pris,
            vat_code: Some("3".into()),
            avdeling: None,
            prosjekt: None,
            product_id: None,
        }],
    };
    let utstedt = crate::invoice::create_invoice_in(
        pool,
        &mut tx,
        drift,
        &draft,
        "abonnement (regnmed abonnement-faktura)",
        None,
    )
    .await?;
    // Kjøringsraden SIST, i samme transaksjon: den finnes bare når
    // fakturaen finnes. Taper vi et kappløp mot en parallell kjøring,
    // bryter unikheten HELE transaksjonen — fakturaen vår rulles
    // tilbake, vinnerens står, og ingen måned faktureres to ganger.
    let resultat = sqlx::query(
        "insert into abonnement_faktura_run (id, company_id, ar, maned, invoice_id)
         values ($1, $2, $3, $4, $5)",
    )
    .bind(Uuid::new_v4())
    .bind(kunde)
    .bind(idag.year())
    .bind(idag.month() as i32)
    .bind(utstedt.invoice_id)
    .execute(&mut *tx)
    .await;
    if let Err(e) = &resultat {
        if let Some(db) = e.as_database_error() {
            if db.code().as_deref() == Some("23505") {
                // Noen andre rakk måneden først; vår faktura forsvinner
                // med rollbacken når tx droppes her.
                return Ok(None);
            }
        }
    }
    resultat?;
    tx.commit().await?;
    Ok(Some(utstedt.invoice_no))
}
