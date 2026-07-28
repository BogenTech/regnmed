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

// ---------------------------------------------------------------------
// Kortskinnen (#74): lagret kort og bokføring av trekk.
// ---------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Kort {
    pub stripe_customer_id: String,
    pub payment_method_id: String,
    pub brand: String,
    pub last4: String,
    pub aktiv: bool,
}

pub async fn kort_for(pool: &PgPool, company_id: Uuid) -> Result<Option<Kort>> {
    let row = sqlx::query(
        "select stripe_customer_id, payment_method_id, brand, last4, aktiv
         from betalingskort where company_id = $1",
    )
    .bind(company_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| Kort {
        stripe_customer_id: r.get("stripe_customer_id"),
        payment_method_id: r.get("payment_method_id"),
        brand: r.get("brand"),
        last4: r.get("last4"),
        aktiv: r.get("aktiv"),
    }))
}

/// Lagrer (eller erstatter) selskapets kort. Nytt kort overtar —
/// tilstand, ikke bevis; trekkloggen (`kortbetaling`) er beviset.
pub async fn lagre_kort(
    pool: &PgPool,
    company_id: Uuid,
    stripe_customer_id: &str,
    payment_method_id: &str,
    brand: &str,
    last4: &str,
) -> Result<()> {
    sqlx::query(
        "insert into betalingskort
             (company_id, stripe_customer_id, payment_method_id, brand, last4, aktiv)
         values ($1,$2,$3,$4,$5,true)
         on conflict (company_id) do update
             set stripe_customer_id = $2, payment_method_id = $3,
                 brand = $4, last4 = $5, aktiv = true, updated_at = now()",
    )
    .bind(company_id)
    .bind(stripe_customer_id)
    .bind(payment_method_id)
    .bind(brand)
    .bind(last4)
    .execute(pool)
    .await?;
    Ok(())
}

/// Registrerer utfallet av et korttrekk — webhookens ene jobb.
///
/// Idempotent: `payment_intent_id` er unik, så samme hendelse levert to
/// ganger bokfører aldri to ganger (returnerer `false`). Ved suksess
/// bokføres betalingsbilaget i DRIFTSSELSKAPETS hovedbok (1570
/// Kortoppgjør mot 1500 med part) og reskontroposten lukkes — alt i ÉN
/// transaksjon med loggraden.
pub async fn registrer_kortbetaling(
    pool: &PgPool,
    drift_company_id: Uuid,
    invoice_id: Uuid,
    payment_intent_id: &str,
    succeeded: bool,
    belop_ore: i64,
    detail: Option<&str>,
) -> Result<bool> {
    // Fakturaen må være driftsselskapets, og beløpet må stemme med
    // fordringen — vi opprettet trekket selv, avvik er en feil.
    let faktura = sqlx::query(
        "select e.id as receivable_entry, e.amount_ore::bigint as gross,
                p.party_no, i.invoice_no
         from invoice i
         join entry e on e.id = i.receivable_entry_id
         join party p on p.id = i.party_id
         where i.id = $1 and i.company_id = $2",
    )
    .bind(invoice_id)
    .bind(drift_company_id)
    .fetch_optional(pool)
    .await?
    .context("ukjent abonnementsfaktura for korttrekket")?;
    let receivable_entry: Uuid = faktura.get("receivable_entry");
    let gross: i64 = faktura.get("gross");
    let party_no: String = faktura.get("party_no");
    let invoice_no: i64 = faktura.get("invoice_no");

    // Kundeselskapet identifiseres av kjøringsloggen (fakturaen ble
    // skapt av fakturer_maned med run-rad i samme tx).
    let kunde: Uuid =
        sqlx::query_scalar("select company_id from abonnement_faktura_run where invoice_id = $1")
            .bind(invoice_id)
            .fetch_optional(pool)
            .await?
            .context("fakturaen er ikke en abonnementsfaktura (ingen kjøringsrad)")?;

    crate::ensure_account(pool, drift_company_id, "1570", "Kortoppgjør").await?;
    let idag: NaiveDate = sqlx::query_scalar("select current_date")
        .fetch_one(pool)
        .await?;

    let mut tx = pool.begin().await?;
    let nytt = sqlx::query(
        "insert into kortbetaling
             (id, company_id, invoice_id, payment_intent_id, status, belop_ore, detail)
         values ($1,$2,$3,$4,$5,$6,$7)
         on conflict (payment_intent_id) do nothing",
    )
    .bind(Uuid::new_v4())
    .bind(kunde)
    .bind(invoice_id)
    .bind(payment_intent_id)
    .bind(if succeeded { "succeeded" } else { "failed" })
    .bind(belop_ore)
    .bind(detail)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if nytt == 0 {
        return Ok(false); // allerede registrert — webhook-replay
    }
    if !succeeded {
        tx.commit().await?;
        return Ok(true); // feilet trekk logges; purring/sperre tar resten (#75)
    }

    ensure!(
        belop_ore == gross,
        "trekket ({belop_ore} øre) stemmer ikke med fordringen ({gross} øre) for faktura {invoice_no}"
    );

    let draft = regnmed_core::voucher::VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: idag,
        description: format!("Kortbetaling faktura {invoice_no}"),
        reverses: None,
        entries: vec![
            regnmed_core::voucher::EntryDraft {
                account_number: "1570".into(),
                amount: regnmed_core::Ore(gross),
                vat_code: None,
                description: None,
                party_no: None,
                avdeling: None,
                prosjekt: None,
                valuta: None,
            },
            regnmed_core::voucher::EntryDraft {
                account_number: "1500".into(),
                amount: regnmed_core::Ore(-gross),
                vat_code: None,
                description: None,
                party_no: Some(party_no),
                avdeling: None,
                prosjekt: None,
                valuta: None,
            },
        ],
    };
    let posted =
        crate::post_voucher_in(&mut tx, drift_company_id, &draft, "kortskinnen (webhook)").await?;
    let betalings_entry: Uuid =
        sqlx::query_scalar("select id from entry where voucher_id = $1 and party_id is not null")
            .bind(posted.id)
            .fetch_one(&mut *tx)
            .await?;
    // Matchen direkte i transaksjonen: fordringen (debet) mot
    // betalingen (kredit) — samme part og konto by construction.
    sqlx::query(
        "insert into reskontro_match (id, entry_a, entry_b, amount_ore, matched_by)
         values ($1,$2,$3,$4,'kortskinnen (webhook)')",
    )
    .bind(Uuid::now_v7())
    .bind(receivable_entry)
    .bind(betalings_entry)
    .bind(gross)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(true)
}

