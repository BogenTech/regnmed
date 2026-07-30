//! Abonnement (#65, docs/abonnement.md): coverage, status and the
//! monthly invoicing run.
//!
//! The status rule lives in `regnmed-core::abonnement`; this module only
//! fetches the facts it needs. The faktura is issued by the ordinary
//! engine (`create_invoice_in`) in the OPS COMPANY's hovedbok — regnmed
//! is its own customer number one, with gap-free numbers, KID and
//! reskontro like everybody else.

use anyhow::{Context, Result, ensure};
use chrono::{Datelike, NaiveDate, Utc};
use regnmed_core::abonnement::Status;
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// The facts the status rule needs, fetched in one query.
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

/// Today's status for one company.
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

/// Should writing actions be refused? Called from the access guard on
/// changing rettigheter — one seam, like the rest of the guard.
pub async fn sperret(pool: &PgPool, company_id: Uuid) -> Result<bool> {
    Ok(status_for(pool, company_id).await?.sperret())
}

/// Starts coverage from `fra`. Open-ended when `til` is None.
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

/// Ends the open coverage: sets `valid_to` (EXCLUSIVE) on the company's
/// open row. History is never touched.
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
// The card rail (#74): stored card and posting of charges.
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

/// Stores (or replaces) the company's card. A new card takes over —
/// state, not evidence; the charge log (`kortbetaling`) is the evidence.
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

/// Records the outcome of a card charge — the webhook's one job.
///
/// Idempotent: `payment_intent_id` is unique, so the same event delivered
/// twice never posts twice (it returns `false`). On success the payment
/// bilag is posted in the OPS COMPANY's hovedbok (1570 Kortoppgjør
/// against 1500 with a party) and the reskontro item is closed — all in
/// ONE transaction with the log row.
pub async fn registrer_kortbetaling(
    pool: &PgPool,
    drift_company_id: Uuid,
    invoice_id: Uuid,
    payment_intent_id: &str,
    succeeded: bool,
    belop_ore: i64,
    detail: Option<&str>,
) -> Result<bool> {
    // The faktura must be the ops company's, and the amount must match
    // the receivable — we created the charge ourselves, so a mismatch is
    // a bug.
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

    // The customer company is identified by the run log (the faktura was
    // created by fakturer_maned with a run row in the same tx).
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
    // The match directly in the transaction: the receivable (debit)
    // against the payment (credit) — same party and account by
    // construction.
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

// ---------------------------------------------------------------------
// Stripe Subscriptions — the abonnement our customers pay US for.
// ---------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct StripeAbo {
    pub stripe_subscription_id: String,
    pub stripe_price_id: String,
    pub plan: String,
    pub interval: String,
    pub status: String,
    pub cancel_at: Option<chrono::DateTime<Utc>>,
}

/// The company's live Stripe subscription, if it has one. Cancelled rows
/// are history and never returned here.
pub async fn stripe_abo_for(pool: &PgPool, company_id: Uuid) -> Result<Option<StripeAbo>> {
    let row = sqlx::query(
        "select stripe_subscription_id, stripe_price_id, plan, interval, status, cancel_at
         from abonnement_stripe
         where company_id = $1 and canceled_at is null",
    )
    .bind(company_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| StripeAbo {
        stripe_subscription_id: r.get("stripe_subscription_id"),
        stripe_price_id: r.get("stripe_price_id"),
        plan: r.get("plan"),
        interval: r.get("interval"),
        status: r.get("status"),
        cancel_at: r.get("cancel_at"),
    }))
}

#[derive(Debug, Clone)]
pub struct StripeAboEier {
    pub company_id: Uuid,
    pub plan: String,
    pub interval: String,
}

/// Resolves a Stripe subscription id to the company it belongs to. The
/// webhook for `invoice.paid` carries only the subscription, so this is
/// how a payment finds its customer — and a cancelled subscription still
/// resolves, because its final invoice may arrive after cancellation.
pub async fn stripe_abo_by_subscription(
    pool: &PgPool,
    subscription_id: &str,
) -> Result<Option<StripeAboEier>> {
    let row = sqlx::query(
        "select company_id, plan, interval from abonnement_stripe
         where stripe_subscription_id = $1",
    )
    .bind(subscription_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| StripeAboEier {
        company_id: r.get("company_id"),
        plan: r.get("plan"),
        interval: r.get("interval"),
    }))
}

/// Records a subscription created at Stripe, and opens coverage in the
/// same transaction. Coverage is OPEN-ENDED: it recurs until cancelled,
/// so there is no end date to write until somebody cancels.
pub async fn lagre_stripe_abo(
    pool: &PgPool,
    company_id: Uuid,
    subscription_id: &str,
    price_id: &str,
    plan: &str,
    interval: &str,
    status: &str,
    av: &str,
) -> Result<bool> {
    let mut tx = pool.begin().await?;
    let ny = sqlx::query(
        "insert into abonnement_stripe
             (company_id, stripe_subscription_id, stripe_price_id, plan, interval, status)
         values ($1,$2,$3,$4,$5,$6)
         on conflict (stripe_subscription_id) do nothing",
    )
    .bind(company_id)
    .bind(subscription_id)
    .bind(price_id)
    .bind(plan)
    .bind(interval)
    .bind(status)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if ny == 0 {
        return Ok(false); // webhook-replay
    }
    // Coverage only if the company does not already have an open row —
    // an existing customer on the old rail keeps the one it has.
    let dekket: bool = sqlx::query_scalar(
        "select exists(select 1 from abonnement
                        where company_id = $1 and valid_to is null)",
    )
    .bind(company_id)
    .fetch_one(&mut *tx)
    .await?;
    if !dekket {
        sqlx::query(
            "insert into abonnement (id, company_id, plan, valid_from, valid_to, note, created_by)
             values ($1,$2,$3,current_date,null,$4,$5)",
        )
        .bind(Uuid::new_v4())
        .bind(company_id)
        .bind(plan)
        .bind(format!("Stripe-abonnement {subscription_id}"))
        .bind(av)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(true)
}

/// Mirrors Stripe's own status. Never touches coverage — access is
/// governed by the coverage rows alone, so a `past_due` at Stripe does
/// not silently lock anyone out while Stripe is still retrying.
pub async fn oppdater_stripe_status(
    pool: &PgPool,
    subscription_id: &str,
    status: &str,
    cancel_at: Option<chrono::DateTime<Utc>>,
) -> Result<()> {
    sqlx::query(
        "update abonnement_stripe
            set status = $2, cancel_at = $3, updated_at = now()
          where stripe_subscription_id = $1",
    )
    .bind(subscription_id)
    .bind(status)
    .bind(cancel_at)
    .execute(pool)
    .await?;
    Ok(())
}

/// The subscription ended at Stripe: close the row AND the coverage, in
/// one transaction. `til` is exclusive, as everywhere else.
pub async fn avslutt_stripe_abo(
    pool: &PgPool,
    subscription_id: &str,
    til: NaiveDate,
) -> Result<Option<Uuid>> {
    let mut tx = pool.begin().await?;
    let company: Option<Uuid> = sqlx::query_scalar(
        "update abonnement_stripe
            set canceled_at = now(), status = 'canceled', updated_at = now()
          where stripe_subscription_id = $1 and canceled_at is null
          returning company_id",
    )
    .bind(subscription_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(company) = company else {
        return Ok(None); // already closed — replay
    };
    sqlx::query(
        "update abonnement set valid_to = $2
          where company_id = $1 and valid_to is null and valid_from < $2",
    )
    .bind(company)
    .bind(til)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Some(company))
}