/// Ny rad i prislisten (prisen er daterte data — en endring er en ny
/// rad med kilde, aldri en omskriving; docs/abonnement.md §4).
pub async fn sett_pris(
    pool: &PgPool,
    plan: &str,
    pris_ore_per_mnd: i64,
    fra: NaiveDate,
    kilde: &str,
) -> Result<()> {
    ensure!(!kilde.trim().is_empty(), "prisraden må ha en kilde");
    ensure!(pris_ore_per_mnd >= 0, "prisen kan ikke være negativ");
    sqlx::query(
        "insert into abonnement_pris (plan, pris_ore_per_mnd, valid_from, kilde)
         values ($1,$2,$3,$4)",
    )
    .bind(plan)
    .bind(pris_ore_per_mnd)
    .bind(fra)
    .bind(kilde.trim())
    .execute(pool)
    .await?;
    Ok(())
}

#[derive(Debug)]
pub struct Prisrad {
    pub plan: String,
    pub pris_ore_per_mnd: i64,
    pub valid_from: NaiveDate,
    pub kilde: String,
}

/// Hele prislisten, nyeste først per plan.
pub async fn list_priser(pool: &PgPool) -> Result<Vec<Prisrad>> {
    let rows = sqlx::query(
        "select plan, pris_ore_per_mnd, valid_from, kilde
         from abonnement_pris order by plan, valid_from desc",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| Prisrad {
            plan: r.get("plan"),
            pris_ore_per_mnd: r.get("pris_ore_per_mnd"),
            valid_from: r.get("valid_from"),
            kilde: r.get("kilde"),
        })
        .collect())
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
    /// Fakturaens id + brutto (inkl. mva) — det kortskinnen trenger for
    /// å trekke (idempotensnøkkel og beløp).
    pub invoice_id: Option<Uuid>,
    pub gross_ore: Option<i64>,
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
            Ok(Some((nr, id, gross))) => FakturaUtfall {
                company_id,
                company_navn: navn,
                invoice_no: Some(nr),
                invoice_id: Some(id),
                gross_ore: Some(gross),
                detail: None,
            },
            // De ufarlige tilfellene: måneden er alt fakturert, eller
            // planen koster ingenting.
            Ok(None) => FakturaUtfall {
                company_id,
                company_navn: navn,
                invoice_no: None,
                invoice_id: None,
                gross_ore: None,
                detail: Some("hoppet over".into()),
            },
            Err(e) => FakturaUtfall {
                company_id,
                company_navn: navn,
                invoice_no: None,
                invoice_id: None,
                gross_ore: None,
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
) -> Result<Option<(i64, Uuid, i64)>> {
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
    Ok(Some((
        utstedt.invoice_no,
        utstedt.invoice_id,
        utstedt.gross_ore,
    )))
}