/// A paid Stripe subscription invoice, booked the regnmed way.
///
/// The customer pays Stripe, but bokføringsloven still wants this sale in
/// the ops company's own hovedbok — so ONE transaction issues the faktura
/// through the ordinary engine (gap-free number, KID, reskontro), posts
/// the payment against it, matches the two, and logs the charge.
/// The faktura is therefore "paid" by construction rather than by a flag:
/// its receivable is fully matched the moment it exists.
///
/// Idempotent on the Stripe invoice id: a redelivered webhook returns
/// `false` and changes nothing.
#[allow(clippy::too_many_arguments)]
pub async fn bokfor_stripe_betaling(
    pool: &PgPool,
    drift_company_id: Uuid,
    kunde: Uuid,
    stripe_invoice_id: &str,
    payment_intent_id: &str,
    brutto_ore: i64,
    plan: &str,
    periode: &str,
    idag: NaiveDate,
) -> Result<bool> {
    ensure!(brutto_ore > 0, "betalingen må være positiv");

    // Same self-configuring setup the monthly run does.
    crate::ensure_journal(pool, drift_company_id, "GL", "Hovedbok").await?;
    for (nr, navn) in [
        ("1500", "Kundefordringer"),
        ("1570", "Kortoppgjør"),
        ("2700", "Utgående mva"),
        ("3000", "Salgsinntekt, avgiftspliktig"),
    ] {
        crate::ensure_account(pool, drift_company_id, nr, navn).await?;
    }
    crate::reskontro::set_account_reskontro(pool, drift_company_id, "1500", Some("kunde")).await?;

    let (kunde_orgnr, kunde_navn): (String, String) =
        sqlx::query_as("select orgnr, name from company where id = $1")
            .bind(kunde)
            .fetch_one(pool)
            .await?;
    let party_no: Option<String> = sqlx::query_scalar(
        "select party_no from party where company_id = $1 and kind = 'kunde' and orgnr = $2",
    )
    .bind(drift_company_id)
    .bind(&kunde_orgnr)
    .fetch_optional(pool)
    .await?;
    let party_no = match party_no {
        Some(no) => no,
        None => {
            crate::reskontro::create_party(
                pool,
                drift_company_id,
                "kunde",
                &kunde_navn,
                Some(&kunde_orgnr),
                None,
            )
            .await?
            .1
        }
    };

    // The Stripe price is GROSS; the faktura line wants the base, and the
    // engine adds mva back at the rate valid on the invoice date. Using
    // split_gross here (rather than trusting Stripe) keeps one authority
    // for Norwegian mva: ours.
    let rates = crate::mva::load_vat_rates(pool).await?;
    let rate_bp = regnmed_core::mva::rate_on(&rates, "regular", idag)
        .context("ingen ordinær mva-sats for betalingsdatoen — satsregisteret må dekke datoen")?;
    let (netto, _mva) = regnmed_core::mva::split_gross(brutto_ore, rate_bp);

    let mut tx = pool.begin().await?;

    // The log row references the faktura, so it cannot be written before
    // one exists — the cheap check comes first, and the unique index on
    // stripe_invoice_id is what actually decides a genuine race: the
    // loser's INSERT fails and takes its whole transaction, faktura
    // included, with it. No Stripe invoice is ever booked twice.
    let finnes: bool = sqlx::query_scalar(
        "select exists(select 1 from kortbetaling where stripe_invoice_id = $1)",
    )
    .bind(stripe_invoice_id)
    .fetch_one(&mut *tx)
    .await?;
    if finnes {
        return Ok(false);
    }

    let draft = crate::invoice::InvoiceDraft {
        party_no,
        invoice_date: idag,
        due_date: idag,
        journal_code: "GL".into(),
        receivable_account: "1500".into(),
        vat_account: "2700".into(),
        valuta: None,
        valuta_kurs_micro: None,
        lines: vec![crate::invoice::InvoiceLineDraft {
            description: format!("regnmed {plan} — {periode}"),
            account_number: "3000".into(),
            quantity_milli: 1000,
            unit_price_ore: netto,
            vat_code: Some("3".into()),
            avdeling: None,
            prosjekt: None,
            product_id: None,
        }],
    };
    let utstedt = crate::invoice::create_invoice_in(
        pool,
        &mut tx,
        drift_company_id,
        &draft,
        "Stripe-abonnement (webhook)",
        None,
    )
    .await?;

    let gross = utstedt.gross_ore;
    let voucher = regnmed_core::voucher::VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: idag,
        description: format!("Kortbetaling abonnement, faktura {}", utstedt.invoice_no),
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
                party_no: Some(
                    sqlx::query_scalar::<_, String>(
                        "select p.party_no from invoice i join party p on p.id = i.party_id
                         where i.id = $1",
                    )
                    .bind(utstedt.invoice_id)
                    .fetch_one(&mut *tx)
                    .await?,
                ),
                avdeling: None,
                prosjekt: None,
                valuta: None,
            },
        ],
    };
    let posted = crate::post_voucher_in(
        &mut tx,
        drift_company_id,
        &voucher,
        "Stripe-abonnement (webhook)",
    )
    .await?;

    let receivable_entry: Uuid =
        sqlx::query_scalar("select receivable_entry_id from invoice where id = $1")
            .bind(utstedt.invoice_id)
            .fetch_one(&mut *tx)
            .await?;
    let betalings_entry: Uuid =
        sqlx::query_scalar("select id from entry where voucher_id = $1 and party_id is not null")
            .bind(posted.id)
            .fetch_one(&mut *tx)
            .await?;
    sqlx::query(
        "insert into reskontro_match (id, entry_a, entry_b, amount_ore, matched_by)
         values ($1,$2,$3,$4,'Stripe-abonnement (webhook)')",
    )
    .bind(Uuid::now_v7())
    .bind(receivable_entry)
    .bind(betalings_entry)
    .bind(gross)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "insert into kortbetaling
             (id, company_id, invoice_id, payment_intent_id, status, belop_ore, stripe_invoice_id)
         values ($1,$2,$3,$4,'succeeded',$5,$6)",
    )
    .bind(Uuid::new_v4())
    .bind(kunde)
    .bind(utstedt.invoice_id)
    .bind(payment_intent_id)
    .bind(gross)
    .bind(stripe_invoice_id)
    .execute(&mut *tx)
    .await?;

    // The month is now invoiced: the monthly cron must not issue a second
    // faktura for the same company and month.
    sqlx::query(
        "insert into abonnement_faktura_run (id, company_id, ar, maned, invoice_id)
         values ($1,$2,$3,$4,$5)
         on conflict (company_id, ar, maned) do nothing",
    )
    .bind(Uuid::new_v4())
    .bind(kunde)
    .bind(idag.year())
    .bind(idag.month() as i32)
    .bind(utstedt.invoice_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(true)
}

/// A new row in the price list (the price is dated data — a change is a
/// new row with its kilde, never a rewrite; docs/abonnement.md §4).
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

/// The whole price list, newest first per plan.
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

/// The Stripe Price currently mirroring a plan+interval, if one has been
/// synced. Newest row wins — a price change adds a row, never rewrites.
pub async fn stripe_price_for(
    pool: &PgPool,
    plan: &str,
    interval: &str,
) -> Result<Option<(String, i64)>> {
    let row = sqlx::query(
        "select stripe_price_id, brutto_ore from abonnement_stripe_price
         where plan = $1 and interval = $2
         order by created_at desc limit 1",
    )
    .bind(plan)
    .bind(interval)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| (r.get("stripe_price_id"), r.get("brutto_ore"))))
}

/// Records a Stripe Price created from our price list.
pub async fn lagre_stripe_price(
    pool: &PgPool,
    plan: &str,
    interval: &str,
    brutto_ore: i64,
    stripe_price_id: &str,
    kilde: &str,
) -> Result<()> {
    sqlx::query(
        "insert into abonnement_stripe_price
             (plan, interval, brutto_ore, stripe_price_id, kilde)
         values ($1,$2,$3,$4,$5)",
    )
    .bind(plan)
    .bind(interval)
    .bind(brutto_ore)
    .bind(stripe_price_id)
    .bind(kilde)
    .execute(pool)
    .await?;
    Ok(())
}

/// The gross (incl. mva) a plan costs for one billing interval, from OUR
/// price list at the rate valid on `dato`.
///
/// Yearly is twelve times the monthly price unless the price list itself
/// carries a `year` row — a discount for paying annually is a pricing
/// decision (a new row with its kilde), never a multiplication hidden in
/// code.
pub async fn brutto_for(pool: &PgPool, plan: &str, interval: &str, dato: NaiveDate) -> Result<i64> {
    let egen: Option<i64> = sqlx::query_scalar(
        "select pris_ore_per_mnd from abonnement_pris
         where plan = $1 and interval = $2 and valid_from <= $3
         order by valid_from desc limit 1",
    )
    .bind(plan)
    .bind(interval)
    .bind(dato)
    .fetch_optional(pool)
    .await?;
    let netto = match egen {
        Some(n) => n,
        None if interval == "year" => pris_pa(pool, plan, dato).await? * 12,
        None => pris_pa(pool, plan, dato).await?,
    };
    let rates = crate::mva::load_vat_rates(pool).await?;
    let rate_bp = regnmed_core::mva::rate_on(&rates, "regular", dato)
        .context("ingen ordinær mva-sats for datoen — satsregisteret må dekke den")?;
    Ok(netto + regnmed_core::mva::vat_of_base(netto, rate_bp))
}

/// The price in force on a date, in øre per month excl. mva.
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
    /// Invoice number in the ops company when the faktura was issued;
    /// None with `detail` when the company was skipped or failed.
    pub invoice_no: Option<i64>,
    /// The faktura's id + gross (incl. mva) — what the card rail needs in
    /// order to charge (idempotency key and amount).
    pub invoice_id: Option<Uuid>,
    pub gross_ore: Option<i64>,
    pub detail: Option<String>,
}

/// Invoices the month `idag` falls in, for every company with coverage on
/// the first of that month — into the OPS COMPANY's hovedbok. Idempotent:
/// the run row is unique per (company, year, month) and is written in the
/// same transaction as the faktura. `bare` narrows to a single customer
/// company (back-billing, testing); None = everyone with coverage.
pub async fn fakturer_maned(
    pool: &PgPool,
    drift_company_id: Uuid,
    idag: NaiveDate,
    bare: Option<Uuid>,
) -> Result<Vec<FakturaUtfall>> {
    let forste = NaiveDate::from_ymd_opt(idag.year(), idag.month(), 1).unwrap();

    // The ops company needs an account and a journal for the sale;
    // ensure is idempotent and makes the first run self-configuring.
    crate::ensure_journal(pool, drift_company_id, "GL", "Hovedbok").await?;
    for (nr, navn) in [
        ("1500", "Kundefordringer"),
        ("2700", "Utgående mva"),
        ("3000", "Salgsinntekt, avgiftspliktig"),
    ] {
        crate::ensure_account(pool, drift_company_id, nr, navn).await?;
    }
    // The receivable account must carry reskontro, otherwise posting
    // refuses the party line — and the abonnement faktura MUST be on the
    // reskontro, since that is how the payment (KID via OCR/bank) closes
    // it.
    crate::reskontro::set_account_reskontro(pool, drift_company_id, "1500", Some("kunde")).await?;

    // Companies on the Stripe rail are billed BY STRIPE and booked by the
    // webhook. Excluding them here is what keeps the two rails from both
    // invoicing the same month: the cron runs on the 1st, while Stripe
    // bills on the subscription's own anniversary, so the run-log check
    // alone would not save us — it would only notice after the fact.
    let kunder = sqlx::query(
        "select c.id, c.orgnr, c.name, a.plan
         from company c
         join abonnement a on a.company_id = c.id
         where c.id <> $1
           and a.valid_from <= $2
           and (a.valid_to is null or a.valid_to > $2)
           and ($3::uuid is null or c.id = $3)
           and not exists (select 1 from abonnement_stripe s
                            where s.company_id = c.id and s.canceled_at is null)
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
            // The harmless cases: the month is already invoiced, or the
            // plan costs nothing.
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
    // The customer party in the ops company's reskontro, keyed on orgnr.
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

    // Fast path: the month is already invoiced. A race between two
    // concurrent runs is not decided here, but by the uniqueness of the
    // run row in the transaction below.
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
    // The run row LAST, in the same transaction: it exists only when the
    // faktura exists. If we lose a race against a parallel run, the
    // uniqueness breaks THE WHOLE transaction — our faktura is rolled
    // back, the winner's stands, and no month is ever invoiced twice.
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
                // Somebody else got the month first; our faktura goes
                // away with the rollback when tx is dropped here.
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
